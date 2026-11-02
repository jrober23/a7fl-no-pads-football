//! Assembles broadcast frames: camera work, field, players, ball and overlay.
//!
//! A frame is produced from a [`PlayRecord`] at a chosen time `t` by replaying the
//! deterministic snap simulation up to that moment, so a rendered still always
//! matches the play-by-play.

use a7fl_core::field::{self, FIELD_HALF_WIDTH, FIELD_LENGTH};
use a7fl_core::game::{GamePhase, GameState, PlayRecord, ScrimmageRecord, ThrowOffRecord};
use a7fl_core::math::{clamp, Rgb, Vec2, Vec3};
use a7fl_core::playbook::OffRole;
use a7fl_core::sim::{PlaySim, Side};
use a7fl_core::team::{Team, Uniform};
use a7fl_core::throwoff::{ThrowOffSim, ThrowPhase, ThrowRole};

use crate::fieldtex;
use crate::hud::{self, Banner, INK};
use crate::players::{self, Pose};
use crate::raster::{Frame, Texture};
use crate::scene::{Camera, Lighting, Mesh, SceneRenderer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraMode {
    /// High sideline camera, the workhorse broadcast shot.
    Broadcast,
    /// Behind the offense, looking downfield at the defense and panning with the
    /// ball: the classic game view.
    Classic,
    /// Behind the quarterback for the pre-snap look.
    BehindQb,
    /// Overhead.
    SkyCam,
    /// Low and tight for a scoring play.
    Replay,
    /// Behind the end zone.
    Endzone,
}

/// Everything the renderer needs about one body on the field, copied out of the
/// simulation so the (borrowed) simulation can be dropped before painting.
#[derive(Clone, Debug)]
struct RenderPlayer {
    team: usize,
    roster: usize,
    pos: Vec2,
    heading: f32,
    run_phase: f32,
    speed: f32,
    carrying: bool,
    blocking: bool,
    down: bool,
    /// 0..1 through a tackle, for the two men in it: one wrapping, one going down.
    wrap: f32,
    tackled: f32,
    celebrate: f32,
    /// The move this man is in the middle of, if he is the carrier, and how far
    /// through it he is.
    motion: Option<a7fl_core::moves::Kind>,
    motion_progress: f32,
}

/// How far the picture swings toward the ball each frame. A camera that snaps to
/// it looks like a camera operator being yanked by the lens; this is the lag a
/// heavy lens gives you, about four frames to get most of the way.
const FOLLOW: f32 = 0.22;

/// The rate while the ball is in the air. A throw is the fastest thing on the
/// field, and a shot still easing at the runner's pace is always a step behind it.
const FOLLOW_AIR: f32 = 0.30;

/// How far toward the landing spot the aim leans while a pass is in the air. The
/// pan has to start when the ball leaves his hand -- the ball is going where it is
/// going, and the shot going with it is what "following the ball" means -- but a
/// lean that is too deep leaves the ball itself out of the picture, so it is a
/// quarter of the way and no more.
const AIR_LEAD: f32 = 0.24;

/// Where the camera is pointed: the ball, eased.
///
/// Football is shot with the ball as the anchor. Every mode used to build its own
/// aim out of a mix of the line of scrimmage and the play's action point, so a run
/// that got past the defence walked out of the frame while the camera stayed on a
/// line the ball had left behind. One tracked point, shared by all of them, makes
/// the picture follow the play.
#[derive(Clone, Copy, Debug, Default)]
struct Focus {
    spot: Option<Vec2>,
}

impl Focus {
    fn track(&mut self, ball: Vec2) -> Vec2 {
        self.track_with(ball, FOLLOW)
    }

    fn track_with(&mut self, ball: Vec2, rate: f32) -> Vec2 {
        let spot = match self.spot {
            // The first frame after a cut starts on the ball, not halfway to it.
            None => ball,
            Some(prev) => prev + (ball - prev) * rate,
        };
        self.spot = Some(spot);
        spot
    }
}

pub struct Broadcast {
    pub w: usize,
    pub h: usize,
    pub field: Texture,
    pub scene: SceneRenderer,
    pub teams: [Team; 2],
    pub uniforms: [Uniform; 2],
    stands: Mesh,
    focus: Focus,
    /// 0..1, how far the word over a dead ball has arrived. The client winds
    /// this up over half a second so the announcement does not cut onto the
    /// frame the play ended; recordings and replays leave it at 1.
    announce: f32,
    /// The lens the player tuned on the settings screen. 1.0 / 0.0 is the shot
    /// the game shipped with, so a fresh settings file changes nothing.
    lens_zoom: f32,
    lens_tilt: f32,
    /// How far the route-tree preview has eased out of the live shot, 0..1.
    preview_ease: f32,
    /// A spot the picture is pinned to instead of the ball: the flag on a
    /// reviewed foul.
    pin: Option<Vec2>,
    /// The crowd, one man at a time, so it can move.
    crowd: Vec<CrowdFigure>,
    crowd_t: f32,
}

impl Broadcast {
    pub fn new(w: usize, h: usize, teams: [Team; 2]) -> Self {
        let field = fieldtex::build_field_texture(&teams[0], &teams[1]);
        let uniforms = [teams[0].home, teams[1].away];
        let mut scene = SceneRenderer::new(w, h, 46f32.to_radians());
        scene.lighting = stadium_lighting();
        let stands = build_stadium(&teams);
        let crowd = build_crowd(&teams);
        Broadcast {
            w,
            h,
            field,
            scene,
            teams,
            uniforms,
            stands,
            focus: Focus::default(),
            announce: 1.0,
            lens_zoom: 1.0,
            lens_tilt: 0.0,
            preview_ease: 0.0,
            pin: None,
            crowd,
            crowd_t: 0.0,
        }
    }

    /// How far the word over a dead ball has arrived, 0..1. The live client
    /// walks this up from zero when the whistle goes, so the result rises onto
    /// the picture instead of appearing on it.
    pub fn set_announce(&mut self, ease: f32) {
        self.announce = ease.clamp(0.0, 1.0);
    }

    /// The lens the game is using right now, so a menu can move it.
    pub fn lens(&self) -> (f32, f32) {
        (self.lens_zoom, self.lens_tilt)
    }

    /// Pin the picture to one spot rather than the ball -- the flag on a reel of a
    /// foul -- or let it go back to following the play. A pin is a cut, not an
    /// ease: the shot is already on the flag when the replay starts.
    pub fn pin_focus(&mut self, at: Option<Vec2>) {
        self.pin = at;
        self.focus = Focus::default();
    }

    /// The lens the player tuned on the settings screen. Zoom pulls the camera
    /// back from the action, tilt drops it so the picture looks further down.
    pub fn set_lens(&mut self, zoom: f32, tilt: f32) {
        self.lens_zoom = if zoom.is_finite() { zoom.clamp(0.6, 1.8) } else { 1.0 };
        self.lens_tilt = if tilt.is_finite() { tilt.clamp(-0.35, 0.45) } else { 0.0 };
    }

    /// Render one frame of a recorded play at time `t` (seconds after the snap).
    pub fn frame_at(
        &mut self,
        game: &GameState,
        record: &PlayRecord,
        t: f32,
        mode: CameraMode,
    ) -> Frame {
        self.frame_at_opt(game, record, t, mode, true)
    }

    /// The same frame with no broadcast overlay: the terminal client draws its
    /// own text HUD underneath the picture.
    pub fn frame_at_clean(
        &mut self,
        game: &GameState,
        record: &PlayRecord,
        t: f32,
        mode: CameraMode,
    ) -> Frame {
        self.frame_at_opt(game, record, t, mode, false)
    }

    fn frame_at_opt(
        &mut self,
        game: &GameState,
        record: &PlayRecord,
        t: f32,
        mode: CameraMode,
        overlay: bool,
    ) -> Frame {
        match record {
            PlayRecord::Scrimmage(r) => self.scrimmage_frame(game, r, t, mode, overlay),
            PlayRecord::ThrowOff(r) => self.throw_off_frame(game, r, t, mode, overlay),
        }
    }

    pub fn sequence(
        &mut self,
        game: &GameState,
        record: &PlayRecord,
        frames: usize,
        duration: f32,
        mode: CameraMode,
    ) -> Vec<Frame> {
        let mut out = Vec::with_capacity(frames);
        for i in 0..frames {
            let t = duration * (i as f32 / (frames.max(2) - 1) as f32);
            out.push(self.frame_at(game, record, t, mode));
        }
        out
    }

    fn scrimmage_frame(
        &mut self,
        game: &GameState,
        record: &ScrimmageRecord,
        t: f32,
        mode: CameraMode,
        overlay: bool,
    ) -> Frame {
        let off = record.setup.offense_team;
        let def = record.setup.defense_team;

        // --- simulate to time t, then copy out render state -------------------
        let (players, flying_ball, action, los, dir, ball_y) = {
            let mut sim = PlaySim::new(
                &self.teams[off],
                &self.teams[def],
                &record.play,
                &record.defense,
                &record.setup,
            );
            let dt = 1.0 / 60.0;
            let mut elapsed = 0.0;
            while elapsed < t && sim.result.is_none() && elapsed < 15.0 {
                sim.step(dt);
                elapsed += dt;
            }
            let carrier = sim.carrier;
            let motion = sim.carrier_move();
            let mut players = Vec::with_capacity(sim.players.len());
            for (i, p) in sim.players.iter().enumerate() {
                let mine = Some(i) == carrier;
                players.push(RenderPlayer {
                    team: if p.side == Side::Offense { off } else { def },
                    roster: p.player,
                    pos: p.pos,
                    heading: p.heading,
                    run_phase: p.run_phase,
                    speed: clamp(p.vel.length() / 8.5, 0.0, 1.0),
                    carrying: sim.ball.holder == Some(i),
                    blocking: p.engaged.is_some(),
                    down: p.down,
                wrap: p.wrap,
                tackled: p.tackled,
                    celebrate: p.celebrate,
                    motion: if mine { motion.kind() } else { None },
                    motion_progress: if mine { motion.progress() } else { 0.0 },
                });
            }
            let flying_ball = if sim.ball.flight.is_some() || sim.ball.holder.is_none() {
                let (bp, bz) = sim.ball_position();
                Some((bp, bz.max(0.2)))
            } else {
                None
            };
            (players, flying_ball, sim.ball_position().0, sim.los, record.setup.direction, record.setup.ball_y)
        };

        let camera = self.camera_for(mode, action, dir, los, ball_y);
        self.sync_lens(&camera);
        let mut frame = Frame::new(self.w, self.h);
        self.paint_environment(&mut frame, &camera);
        self.paint_lines(&mut frame, &camera, los, dir, record.to_go);
        self.draw_bodies(&mut frame, &camera, &players);

        if let Some((bp, bz)) = flying_ball {
            let ball = players::ball_mesh(Vec3::new(bp.x, bp.y, bz));
            self.scene.draw_mesh(&mut frame, &ball, &camera);
            self.paint_ball_marker(&mut frame, &camera, bp, bz);
        }

        if overlay {
            let banner = self.banner_for_scrimmage(record);
            let caption = Some(record.result.description.clone());
            let hud_state = self.hud_state_for(game, record);
            hud::draw(&mut frame, &hud_state, banner.as_ref(), caption.as_deref());
        }
        frame
    }

    /// The overlay must show the state as it was during *this* play, not the final
    /// score of the game we simulated to find it.
    fn hud_state_for<'g>(&self, game: &'g GameState, record: &ScrimmageRecord) -> hud::HudState<'g> {
        let mut state = hud::HudState::from_game(game);
        state.quarter = record.quarter;
        state.clock = record.clock;
        state.possession = record.setup.offense_team;
        state.score = record.score_before;
        state.play_clock = 30.0;
        state.is_overtime = record.quarter > 4;
        state.under_two = (record.quarter == 2 || record.quarter == 4) && record.clock <= 120.0;
        if record.is_try {
            state.down = 1;
            state.to_go = a7fl_core::field::yards_to_goal(record.setup.los, record.setup.direction);
            state.yards_to_goal = state.to_go;
            state.phase = GamePhase::Conversion { offense: record.setup.offense_team, points: 1 };
        } else {
            state.down = record.down;
            state.to_go = record.to_go;
            state.yards_to_goal = a7fl_core::field::yards_to_goal(record.setup.los, record.setup.direction);
            state.phase = GamePhase::Scrimmage;
        }
        if record.result.touchdown {
            state.score[record.setup.offense_team] = record.score_after[record.setup.offense_team];
        }
        state
    }

    fn throw_off_frame(
        &mut self,
        game: &GameState,
        record: &ThrowOffRecord,
        t: f32,
        mode: CameraMode,
        overlay: bool,
    ) -> Frame {
        let throwing = record.setup.throwing_team;
        let receiving = record.setup.receiving_team;

        let (players, ball, camera) = {
            let mut sim = ThrowOffSim::new(&record.setup, &self.teams[throwing], &self.teams[receiving]);
            let dt = 1.0 / 60.0;
            let mut elapsed = 0.0;
            while elapsed < t && sim.result.is_none() && elapsed < 22.0 {
                sim.step(dt);
                elapsed += dt;
            }
            let (players, ball) = Broadcast::throw_off_players(&sim);
            // A replay has no eased lens: the shot sits on the ball itself.
            let camera = throw_off_camera(
                &sim,
                sim.ball_position().0,
                mode,
                (self.lens_zoom, self.lens_tilt),
            );
            (players, ball, camera)
        };
        let mut frame = self.paint_throw_off(&players, ball, camera);

        if overlay {
            let banner = if record.result.touchdown {
                Some(Banner {
                    title: "TOUCHDOWN".to_string(),
                    subtitle: format!("{} RETURN", self.teams[record.result.next_offense].abbr),
                    color: self.teams[record.result.next_offense].primary,
                })
            } else if record.result.safety {
                Some(Banner {
                    title: "SAFETY".to_string(),
                    subtitle: "TWO POINTS".to_string(),
                    color: hud::RED,
                })
            } else {
                None
            };
            let caption = Some(record.result.description.clone());
            let mut hud_state = hud::HudState::from_game(game);
            hud_state.quarter = record.quarter;
            hud_state.clock = record.clock;
            hud_state.score = record.score_before;
            hud_state.possession = record.result.next_offense;
            hud_state.play_clock = 30.0;
            hud_state.down = 1;
            hud_state.to_go = 10.0;
            // Next snap comes from the return spot, so show that spot on the card.
            hud_state.yards_to_goal =
                field::yards_to_goal(record.result.start_spot, record.setup.receive_direction);
            hud_state.is_overtime = record.quarter > 4;
            hud_state.under_two =
                (record.quarter == 2 || record.quarter == 4) && record.clock <= 120.0;
            hud_state.phase = GamePhase::ThrowOff {
                receiving: record.result.next_offense,
                from_own: record.setup.from_own_yards,
            };
            hud::draw(&mut frame, &hud_state, banner.as_ref(), caption.as_deref());
        }
        frame
    }

    /// Where every man is and where the ball is, from a throw-off being played or
    /// replayed.
    pub fn throw_off_players(sim: &ThrowOffSim) -> (Vec<RenderPlayer>, (a7fl_core::math::Vec2, f32)) {
        let ball = sim.ball_position();
        let players = sim
            .players
            .iter()
            // The thrower is not a coverage man. The rules send three gunners
            // downfield against one returner, and leaving the thrower standing
            // there in the same colours made the throw-off read as four men
            // chasing one. He has done his job when the ball leaves his hand.
            .filter(|p| p.role != ThrowRole::Thrower || sim.phase == ThrowPhase::Presnap)
            .map(|p| RenderPlayer {
                team: p.team,
                roster: p.roster,
                pos: p.pos,
                heading: p.heading,
                run_phase: p.run_phase,
                speed: clamp(p.vel.length() / 8.5, 0.0, 1.0),
                carrying: p.role == ThrowRole::Returner
                    && ball.0.dist(p.pos) < 0.8
                    && ball.1 < 1.3,
                blocking: false,
                down: p.down,
                wrap: p.wrap,
                tackled: p.tackled,
                celebrate: 0.0,
                // A returner is not moved to a dive or a spin yet: the throw-off
                // has its own state machine and no move set of its own.
                motion: None,
                motion_progress: 0.0,
            })
            .collect();
        (players, ball)
    }

    /// The called play on the field: each eligible man's route as the sim has it
    /// built, drawn on the turf from where he is standing to where it ends.
    fn paint_routes(&self, frame: &mut Frame, camera: &Camera, sim: &PlaySim, emphasis: bool) {
        const ROUTE: a7fl_core::math::Rgb = a7fl_core::math::Rgb::hex(0xEAF3FF);
        const END: a7fl_core::math::Rgb = a7fl_core::math::Rgb::hex(0xFFD84A);
        for i in 0..7 {
            let p = &sim.players[i];
            if p.route.is_empty() {
                continue;
            }
            let mut last = p.pos;
            for &pt in &p.route {
                let a = Vec3::new(last.x, last.y, 0.04);
                let b = Vec3::new(pt.x, pt.y, 0.04);
                if let (Some((x0, y0, _)), Some((x1, y1, _))) = (
                    self.scene.projector.project(camera.view(a)),
                    self.scene.projector.project(camera.view(b)),
                ) {
                    let weight = if emphasis { 2 } else { 1 };
                    frame.line(x0 as i32, y0 as i32, x1 as i32, y1 as i32, ROUTE, weight);
                    if emphasis {
                        // A halo under the brighter line, so the route still reads
                        // where the turf is bright.
                        frame.line(x0 as i32 + 1, y0 as i32, x1 as i32 + 1, y1 as i32, END, 1);
                    }
                }
                last = pt;
            }
            // A bar across the end of the route, so its depth can be read.
            if let Some((x, y, _)) = self
                .scene
                .projector
                .project(camera.view(Vec3::new(last.x, last.y, 0.04)))
            {
                let (w, h) = if emphasis { (10, 5) } else { (8, 3) };
                frame.fill_rect_alpha(x as i32 - w / 2, y as i32 - h / 2, w, h, END, 0.95);
            }
        }
    }

    /// The ball has no shadow once it is above head height, and a throw-off
    /// sends it forty yards up. A ring on the turf under it is the only way to
    /// read where it is coming down, so it is drawn for every ball in flight --
    /// scrimmage, throw-off, live play and replay alike -- rather than only on
    /// the throw-off.
    fn paint_ball_marker(&self, frame: &mut Frame, camera: &Camera, at: Vec2, z: f32) {
        // At rest on the tee or in a carrier's hands there is nothing to track.
        if z < 1.15 {
            return;
        }
        self.scene
            .draw_ground_ring(frame, camera, Vec3::new(at.x, at.y, 0.0), 0.62, hud::YELLOW);
    }

    fn paint_throw_off(
        &mut self,
        players: &[RenderPlayer],
        ball: (a7fl_core::math::Vec2, f32),
        camera: Camera,
    ) -> Frame {
        self.sync_lens(&camera);
        let mut frame = Frame::new(self.w, self.h);
        self.paint_environment(&mut frame, &camera);
        self.draw_bodies(&mut frame, &camera, players);
        if ball.1 > 1.3 {
            let mesh = players::ball_mesh(Vec3::new(ball.0.x, ball.0.y, ball.1));
            self.scene.draw_mesh(&mut frame, &mesh, &camera);
        }
        self.paint_ball_marker(&mut frame, &camera, ball.0, ball.1);
        frame
    }

    /// A throw-off as it is being played, with the client's meter and landing
    /// marker drawn through the same camera as the field.
    pub fn throw_off_frame_live(
        &mut self,
        sim: &ThrowOffSim,
        mode: CameraMode,
        ui: &crate::passing::PassUi,
    ) -> Frame {
        let (players, ball) = Broadcast::throw_off_players(sim);
        if sim.phase == ThrowPhase::Presnap {
            // A throw-off gets its own cut rather than a swoop in from wherever the
            // last play left the lens.
            self.focus = Focus::default();
        }
        let focus = self.focus.track_with(ball.0, FOLLOW_AIR);
        let camera = throw_off_camera(sim, focus, mode, (self.lens_zoom, self.lens_tilt));
        let mut frame = self.paint_throw_off(&players, ball, camera.clone());
        ui.draw(&mut frame, &camera, &self.scene.projector);
        frame
    }

    /// Render the live simulation an interactive client is driving. No overlay:
    /// the terminal client draws its own text HUD underneath the picture.
    pub fn live_frame(&mut self, sim: &PlaySim, mode: CameraMode) -> Frame {
        let (frame, _camera) = self.live_scene(sim, mode);
        frame
    }

    /// The live picture plus the passing overlay, projected through the same
    /// camera the scene used so the badges land on the players' heads.
    pub fn live_frame_ui(
        &mut self,
        sim: &PlaySim,
        mode: CameraMode,
        ui: &crate::passing::PassUi,
    ) -> Frame {
        let (mut frame, camera) = self.live_scene(sim, mode);
        ui.draw(&mut frame, &camera, &self.scene.projector);
        frame
    }

    fn live_scene(&mut self, sim: &PlaySim, mode: CameraMode) -> (Frame, Camera) {
        let off = sim.setup.offense_team;
        let def = sim.setup.defense_team;
        let carrier = sim.carrier;
        let motion = sim.carrier_move();
        let mut players = Vec::with_capacity(sim.players.len());
        for (i, p) in sim.players.iter().enumerate() {
            let mine = Some(i) == carrier;
            players.push(RenderPlayer {
                team: if p.side == Side::Offense { off } else { def },
                roster: p.player,
                pos: p.pos,
                heading: p.heading,
                run_phase: p.run_phase,
                speed: clamp(p.vel.length() / 8.5, 0.0, 1.0),
                carrying: sim.ball.holder == Some(i),
                blocking: p.engaged.is_some(),
                down: p.down,
                wrap: p.wrap,
                tackled: p.tackled,
                celebrate: p.celebrate,
                motion: if mine { motion.kind() } else { None },
                motion_progress: if mine { motion.progress() } else { 0.0 },
            });
        }

        let (ball_pos, ball_z) = sim.ball_position();
        // The ball is the anchor: the picture goes where it goes, eased -- and once
        // it is in the air, a little ahead of it, on the way to where it is going.
        let (anchor, rate) = match &sim.ball.flight {
            Some(flight) => (ball_pos + (flight.to - ball_pos) * AIR_LEAD, FOLLOW_AIR),
            None => (ball_pos, FOLLOW),
        };
        // A pinned shot holds the flag, not the ball: on a reviewed foul the foul
        // is the story, wherever the play itself ended up.
        let focus = match self.pin {
            Some(at) => at,
            None => self.focus.track_with(anchor, rate),
        };
        // An interception turns the play round: the picture has to face the way
        // the man with the ball is running, not the way the ball was snapped.
        let dir = sim.facing();
        // Holding the preview at the line lifts the shot over the whole route
        // tree instead of the sliver of it a runner's camera holds.
        // The preview is a zoom out of the shot the player is already watching,
        // not a cut to another camera: the eye keeps its bearings while the route
        // tree opens up in front of it, and it eases back the moment LT is let go.
        let want = if sim.previewing() { 1.0 } else { 0.0 };
        self.preview_ease += (want - self.preview_ease) * 0.14;
        if (self.preview_ease - want).abs() < 0.002 {
            self.preview_ease = want;
        }
        let camera = match self.pin {
            // A pinned shot is cut on the flag and holds it: no ball tracking, no
            // preview blend, just the spot the foul was called on.
            Some(at) => self.flag_camera(at, dir),
            None => {
                let shot = self.camera_for(mode, focus, dir, sim.los, sim.setup.ball_y);
                let wide =
                    self.camera_for(CameraMode::SkyCam, focus, dir, sim.los, sim.setup.ball_y);
                blend_camera(&shot, &wide, smooth(self.preview_ease))
            }
        };
        self.sync_lens(&camera);
        let mut frame = Frame::new(self.w, self.h);
        self.paint_environment(&mut frame, &camera);
        self.paint_lines(&mut frame, &camera, sim.los, dir, sim.setup.to_go);
        // The call is only on the grass while the preview is held. Drawn all the
        // time it was a call sheet lying open for whoever is on the other side.
        if sim.previewing() {
            self.paint_routes(&mut frame, &camera, sim, true);
        }
        self.draw_bodies(&mut frame, &camera, &players);
        if let Some(flag) = &sim.flag {
            self.paint_flag(&mut frame, &camera, flag, sim.t - sim.flag_at);
        }
        if sim.ball.flight.is_some() || sim.ball.holder.is_none() {
            let ball = players::ball_mesh(Vec3::new(ball_pos.x, ball_pos.y, ball_z.max(0.2)));
            self.scene.draw_mesh(&mut frame, &ball, &camera);
            self.paint_ball_marker(&mut frame, &camera, ball_pos, ball_z);
        }
        if let Some(result) = &sim.result {
            self.paint_result_word(&mut frame, result);
        }
        (frame, camera)
    }

    /// The flag itself, on the turf where the foul happened, plus a caption on the
    /// picture. A live-ball foul does not stop the play, so all the picture has to do
    /// is say that one is on the ground and let the snap run.
    fn paint_flag(
        &mut self,
        frame: &mut Frame,
        camera: &Camera,
        flag: &a7fl_core::rules::PenaltyCall,
        since: f32,
    ) {
        let pulse = 0.55 + 0.45 * (since * 7.0).sin().abs();
        let spot = Vec3::new(flag.spot_of_foul, 0.0, 1.0);
        if let Some((x, y, _)) = self.scene.projector.project(camera.view(spot)) {
            let (x, y) = (x.round() as i32, y.round() as i32);
            // A pennant on a stick, standing where the foul happened.
            frame.line(x, y, x, y - 18, hud::WHITE, 1);
            for row in 0..9 {
                let w = 9 - (row / 3) * 2;
                frame.fill_rect_alpha(x + 1, y - 18 + row, w, 1, hud::YELLOW, pulse);
            }
            frame.fill_rect_alpha(x - 8, y - 2, 17, 3, hud::INK, 0.5);
        }

        let label = format!("FLAG {}", flag.kind.label().to_uppercase());
        let tw = crate::font::text_width(&label, 2) as i32;
        let th = crate::font::text_height(2) as i32;
        let x = frame.w as i32 - tw - 14;
        frame.fill_rect_alpha(x - 8, 8, tw + 16, th + 8, hud::INK, 0.62);
        frame.text_shadowed(x, 12, &label, 2, hud::YELLOW, 1);
    }

    /// The word over a dead ball: SACK, TACKLE FOR LOSS, INTERCEPTION, PASS BATTED
    /// DOWN. The card explains it a beat later; this is what the picture says while
    /// the two men who just hit each other are still on the ground.
    fn paint_result_word(&self, frame: &mut Frame, result: &a7fl_core::sim::PlayResult) {
        let Some(word) = result.headline() else {
            return;
        };
        let ease = self.announce.clamp(0.0, 1.0);
        if ease <= 0.001 {
            return;
        }
        let scale = 3;
        let tw = crate::font::text_width(word, scale) as i32;
        let th = crate::font::text_height(scale) as i32;
        let x = (frame.w as i32 - tw) / 2;
        // It settles onto its line rather than being stamped there, and the
        // colour comes up out of the panel before it reads as white type.
        let y = 20 + ((1.0 - ease) * 12.0) as i32;
        let color = if result.turnover {
            hud::YELLOW
        } else {
            hud::WHITE
        };
        let color = hud::INK.lerp(color, ease);
        frame.fill_rect_alpha(x - 14, y - 8, tw + 28, th + 16, hud::INK, 0.74 * ease);
        frame.text_shadowed(x, y, word, scale, color, 1);

        // The sentence lands after the word, not with it.
        let sub_ease = ((ease - 0.55) / 0.45).clamp(0.0, 1.0);
        if sub_ease > 0.05 {
            let lower = result.description.to_uppercase();
            let sub: String = lower.chars().take(58).collect();
            let sw = crate::font::text_width(&sub, 1) as i32;
            let sx = (frame.w as i32 - sw) / 2;
            frame.fill_rect_alpha(sx - 8, y + th + 12, sw + 16, 18, hud::INK, 0.60 * sub_ease);
            frame.text_shadowed(
                sx,
                y + th + 17,
                &sub,
                1,
                hud::WHITE.lerp(hud::INK, 1.0 - sub_ease),
                1,
            );
        }
    }

    fn draw_bodies(&mut self, frame: &mut Frame, camera: &Camera, bodies: &[RenderPlayer]) {
        // Shadows first so every player casts one.
        for p in bodies {
            let radius = if p.down { 0.72 } else { 0.5 };
            self.scene
                .draw_shadow(frame, camera, Vec3::new(p.pos.x, p.pos.y, 0.0), radius, 0.42);
        }
        let mut close_builds = 0;
        for p in bodies {
            let roster = &self.teams[p.team].roster[p.roster];
            let uniform = &self.uniforms[p.team];
            let pose = Pose {
                run_phase: p.run_phase,
                speed: p.speed,
                carrying: p.carrying,
                blocking: p.blocking,
                down: p.down,
                wrapping: p.wrap,
                tackled: p.tackled,
                celebrating: p.celebrate,
                strafe: 0.0,
                motion: p.motion,
                motion_progress: p.motion_progress,
            };
            // A man a long way from the lens is built cheaply: at that size the
            // eyes and the quadriceps are less than a pixel each.
            let dist = (camera.pos - Vec3::new(p.pos.x, p.pos.y, 1.0)).length();
            let mut detail = players::Detail::for_distance(dist);
            // The modelled face is worth two men. A pile-up would otherwise ask
            // for six of them at once and spend the frame on the far hash.
            if detail == players::Detail::Close {
                if close_builds >= 2 {
                    detail = players::Detail::Full;
                } else {
                    close_builds += 1;
                }
            }
            let mut mesh = players::build_mesh_at(roster, uniform, &pose, p.roster as u32, detail);
            mesh.place(Vec3::new(p.pos.x, p.pos.y, 0.0), p.heading, 1.0);
            self.scene.draw_mesh(frame, &mesh, camera);
        }
    }

    /// The crowd for this frame: everybody bobbing on his own phase, with a few
    /// arms up. Two quads a man, doubled over so the far side of the ground is
    /// not invisible to a backface cull.
    fn crowd_frame(&mut self) -> Mesh {
        let t = self.crowd_t;
        self.crowd_t += 1.0 / 60.0;
        let mut mesh = Mesh::new();
        mesh.tris.reserve(self.crowd.len() * 8);
        for f in &self.crowd {
            let bob = (t * 1.7 + f.phase).sin() * 0.05;
            let cheer = (t * 2.4 + f.phase).sin().max(0.0);
            let lift = if f.arms { cheer * 0.18 } else { 0.0 };
            let z = f.at.z + bob + lift;
            let (x, y) = (f.at.x, f.at.y);
            let w = 0.20;
            let body = f.color;
            mesh.quad(
                Vec3::new(x - w, y, z),
                Vec3::new(x + w, y, z),
                Vec3::new(x + w, y, z + f.tall),
                Vec3::new(x - w, y, z + f.tall),
                body,
            );
            mesh.quad(
                Vec3::new(x - w, y, z + f.tall),
                Vec3::new(x + w, y, z + f.tall),
                Vec3::new(x + w, y, z),
                Vec3::new(x - w, y, z),
                body,
            );
            let head = body.scale(1.18);
            mesh.quad(
                Vec3::new(x - 0.10, y, z + f.tall),
                Vec3::new(x + 0.10, y, z + f.tall),
                Vec3::new(x + 0.10, y, z + f.tall + 0.17),
                Vec3::new(x - 0.10, y, z + f.tall + 0.17),
                head,
            );
            mesh.quad(
                Vec3::new(x + 0.10, y, z + f.tall + 0.17),
                Vec3::new(x - 0.10, y, z + f.tall + 0.17),
                Vec3::new(x - 0.10, y, z + f.tall),
                Vec3::new(x + 0.10, y, z + f.tall),
                head,
            );
            if f.arms {
                let up = cheer * 0.30;
                let arm = body.scale(0.9);
                for s in [-1.0f32, 1.0] {
                    let ax = x + s * (w + 0.03);
                    let top = z + f.tall + 0.10 + up;
                    mesh.quad(
                        Vec3::new(ax, y, z + f.tall * 0.55),
                        Vec3::new(ax - s * 0.04, y, z + f.tall * 0.55),
                        Vec3::new(ax - s * 0.04, y, top),
                        Vec3::new(ax, y, top),
                        arm,
                    );
                    mesh.quad(
                        Vec3::new(ax, y, top),
                        Vec3::new(ax - s * 0.04, y, top),
                        Vec3::new(ax - s * 0.04, y, z + f.tall * 0.55),
                        Vec3::new(ax, y, z + f.tall * 0.55),
                        arm,
                    );
                }
            }
        }
        mesh
    }

    fn banner_for_scrimmage(&self, record: &ScrimmageRecord) -> Option<Banner> {
        let off = record.setup.offense_team;
        if record.result.touchdown {
            return Some(Banner {
                title: "TOUCHDOWN".to_string(),
                subtitle: format!("{} · {}", self.teams[off].abbr, record.result.description)
                    .to_uppercase(),
                color: self.teams[off].primary,
            });
        }
        if record.result.safety {
            let def = record.setup.defense_team;
            return Some(Banner {
                title: "SAFETY".to_string(),
                subtitle: format!("Two points for the {}", self.teams[def].name),
                color: hud::RED,
            });
        }
        if record.result.turnover {
            let def = record.setup.defense_team;
            return Some(Banner {
                title: "TURNOVER".to_string(),
                subtitle: format!("{} takes over", self.teams[def].full_name()),
                color: self.teams[def].primary,
            });
        }
        if let Some(pen) = &record.result.penalty {
            return Some(Banner {
                title: "FLAG".to_string(),
                subtitle: pen.kind.label().to_string(),
                color: hud::YELLOW,
            });
        }
        if record.result.sack {
            let def = record.setup.defense_team;
            return Some(Banner {
                title: "SACK".to_string(),
                subtitle: self.teams[def].full_name().to_uppercase(),
                color: self.teams[def].primary,
            });
        }
        if record.result.batted_down {
            let def = record.setup.defense_team;
            return Some(Banner {
                title: "PASS BATTED DOWN".to_string(),
                subtitle: record.result.description.to_uppercase(),
                color: self.teams[def].primary,
            });
        }
        if !record.result.incomplete && record.result.yards < -0.5 {
            let def = record.setup.defense_team;
            return Some(Banner {
                title: "TACKLE FOR LOSS".to_string(),
                subtitle: record.result.description.to_uppercase(),
                color: self.teams[def].primary,
            });
        }
        None
    }

    fn paint_environment(&mut self, frame: &mut Frame, camera: &Camera) {
        // Sky gradient: night game under the lights, with the sodium glow of the
        // city sitting on the horizon behind the far stand.
        let top = Rgb::hex(0x0A1428);
        let mid = Rgb::hex(0x20364E);
        let horizon = Rgb::hex(0x7E8F96);
        let h = frame.h.max(1) as f32;
        for row in 0..frame.h as i32 {
            let t = row as f32 / h;
            let c = if t < 0.55 {
                top.lerp(mid, t / 0.55)
            } else {
                mid.lerp(horizon, (t - 0.55) / 0.45)
            };
            frame.fill_rect(0, row, frame.w as i32, 1, c);
        }

        let (x0, x1, y0, y1) = fieldtex::coverage();
        self.scene.draw_ground(frame, &self.field, camera, x0, x1, y0, y1, 2.2);
        self.scene.draw_mesh(frame, &self.stands, camera);
        // The crowd moves: bodies bob on their own phases and a few put their
        // arms up. It is rebuilt every frame because placing a mesh bakes it.
        let crowd = self.crowd_frame();
        self.scene.draw_mesh(frame, &crowd, camera);
    }

    /// Yellow line of scrimmage and blue first-down marker, painted on the turf.
    fn paint_lines(&mut self, frame: &mut Frame, camera: &Camera, los: f32, direction: i8, to_go: f32) {
        let first_down = los + direction as f32 * to_go;
        for (x, color) in [(los, Rgb::hex(0xF2C14E)), (first_down, Rgb::hex(0x4EA8F2))] {
            if x < -field::ENDZONE_DEPTH || x > FIELD_LENGTH + field::ENDZONE_DEPTH {
                continue;
            }
            let half_w = 0.14;
            let y0 = -FIELD_HALF_WIDTH;
            let y1 = FIELD_HALF_WIDTH;
            let corners = [
                Vec3::new(x - half_w, y0, 0.05),
                Vec3::new(x - half_w, y1, 0.05),
                Vec3::new(x + half_w, y1, 0.05),
                Vec3::new(x + half_w, y0, 0.05),
            ];
            let mut pts = Vec::new();
            let mut ok = true;
            for c in corners {
                match self.scene.projector.project(camera.view(c)) {
                    Some(p) => pts.push(p),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok && pts.len() == 4 {
                for i in 1..3 {
                    frame.flat_tri_alpha([pts[0], pts[i], pts[i + 1]], color, 0.55);
                }
            }
        }
    }

    /// The shot on a flag: tight, low and side-on, with the pennant in the middle
    /// of it. The replay camera leads the runner by three yards, which on a foul
    /// would put the flag off to one side of a picture about something else.
    fn flag_camera(&self, at: Vec2, direction: i8) -> Camera {
        let d = direction as f32;
        let pos = Vec3::new(at.x - d * 6.0, at.y + 9.0, 4.0);
        let look = Vec3::new(at.x, at.y, 0.9);
        let (pos, look) = lens_shift(pos, look, at, self.lens_zoom, self.lens_tilt);
        let mut cam = Camera::new(pos, 0.0, 0.0);
        cam.look_at(look);
        cam.fov = 34f32.to_radians();
        cam
    }

    /// `action` is the point the camera is anchored on: the ball, eased, for a
    /// live snap. Every mode places itself relative to it, so no shot can lose
    /// track of where the ball is.
    fn camera_for(
        &self,
        mode: CameraMode,
        action: Vec2,
        direction: i8,
        los: f32,
        ball_y: f32,
    ) -> Camera {
        let d = direction as f32;
        let (pos, look, fov) = match mode {
            CameraMode::Broadcast => (
                Vec3::new(action.x - d * 3.0, 32.0, 17.5),
                Vec3::new(action.x + d * 5.0, action.y * 0.62, 0.8),
                30f32,
            ),
            // Behind the ball and looking past it: the old version was pinned to
            // the line of scrimmage, so the further the runner got the smaller he
            // was in the corner of a shot still framing the snap.
            // Tilted down a touch more than it used to be: the running back
            // lines up behind the ball, and the old lens had his feet under the
            // bottom of the frame.
            CameraMode::Classic => (
                Vec3::new(action.x - d * 11.0, action.y * 0.5, 11.2),
                Vec3::new(action.x + d * 8.0, action.y * 0.8, 0.1),
                42f32,
            ),
            CameraMode::BehindQb => (
                Vec3::new(los - d * 15.0, ball_y + 0.8, 8.4),
                Vec3::new(los + d * 13.0, ball_y, 1.3),
                44f32,
            ),
            // Straight up the field: the lens sits behind the ball, in the ball's
            // own lane, and looks down it. It used to stand five yards off that
            // lane and aim back across at the ball, which is why every sky shot --
            // and the pre-snap preview, which eases into this one -- was tilted.
            CameraMode::SkyCam => (
                Vec3::new(action.x - d * 17.0, action.y, 34.0),
                Vec3::new(action.x + d * 19.0, action.y, 0.0),
                50f32,
            ),
            CameraMode::Replay => (
                Vec3::new(action.x - d * 7.0, action.y + 9.0, 3.0),
                Vec3::new(action.x + d * 3.0, action.y, 1.0),
                34f32,
            ),
            CameraMode::Endzone => (
                Vec3::new(field::opponent_goal_side(direction) - d * 12.0, 6.0, 7.5),
                Vec3::new(action.x, action.y, 0.6),
                38f32,
            ),
        };
        // The tuned lens sits on top of every shot, so a zoom the player likes
        // carries across the camera modes instead of having to be set three times.
        let (pos, look) = lens_shift(pos, look, action, self.lens_zoom, self.lens_tilt);
        let mut cam = Camera::new(pos, 0.0, 0.0);
        cam.look_at(look);
        cam.fov = fov.to_radians();
        cam
    }

    /// Rebuild the projector for the camera's field of view. The lights belong to
    /// the building, not to the lens, so they survive every cut.
    pub fn sync_lens(&mut self, camera: &Camera) {
        let lighting = self.scene.lighting;
        self.scene = SceneRenderer::new(self.w, self.h, camera.fov);
        self.scene.lighting = lighting;
    }
}

/// Which role label to draw on a pre-snap card.
pub fn role_label(role: OffRole) -> &'static str {
    role.label()
}

/// The lights of a night game: a warm key from above the far stand, a green
/// bounce off the turf, and enough edge light to draw the silhouette of a man
/// against the crowd.
/// The player's lens, laid over any shot: the picture pulls back from the action
/// by `zoom`, then tips by `tilt` so it looks further down the field. Every camera
/// goes through it -- the scrimmage shots, the throw-off, the replay -- so one
/// setting is the whole game's lens rather than a per-camera one.
fn lens_shift(pos: Vec3, look: Vec3, anchor: Vec2, zoom: f32, tilt: f32) -> (Vec3, Vec3) {
    let a = Vec3::new(anchor.x, anchor.y, 0.0);
    let pos = a + (pos - a) * zoom;
    let pos = Vec3::new(pos.x, pos.y, pos.z * (1.0 + tilt * 1.4));
    let look = Vec3::new(look.x, look.y, look.z + tilt * 2.2);
    (pos, look)
}

fn stadium_lighting() -> Lighting {
    Lighting {
        sun: Vec3::new(-0.30, 0.26, 0.92).normalize_or_zero(),
        ambient: 0.46,
        fog: Rgb::hex(0x9DB2C4),
        fog_density: 0.008,
        bounce: Vec3::new(0.05, 0.0, -1.0).normalize_or_zero(),
        bounce_strength: 0.13,
        rim: 0.20,
    }
}

/// Paint lettering onto a flat surface of the stadium: the scoreboard and the
/// fascia read as a real building rather than a grey slab if they carry a word.
fn paint_word(
    mesh: &mut Mesh,
    text: &str,
    origin: Vec3,
    right: Vec3,
    up: Vec3,
    cell: f32,
    color: Rgb,
) {
    let mut cursor = 0.0f32;
    for ch in text.chars() {
        if ch == ' ' {
            cursor += 4.0;
            continue;
        }
        let rows = crate::font::glyph(ch);
        for (row, bits) in rows.iter().enumerate() {
            for col in 0..5u8 {
                if bits & (1 << (4 - col)) == 0 {
                    continue;
                }
                let p = origin
                    + right * ((cursor + col as f32) * cell)
                    + up * ((6.0 - row as f32) * cell);
                mesh.quad_shaded(
                    p,
                    p + right * (cell * 0.92),
                    p + right * (cell * 0.92) + up * (cell * 0.92),
                    p + up * (cell * 0.92),
                    color,
                    1.5,
                );
            }
        }
        cursor += 6.0;
    }
}

/// The building around the field: a run-off apron, advertising boards, a bowl of
/// seating with a crowd in it, a stand behind each end zone, a roof over the far
/// side, a scoreboard and four banks of lights.
///
/// Heights are kept well under the broadcast lens, which sits eight yards above
/// the top row: the building has to be in the picture without ever getting
/// between the camera and the ball.
fn build_stadium(teams: &[Team; 2]) -> Mesh {
    let mut mesh = Mesh::new();
    let mut seed = 0x1F35_1A7Bu32;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed >> 8) as f32 / 16_777_215.0
    };

    let crowd_colors = [
        teams[0].primary,
        teams[1].primary,
        Rgb::hex(0xD8DDE4),
        Rgb::hex(0x49566A),
        Rgb::hex(0x27303C),
        Rgb::hex(0x8C4B3B),
    ];
    let concrete = Rgb::hex(0x3A4450);
    let wall = Rgb::hex(0x252D38);
    let steel = Rgb::hex(0x1B222C);
    let x0 = -14.0f32;
    let x1 = 114.0f32;

    // Run-off apron: the strip of ground between the paint and the wall.
    for side in [-1.0f32, 1.0] {
        mesh.quad(
            Vec3::new(x0, side * FIELD_HALF_WIDTH, 0.004),
            Vec3::new(x1, side * FIELD_HALF_WIDTH, 0.004),
            Vec3::new(x1, side * (FIELD_HALF_WIDTH + 4.8), 0.004),
            Vec3::new(x0, side * (FIELD_HALF_WIDTH + 4.8), 0.004),
            Rgb::hex(0x35503C),
        );
    }

    // Advertising boards along both sidelines, with a white rail on top.
    let mut x = x0;
    let mut i = 0;
    while x < x1 {
        let color = match i % 3 {
            0 => teams[0].primary,
            1 => teams[1].primary,
            _ => Rgb::hex(0xE8EAE2),
        };
        for side in [-1.0f32, 1.0] {
            // On the bleacher line, not on the paint: one barrier at the foot of
            // the stand, and a run-off apron between it and the white sideline.
            let y = side * (FIELD_HALF_WIDTH + 4.8);
            mesh.quad(
                Vec3::new(x, y, 0.0),
                Vec3::new(x + 5.7, y, 0.0),
                Vec3::new(x + 5.7, y, 1.05),
                Vec3::new(x, y, 1.05),
                color,
            );
            mesh.quad(
                Vec3::new(x, y, 1.05),
                Vec3::new(x + 5.7, y, 1.05),
                Vec3::new(x + 5.7, y, 1.18),
                Vec3::new(x, y, 1.18),
                Rgb::hex(0xE8EAE2),
            );
        }
        x += 6.0;
        i += 1;
    }

    // ------------------------------------------------------------------
    // The bowl. One tier on each sideline: a concrete riser and a tread per
    // row, with a crowd standing on the tread. Runs of people are drawn as one
    // block each so a full house costs a couple of thousand triangles, not tens
    // of thousands.
    // ------------------------------------------------------------------
    let rows = 7;
    let tread = 1.30f32;
    let riser = 0.62f32;
    let mut z = 1.18f32;
    // Sits back from the boards: the old front row was close enough that a man
    // running out of bounds looked like he was standing in the crowd.
    let mut y_front = FIELD_HALF_WIDTH + 4.8;
    for row in 0..rows {
        let z_top = z + riser;
        let y_back = y_front + tread;
        let tone = concrete.scale(1.0 - row as f32 * 0.012);
        for side in [-1.0f32, 1.0] {
            // Riser, then the tread behind it.
            mesh.quad(
                Vec3::new(x0, side * y_front, z),
                Vec3::new(x1, side * y_front, z),
                Vec3::new(x1, side * y_front, z_top),
                Vec3::new(x0, side * y_front, z_top),
                tone,
            );
            mesh.quad(
                Vec3::new(x0, side * y_front, z_top),
                Vec3::new(x1, side * y_front, z_top),
                Vec3::new(x1, side * y_back, z_top),
                Vec3::new(x0, side * y_back, z_top),
                tone.scale(0.92),
            );
            // The crowd is drawn by the broadcast, one body at a time, so it can
            // move; the stand itself is just concrete here.
        }
        z = z_top;
        y_front = y_back;
    }

    // Back wall and, over the far side only, a roof with a coloured fascia: the
    // near side stays open so the camera can see over it.
    for side in [-1.0f32, 1.0] {
        let y = side * (y_front + 0.3);
        mesh.quad(
            Vec3::new(x0, y, 0.0),
            Vec3::new(x1, y, 0.0),
            Vec3::new(x1, y, z + 1.1),
            Vec3::new(x0, y, z + 1.1),
            wall,
        );
    }
    let y_roof_back = -(y_front + 0.35);
    let y_roof_front = -(FIELD_HALF_WIDTH + 5.6);
    mesh.quad(
        Vec3::new(x0, y_roof_back, z + 1.9),
        Vec3::new(x1, y_roof_back, z + 1.9),
        Vec3::new(x1, y_roof_front, z + 2.2),
        Vec3::new(x0, y_roof_front, z + 2.2),
        steel,
    );
    mesh.quad(
        Vec3::new(x0, y_roof_front, z + 1.5),
        Vec3::new(x1, y_roof_front, z + 1.5),
        Vec3::new(x1, y_roof_front, z + 2.25),
        Vec3::new(x0, y_roof_front, z + 2.25),
        teams[0].primary,
    );

    // ------------------------------------------------------------------
    // End stands, so the end-zone and overhead shots are not looking at the
    // back of an empty field.
    // ------------------------------------------------------------------
    for end in [0, 1] {
        let dir = if end == 0 { -1.0f32 } else { 1.0 };
        let face = if end == 0 { x0 } else { x1 };
        let mut z = 0.7f32;
        let mut depth = if end == 0 { 3.0 } else { 3.0 };
        for row in 0..4 {
            let z_top = z + 0.50;
            let x_in = face - dir * depth;
            let x_out = x_in - dir * 1.1;
            let tone = concrete.scale(1.0 - row as f32 * 0.02);
            mesh.quad(
                Vec3::new(x_in, -26.0, z),
                Vec3::new(x_in, 26.0, z),
                Vec3::new(x_in, 26.0, z_top),
                Vec3::new(x_in, -26.0, z_top),
                tone,
            );
            mesh.quad(
                Vec3::new(x_in, -26.0, z_top),
                Vec3::new(x_in, 26.0, z_top),
                Vec3::new(x_out, 26.0, z_top),
                Vec3::new(x_out, -26.0, z_top),
                tone.scale(0.92),
            );
            let mut py = -26.0f32;
            while py < 26.0 {
                let len = 2.2 + rand() * 1.2;
                let color = crowd_colors[(rand() * crowd_colors.len() as f32) as usize
                    % crowd_colors.len()];
                let height = 0.50 + rand() * 0.14;
                let xc = x_in - dir * (0.28 + rand() * 0.2);
                mesh.quad(
                    Vec3::new(xc, py, z_top),
                    Vec3::new(xc, py + len, z_top),
                    Vec3::new(xc, py + len, z_top + height),
                    Vec3::new(xc, py, z_top + height),
                    color.scale(0.85 + rand() * 0.25),
                );
                py += len + 0.2;
            }
            z = z_top;
            depth += 1.1;
        }
    }

    // ------------------------------------------------------------------
    // Scoreboard behind the left end zone, with the league mark on it.
    // ------------------------------------------------------------------
    mesh.box_shaded(
        Vec3::new(-23.5, 0.0, 8.4),
        Vec3::new(0.45, 9.0, 3.8),
        0.0,
        steel,
        1.0,
    );
    // Stand the lettering a quarter of a yard proud of the panel: a face that is
    // only a hair in front of a board z-fights with it and the words smear.
    let board_face = -22.78f32;
    // Centre a word on the board rather than hanging it off one corner.
    let centred = |text: &str, cell: f32, z: f32, color: Rgb, mesh: &mut Mesh| {
        let cols = text.chars().count() as f32 * 6.0 - 1.0;
        let width = cols * cell;
        paint_word(
            mesh,
            text,
            Vec3::new(board_face, width * 0.5, z),
            // The face points +x, and in this lens screen-right for that face is
            // -y: letters run backwards if they are painted the other way.
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            cell,
            color,
        );
    };
    centred(&teams[0].abbr.to_uppercase(), 0.55, 8.8, Rgb::hex(0xE8EAE2), &mut mesh);
    centred("NO PADS", 0.26, 6.5, teams[0].accent, &mut mesh);

    // ------------------------------------------------------------------
    // Four banks of lights, out at the corners of the site.
    // ------------------------------------------------------------------
    for (px, py) in [(-18.0f32, -28.0f32), (-18.0, 28.0), (118.0, -28.0), (118.0, 28.0)] {
        let base = Vec3::new(px, py, 0.0);
        let top = Vec3::new(px, py, 23.0);
        mesh.cylinder(base, top, 0.22, 0.16, 6, steel);
        // Lamp bank facing the middle of the field.
        let to_field = Vec3::new(50.0 - px, -py, 0.0).normalize_or_zero();
        let side_dir = Vec3::new(-to_field.y, to_field.x, 0.0);
        for lr in 0..3 {
            for lc in 0..4 {
                let c = top
                    + Vec3::new(0.0, 0.0, 0.6 + lr as f32 * 0.55)
                    + side_dir * ((lc as f32 - 1.5) * 0.62);
                let hw = side_dir * 0.24;
                let hv = Vec3::new(0.0, 0.0, 0.22);
                mesh.quad_shaded(
                    c - hw - hv,
                    c + hw - hv,
                    c + hw + hv,
                    c - hw + hv,
                    Rgb::hex(0xFFF6D8),
                    1.9,
                );
            }
        }
    }

    mesh
}

/// The throw-off shot. It opens from the throwing team's end looking downfield at
/// the man waiting for the ball, then turns upfield once he has it — the way he is
/// about to run — and rides with him.
/// The throw-off shot, placed relative to the ball it is following.
///
/// The old shot sat fifteen and a half yards up, planted behind the thrower and
/// aimed at the spot the ball was going to land: for the whole flight you watched
/// a dot travel toward a dot, and the men doing anything about it were the small
/// end of the picture. `dir` is the way the camera should point -- downfield with
/// the throw, and behind the returner once he has it.
fn throw_off_lens(dir: f32, focus: Vec2, mode: CameraMode, lens: (f32, f32)) -> Camera {
    let (dist, height, lead, fov): (f32, f32, f32, f32) = match mode {
        // Low and close, behind the ball, like a sideline camera that gave chase.
        CameraMode::Classic => (7.5, 6.1, 6.0, 36.0),
        // Overhead: the one view where the whole return is readable.
        CameraMode::SkyCam => (4.0, 24.0, 3.0, 42.0),
        // The workhorse: a runner's height, close enough to see hands and feet,
        // tipped up enough that the men are not pinned to the bottom of the frame.
        _ => (10.0, 7.0, 6.0, 33.0),
    };
    // The sky shot sits in the return lane and looks straight up it; the rest sit
    // a little off the line, which is what gives the field some width to them.
    let (pos_y, look_y) = if mode == CameraMode::SkyCam {
        (focus.y, focus.y)
    } else {
        (focus.y * 0.55, focus.y * 0.72)
    };
    let pos = Vec3::new(focus.x - dir * dist, pos_y, height);
    let look = Vec3::new(focus.x + dir * lead, look_y, 0.35);
    let (pos, look) = lens_shift(pos, look, focus, lens.0, lens.1);
    let mut cam = Camera::new(pos, 0.0, 0.0);
    cam.fov = fov.to_radians();
    cam.look_at(look);
    cam
}

fn throw_off_camera(
    sim: &ThrowOffSim,
    focus: Vec2,
    mode: CameraMode,
    lens: (f32, f32),
) -> Camera {
    // Before the catch the shot belongs to the ball going downfield; after it, to
    // the man running it back, so the lens swings behind whichever is live.
    let dir = if matches!(
        sim.phase,
        ThrowPhase::Loose | ThrowPhase::Return | ThrowPhase::Dead
    ) {
        sim.setup.receive_direction as f32
    } else {
        sim.setup.throwing_direction() as f32
    };
    throw_off_lens(dir, focus, mode, lens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use a7fl_core::playbook::{defensive_playbook, playbook};
    use a7fl_core::sim::SnapSetup;
    use a7fl_core::team::league_teams;

    /// Three gunners, one returner, and the thrower gone from the coverage.
    #[test]
    fn the_throw_off_shot_shows_three_gunners_and_no_thrower() {
        let teams = league_teams();
        let setup = a7fl_core::throwoff::ThrowOffSetup::normal();
        let mut sim = ThrowOffSim::new(&setup, &teams[0], &teams[1]);
        let (presnap, _) = Broadcast::throw_off_players(&sim);
        assert_eq!(presnap.len(), 5, "the thrower is on the field until he throws");
        while sim.phase == ThrowPhase::Presnap {
            sim.step(1.0 / 60.0);
        }
        let (players, _) = Broadcast::throw_off_players(&sim);
        assert_eq!(players.len(), 4, "three gunners and one returner");
        let in_throwing_kit = players.iter().filter(|p| p.team == setup.throwing_team).count();
        assert_eq!(in_throwing_kit, 3, "the thrower has left the picture");
    }

    #[test]
    fn the_live_frame_puts_the_teams_on_the_turf() {
        let teams = league_teams();
        let play = playbook()
            .into_iter()
            .find(|p| p.family.is_pass())
            .expect("a pass play");
        let def = defensive_playbook()[0].clone();
        let setup = SnapSetup {
            los: 45.0,
            ball_y: 0.0,
            direction: 1,
            down: 2,
            to_go: 7.0,
            seed: 3,
            offense_team: 0,
            defense_team: 1,
            is_try: false,
        };
        let mut sim = PlaySim::new(&teams[0], &teams[1], &play, &def, &setup);
        for _ in 0..90 {
            if sim.result.is_some() {
                break;
            }
            sim.step(1.0 / 60.0);
        }

        let mut broadcast = Broadcast::new(320, 180, [teams[0].clone(), teams[1].clone()]);
        let frame = broadcast.live_frame(&sim, CameraMode::Broadcast);
        let grass = frame.color.iter().filter(|c| c.g > c.r && c.g > c.b).count();
        let total = frame.w * frame.h;
        assert!(grass > total / 6, "almost no turf in the live frame: {grass}/{total}");
    }

    /// A ball above head height casts no shadow, so it gets a ring on the turf
    /// under it -- and a ball on the grass gets nothing, because there is
    /// nothing to track.
    #[test]
    fn a_ball_in_the_air_gets_a_ring_on_the_turf() {
        let teams = league_teams();
        let mut b = Broadcast::new(320, 200, [teams[0].clone(), teams[1].clone()]);
        let mut cam = Camera::new(Vec3::new(5.0, -4.0, 7.0), 0.0, 0.0);
        cam.look_at(Vec3::ZERO);
        let mut high = Frame::new(320, 200);
        b.paint_ball_marker(&mut high, &cam, Vec2::new(0.0, 0.0), 6.0);
        let lit = high.color.iter().filter(|c| **c == hud::YELLOW).count();
        assert!(lit > 80, "no ring under a ball in the air: {lit} pixels");
        let mut low = Frame::new(320, 200);
        b.paint_ball_marker(&mut low, &cam, Vec2::new(0.0, 0.0), 0.40);
        let lit_low = low.color.iter().filter(|c| **c == hud::YELLOW).count();
        assert_eq!(lit_low, 0, "a ball on the grass should not carry a marker");
    }

    /// A foul does not stop the snap any more, so the picture is the only warning
    /// that one is on the ground: a pennant on the turf and the call in the corner.
    #[test]
    fn a_flag_stands_on_the_turf_with_a_caption() {
        let teams = league_teams();
        let play = playbook()
            .into_iter()
            .find(|p| p.family.is_pass())
            .expect("a pass play");
        let def = defensive_playbook()[0].clone();
        let setup = SnapSetup {
            los: 45.0,
            ball_y: 0.0,
            direction: 1,
            down: 2,
            to_go: 7.0,
            seed: 5,
            offense_team: 0,
            defense_team: 1,
            is_try: false,
        };
        let mut sim = PlaySim::new(&teams[0], &teams[1], &play, &def, &setup);
        let mut broadcast = Broadcast::new(640, 360, [teams[0].clone(), teams[1].clone()]);
        let clean = broadcast.live_frame(&sim, CameraMode::Broadcast);
        let amber = |f: &Frame| f.color.iter().filter(|c| **c == hud::YELLOW).count();

        sim.flag = Some(a7fl_core::rules::PenaltyCall::new(
            a7fl_core::rules::PenaltyKind::Holding,
            45.0,
        ));
        sim.flag_at = sim.t;
        let flagged = broadcast.live_frame(&sim, CameraMode::Broadcast);
        assert!(
            amber(&flagged) > amber(&clean) + 40,
            "the flag drew {} amber pixels over {} clean",
            amber(&flagged),
            amber(&clean)
        );
        // The caption is pinned to the top-right corner, clear of the picture's
        // subject.
        let caption = (0..40)
            .flat_map(|y| (380..640).map(move |x| (x, y)))
            .filter(|(x, y)| flagged.get(*x, *y) == hud::YELLOW)
            .count();
        assert!(caption > 20, "no caption on the picture: {caption} amber px");
    }

    /// The client turns the stick into world space, so the arcade camera's
    /// orientation has to stay what that mapping assumes: screen-right is world
    /// `+x` (downfield on a rightward drive) and screen-up is world `-y` (away
    /// from the sideline the camera sits on).
    #[test]
    fn the_broadcast_camera_puts_x_right_and_neg_y_up() {
        let mut cam = Camera::new(Vec3::new(-6.0, 32.0, 17.5), 0.0, 0.0);
        cam.look_at(Vec3::new(4.0, 0.0, 0.8));
        let lens = crate::scene::Projector::new(1920, 1080, cam.fov);
        let project = |w: Vec3| lens.project(cam.view(w)).expect("in front of the lens");
        let centre = project(Vec3::new(0.0, 0.0, 0.5));
        let right = project(Vec3::new(6.0, 0.0, 0.5));
        let away = project(Vec3::new(0.0, -6.0, 0.5));

        assert!(right.0 > centre.0 + 20.0, "world +x should sit screen right");
        assert!(away.1 < centre.1 - 20.0, "world -y should sit screen up");
    }

    /// A throw-off used to be shot from fifteen and a half yards up, planted behind
    /// the thrower and aimed at the landing spot: the ball was a dot travelling
    /// toward a dot, and the return was somebody else's problem. It sits at a
    /// runner's height now and travels with the ball.
    #[test]
    fn the_throw_off_shot_sits_low_and_goes_with_the_ball() {
        for mode in [CameraMode::Classic, CameraMode::Broadcast, CameraMode::SkyCam] {
            for (x, y) in [(20.0, 0.0), (60.0, -12.0), (95.0, 9.0)] {
                let ball = Vec2::new(x, y);
                let cam = throw_off_lens(1.0, ball, mode, (1.0, 0.0));
                let lens = crate::scene::Projector::new(640, 360, cam.fov);
                let (sx, sy, _) = lens
                    .project(cam.view(Vec3::new(x, y, 1.0)))
                    .unwrap_or_else(|| panic!("{mode:?} lost a throw-off ball at ({x}, {y})"));
                assert!(
                    (0.0..640.0).contains(&sx) && (0.0..360.0).contains(&sy),
                    "{mode:?} put the ball at ({x}, {y}) off the frame: screen ({sx}, {sy})"
                );
                if mode != CameraMode::SkyCam {
                    assert!(
                        cam.pos.z < 9.0,
                        "{mode:?} still sits {:.1} yards up on a throw-off",
                        cam.pos.z
                    );
                }
            }
        }
        // And it has to travel: a ball ninety-five yards downfield is shot from
        // ninety-five yards downfield, not from where the throw-off was taken.
        let near = throw_off_lens(1.0, Vec2::new(20.0, 0.0), CameraMode::Broadcast, (1.0, 0.0));
        let far = throw_off_lens(1.0, Vec2::new(95.0, 0.0), CameraMode::Broadcast, (1.0, 0.0));
        assert!(
            (far.pos.x - near.pos.x - 75.0).abs() < 0.01,
            "the throw-off shot stays put: {:.1} -> {:.1}",
            near.pos.x,
            far.pos.x
        );
    }

    /// The ball is the anchor. The classic camera used to be pinned to the line of
    /// scrimmage, so a run down the field walked out of a shot still framing the
    /// snap; wherever the ball gets to, it has to be in the picture.
    #[test]
    fn the_cameras_keep_the_ball_in_the_picture_wherever_it_gets_to() {
        let teams = league_teams();
        let b = Broadcast::new(640, 360, [teams[0].clone(), teams[1].clone()]);
        for (x, y) in [(20.0, 0.0), (55.0, -12.0), (95.0, 8.0), (108.0, 0.0)] {
            for mode in [CameraMode::Classic, CameraMode::Broadcast, CameraMode::SkyCam] {
                let ball = Vec2::new(x, y);
                let cam = b.camera_for(mode, ball, 1, 40.0, 0.0);
                let lens = crate::scene::Projector::new(640, 360, cam.fov);
                let (sx, sy, _) = lens
                    .project(cam.view(Vec3::new(x, y, 1.0)))
                    .unwrap_or_else(|| panic!("{mode:?} lost the ball at rest at ({x}, {y})"));
                assert!(
                    (0.0..640.0).contains(&sx) && (0.0..360.0).contains(&sy),
                    "{mode:?} put the ball at ({x}, {y}) off the frame: screen ({sx}, {sy})"
                );
            }
        }
    }

    /// The sky shot is straight: behind the ball, in the ball's own lane, looking
    /// down the field. It used to stand five yards off that lane, which is what
    /// made it -- and the preview that eases into it -- read as tilted.
    #[test]
    fn the_sky_shot_looks_straight_down_the_field() {
        let teams = league_teams();
        let b = Broadcast::new(640, 360, [teams[0].clone(), teams[1].clone()]);
        for y in [0.0f32, -12.0, 9.0] {
            for dir in [1i8, -1] {
                let cam = b.camera_for(CameraMode::SkyCam, Vec2::new(50.0, y), dir, 40.0, 0.0);
                assert!(
                    (cam.pos.y - y).abs() < 1e-3,
                    "the sky lens sits {:.2} yards off the ball's lane",
                    cam.pos.y - y
                );
                let expect = if dir > 0 { 0.0 } else { std::f32::consts::PI };
                let mut off = cam.yaw - expect;
                while off > std::f32::consts::PI {
                    off -= std::f32::consts::TAU;
                }
                while off < -std::f32::consts::PI {
                    off += std::f32::consts::TAU;
                }
                assert!(
                    off.abs() < 1e-3,
                    "the sky lens is turned {off:.2} rad off the field"
                );
                assert!(cam.pitch < 0.0, "the sky lens is not looking at the field");
            }
        }
    }

    /// A reviewed foul is cut to the flag, not to the ball.
    #[test]
    fn a_pinned_shot_frames_the_flag_instead_of_the_ball() {
        let teams = league_teams();
        let play = playbook()
            .into_iter()
            .find(|p| p.family.is_pass())
            .expect("a pass play");
        let def = defensive_playbook()[0].clone();
        let setup = SnapSetup {
            los: 55.0,
            ball_y: 0.0,
            direction: 1,
            down: 1,
            to_go: 10.0,
            seed: 9,
            offense_team: 0,
            defense_team: 1,
            is_try: false,
        };
        let mut sim = PlaySim::new(&teams[0], &teams[1], &play, &def, &setup);
        sim.step(1.0 / 60.0);
        let mut b = Broadcast::new(640, 360, [teams[0].clone(), teams[1].clone()]);
        let flag = Vec2::new(30.0, 0.0);
        b.pin_focus(Some(flag));
        let cam = b.live_scene(&sim, CameraMode::Replay).1;
        let lens = crate::scene::Projector::new(640, 360, cam.fov);
        let (fx, _, _) = lens
            .project(cam.view(Vec3::new(flag.x, flag.y, 0.5)))
            .expect("the flag is not in the picture");
        assert!(
            (fx - 320.0).abs() < 90.0,
            "the flag sits at screen {fx}, not in the middle of the shot"
        );
        // The shot must not drift with the play: a pinned focus is the flag, and
        // the flag has not moved while the men have.
        for _ in 0..45 {
            sim.step(1.0 / 60.0);
        }
        let later = b.live_scene(&sim, CameraMode::Replay).1;
        assert!(
            (cam.pos - later.pos).length() < 1e-4,
            "the pinned shot drifted with the ball: {:?} -> {:?}",
            cam.pos,
            later.pos
        );
        b.pin_focus(None);
    }

    #[test]
    fn the_aim_eases_toward_the_ball_instead_of_snapping_to_it() {
        let mut focus = Focus::default();
        assert_eq!(focus.track(Vec2::new(10.0, 0.0)), Vec2::new(10.0, 0.0));
        let eased = focus.track(Vec2::new(30.0, 0.0));
        assert!(
            eased.x > 10.0 && eased.x < 30.0,
            "the aim should lag behind the ball, not jump to it: {}",
            eased.x
        );
        for _ in 0..40 {
            focus.track(Vec2::new(30.0, 0.0));
        }
        assert!(
            (focus.spot.unwrap().x - 30.0).abs() < 0.2,
            "it still has to arrive: {:?}",
            focus.spot
        );
    }

    /// A ball thrown thirty yards downfield is the fastest thing on the field, and
    /// the camera has to be on it for the whole flight -- not merely somewhere near
    /// where it left from. This walks a real pass frame by frame and projects the
    /// ball through the same lens the picture uses.
    #[test]
    fn the_shot_keeps_up_with_a_ball_thrown_downfield() {
        for mode in [CameraMode::Classic, CameraMode::Broadcast, CameraMode::SkyCam] {
            flight_stays_in_frame(mode);
        }
    }

    fn flight_stays_in_frame(mode: CameraMode) {
        use a7fl_core::sim::{QbControl, SNAP_DELAY};

        let teams = league_teams();
        let play = playbook()
            .into_iter()
            .find(|p| p.family.is_pass())
            .expect("a pass play");
        let def = defensive_playbook()[0].clone();
        let setup = SnapSetup {
            los: 30.0,
            ball_y: 0.0,
            direction: 1,
            down: 1,
            to_go: 10.0,
            seed: 5,
            offense_team: 0,
            defense_team: 1,
            is_try: false,
        };
        let mut sim = PlaySim::new(&teams[0], &teams[1], &play, &def, &setup);
        while sim.t < SNAP_DELAY + 1.0 && sim.result.is_none() {
            sim.step(1.0 / 60.0);
        }
        let qb = sim.qb_pos();
        let deep = sim
            .eligible_targets()
            .into_iter()
            .map(|(_, i)| i)
            .max_by(|a, b| {
                let qb = sim.qb_pos();
                sim.players[*a].pos.dist(qb).total_cmp(&sim.players[*b].pos.dist(qb))
            })
            .expect("a receiver");
        sim.set_control(Some(QbControl {
            move_dir: Vec2::ZERO,
            target: Some(deep),
            throw: true,
            power: 1.0,
        }));
        for _ in 0..3 {
            sim.step(1.0 / 60.0);
        }
        assert!(sim.ball.flight.is_some(), "the ball never left his hand");

        let mut broadcast = Broadcast::new(640, 360, [teams[0].clone(), teams[1].clone()]);
        let mut travelled = 0.0f32;
        for frame_no in 0..120 {
            if sim.ball.flight.is_none() || sim.result.is_some() {
                break;
            }
            sim.step(1.0 / 60.0);
            let cam = broadcast.live_scene(&sim, mode).1;
            let (bp, bz) = sim.ball_position();
            travelled = travelled.max(bp.dist(qb));
            let lens = crate::scene::Projector::new(640, 360, cam.fov);
            let shot = lens.project(cam.view(Vec3::new(bp.x, bp.y, bz.max(0.2))));
            let Some((sx, sy, depth)) = shot else {
                panic!("{mode:?} frame {frame_no}: the ball is behind the lens at ({}, {})", bp.x, bp.y);
            };
            assert!(
                (0.0..640.0).contains(&sx) && (0.0..360.0).contains(&sy) && depth > 0.0,
                "{mode:?} frame {frame_no}: the ball travelled {travelled:.1} yards and left the picture at screen ({sx:.0}, {sy:.0})"
            );
        }
        assert!(
            travelled > 15.0,
            "{mode:?}: the test never threw the ball far enough to prove anything: {travelled:.1} yards"
        );
    }
}

/// Darken the top and bottom edges so overlay text reads cleanly.
pub fn vignette(frame: &mut Frame) {
    let w = frame.w as i32;
    let h = frame.h as i32;
    for i in 0..60 {
        let alpha = (i as f32 / 60.0) * 0.15;
        frame.fill_rect_alpha(0, h - 60 + i, w, 1, INK, alpha);
        frame.fill_rect_alpha(0, i, w, 1, INK, alpha * 0.5);
    }
}

/// One man in the stands.
struct CrowdFigure {
    /// Where he stands on the tread of his row.
    at: Vec3,
    color: Rgb,
    /// His own place in the bob, so the house does not rise and fall together.
    phase: f32,
    tall: f32,
    arms: bool,
}

/// Everybody in the two sidelines, laid out on the same rows the stand is built
/// from. The bodies are the broadcast's, so they can move; the concrete is not.
fn build_crowd(teams: &[Team; 2]) -> Vec<CrowdFigure> {
    let mut seed = 0x5EED_C0DEu32;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed >> 8) as f32 / 16_777_215.0
    };
    let colors = [
        teams[0].primary,
        teams[1].primary,
        Rgb::hex(0xD8DDE4),
        Rgb::hex(0x49566A),
        Rgb::hex(0x27303C),
        Rgb::hex(0x8C4B3B),
    ];
    let rows = 7;
    let tread = 1.30f32;
    let riser = 0.62f32;
    let x0 = -14.0f32;
    let x1 = 114.0f32;
    let mut z = 1.18f32;
    let mut y_front = FIELD_HALF_WIDTH + 4.8;
    let mut out = Vec::new();
    for _ in 0..rows {
        let z_top = z + riser;
        let y_back = y_front + tread;
        for side in [-1.0f32, 1.0] {
            let mut px_i = x0;
            while px_i < x1 {
                let gap = 0.74 + rand() * 0.42;
                if rand() < 0.92 {
                    let y_crowd = (y_front + 0.30 + rand() * 0.25) * side;
                    out.push(CrowdFigure {
                        at: Vec3::new(px_i + rand() * 0.3, y_crowd, z_top),
                        color: colors[(rand() * colors.len() as f32) as usize % colors.len()],
                        phase: rand() * std::f32::consts::TAU,
                        tall: 0.62 + rand() * 0.16,
                        arms: rand() < 0.12,
                    });
                }
                px_i += gap;
            }
        }
        z = z_top;
        y_front = y_back;
    }
    out
}

/// Where a camera sits between two shots. Angles take the short way round, so a
/// zoom out never swings through the back of the stand on the way.
fn blend_camera(a: &Camera, b: &Camera, t: f32) -> Camera {
    let t = t.clamp(0.0, 1.0);
    let mut yaw = b.yaw - a.yaw;
    while yaw > std::f32::consts::PI {
        yaw -= std::f32::consts::TAU;
    }
    while yaw < -std::f32::consts::PI {
        yaw += std::f32::consts::TAU;
    }
    let mut cam = Camera::new(a.pos + (b.pos - a.pos) * t, a.yaw + yaw * t, a.pitch + (b.pitch - a.pitch) * t);
    cam.fov = a.fov + (b.fov - a.fov) * t;
    cam.roll = a.roll + (b.roll - a.roll) * t;
    cam
}

/// Smoothstep, so the preview leaves and arrives without a snap at either end.
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}


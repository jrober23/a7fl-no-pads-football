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
use a7fl_core::throwoff::{ThrowOffSim, ThrowRole};

use crate::fieldtex;
use crate::hud::{self, Banner, INK};
use crate::players::{self, Pose};
use crate::raster::{Frame, Texture};
use crate::scene::{Camera, Mesh, SceneRenderer};

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
}

impl Broadcast {
    pub fn new(w: usize, h: usize, teams: [Team; 2]) -> Self {
        let field = fieldtex::build_field_texture(&teams[0], &teams[1]);
        let uniforms = [teams[0].home, teams[1].away];
        let scene = SceneRenderer::new(w, h, 46f32.to_radians());
        let stands = build_stands(&teams);
        Broadcast { w, h, field, scene, teams, uniforms, stands, focus: Focus::default() }
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
                    carrying: sim.ball.holder == Some(i) && !p.down,
                    blocking: p.engaged.is_some(),
                    down: p.down,
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
        _mode: CameraMode,
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
            (players, ball, throw_off_camera(&sim))
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
                celebrate: 0.0,
                // A returner is not moved to a dive or a spin yet: the throw-off
                // has its own state machine and no move set of its own.
                motion: None,
                motion_progress: 0.0,
            })
            .collect();
        (players, ball)
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
        frame
    }

    /// A throw-off as it is being played, with the client's meter and landing
    /// marker drawn through the same camera as the field.
    pub fn throw_off_frame_live(&mut self, sim: &ThrowOffSim, ui: &crate::passing::PassUi) -> Frame {
        let (players, ball) = Broadcast::throw_off_players(sim);
        let camera = throw_off_camera(sim);
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
                carrying: sim.ball.holder == Some(i) && !p.down,
                blocking: p.engaged.is_some(),
                down: p.down,
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
        let focus = self.focus.track_with(anchor, rate);
        let camera = self.camera_for(mode, focus, sim.setup.direction, sim.los, sim.setup.ball_y);
        self.sync_lens(&camera);
        let mut frame = Frame::new(self.w, self.h);
        self.paint_environment(&mut frame, &camera);
        self.paint_lines(&mut frame, &camera, sim.los, sim.setup.direction, sim.setup.to_go);
        self.draw_bodies(&mut frame, &camera, &players);
        if sim.ball.flight.is_some() || sim.ball.holder.is_none() {
            let ball = players::ball_mesh(Vec3::new(ball_pos.x, ball_pos.y, ball_z.max(0.2)));
            self.scene.draw_mesh(&mut frame, &ball, &camera);
        }
        (frame, camera)
    }

    fn draw_bodies(&mut self, frame: &mut Frame, camera: &Camera, bodies: &[RenderPlayer]) {
        // Shadows first so every player casts one.
        for p in bodies {
            let radius = if p.down { 0.72 } else { 0.5 };
            self.scene
                .draw_shadow(frame, camera, Vec3::new(p.pos.x, p.pos.y, 0.0), radius, 0.42);
        }
        for p in bodies {
            let roster = &self.teams[p.team].roster[p.roster];
            let uniform = &self.uniforms[p.team];
            let pose = Pose {
                run_phase: p.run_phase,
                speed: p.speed,
                carrying: p.carrying,
                blocking: p.blocking,
                down: p.down,
                celebrating: p.celebrate,
                strafe: 0.0,
                motion: p.motion,
                motion_progress: p.motion_progress,
            };
            let mut mesh = players::build_mesh(roster, uniform, &pose, p.roster as u32);
            mesh.place(Vec3::new(p.pos.x, p.pos.y, 0.0), p.heading, 1.0);
            self.scene.draw_mesh(frame, &mesh, camera);
        }
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
        None
    }

    fn paint_environment(&mut self, frame: &mut Frame, camera: &Camera) {
        // Sky gradient: night game under the lights.
        let top = Rgb::hex(0x0B1520);
        let bottom = Rgb::hex(0x4C6274);
        frame.gradient_rect(0, 0, frame.w as i32, frame.h as i32, top, bottom);

        let (x0, x1, y0, y1) = fieldtex::coverage();
        self.scene.draw_ground(frame, &self.field, camera, x0, x1, y0, y1, 2.5);
        self.scene.draw_mesh(frame, &self.stands, camera);
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
            CameraMode::Classic => (
                Vec3::new(action.x - d * 11.0, action.y * 0.5, 10.2),
                Vec3::new(action.x + d * 8.0, action.y * 0.8, 0.9),
                42f32,
            ),
            CameraMode::BehindQb => (
                Vec3::new(los - d * 15.0, ball_y + 0.8, 8.4),
                Vec3::new(los + d * 13.0, ball_y, 1.3),
                44f32,
            ),
            CameraMode::SkyCam => (
                Vec3::new(action.x - d * 15.0, action.y - 5.0, 40.0),
                Vec3::new(action.x, action.y, 0.0),
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
        let mut cam = Camera::new(pos, 0.0, 0.0);
        cam.look_at(look);
        cam.fov = fov.to_radians();
        cam
    }

    /// Rebuild the projector for the camera's field of view.
    pub fn sync_lens(&mut self, camera: &Camera) {
        self.scene = SceneRenderer::new(self.w, self.h, camera.fov);
    }
}

/// Which role label to draw on a pre-snap card.
pub fn role_label(role: OffRole) -> &'static str {
    role.label()
}

fn build_stands(teams: &[Team; 2]) -> Mesh {
    let mut mesh = Mesh::new();
    // Advertising boards along both sidelines.
    let mut x = -12.0f32;
    let mut i = 0;
    while x < 112.0 {
        let color = match i % 3 {
            0 => teams[0].primary,
            1 => teams[1].primary,
            _ => Rgb::hex(0xE8EAE2),
        };
        for side in [-1.0f32, 1.0] {
            let y = side * (FIELD_HALF_WIDTH + 0.9);
            mesh.quad(
                Vec3::new(x, y, 0.0),
                Vec3::new(x + 5.7, y, 0.0),
                Vec3::new(x + 5.7, y, 1.15),
                Vec3::new(x, y, 1.15),
                color,
            );
        }
        x += 6.0;
        i += 1;
    }
    // Bleachers: stepped rows climbing away from the field.
    for side in [-1.0f32, 1.0] {
        for row in 0..7 {
            let y = side * (FIELD_HALF_WIDTH + 3.0 + row as f32 * 1.6);
            let z = 1.3 + row as f32 * 0.95;
            let shade = Rgb::hex(0x2A3440).scale(1.0 - row as f32 * 0.05);
            mesh.quad(
                Vec3::new(-12.0, y, z - 0.95),
                Vec3::new(112.0, y, z - 0.95),
                Vec3::new(112.0, y, z),
                Vec3::new(-12.0, y, z),
                shade,
            );
            if row % 2 == 0 {
                let crowd = Rgb::hex(0x3B4656).scale(1.0 + row as f32 * 0.02);
                mesh.quad(
                    Vec3::new(-12.0, y, z - 1.4),
                    Vec3::new(112.0, y, z - 1.4),
                    Vec3::new(112.0, y, z - 0.95),
                    Vec3::new(-12.0, y, z - 0.95),
                    crowd,
                );
            }
        }
    }
    mesh
}

/// The throw-off shot. It opens from the throwing team's end looking downfield at
/// the man waiting for the ball, then turns upfield once he has it — the way he is
/// about to run — and rides with him.
fn throw_off_camera(sim: &ThrowOffSim) -> Camera {
    let throw_dir = sim.setup.throwing_direction() as f32;
    let recv_dir = sim.setup.receive_direction as f32;
    let spot = sim.setup.spot();
    let ball = sim.ball_position().0;
    let returner = sim.returner_position().unwrap_or(ball);
    let landing = sim.landing_spot().unwrap_or(returner);

    // Downfield: from behind the thrower, on the man under the ball.
    let from_a = Vec3::new(spot - throw_dir * 12.0, 0.0, 15.5);
    let to_a = Vec3::new(landing.x + throw_dir * 6.0, landing.y * 0.6, 0.6);
    // Upfield: behind the returner, looking the way he is running.
    let from_b = Vec3::new(returner.x - recv_dir * 12.0, returner.y * 0.45, 10.5);
    let to_b = Vec3::new(returner.x + recv_dir * 16.0, returner.y * 0.25, 0.9);

    let turn = sim
        .catch_time()
        .map(|t| ((sim.t - t) / 0.55).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let blend = turn * turn * (3.0 - 2.0 * turn);
    let pos = from_a + (from_b - from_a) * blend;
    let look = to_a + (to_b - to_a) * blend;
    let mut cam = Camera::new(pos, 0.0, 0.0);
    cam.fov = (33.0 + 9.0 * blend).to_radians();
    cam.look_at(look);
    cam
}

#[cfg(test)]
mod tests {
    use super::*;
    use a7fl_core::playbook::{defensive_playbook, playbook};
    use a7fl_core::sim::SnapSetup;
    use a7fl_core::team::league_teams;

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
            aim: Vec2::ZERO,
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

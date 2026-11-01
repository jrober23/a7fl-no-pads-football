//! Continuous-time 7-on-7 scrimmage play simulation.
//!
//! Every snap is advanced in fixed timesteps so the broadcast renderer can draw any
//! frame of it, and so a whole game can be simulated headlessly at high speed. The
//! simulation owns all of the A7FL mechanics that matter on a snap:
//!
//! * two down linemen who may release up to five yards, four eligibles,
//! * no blitz restrictions (a defense may rush as many as it likes, which leaves
//!   receivers uncovered),
//! * wrap-tackle resolution with break tackles, plus the rulebook's high-tackle and
//!   block-in-the-back infractions,
//! * pass interference, holding while the quarterback is in the pocket, roughing
//!   the passer, intentional grounding,
//! * sideline / end line / goal line geometry including safeties.

use crate::field::{self, FIELD_HALF_WIDTH, FIELD_LENGTH};
use crate::math::{approach_angle, clamp, lerp, Rng, Vec2};
use crate::playbook::{
    defensive_alignment, Assign, Coverage, DefCall, DefRole, OffRole, Play, PlayFamily,
};
use crate::rules::{PenaltyCall, PenaltyKind};
use crate::team::{Attributes, Team};

pub const TACKLE_RADIUS: f32 = 0.95;
pub const CATCH_RADIUS: f32 = 1.25;
pub const SNAP_DELAY: f32 = 0.85;
pub const MAX_PLAY_TIME: f32 = 13.0;
pub const FIXED_DT: f32 = 1.0 / 60.0;
/// Ground covered by one full stride cycle (two steps). Drives the run animation:
/// about 3.2 yards at speed, which is five or so steps a second flat out.
pub const STRIDE_YARDS: f32 = 3.2;

// ---------------------------------------------------------------------------
// Setup / results
// ---------------------------------------------------------------------------

/// Everything needed to reconstruct a snap exactly.
#[derive(Clone, Debug)]
pub struct SnapSetup {
    pub los: f32,
    pub ball_y: f32,
    pub direction: i8,
    pub down: u8,
    pub to_go: f32,
    pub seed: u64,
    pub offense_team: usize,
    pub defense_team: usize,
    /// Conversions are timed differently and don't advance the game clock.
    pub is_try: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayOutcome {
    Run,
    QbScramble,
    PassComplete,
    PassIncomplete,
    Sack,
    Interception,
    FumbleLost,
    FumbleRecovered,
    Touchdown,
    Safety,
    Penalty,
    ThrowAway,
}

impl PlayOutcome {
    pub fn is_pass_attempt(self) -> bool {
        matches!(
            self,
            PlayOutcome::PassComplete
                | PlayOutcome::PassIncomplete
                | PlayOutcome::Interception
                | PlayOutcome::ThrowAway
        )
    }

    pub fn is_completion(self) -> bool {
        matches!(self, PlayOutcome::PassComplete)
    }
}

#[derive(Clone, Debug)]
pub struct PlayResult {
    pub outcome: PlayOutcome,
    pub yards: f32,
    pub end_spot: f32,
    /// Seconds of game clock burned by the snap itself.
    pub elapsed: f32,
    pub touchdown: bool,
    pub safety: bool,
    pub turnover: bool,
    pub incomplete: bool,
    pub out_of_bounds: bool,
    pub sack: bool,
    pub air_yards: f32,
    pub penalty: Option<PenaltyCall>,
    pub passer: Option<usize>,
    pub receiver: Option<usize>,
    pub carrier: Option<usize>,
    pub tackler: Option<usize>,
    pub ejection: Option<usize>,
    pub return_yards: f32,
    pub description: String,
    /// Frames of broadcast-worthy events for the highlight package.
    pub highlight: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Offense,
    Defense,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CoverageJob {
    Rush,
    Man(usize),
    DeepZone { y: f32, depth: f32 },
    UnderZone { y: f32, depth: f32 },
    Spy(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    PreSnap,
    Run,
    Dropback,
    Scramble,
    BallInAir,
    Dead,
}

#[derive(Clone, Debug)]
pub struct SimPlayer {
    pub side: Side,
    pub role_index: usize,
    pub player: usize,
    pub pos: Vec2,
    pub vel: Vec2,
    pub heading: f32,
    pub stamina: f32,
    pub down: bool,
    pub stun: f32,
    pub tackle_cooldown: f32,
    pub engaged: Option<usize>,
    pub pressure: f32,
    pub beaten: f32,
    pub waypoint: usize,
    pub route: Vec<Vec2>,
    pub job: CoverageJob,
    pub run_phase: f32,
    pub celebrate: f32,
}

#[derive(Clone, Debug)]
pub struct Flight {
    pub from: Vec2,
    pub to: Vec2,
    pub t: f32,
    pub duration: f32,
    pub peak: f32,
    pub target: Option<usize>,
    pub thrower: usize,
    pub air_yards: f32,
}

#[derive(Clone, Debug)]
pub struct Ball {
    pub holder: Option<usize>,
    pub pos: Vec2,
    pub z: f32,
    pub flight: Option<Flight>,
}

#[derive(Clone, Debug)]
pub struct SimEvent {
    pub t: f32,
    pub text: String,
}

/// Live human input for the ball in play, set by an interactive client: the
/// quarterback before the throw, and whoever is carrying it afterwards.
///
/// Absent during ordinary AI play, so the simulation behaves exactly as it does
/// in `sim` and `film` unless somebody is holding the stick.
#[derive(Clone, Copy, Debug, Default)]
pub struct QbControl {
    /// Stick direction in **world space**, length 0..1: `x` runs down the field,
    /// `y` across it. Mapping the stick out of screen space is the client's job,
    /// because only the client knows where its camera is pointing.
    pub move_dir: Vec2,
    /// Locked-on receiver, as an index into `players`.
    pub target: Option<usize>,
    /// Set for the frame the throw button went down.
    pub throw: bool,
}

pub struct PlaySim<'a> {
    pub setup: SnapSetup,
    pub play: Play,
    pub def: DefCall,
    pub offense: &'a Team,
    pub defense: &'a Team,
    pub players: Vec<SimPlayer>,
    pub ball: Ball,
    pub phase: Phase,
    pub t: f32,
    pub rng: Rng,
    pub los: f32,
    pub dir: i8,
    pub qb: usize,
    pub carrier: Option<usize>,
    pub intended: Option<usize>,
    pub handoff: Option<(usize, f32)>,
    pub result: Option<PlayResult>,
    /// `Some` when a human is driving the quarterback.
    pub control: Option<QbControl>,
    pub events: Vec<SimEvent>,
    read_timer: f32,
    catch_spot: f32,
    scramble_started: bool,
    throw_away: bool,
    pass_caught: bool,
    possession_start: Option<usize>,
}

fn roster_of<'t>(team: &'t Team, idx: usize) -> &'t crate::team::Player {
    &team.roster[idx]
}

impl<'a> PlaySim<'a> {
    pub fn new(
        offense: &'a Team,
        defense: &'a Team,
        play: &Play,
        dcall: &DefCall,
        setup: &SnapSetup,
    ) -> Self {
        let mut rng = Rng::new(setup.seed ^ 0x5EED_1234);
        let dirf = setup.direction as f32;
        let off_p = offense.offense_personnel();
        let def_p = defense.defense_personnel();

        let mut players: Vec<SimPlayer> = Vec::with_capacity(14);
        for role in OffRole::ALL {
            let a = play.formation.alignment(role);
            let pos = Vec2::new(
                setup.los + dirf * a.x,
                clamp(setup.ball_y + a.y, -FIELD_HALF_WIDTH + 0.6, FIELD_HALF_WIDTH - 0.6),
            );
            players.push(SimPlayer {
                side: Side::Offense,
                role_index: role.index(),
                player: off_p.players[role.index()],
                pos,
                vel: Vec2::ZERO,
                heading: if setup.direction > 0 { 0.0 } else { std::f32::consts::PI },
                stamina: 1.0,
                down: false,
                stun: 0.0,
                tackle_cooldown: 0.0,
                engaged: None,
                pressure: 0.0,
                beaten: 0.0,
                waypoint: 0,
                route: Vec::new(),
                job: CoverageJob::Rush,
                run_phase: 0.0,
                celebrate: 0.0,
            });
        }
        for role in DefRole::ALL {
            let a = defensive_alignment(role, dcall.coverage, setup.los, setup.direction, play.run_side);
            let pos = Vec2::new(
                a.x,
                clamp(setup.ball_y + a.y, -FIELD_HALF_WIDTH + 0.8, FIELD_HALF_WIDTH - 0.8),
            );
            players.push(SimPlayer {
                side: Side::Defense,
                role_index: role.index(),
                player: def_p.players[role.index()],
                pos,
                vel: Vec2::ZERO,
                heading: if setup.direction > 0 {
                    std::f32::consts::PI
                } else {
                    0.0
                },
                stamina: 1.0,
                down: false,
                stun: 0.0,
                tackle_cooldown: 0.0,
                engaged: None,
                pressure: 0.0,
                beaten: 0.0,
                waypoint: 0,
                route: Vec::new(),
                job: CoverageJob::Rush,
                run_phase: 0.0,
                celebrate: 0.0,
            });
        }

        let mut sim = PlaySim {
            setup: setup.clone(),
            play: play.clone(),
            def: dcall.clone(),
            offense,
            defense,
            players,
            ball: Ball { holder: Some(0), pos: Vec2::ZERO, z: 1.15, flight: None },
            phase: Phase::PreSnap,
            t: 0.0,
            rng,
            los: setup.los,
            dir: setup.direction,
            qb: 0,
            carrier: None,
            intended: None,
            handoff: None,
            result: None,
            events: Vec::new(),
            read_timer: 0.0,
            catch_spot: setup.los,
            scramble_started: false,
            control: None,
            throw_away: false,
            pass_caught: false,
            possession_start: None,
        };
        sim.ball.pos = sim.players[0].pos;
        sim.build_routes();
        sim.assign_coverage();
        sim.setup_handoff();
        sim
    }

    fn dirf(&self) -> f32 {
        self.dir as f32
    }

    fn attrs(&self, i: usize) -> Attributes {
        let p = &self.players[i];
        match p.side {
            Side::Offense => self.offense.roster[p.player].attrs,
            Side::Defense => self.defense.roster[p.player].attrs,
        }
    }

    fn roster_index(&self, i: usize) -> usize {
        self.players[i].player
    }

    fn player_of(&self, i: usize) -> &crate::team::Player {
        let p = &self.players[i];
        match p.side {
            Side::Offense => roster_of(self.offense, p.player),
            Side::Defense => roster_of(self.defense, p.player),
        }
    }

    fn name_of(&self, i: usize) -> String {
        self.player_of(i).short_name()
    }

    fn speed_cap(&self, i: usize) -> f32 {
        let a = self.attrs(i);
        a.top_speed() * (0.74 + 0.26 * self.players[i].stamina)
    }

    fn accel_of(&self, i: usize) -> f32 {
        self.attrs(i).acceleration()
    }

    fn turn_of(&self, i: usize) -> f32 {
        self.attrs(i).turn_rate()
    }

    // -----------------------------------------------------------------
    // Construction helpers
    // -----------------------------------------------------------------

    fn build_routes(&mut self) {
        for i in 0..7 {
            let role = OffRole::ALL[self.players[i].role_index];
            let align = self.play.formation.alignment(role);
            let dirf = self.dirf();
            let mut pts: Vec<Vec2> = Vec::new();
            match self.play.assignment(role) {
                Some(Assign::Route { points, .. }) => {
                    for p in points {
                        pts.push(Vec2::new(
                            self.los + dirf * (align.x + p.x),
                            clamp(self.setup.ball_y + align.y + p.y, -FIELD_HALF_WIDTH + 0.4, FIELD_HALF_WIDTH - 0.4),
                        ));
                    }
                }
                Some(Assign::Fake { points, .. }) => {
                    for p in points {
                        pts.push(Vec2::new(
                            self.los + dirf * (align.x + p.x),
                            clamp(self.setup.ball_y + align.y + p.y, -FIELD_HALF_WIDTH + 0.4, FIELD_HALF_WIDTH - 0.4),
                        ));
                    }
                }
                Some(Assign::Carry { points }) | Some(Assign::QbCarry { points }) => {
                    for p in points {
                        pts.push(Vec2::new(
                            self.los + dirf * p.x,
                            clamp(self.setup.ball_y + align.y + p.y, -FIELD_HALF_WIDTH + 0.4, FIELD_HALF_WIDTH - 0.4),
                        ));
                    }
                }
                _ => {}
            }
            self.players[i].route = pts;
        }
    }

    fn eligible_indices(&self) -> Vec<usize> {
        (0..7)
            .filter(|&i| OffRole::ALL[self.players[i].role_index].is_eligible())
            .collect()
    }

    fn assign_coverage(&mut self) {
        let eligible = self.eligible_indices();
        // Rushers: both edge players plus any called blitzers.
        let mut rushers: Vec<usize> = Vec::new();
        for i in 7..14 {
            let role = DefRole::ALL[self.players[i].role_index];
            let blitzes = self.def.blitzers.contains(&role);
            if role.is_lineman() || blitzes || self.def.coverage == Coverage::GoalLine && role == DefRole::Mike {
                rushers.push(i);
                self.players[i].job = CoverageJob::Rush;
            }
        }
        let mut available: Vec<usize> = (7..14).filter(|i| !rushers.contains(i)).collect();
        // Prefer cornerbacks and safeties for coverage duty, linebackers last.
        let rank = |sim: &PlaySim, i: usize| -> u8 {
            match DefRole::ALL[sim.players[i].role_index] {
                DefRole::CbL | DefRole::CbR => 0,
                DefRole::Safety => 1,
                DefRole::Mike | DefRole::Will => 2,
                _ => 3,
            }
        };
        available.sort_by_key(|&i| rank(self, i));

        let deep_wanted = match self.def.coverage {
            Coverage::Cover2 => 2,
            Coverage::Cover3 => 3,
            Coverage::Cover4 => 4.min(available.len().saturating_sub(1)),
            Coverage::ManFree | Coverage::ManPress => 1,
            Coverage::GoalLine => 0,
        }
        .min(available.len());

        // Deep defenders take the last (deepest ranked) slots so the best cover men
        // stay in man / underneath.
        let mut deep: Vec<usize> = Vec::new();
        for _ in 0..deep_wanted {
            if let Some(i) = available.pop() {
                deep.push(i);
            }
        }
        let mut under: Vec<usize> = available;

        // Man coverage: put defenders over the four eligibles.
        if self.def.coverage.is_man() || self.def.coverage == Coverage::GoalLine {
            let mut sorted_elig = eligible.clone();
            sorted_elig.sort_by_key(|&i| std::cmp::Reverse(self.play_route_depth(i)));
            let mut remaining = under.clone();
            under.clear();
            for &e in &sorted_elig {
                if let Some(d) = remaining.pop() {
                    self.players[d].job = CoverageJob::Man(e);
                    // Align across from the receiver with a cushion (or press).
                    let cushion = if self.def.press { 1.1 } else { 6.0 };
                    let receiver_y = self.players[e].pos.y;
                    self.players[d].pos = Vec2::new(
                        self.los + self.dirf() * cushion,
                        clamp(receiver_y, -FIELD_HALF_WIDTH + 0.8, FIELD_HALF_WIDTH - 0.8),
                    );
                }
            }
            under = remaining;
        }

        // Zone assignments. Deep defenders split the field into thirds / halves.
        let deep_ys: Vec<f32> = match self.def.coverage {
            Coverage::Cover2 => vec![-9.5, 9.5],
            Coverage::Cover3 => vec![-12.0, 0.0, 12.0],
            Coverage::Cover4 => vec![-14.0, -5.0, 5.0, 14.0],
            Coverage::GoalLine => vec![],
            _ => vec![0.0],
        };
        for (n, &d) in deep.iter().enumerate() {
            let y = deep_ys.get(n).copied().unwrap_or(0.0);
            let depth = match self.def.coverage {
                Coverage::Cover4 => 17.0,
                Coverage::Cover2 => 15.0,
                _ => 16.0,
            };
            self.players[d].job = CoverageJob::DeepZone { y, depth };
            self.players[d].pos = Vec2::new(
                self.los + self.dirf() * depth,
                clamp(self.setup.ball_y + y, -FIELD_HALF_WIDTH + 1.0, FIELD_HALF_WIDTH - 1.0),
            );
        }

        let under_ys: Vec<f32> = match self.def.coverage {
            Coverage::Cover4 => vec![-8.0, 8.0],
            Coverage::Cover2 => vec![-11.0, 0.0, 9.0],
            _ => vec![-7.0, 7.0],
        };
        for (n, &d) in under.iter().enumerate() {
            let y = under_ys.get(n).copied().unwrap_or(0.0);
            let depth = if self.def.coverage == Coverage::Cover2 { 6.0 } else { 5.0 };
            self.players[d].job = CoverageJob::UnderZone { y, depth };
            self.players[d].pos = Vec2::new(
                self.los + self.dirf() * depth,
                clamp(self.setup.ball_y + y, -FIELD_HALF_WIDTH + 1.0, FIELD_HALF_WIDTH - 1.0),
            );
        }
    }

    fn play_route_depth(&self, sim_index: usize) -> u8 {
        let role = OffRole::ALL[self.players[sim_index].role_index];
        self.play
            .assignment(role)
            .and_then(|a| a.route_depth())
            .unwrap_or(0)
    }

    fn setup_handoff(&mut self) {
        let carrier_role = self.play.carrier_role();
        self.possession_start = Some(0);
        match self.play.family {
            PlayFamily::RunInside | PlayFamily::RunOutside | PlayFamily::QbRun => {
                if let Some(role) = carrier_role {
                    let idx = role.index();
                    // No kicking and no pitches: the ball changes hands at the mesh
                    // point, then the runner owns it.
                    if role == OffRole::Qb {
                        self.carrier = Some(idx);
                        self.ball.holder = Some(idx);
                    } else {
                        self.handoff = Some((idx, SNAP_DELAY + 0.55));
                    }
                }
            }
            PlayFamily::PlayAction => {
                // Sell the run, then the QB keeps it to throw.
                if let Some(role) = carrier_role {
                    self.handoff = Some((role.index(), SNAP_DELAY + 0.5));
                }
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------
    // Main loop
    // -----------------------------------------------------------------

    pub fn run_to_completion(&mut self) -> PlayResult {
        let mut guard = 0;
        while self.result.is_none() && guard < 4000 {
            self.step(FIXED_DT);
            guard += 1;
        }
        self.result
            .clone()
            .unwrap_or_else(|| self.finish(PlayOutcome::PassIncomplete, self.los, "No play.".to_string()))
    }

    pub fn step(&mut self, dt: f32) -> Option<&PlayResult> {
        if self.result.is_some() {
            return self.result.as_ref();
        }
        let dt = dt.min(0.05);
        self.t += dt;
        for p in self.players.iter_mut() {
            p.tackle_cooldown = (p.tackle_cooldown - dt).max(0.0);
            if p.stun > 0.0 {
                p.stun = (p.stun - dt).max(0.0);
            }
            if p.beaten > 0.0 {
                p.beaten = (p.beaten - dt).max(0.0);
            }
            // A stride is one pair of steps of a fixed length, so the cycle is
            // advanced by the distance covered rather than by a frame rate. This
            // keeps the feet gripping the turf at any speed and holds the cadence
            // in a range a person could run in.
            p.run_phase = (p.run_phase + p.vel.length() * dt / STRIDE_YARDS).rem_euclid(1.0);
        }

        if self.phase == Phase::PreSnap {
            // Quick pre-snap shuffle so the broadcast shot has life.
            for i in 0..14 {
                let jitter = Vec2::new(0.0, (self.t * 1.7 + i as f32).sin() * 0.02);
                self.players[i].pos += jitter;
            }
            if self.t >= SNAP_DELAY {
                self.snap();
            }
            return None;
        }

        self.update_offense(dt);
        self.update_defense(dt);
        self.integrate(dt);
        self.resolve_blocks(dt);
        self.resolve_tackles(dt);
        self.update_ball(dt);
        if self.result.is_none() && self.t > SNAP_DELAY + MAX_PLAY_TIME {
            let msg = "The play dies after an eternity of scrambling.".to_string();
            let spot = self.players[self.qb].pos.x;
            self.finish(PlayOutcome::PassIncomplete, self.los, msg);
            let _ = spot;
        }
        self.result.as_ref()
    }

    fn snap(&mut self) {
        self.phase = match self.play.family {
            PlayFamily::QuickPass | PlayFamily::DropbackPass | PlayFamily::Screen => Phase::Dropback,
            PlayFamily::PlayAction => Phase::Dropback,
            _ => Phase::Run,
        };
        self.ball.holder = Some(self.qb);
        self.events.push(SimEvent { t: self.t, text: "Snap".to_string() });
    }

    // -----------------------------------------------------------------
    // Offense
    // -----------------------------------------------------------------

    fn update_offense(&mut self, dt: f32) {
        // The mesh point. The handoff is scheduled after the snap, so it has to be
        // completed here rather than at the snap itself: a run play gives the ball
        // to the back on its own beat, while a play-action fake schedules one too
        // and the quarterback keeps it.
        if self.carrier.is_none() && self.play.family.is_run() {
            if let Some((idx, at)) = self.handoff {
                if self.t >= at && !self.players[idx].down {
                    self.carrier = Some(idx);
                    self.ball.holder = Some(idx);
                    let name = self.name_of(idx);
                    self.events.push(SimEvent {
                        t: self.t,
                        text: format!("{name} takes the handoff"),
                    });
                }
            }
        }
        for i in 0..7 {
            if self.players[i].down || self.players[i].stun > 0.0 {
                continue;
            }
            let role = OffRole::ALL[self.players[i].role_index];
            let is_qb = role == OffRole::Qb;
            if is_qb && self.handoff.is_some() {
                // Sell the fake and mesh with the back.
                let (car, _) = self.handoff.unwrap();
                let target = self.players[car].pos + Vec2::new(-self.dirf() * 0.9, 0.0);
                let spd = self.speed_cap(i) * 0.5;
                self.steer(i, target, spd, dt);
                continue;
            }
            if Some(i) == self.carrier {
                // Whoever has the ball answers the stick — the quarterback on a
                // scramble, the back on a handoff, the receiver after a catch.
                // With the stick at rest he coasts instead of being handed back
                // to the AI, so the runner never cuts away under the player.
                match self.control {
                    Some(ctl) if ctl.move_dir.length() > 0.18 => {
                        self.update_carrier_human(i, dt, ctl)
                    }
                    Some(_) => self.update_carrier_idle(i, dt),
                    None => self.update_carrier(i, dt),
                }
            } else if is_qb && self.ball.holder == Some(i) {
                self.update_qb(i, dt);
            } else {
                self.update_off_other(i, dt);
            }
        }
    }

    fn update_qb(&mut self, i: usize, dt: f32) {
        if let Some(ctl) = self.control {
            self.update_qb_human(i, dt, ctl);
            return;
        }
        let pressure = self.qb_pressure();
        match self.phase {
            Phase::Dropback => {
                let drop = self.play.drop.max(1.0);
                let target = Vec2::new(
                    self.los - self.dirf() * drop,
                    self.players[i].pos.y * 0.85,
                );
                let spd = self.speed_cap(i) * if self.t < SNAP_DELAY + 0.7 { 0.85 } else { 0.6 };
                self.steer(i, target, spd, dt);
                self.read_timer += dt;
                if self.t > SNAP_DELAY + 0.7 && self.read_timer > 0.22 {
                    self.read_timer = 0.0;
                    self.maybe_throw(i, pressure);
                }
                if self.result.is_none() && self.t > SNAP_DELAY + 5.0 {
                    self.start_scramble(i);
                }
            }
            Phase::Scramble => {
                self.update_carrier(i, dt);
            }
            _ => {}
        }
    }

    /// Quarterback under human control: the stick moves him, the throw button
    /// releases it to the locked-on receiver, and crossing the line of
    /// scrimmage turns the dropback into a scramble.
    fn update_qb_human(&mut self, i: usize, dt: f32, ctl: QbControl) {
        let mag = ctl.move_dir.length().min(1.0);
        if mag > 0.18 {
            let target = self.players[i].pos + ctl.move_dir.normalize_or_zero() * 3.5;
            let spd = self.speed_cap(i) * (0.5 + 0.5 * mag);
            self.steer(i, target, spd, dt);
        } else {
            // Standing in the pocket, drifting to the spot the play asks for.
            let anchor = Vec2::new(
                self.los - self.dirf() * self.play.drop.max(1.0),
                self.players[i].pos.y * 0.9,
            );
            self.steer(i, anchor, self.speed_cap(i) * 0.35, dt);
        }

        let past_los = (self.players[i].pos.x - self.los) * self.dirf() > 0.0;
        if past_los && self.phase == Phase::Dropback && self.t > SNAP_DELAY + 0.4 {
            self.start_scramble(i);
            return;
        }

        if ctl.throw && self.phase == Phase::Dropback && self.t > SNAP_DELAY + 0.25 {
            if let Some(target) = ctl.target {
                if !self.players[target].down {
                    self.throw_to(i, Some(target));
                }
            }
        }
    }

    fn qb_pressure(&self) -> f32 {
        let mut best = 99.0f32;
        for i in 7..14 {
            if self.players[i].down {
                continue;
            }
            let d = self.players[i].pos.dist(self.players[self.qb].pos);
            if d < best {
                best = d;
            }
        }
        best
    }

    fn maybe_throw(&mut self, qb: usize, pressure: f32) {
        let (best, score) = self.best_target(qb);
        // In tight space (goal to go) there is no such thing as a wide open window:
        // the quarterback has to fit it in.
        let tight = self.setup.to_go <= 6.0;
        let bar = if tight {
            0.95
        } else if pressure < 2.4 {
            1.45
        } else {
            1.15
        };
        let desperate = self.t > SNAP_DELAY + 3.4;
        if let Some(target) = best {
            if score > bar || (desperate && score > 0.5) {
                self.throw_to(qb, Some(target));
                return;
            }
        }
        if self.t > SNAP_DELAY + 4.4 {
            // Out of options.
            let outside_hashes = (self.players[qb].pos.y - self.setup.ball_y).abs() > 8.0;
            if outside_hashes {
                self.throw_to(qb, None);
            } else {
                self.start_scramble(qb);
            }
        }
    }

    /// Score every eligible receiver; returns the best and its openness score.
    fn best_target(&self, _qb: usize) -> (Option<usize>, f32) {
        let mut best = None;
        let mut best_score = -10.0;
        let arm = self.attrs(self.qb).arm_strength();
        let need = self.setup.to_go;
        let late_down = self.setup.down >= 3;
        for &e in &self.eligible_indices() {
            if self.players[e].down {
                continue;
            }
            let air = ((self.players[e].pos.x - self.los) * self.dirf()).max(0.0);
            if air > arm + 6.0 {
                continue;
            }
            let sep = self.nearest_defender_dist(e);
            // Reward separation, and reward progress: a checkdown that gains two
            // yards is not the same throw as a dig at twelve.
            let mut score = (sep / 3.0).min(1.0) * 1.15;
            score += (air / 22.0).min(0.65);
            if air > 26.0 {
                score -= 0.30;
            }
            if sep < 1.1 {
                score -= 0.85;
            }
            if sep > 3.5 {
                score += 0.30;
            }
            if late_down {
                // On third and fourth down, the sticks matter.
                if air >= need {
                    score += 0.45;
                } else {
                    score -= 0.40;
                }
            }
            // A receiver who hasn't uncovered yet isn't a target on a deep route.
            let depth = self.play_route_depth(e) as f32;
            if depth > 14.0 && self.t < SNAP_DELAY + 1.1 {
                score -= 0.8;
            }
            if score > best_score {
                best_score = score;
                best = Some(e);
            }
        }
        (best, best_score)
    }

    fn nearest_defender_dist(&self, i: usize) -> f32 {
        let mut best = 99.0;
        for d in 7..14 {
            if self.players[d].down {
                continue;
            }
            let dist = self.players[d].pos.dist(self.players[i].pos);
            if dist < best {
                best = dist;
            }
        }
        best
    }

    fn throw_to(&mut self, qb: usize, target: Option<usize>) {
        let qb_pos = self.players[qb].pos;

        let (to, air, receiver) = match target {
            Some(t) => {
                let receiver = self.players[t].pos;
                let air_yards = ((receiver.x - qb_pos.x) * self.dirf()).max(1.0);
                // Lead the receiver based on how long the ball is in the air.
                let arm = self.attrs(qb).arm_strength();
                let speed = 17.0 + arm * 0.5;
                let dur = (air_yards / speed).clamp(0.3, 2.4);
                let lead = self.players[t].vel * dur * 0.75;
                let accuracy = self.attrs(qb).accuracy_factor();
                let pressure = self.qb_pressure();
                let sigma = 0.22 + air_yards * 0.026 + (1.0 - accuracy) * 1.5
                    + (2.5 - pressure).max(0.0) * 0.35;
                let err = Vec2::new(self.rng.gauss() * sigma, self.rng.gauss() * sigma * 0.85);
                (
                    receiver + lead + err,
                    air_yards,
                    Some(t),
                )
            }
            None => {
                // Throw it away: outside the hashes and past the line of scrimmage.
                let side = if self.players[qb].pos.y >= self.setup.ball_y { 1.0 } else { -1.0 };
                let spot = Vec2::new(
                    self.los + self.dirf() * 9.0,
                    clamp(self.setup.ball_y + side * 17.0, -FIELD_HALF_WIDTH + 0.5, FIELD_HALF_WIDTH - 0.5),
                );
                self.throw_away = true;
                (spot, 9.0, None)
            }
        };

        let dur = (air / (17.0 + self.attrs(qb).arm_strength() * 0.5)).clamp(0.35, 2.4);
        let peak = 0.7 + air * 0.18;
        self.ball.flight = Some(Flight {
            from: qb_pos,
            to: Vec2::new(to.x, clamp(to.y, -FIELD_HALF_WIDTH, FIELD_HALF_WIDTH)),
            t: 0.0,
            duration: dur,
            peak,
            target: receiver,
            thrower: qb,
            air_yards: air,
        });
        self.ball.holder = None;
        self.ball.pos = qb_pos;
        self.ball.z = 1.5;
        self.intended = receiver;
        self.phase = Phase::BallInAir;
        self.events.push(SimEvent {
            t: self.t,
            text: format!("{} lets it go", self.name_of(qb)),
        });
    }

    fn start_scramble(&mut self, qb: usize) {
        if self.phase == Phase::Scramble || self.result.is_some() {
            return;
        }
        self.phase = Phase::Scramble;
        self.carrier = Some(qb);
        self.ball.holder = Some(qb);
        self.scramble_started = true;
        self.events.push(SimEvent {
            t: self.t,
            text: format!("{} takes off", self.name_of(qb)),
        });
    }

    fn update_carrier(&mut self, i: usize, dt: f32) {
        let target = self.carry_target(i);
        let speed = self.speed_cap(i);
        self.carry_step(i, dt, target, speed);
    }

    fn update_carrier_human(&mut self, i: usize, dt: f32, ctl: QbControl) {
        let target = self.players[i].pos + ctl.move_dir.normalize_or_zero() * 4.0;
        let speed = self.speed_cap(i) * ctl.move_dir.length().clamp(0.55, 1.0);
        self.carry_step(i, dt, target, speed);
    }

    /// Carrier with the stick at rest: no AI lanes and no surprise cuts, just a
    /// runner running out of steam on his current line.
    fn update_carrier_idle(&mut self, i: usize, dt: f32) {
        let p = &self.players[i];
        let ahead = if p.vel.length() > 0.4 {
            p.vel.normalize_or_zero()
        } else {
            Vec2::new(self.dirf(), 0.0)
        };
        let target = p.pos + ahead * 1.1;
        let speed = self.speed_cap(i) * 0.4;
        self.carry_step(i, dt, target, speed);
    }

    fn carry_step(&mut self, i: usize, dt: f32, target: Vec2, speed: f32) {
        self.steer(i, target, speed, dt);
        self.ball.pos = self.players[i].pos;
        self.ball.z = 1.0;
        self.ball.holder = Some(i);

        // Goal line, sidelines and the back of the end zone.
        let pos = self.players[i].pos;
        let goal = field::opponent_goal_side(self.dir);
        let crossed = if self.dir > 0 { pos.x >= goal } else { pos.x <= goal };
        if crossed {
            let end = field::spot_from_yards_to_goal(0.0, self.dir);
            let desc = format!(
                "{} gets to the corner and into the end zone!",
                self.name_of(i)
            );
            self.finish(PlayOutcome::Touchdown, end, desc);
            return;
        }
        if pos.y.abs() > FIELD_HALF_WIDTH {
            let desc = format!("{} is pushed out of bounds.", self.name_of(i));
            self.finish(PlayOutcome::Run, pos.x, desc);
            self.mark_out_of_bounds();
        }
    }

    fn mark_out_of_bounds(&mut self) {
        if let Some(r) = self.result.as_mut() {
            r.out_of_bounds = true;
        }
    }

    fn carry_target(&self, i: usize) -> Vec2 {
        let p = &self.players[i];
        let dirf = self.dirf();
        let mut base = if p.waypoint < p.route.len() {
            p.route[p.waypoint]
        } else {
            // Head for the end zone, aiming at an open lane.
            let goal = field::opponent_goal_side(self.dir);
            let mut lane = 0.0f32;
            for d in 7..14 {
                if self.players[d].down {
                    continue;
                }
                let rel = self.players[d].pos - p.pos;
                if rel.x * dirf > 0.0 && rel.length() < 7.0 {
                    lane -= rel.y.signum() * (7.0 - rel.length()) * 0.35;
                }
            }
            Vec2::new(goal + dirf * 3.0, clamp(p.pos.y + lane, -FIELD_HALF_WIDTH + 1.0, FIELD_HALF_WIDTH - 1.0))
        };
        // Look for the nearest threat and lean away from it.
        let mut evade = Vec2::ZERO;
        for d in 7..14 {
            if self.players[d].down || self.players[d].stun > 0.0 {
                continue;
            }
            let rel = self.players[d].pos - p.pos;
            let dist = rel.length();
            if dist < 3.4 && rel.dot(Vec2::new(dirf, 0.0)) > -0.5 {
                evade += rel.normalize_or_zero() * (3.4 - dist) * 0.9;
            }
        }
        if evade.length() > 0.1 {
            base += Vec2::new(evade.x * 0.15, evade.y * 1.1);
        }
        base
    }

    fn update_off_other(&mut self, i: usize, dt: f32) {
        let role = OffRole::ALL[self.players[i].role_index];
        let assignment = self.play.assignment(role).cloned();
        let p = self.players[i].pos;
        let dirf = self.dirf();

        // Ball in the air: receivers attack the landing spot, blockers find work.
        if let Some(flight) = self.ball.flight.clone() {
            let is_target = flight.target == Some(i);
            let distance = (flight.to.x - p.x).abs();
            if is_target || (role.is_eligible() && distance < 10.0) {
                let speed = self.speed_cap(i);
                self.steer(i, flight.to, speed, dt);
                return;
            }
        }

        match assignment {
            Some(Assign::Route { depth, .. }) => {
                if self.players[i].waypoint < self.players[i].route.len() {
                    let target = self.players[i].route[self.players[i].waypoint];
                    let spd = self.speed_cap(i) * route_pace(depth);
                    self.steer(i, target, spd, dt);
                    if self.players[i].pos.dist(target) < 0.55 {
                        self.players[i].waypoint += 1;
                    }
                } else {
                    // Settle down and find the quarterback's eyes.
                    let qb = self.players[self.qb].pos;
                    let settle = Vec2::new(
                        p.x - dirf * 0.4,
                        p.y + if qb.y > p.y { 0.4 } else { -0.4 },
                    );
                    let spd = self.speed_cap(i) * 0.45;
                    self.steer(i, settle, spd, dt);
                }
            }
            Some(Assign::Fake { aim, .. }) => {
                if self.t < SNAP_DELAY + 0.8 {
                    let target = Vec2::new(self.los + dirf * aim.x, self.setup.ball_y + aim.y);
                    let spd = self.speed_cap(i) * 0.8;
                    self.steer(i, target, spd, dt);
                } else if self.players[i].waypoint < self.players[i].route.len() {
                    let target = self.players[i].route[self.players[i].waypoint];
                    let spd = self.speed_cap(i) * 0.9;
                    self.steer(i, target, spd, dt);
                    if self.players[i].pos.dist(target) < 0.55 {
                        self.players[i].waypoint += 1;
                    }
                }
            }
            Some(Assign::PassBlock { slide, .. }) => {
                let pocket = self.pocket_anchor(slide);
                let target = self.block_target(i, pocket);
                let spd = self.speed_cap(i) * 0.85;
                self.steer(i, target, spd, dt);
            }
            Some(Assign::RunBlock { aim, .. }) | Some(Assign::ScreenBlock { aim }) => {
                let aim_point = Vec2::new(self.los + dirf * aim.x, self.setup.ball_y + aim.y);
                let target = self.block_target(i, aim_point);
                let spd = self.speed_cap(i) * 0.95;
                self.steer(i, target, spd, dt);
            }
            _ => {
                // Nothing on: drift and look busy.
                let spd = self.speed_cap(i) * 0.4;
                self.steer(i, p + Vec2::new(dirf * 2.0, 0.0), spd, dt);
            }
        }
    }

    fn pocket_anchor(&self, slide: f32) -> Vec2 {
        let qb = self.players[self.qb].pos;
        Vec2::new(qb.x - self.dirf() * 0.6, qb.y + slide * 1.1)
    }

    /// Blockers mirror the nearest threat to their aim point.
    fn block_target(&self, i: usize, aim: Vec2) -> Vec2 {
        let mut best: Option<(usize, f32)> = None;
        for d in 7..14 {
            if self.players[d].down || self.players[d].stun > 0.0 {
                continue;
            }
            let pos = self.players[d].pos;
            let dist = pos.dist(aim);
            if dist < 7.0 {
                let closer = best.map(|(_, b)| dist < b).unwrap_or(true);
                if closer {
                    best = Some((d, dist));
                }
            }
        }
        match best {
            Some((d, _)) => {
                let threat = self.players[d].pos;
                let to_threat = (threat - self.players[i].pos).normalize_or_zero();
                // Take the defender on the upfield shoulder.
                threat - to_threat * 0.35
            }
            None => aim,
        }
    }

    // -----------------------------------------------------------------
    // Defense
    // -----------------------------------------------------------------

    fn update_defense(&mut self, dt: f32) {
        let carrier = self.carrier;
        let ball_landing = self.ball.flight.as_ref().map(|f| f.to);
        for i in 7..14 {
            if self.players[i].down || self.players[i].stun > 0.0 {
                continue;
            }
            if self.players[i].beaten > 0.0 {
                // Blown past his man, keep chasing the quarterback.
                let qb = self.players[self.qb].pos;
                let spd = self.speed_cap(i);
                self.steer(i, qb, spd, dt);
                continue;
            }
            let job = self.players[i].job;
            match job {
                CoverageJob::Rush => {
                    if let Some(c) = carrier {
                        let target = self.players[c].pos;
                        let spd = self.speed_cap(i);
                        self.steer(i, target, spd, dt);
                    } else {
                        let qb = self.players[self.qb].pos;
                        // Aim at the quarterback's upfield shoulder.
                        let target = qb + Vec2::new(self.dirf() * 0.4, 0.0);
                        let spd = self.speed_cap(i);
                        self.steer(i, target, spd, dt);
                    }
                }
                CoverageJob::Man(man) => {
                    if let Some(landing) = ball_landing {
                        let spd = self.speed_cap(i);
                        self.steer(i, landing, spd, dt);
                    } else if let Some(c) = carrier.filter(|&c| c != man) {
                        // Rally to the ball on a run or scramble.
                        let dist = self.players[man].pos.dist(self.players[c].pos);
                        if dist > 2.2 {
                            let target = self.players[c].pos;
                            let spd = self.speed_cap(i) * 0.95;
                            self.steer(i, target, spd, dt);
                        } else {
                            self.play_man(i, man, dt);
                        }
                    } else {
                        self.play_man(i, man, dt);
                    }
                }
                CoverageJob::DeepZone { y, depth } => {
                    if let Some(landing) = ball_landing {
                        let spd = self.speed_cap(i);
                        self.steer(i, landing, spd, dt);
                    } else if let Some(c) = carrier {
                        let rel = (self.players[c].pos - self.players[i].pos).length();
                        if rel < 8.0 || self.play.family.is_run() {
                            let target = self.players[c].pos;
                            let spd = self.speed_cap(i);
                            self.steer(i, target, spd, dt);
                        } else {
                            self.hold_zone(i, y, depth, dt);
                        }
                    } else {
                        self.hold_zone(i, y, depth, dt);
                    }
                }
                CoverageJob::UnderZone { y, depth } => {
                    if let Some(landing) = ball_landing {
                        let spd = self.speed_cap(i);
                        self.steer(i, landing, spd, dt);
                    } else if let Some(c) = carrier {
                        let target = self.players[c].pos;
                        let spd = self.speed_cap(i);
                        self.steer(i, target, spd, dt);
                    } else {
                        self.hold_zone(i, y, depth, dt);
                    }
                }
                CoverageJob::Spy(target) => {
                    let pos = self.players[target].pos;
                    let spd = self.speed_cap(i) * 0.9;
                    self.steer(i, pos, spd, dt);
                }
            }
        }
    }

    fn play_man(&mut self, i: usize, man: usize, dt: f32) {
        let a = self.attrs(i);
        let awareness = a.awareness_factor();
        // Trail the receiver, with a reaction delay baked into the aim point.
        let lead = 0.10 + (1.0 - awareness) * 0.22;
        let man_pos = self.players[man].pos;
        let man_vel = self.players[man].vel;
        let aim = man_pos + man_vel * lead;
        let to_qb = self.players[self.qb].pos - aim;
        let target = aim + to_qb.normalize_or_zero() * 0.35;
        let spd = self.speed_cap(i);
        self.steer(i, target, spd, dt);
    }

    fn hold_zone(&mut self, i: usize, y: f32, depth: f32, dt: f32) {
        // Read the nearest receiver in the zone and lean that way.
        let anchor = Vec2::new(
            self.los + self.dirf() * depth,
            self.setup.ball_y + y,
        );
        let mut adjust = Vec2::ZERO;
        let mut best = 6.0f32;
        for &e in &self.eligible_indices() {
            let p = self.players[e].pos;
            let d = p.dist(anchor);
            if d < best {
                best = d;
                adjust = (p - anchor) * 0.45;
            }
        }
        let target = anchor + adjust;
        let spd = self.speed_cap(i) * 0.8;
        self.steer(i, target, spd, dt);
    }

    // -----------------------------------------------------------------
    // Physics / contact
    // -----------------------------------------------------------------

    fn steer(&mut self, i: usize, target: Vec2, max_speed: f32, dt: f32) {
        let accel = self.accel_of(i);
        let turn = self.turn_of(i);
        let p = &mut self.players[i];
        if p.engaged.is_some() {
            // Blocked players shuffle rather than run.
            p.vel *= 0.55;
            p.pos += p.vel * dt;
            return;
        }
        let to = target - p.pos;
        let dist = to.length();
        let cap = if dist < 0.45 { max_speed * (dist / 0.45) } else { max_speed };
        let desired = to.normalize_or_zero() * cap;
        let dv = desired - p.vel;
        p.vel += dv.clamp_len(accel * dt);
        p.pos += p.vel * dt;
        if p.vel.length() > 0.4 {
            p.heading = approach_angle(p.heading, p.vel.angle(), turn * dt);
        }
    }

    fn integrate(&mut self, dt: f32) {
        for i in 0..14 {
            let p = &mut self.players[i];
            if p.down {
                p.vel = Vec2::ZERO;
                continue;
            }
            p.pos += Vec2::ZERO; // movement already applied in steer
            p.stamina = (p.stamina - p.vel.length() * dt * 0.006).max(0.42);
            // Keep bodies on the planet.
            p.pos.y = clamp(p.pos.y, -FIELD_HALF_WIDTH - 1.5, FIELD_HALF_WIDTH + 1.5);
        }
        // Simple separation so bodies never merge.
        for a in 0..14 {
            for b in (a + 1)..14 {
                if self.players[a].down || self.players[b].down {
                    continue;
                }
                let delta = self.players[b].pos - self.players[a].pos;
                let d = delta.length();
                if d < 0.55 && d > 1e-4 {
                    let push = delta.normalize_or_zero() * (0.55 - d) * 0.5;
                    self.players[a].pos -= push;
                    self.players[b].pos += push;
                }
            }
        }
    }

    fn resolve_blocks(&mut self, dt: f32) {
        for i in 0..7 {
            if self.players[i].down || self.players[i].stun > 0.0 {
                continue;
            }
            let role = OffRole::ALL[self.players[i].role_index];
            let is_blocker = matches!(
                self.play.assignment(role),
                Some(Assign::PassBlock { .. })
                    | Some(Assign::RunBlock { .. })
                    | Some(Assign::ScreenBlock { .. })
            ) || (role != OffRole::Qb && self.carrier.is_some() && self.play.family.is_run());
            if !is_blocker {
                continue;
            }
            // Find a defender to lean on.
            let mut target = None;
            let mut best = 1.35f32;
            for d in 7..14 {
                if self.players[d].down || self.players[d].stun > 0.0 || self.players[d].beaten > 0.0 {
                    continue;
                }
                let dist = self.players[d].pos.dist(self.players[i].pos);
                if dist < best {
                    best = dist;
                    target = Some(d);
                }
            }
            let Some(d) = target else {
                self.players[i].engaged = None;
                continue;
            };
            self.players[i].engaged = Some(d);
            self.players[d].engaged = Some(i);

            let blocker = self.attrs(i);
            let defender = self.attrs(d);
            let pass_rush = (defender.pass_rush as f32 - blocker.pass_block as f32) / 100.0;
            let run_stuff = (defender.strength as f32 - blocker.run_block as f32) / 100.0;
            let advantage = pass_rush.max(run_stuff);
            let noise = self.rng.gauss() * 0.5;
            self.players[d].pressure += dt * (0.55 + advantage * 1.8) + noise * dt * 0.4;

            // Grind along the line: the defender is steered toward the QB, the
            // blocker mirrors him.
            let qb = self.players[self.qb].pos;
            let toward = (qb - self.players[d].pos).normalize_or_zero();
            let slide = toward.perp();
            let drift = slide * (advantage * 0.9 + 0.15) * dt;
            self.players[d].pos += drift;
            self.players[i].pos -= drift * 0.9;

            if self.players[d].pressure > 1.0 {
                self.players[d].pressure = 0.0;
                self.players[d].beaten = 0.0;
                self.players[d].engaged = None;
                self.players[i].engaged = None;
                self.players[i].stun = 0.45;
                // A hold while getting beaten is the classic A7FL 10-yard foul.
                if self.rng.chance(0.012) {
                    let mut call = PenaltyCall::new(PenaltyKind::Holding, self.players[i].pos.x);
                    call.player = Some(self.roster_index(i));
                    self.penalty(call);
                    return;
                }
            }

            // Illegal block in the back on run blocks (roughly one every few games).
            if self.play.family.is_run() && self.rng.chance(0.0035 * dt) {
                let mut call = PenaltyCall::new(PenaltyKind::IllegalBlockInTheBack, self.players[i].pos.x);
                call.player = Some(self.roster_index(i));
                self.penalty(call);
                return;
            }
        }
    }

    fn resolve_tackles(&mut self, _dt: f32) {
        let holder = self.ball.holder;
        // A quarterback still holding the ball in the pocket is a live target: this
        // is where sacks come from.
        let carrier = match self.carrier {
            Some(c) => c,
            None => {
                if holder == Some(self.qb) && matches!(self.phase, Phase::Dropback) {
                    self.qb
                } else {
                    return;
                }
            }
        };
        if self.players[carrier].down {
            return;
        }
        // Only defenders near the ball carrier can attempt a tackle.
        let mut attempts: Vec<(usize, f32)> = Vec::new();
        for d in 7..14 {
            if self.players[d].down || self.players[d].stun > 0.0 || self.players[d].tackle_cooldown > 0.0 {
                continue;
            }
            let dist = self.players[d].pos.dist(self.players[carrier].pos);
            if dist <= TACKLE_RADIUS {
                attempts.push((d, dist));
            }
        }
        attempts.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (d, _) in attempts {
            let carrier_attrs = self.attrs(carrier);
            let defender_attrs = self.attrs(d);
            // Wrap tackling: the A7FL requires an actual attempt to wrap up.
            let closing = (self.players[d].vel - self.players[carrier].vel).length();
            let mut p = 0.30
                + (defender_attrs.tackle_factor() - carrier_attrs.break_tackle_factor()) * 0.55
                + (closing * 0.02).min(0.15);
            p = clamp(p, 0.08, 0.93);
            self.players[d].tackle_cooldown = 0.28;

            if !self.rng.chance(p) {
                // Missed tackle: the defender is left grasping.
                self.players[d].stun = 0.65;
                self.players[carrier].vel *= 0.9;
                self.events.push(SimEvent {
                    t: self.t,
                    text: format!("{} breaks the tackle!", self.name_of(carrier)),
                });
                continue;
            }

            // A7FL: a tackle above the nameplate is unnecessary roughness.
            let high_tackle =
                self.rng.chance(0.02 * (1.2 - defender_attrs.discipline as f32 / 100.0));
            let spot = clamp(
                self.players[carrier].pos.x,
                -field::ENDZONE_DEPTH,
                FIELD_LENGTH + field::ENDZONE_DEPTH,
            );
            let fumble_chance = 0.012
                + (1.0 - carrier_attrs.break_tackle_factor()) * 0.022
                + (1.0 - carrier_attrs.awareness_factor()) * 0.012;
            let fumbled = self.rng.chance(fumble_chance);
            let scramble = self.scramble_started && carrier == self.qb;
            let sack = !self.pass_caught
                && !scramble
                && holder == Some(self.qb)
                && self.play.family.is_pass();
            let tackler_roster = self.roster_index(d);
            self.finish_tackle(carrier, Some(d));

            if high_tackle {
                let mut call = PenaltyCall::new(PenaltyKind::Unsportsmanlike, spot);
                call.player = Some(tackler_roster);
                self.penalty(call);
                return;
            }
            if fumbled {
                self.fumble(carrier, d);
                return;
            }

            let outcome = if sack {
                PlayOutcome::Sack
            } else if self.pass_caught {
                PlayOutcome::PassComplete
            } else if scramble {
                PlayOutcome::QbScramble
            } else {
                PlayOutcome::Run
            };
            let yards = (spot - self.los) * self.dirf();
            let desc = match outcome {
                PlayOutcome::Sack => format!(
                    "{} wraps up {} for the sack.",
                    self.name_of(d),
                    self.name_of(carrier)
                ),
                PlayOutcome::PassComplete => format!(
                    "{} hauls it in and is brought down by {} after {} yards.",
                    self.name_of(carrier),
                    self.name_of(d),
                    yards.round() as i32
                ),
                PlayOutcome::QbScramble => format!(
                    "{} scrambles for {} before {} drags him down.",
                    self.name_of(carrier),
                    yards.round() as i32,
                    self.name_of(d)
                ),
                _ => format!(
                    "{} stops {} after {} yards.",
                    self.name_of(d),
                    self.name_of(carrier),
                    yards.round() as i32
                ),
            };
            let mut r = self.finish(outcome, spot, desc);
            r.tackler = Some(tackler_roster);
            self.result = Some(r);
            return;
        }
    }

    fn finish_tackle(&mut self, carrier: usize, _tackler: Option<usize>) {
        self.players[carrier].down = true;
        self.players[carrier].vel = Vec2::ZERO;
        self.ball.holder = None;
    }

    fn fumble(&mut self, carrier: usize, defender: usize) {
        let spot = self.players[carrier].pos;
        // Recoveries favour the pile nearest the ball.
        let offense_closer = (0..7)
            .filter(|&i| i != carrier)
            .map(|i| self.players[i].pos.dist(spot))
            .fold(99.0f32, f32::min)
            < self.players[defender].pos.dist(spot);
        let offense_recovers = self.rng.chance(if offense_closer { 0.62 } else { 0.38 });
        let was_own_endzone = field::is_in_own_endzone(spot.x, self.dir);
        if was_own_endzone {
            let desc = format!("{} recovers the fumble in the end zone for a safety!", self.name_of(defender));
            self.finish(PlayOutcome::Safety, spot.x, desc);
            return;
        }
        if offense_recovers {
            let desc = format!("The ball squirts loose and {} falls on it.", self.name_of(carrier));
            self.finish(PlayOutcome::FumbleRecovered, spot.x, desc);
        } else {
            let desc = format!("{} punches it out and recovers!", self.name_of(defender));
            self.finish(PlayOutcome::FumbleLost, spot.x, desc);
            if let Some(r) = self.result.as_mut() {
                r.turnover = true;
            }
        }
    }

    // -----------------------------------------------------------------
    // Ball flight
    // -----------------------------------------------------------------

    fn update_ball(&mut self, dt: f32) {
        let Some(mut flight) = self.ball.flight.clone() else {
            // Ball in somebody's hands.
            if let Some(h) = self.ball.holder {
                if !self.players[h].down {
                    self.ball.pos = self.players[h].pos;
                    self.ball.z = 1.0;
                }
            }
            return;
        };
        flight.t += dt;
        let progress = (flight.t / flight.duration).clamp(0.0, 1.0);
        self.ball.pos = flight.from.lerp(flight.to, progress);
        self.ball.z = lerp(1.5, 1.0, progress) + flight.peak * (std::f32::consts::PI * progress).sin();
        if progress < 1.0 {
            self.ball.flight = Some(flight);
            return;
        }
        self.ball.flight = None;
        self.resolve_catch(&flight);
    }

    fn resolve_catch(&mut self, flight: &Flight) {
        let landing = flight.to;
        let target = flight.target;
        self.catch_spot = landing.x;
        self.ball.pos = landing;
        self.ball.z = 1.0;

        // Contest: the closest defender to the landing spot.
        let mut contest: Option<(usize, f32)> = None;
        for d in 7..14 {
            if self.players[d].down {
                continue;
            }
            let dist = self.players[d].pos.dist(landing);
            let closer = contest.map(|(_, b)| dist < b).unwrap_or(true);
            if closer {
                contest = Some((d, dist));
            }
        }

        // Pass interference: contact before the ball arrives.
        if let (Some(t), Some((d, dist))) = (target, contest) {
            if dist < 1.15 {
                let defender_aware = self.attrs(d).awareness_factor();
                let pi_chance = 0.06 * (1.0 - defender_aware * 0.6);
                if self.rng.chance(pi_chance) {
                    let mut call = PenaltyCall::new(PenaltyKind::DefensivePassInterference, landing.x);
                    call.player = Some(self.roster_index(d));
                    self.penalty(call);
                    return;
                }
            }
            let _ = t;
        }

        // Interception: a defender right on the spot can steal it.
        if let Some((d, dist)) = contest {
            if dist < CATCH_RADIUS * 0.95 {
                let cov = self.attrs(d).coverage_factor();
                let catching = self.attrs(d).catching as f32 / 100.0;
                let pick_chance = (0.035 + cov * 0.11 + catching * 0.06)
                    * if target.is_some() { 1.0 } else { 2.2 };
                if self.rng.chance(pick_chance) {
                    let return_yards = self.rng.range(0.0, 9.0);
                    let end = clamp(landing.x - self.dirf() * return_yards, 0.0, FIELD_LENGTH);
                    let desc = format!("{} steps in front and picks it off!", self.name_of(d));
                    let tackler = Some(self.roster_index(d));
                    let mut r = self.finish(PlayOutcome::Interception, end, desc);
                    r.turnover = true;
                    r.tackler = tackler;
                    r.return_yards = return_yards;
                    self.result = Some(r);
                    return;
                }
            }
        }

        // Catch or incompletion.
        if let Some(t) = target {
            let separation = contest.map(|(_, d)| d).unwrap_or(6.0);
            let catch_factor = self.attrs(t).catch_factor();
            let contesting = (separation < 1.4) as i32 as f32;
            let accuracy_penalty = ((self.players[t].pos.dist(landing) - 1.0).max(0.0) * 0.35).min(0.5);
            // Short throws are easier: less air time for the defence to close.
            let short_bonus = if (self.los - landing.x).abs() < 10.0 { 0.10 } else { 0.0 };
            let p = clamp(
                0.16 + catch_factor * 0.42 + (separation * 0.05).min(0.15)
                    - contesting * 0.30
                    - accuracy_penalty
                    + short_bonus,
                0.08,
                0.95,
            );
            if self.rng.chance(p) {
                // Caught.
                let receiver = t;
                self.carrier = Some(receiver);
                self.intended = None;
                self.pass_caught = true;
                self.catch_spot = self.players[receiver].pos.x;
                self.ball.holder = Some(receiver);
                self.phase = Phase::Run;
                let air = ((self.players[receiver].pos.x - self.los) * self.dirf()).max(0.0);
                self.events.push(SimEvent {
                    t: self.t,
                    text: format!("{} pulls it in for {} air yards", self.name_of(receiver), air.round() as i32),
                });
                return;
            }
        }

        // Incomplete.
        let (outcome, desc) = if self.throw_away {
            (
                PlayOutcome::ThrowAway,
                format!("{} throws it away.", self.name_of(flight.thrower)),
            )
        } else {
            (
                PlayOutcome::PassIncomplete,
                "Incomplete. Nobody could come down with it.".to_string(),
            )
        };
        let receiver = self.intended.map(|i| self.roster_index(i));
        let passer = Some(self.roster_index(flight.thrower));
        let air = flight.air_yards;
        let mut r = self.finish(outcome, self.los, desc);
        r.receiver = receiver;
        r.air_yards = air;
        r.passer = passer;
        self.result = Some(r);
    }

    // -----------------------------------------------------------------
    // Finishing
    // -----------------------------------------------------------------

    fn penalty(&mut self, call: PenaltyCall) {
        let outcome = PlayOutcome::Penalty;
        let desc = format!("Flag on the play: {}.", call.kind.label().to_lowercase());
        let spot = self.los;
        let mut result = self.finish(outcome, spot, desc);
        result.penalty = Some(call);
        self.result = Some(result);
    }

    fn finish(&mut self, outcome: PlayOutcome, end_spot: f32, description: String) -> PlayResult {
        let yards = (end_spot - self.los) * self.dirf();
        let score_play = outcome == PlayOutcome::Touchdown;
        let safety = outcome == PlayOutcome::Safety;
        let elapsed = (self.t - SNAP_DELAY).max(0.0);
        let mut result = PlayResult {
            outcome,
            yards: if score_play {
                field::yards_to_goal(self.los, self.dir)
            } else if safety {
                -field::yards_to_goal(self.los, self.dir)
            } else {
                yards
            },
            end_spot,
            elapsed,
            touchdown: score_play,
            safety,
            turnover: matches!(outcome, PlayOutcome::Interception | PlayOutcome::FumbleLost),
            incomplete: outcome == PlayOutcome::PassIncomplete,
            out_of_bounds: false,
            sack: outcome == PlayOutcome::Sack,
            air_yards: 0.0,
            penalty: None,
            passer: None,
            receiver: None,
            carrier: self.carrier.map(|i| self.roster_index(i)),
            tackler: None,
            ejection: None,
            return_yards: 0.0,
            description,
            highlight: score_play
                || matches!(outcome, PlayOutcome::Interception | PlayOutcome::FumbleLost)
                || yards.abs() > 18.0,
        };
        if let Some(c) = self.carrier {
            if outcome == PlayOutcome::PassComplete {
                result.receiver = Some(self.roster_index(c));
            }
            result.carrier = Some(self.roster_index(c));
        }
        if let Some(f) = &self.ball.flight {
            result.air_yards = f.air_yards;
            result.passer = Some(self.roster_index(f.thrower));
        }
        // Anyone who threw the ball counts as the passer, even on a sack.
        if matches!(
            self.play.family,
            PlayFamily::QuickPass | PlayFamily::DropbackPass | PlayFamily::PlayAction | PlayFamily::Screen
        ) && result.passer.is_none()
        {
            result.passer = Some(self.roster_index(self.qb));
        }
        self.phase = Phase::Dead;
        self.result = Some(result.clone());
        result
    }

    // -----------------------------------------------------------------
    // Renderer helpers
    // -----------------------------------------------------------------

    /// Hand this snap to a human quarterback, or take it back with `None`.
    pub fn set_control(&mut self, control: Option<QbControl>) {
        self.control = control;
    }

    /// Eligible receivers with the index to drive [`QbControl::target`].
    pub fn eligible_targets(&self) -> Vec<(OffRole, usize)> {
        self.eligible_indices()
            .into_iter()
            .map(|i| (OffRole::ALL[self.players[i].role_index], i))
            .collect()
    }

    /// Yards to the nearest defender: what a play-call screen calls "open".
    pub fn separation(&self, receiver: usize) -> f32 {
        self.nearest_defender_dist(receiver)
    }

    pub fn qb_pos(&self) -> Vec2 {
        self.players[self.qb].pos
    }

    pub fn is_snapped(&self) -> bool {
        !matches!(self.phase, Phase::PreSnap)
    }

    pub fn ball_position(&self) -> (Vec2, f32) {
        (self.ball.pos, self.ball.z)
    }

    pub fn scoreboard_spot(&self) -> f32 {
        if self.result.is_some() {
            self.result.as_ref().map(|r| r.end_spot).unwrap_or(self.los)
        } else if self.carrier.is_some() {
            self.players[self.carrier.unwrap()].pos.x
        } else {
            self.ball.pos.x
        }
    }
}

fn route_pace(depth: u8) -> f32 {
    if depth >= 18 {
        1.0
    } else if depth >= 10 {
        0.96
    } else {
        0.86
    }
}

/// Convenience: run one snap to completion.
pub fn simulate_play(
    offense: &Team,
    defense: &Team,
    play: &Play,
    dcall: &DefCall,
    setup: &SnapSetup,
) -> PlayResult {
    let mut sim = PlaySim::new(offense, defense, play, dcall, setup);
    sim.run_to_completion()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playbook::{defensive_playbook, playbook};
    use crate::rules::DOWNS;
    use crate::team::league_teams;

    fn teams() -> (Team, Team) {
        let t = league_teams();
        (t[0].clone(), t[1].clone())
    }

    fn setup(los: f32, down: u8, to_go: f32, seed: u64) -> SnapSetup {
        SnapSetup {
            los,
            ball_y: 0.0,
            direction: 1,
            down,
            to_go,
            seed,
            offense_team: 0,
            defense_team: 1,
            is_try: false,
        }
    }

    #[test]
    fn a_snap_produces_a_legal_result() {
        let (off, def_) = teams();
        let book = playbook();
        let d = defensive_playbook();
        let mut plays = 0;
        for seed in 0..40u64 {
            let play = &book[(seed as usize) % book.len()];
            let dcall = &d[(seed as usize) % d.len()];
            let r = simulate_play(&off, &def_, play, dcall, &setup(40.0, 2, 8.0, seed));
            assert!(r.end_spot >= -10.0 && r.end_spot <= 110.0, "spot {}", r.end_spot);
            assert!(r.elapsed >= 0.0);
            assert!(r.yards.is_finite());
            assert!(!r.description.is_empty());
            plays += 1;
        }
        assert_eq!(plays, 40);
    }

    #[test]
    fn every_formation_and_family_runs_without_panicking() {
        let (off, def_) = teams();
        let book = playbook();
        let d = defensive_playbook();
        for (pi, play) in book.iter().enumerate() {
            for (di, dcall) in d.iter().enumerate() {
                let r = simulate_play(
                    &off,
                    &def_,
                    play,
                    dcall,
                    &setup(35.0, 1, 10.0, (pi * 31 + di) as u64),
                );
                assert!(
                    r.end_spot.is_finite() && r.elapsed.is_finite(),
                    "{} vs {} broke the sim",
                    play.name,
                    dcall.name
                );
            }
        }
    }

    #[test]
    fn statistical_sanity_over_many_plays() {
        let (off, def_) = teams();
        let book = playbook();
        let d = defensive_playbook();
        let mut runs = 0;
        let mut pass_att = 0;
        let mut completions = 0;
        let mut sacks = 0;
        let mut tds = 0;
        let mut turnover = 0;
        let mut yards_total = 0.0;
        let n = 600u64;
        for seed in 0..n {
            let play = &book[(seed as usize * 7) % book.len()];
            let dcall = &d[(seed as usize * 3) % d.len()];
            let r = simulate_play(&off, &def_, play, dcall, &setup(45.0, 1, 10.0, seed));
            yards_total += r.yards;
            if play.family.is_run() {
                runs += 1;
            } else {
                pass_att += 1;
            }
            if r.outcome.is_completion() {
                completions += 1;
            }
            if r.sack {
                sacks += 1;
            }
            if r.touchdown {
                tds += 1;
            }
            if r.turnover {
                turnover += 1;
            }
        }
        let comp_pct = completions as f32 / pass_att as f32;
        let avg = yards_total / n as f32;
        let sack_pct = sacks as f32 / pass_att as f32;
        assert!(runs > 0 && pass_att > 0);
        assert!(
            (0.25..0.80).contains(&comp_pct),
            "completion rate implausible: {comp_pct}"
        );
        assert!((-3.0..12.0).contains(&avg), "average yards implausible: {avg}");
        assert!(sack_pct < 0.35, "sacks too common: {sack_pct}");
        assert!(tds > 0, "no touchdowns in 600 plays from the 45");
        assert!(turnover > 0, "no turnovers in 600 plays");
        assert_eq!(runs + pass_att, n as i32);
        let _ = DOWNS;
    }

    #[test]
    fn goal_line_plays_score_sometimes_and_never_spot_outside_the_field() {
        let (off, def_) = teams();
        let book = playbook();
        let d = defensive_playbook();
        let mut scores = 0;
        for seed in 0..80u64 {
            let play = &book[(seed as usize) % book.len()];
            let dcall = &d[(seed as usize) % d.len()];
            let r = simulate_play(&off, &def_, play, dcall, &setup(97.0, 1, 3.0, seed));
            assert!(r.end_spot <= 110.0);
            if r.touchdown {
                scores += 1;
            }
        }
        assert!(scores > 0, "nobody ever scored from the 3");
    }
}

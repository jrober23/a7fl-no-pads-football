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
use crate::moves::{CarrierMove, Kind};
use crate::rules::{PenaltyCall, PenaltyKind};
use crate::team::{Attributes, Team};

pub const TACKLE_RADIUS: f32 = 0.95;

/// How long the wrap takes: the arms go on him, the feet drive, and the two of
/// them go to the turf. Contact is not the end of the play. A tackle that
/// resolved on the frame the defender arrived was a teleport with a sound effect:
/// there was nothing on the field to see, and the next screen was already coming.
pub const WRAP_SECONDS: f32 = 0.42;
pub const CATCH_RADIUS: f32 = 1.25;
pub const SNAP_DELAY: f32 = 0.85;
pub const MAX_PLAY_TIME: f32 = 13.0;
/// How long a runner may go without gaining a yard before the play is dead. A
/// man who is still moving the ball is playing; a man who is not is stuck in a
/// pile, and the clock is the wrong thing to measure him by.
pub const STALL_SECONDS: f32 = 3.6;
/// The absolute backstop for a run, long enough for a run the length of the field
/// at a jog. Only a physics bug should ever reach it.
pub const RUN_PLAY_LIMIT: f32 = 40.0;
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
    /// A defender got a hand in the way and knocked the ball down before it
    /// arrived, rather than the throw simply missing its man.
    pub batted_down: bool,
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

impl PlayResult {
    /// The word the picture puts over a dead ball: what just happened, in the one
    /// word a broadcast says first. The card can explain it afterwards; while the
    /// players are still getting up, this is the whole caption.
    pub fn headline(&self) -> Option<&'static str> {
        if self.touchdown {
            return Some("TOUCHDOWN");
        }
        if self.safety {
            return Some("SAFETY");
        }
        if self.outcome == PlayOutcome::Interception {
            return Some("INTERCEPTION");
        }
        if self.sack {
            return Some("SACK");
        }
        if self.batted_down {
            return Some("PASS BATTED DOWN");
        }
        if self.turnover {
            return Some("TURNOVER");
        }
        if !self.incomplete && self.yards < -0.5 {
            return Some("TACKLE FOR LOSS");
        }
        if self.incomplete {
            return Some("INCOMPLETE");
        }
        None
    }
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

/// A tackle in progress.
///
/// The hit has landed and the result is already decided, but the ball is not dead
/// until the two men are on the ground: the defender closes and wraps, the carrier
/// goes over the top of it, and only then is the play over.
#[derive(Clone, Debug)]
pub struct Wrap {
    pub carrier: usize,
    pub tackler: usize,
    /// Seconds since the arms went on him.
    pub t: f32,
    pub outcome: PlayOutcome,
    pub spot: f32,
    pub description: String,
    /// Where the game clock stopped: at the contact, not at the landing.
    pub elapsed: f32,
    pub tackler_roster: usize,
    pub fumbled: bool,
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
    /// 0..1 through a tackle: a defender driving one home, or the man going down
    /// under it. The picture animates from these; the sim decides when they pass 1.
    pub wrap: f32,
    pub tackled: f32,
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
    /// How fast the ball left the hand, in yards per second. The picture does not
    /// need it; it is the honest measure of how hard a throw was, and it is what
    /// "too fast" means.
    pub speed: f32,
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
    /// How hard the ball is thrown, 0..1: the pass meter. The AI ignores this
    /// because it picks its own weight for every throw.
    pub power: f32,
    /// Where to put the ball relative to the receiver, in yards: `x` down the
    /// field, `y` across it. Zero aims at the receiver's numbers, which is what
    /// the AI does.
    pub aim: Vec2,
}

/// Live human input for a defender, set by an interactive client.
///
/// Movement is the stick; the buttons are the only way a player can make a tackle,
/// a pass breakup or a pick happen on his own man rather than waiting for the
/// coordinator's dice. Absent during ordinary AI play, so `sim` and `film` behave
/// exactly as they always have.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefControl {
    /// Stick direction in world space, `x` down the field and `y` across it.
    pub move_dir: Vec2,
    /// One frame of the tackle button.
    pub tackle: bool,
    /// One frame of the swat button: knock the ball down.
    pub swat: bool,
    /// One frame of the pick button: go and take it away.
    pub intercept: bool,
}

/// What a throw of a given weight, bent off a receiver, would do. The interactive
/// client draws its aim guide from this, so the guide and the ball cannot
/// disagree about where the pass is going.
#[derive(Clone, Copy, Debug)]
pub struct ThrowPlan {
    pub landing: Vec2,
    pub air_yards: f32,
    /// Yards from the passer to the spot being asked for.
    pub distance: f32,
    /// How far a throw of this weight actually carries.
    pub reach: f32,
    /// Meter reading that would just reach the spot.
    pub power_needed: f32,
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
    /// `Some` when a human is driving a defender.
    pub def_control: Option<DefControl>,
    /// Which defender the pad is on, as an index into `players`.
    pub human_defender: Option<usize>,
    /// The snap waits for the button while this is set. An offense at the line
    /// decides when the ball goes, so sitting in the audible cannot run the play
    /// out from under him.
    awaiting_snap: bool,
    /// Where every man stood when the formation was set. The pre-snap shuffle is
    /// an offset from these, so a long look at the defence cannot walk the whole
    /// formation down the field.
    presnap: Vec<Vec2>,
    /// Furthest downfield the ball has been this snap, and how long it has been
    /// since it last got further. This is what decides that a run has stalled.
    best_spot: f32,
    stall: f32,
    /// The move the carrier is in the middle of, if somebody asked for one.
    carrier_moves: CarrierMove,
    pub events: Vec<SimEvent>,
    read_timer: f32,
    catch_spot: f32,
    scramble_started: bool,
    throw_away: bool,
    pass_caught: bool,
    possession_start: Option<usize>,
    /// A flag on the field. A live-ball foul does not stop anything: the play runs
    /// out and the ball is walked off once it is dead, so this rides along until
    /// the result is built.
    pub flag: Option<PenaltyCall>,
    /// Sim time the flag came in, so the picture can pulse it.
    pub flag_at: f32,
    /// A tackle in progress: the hit is on and the two men are going down.
    pub wrap: Option<Wrap>,
    /// Whether a defender has already had his chance at a ball in the air, so one
    /// throw is not swatted at on every frame of its flight.
    pass_contacted: bool,
    /// What the game clock stopped at, when the ball died before the wrap finished.
    elapsed_override: Option<f32>,
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
            let a = play.alignment(role);
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
                wrap: 0.0,
                tackled: 0.0,
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
                wrap: 0.0,
                tackled: 0.0,
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
            def_control: None,
            human_defender: None,
            awaiting_snap: false,
            presnap: Vec::new(),
            best_spot: f32::NEG_INFINITY,
            stall: 0.0,
            carrier_moves: CarrierMove::default(),
            throw_away: false,
            pass_caught: false,
            possession_start: None,
            flag: None,
            flag_at: 0.0,
            wrap: None,
            pass_contacted: false,
            elapsed_override: None,
        };
        sim.ball.pos = sim.players[0].pos;
        sim.build_routes();
        sim.assign_coverage();
        sim.capture_presnap();
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

    /// Put every man back where his assignment says he lines up. Used when an
    /// audible changes the play at the line.
    fn line_up(&mut self) {
        let dirf = self.dirf();
        for i in 0..7 {
            let role = OffRole::ALL[self.players[i].role_index];
            let a = self.play.alignment(role);
            let p = &mut self.players[i];
            p.pos = Vec2::new(
                self.setup.los + dirf * a.x,
                clamp(self.setup.ball_y + a.y, -FIELD_HALF_WIDTH + 0.6, FIELD_HALF_WIDTH - 0.6),
            );
            p.vel = Vec2::ZERO;
            p.heading = if self.setup.direction > 0 { 0.0 } else { std::f32::consts::PI };
            p.waypoint = 0;
            p.route.clear();
        }
        for i in 7..14 {
            let role = DefRole::ALL[self.players[i].role_index];
            let a = defensive_alignment(
                role,
                self.def.coverage,
                self.setup.los,
                self.setup.direction,
                self.play.run_side,
            );
            let p = &mut self.players[i];
            p.pos = Vec2::new(
                a.x,
                clamp(self.setup.ball_y + a.y, -FIELD_HALF_WIDTH + 0.8, FIELD_HALF_WIDTH - 0.8),
            );
            p.vel = Vec2::ZERO;
            p.heading = if self.setup.direction > 0 {
                std::f32::consts::PI
            } else {
                0.0
            };
            p.waypoint = 0;
            p.route.clear();
            p.pressure = 0.0;
            p.beaten = 0.0;
            p.engaged = None;
        }
    }

    fn capture_presnap(&mut self) {
        self.presnap = self.players.iter().map(|p| p.pos).collect();
    }

    fn build_routes(&mut self) {
        for i in 0..7 {
            let role = OffRole::ALL[self.players[i].role_index];
            let align = self.play.alignment(role);
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
            // Quick pre-snap shuffle so the broadcast shot has life. It is an
            // offset from where the formation was set, not a nudge applied every
            // frame: with the snap waiting on a button the old version walked the
            // whole offense sideways while the player read the defence.
            for i in 0..14 {
                let jitter = Vec2::new(0.0, (self.t * 1.7 + i as f32).sin() * 0.02);
                let base = self.presnap.get(i).copied().unwrap_or(self.players[i].pos);
                self.players[i].pos = base + jitter;
            }
            if self.awaiting_snap {
                // Hold at the line: every timing constant in the play is measured
                // from SNAP_DELAY, so the clock stops here and starts again when
                // the player asks for the ball.
                self.t = self.t.min(SNAP_DELAY);
            } else if self.t >= SNAP_DELAY {
                self.snap();
            }
            return None;
        }

        // A tackle in flight: the two men are still going to the ground, so
        // nothing else on the field moves and the ball is not dead until they are.
        if self.wrap.is_some() {
            self.advance_wrap(dt);
            return self.result.as_ref();
        }

        self.update_offense(dt);
        self.update_defense(dt);
        self.integrate(dt);
        self.resolve_blocks(dt);
        self.resolve_tackles(dt);
        self.update_ball(dt);
        // A runner who keeps gaining ground is not a play that has gone stale.
        //
        // The old rule was a flat thirteen seconds from the snap, and it booked
        // whatever was happening as an incomplete pass at the line of scrimmage.
        // So a run that broke through the defense and was still going at thirteen
        // seconds came back as "the play dies after an eternity of scrambling",
        // the whole gain was thrown away, and the next down started where this one
        // had. What makes a play dead is the ball going nowhere, not the clock.
        if self.result.is_none() && self.carrier.is_some() && self.face_buttons_are_moves() {
            let progress = (self.ball.pos.x - self.los) * self.dirf();
            if progress > self.best_spot + 0.75 {
                self.best_spot = progress;
                self.stall = 0.0;
            } else {
                self.stall += dt;
            }
        } else {
            self.stall = 0.0;
        }
        if self.result.is_none() {
            let running = self.carrier.is_some() && self.face_buttons_are_moves();
            let limit = if running { RUN_PLAY_LIMIT } else { MAX_PLAY_TIME };
            let stalled = running && self.stall > STALL_SECONDS;
            if stalled || self.t > SNAP_DELAY + limit {
                self.abandon_play(stalled);
            }
        }
        self.result.as_ref()
    }

    /// End a play that has run out of ground or time, and book it as what it
    /// actually was. A runner caught from behind after forty yards gets forty
    /// yards; he does not get an incomplete pass at the line of scrimmage.
    fn abandon_play(&mut self, stalled: bool) {
        let Some(h) = self.ball.holder else {
            let msg = "The throw hangs up there and comes down to nobody.".to_string();
            self.finish(PlayOutcome::PassIncomplete, self.los, msg);
            return;
        };
        let spot = clamp(
            self.players[h].pos.x,
            -field::ENDZONE_DEPTH,
            FIELD_LENGTH + field::ENDZONE_DEPTH,
        );
        let gain = (spot - self.los) * self.dirf();
        let name = self.name_of(h);
        let running = self.carrier == Some(h) && self.face_buttons_are_moves();
        if running {
            let outcome = if self.pass_caught {
                PlayOutcome::PassComplete
            } else if self.scramble_started && h == self.qb {
                PlayOutcome::QbScramble
            } else {
                PlayOutcome::Run
            };
            let msg = if gain < -0.5 {
                format!(
                    "{name} is caught {} yards behind the line.",
                    (-gain).round() as i32
                )
            } else if stalled {
                format!("{name} is caught from behind after {} yards.", gain.round() as i32)
            } else {
                format!(
                    "{name} finally runs out of field after {} yards.",
                    gain.round() as i32
                )
            };
            self.finish(outcome, spot, msg);
        } else {
            // Still in the pocket with nowhere to go: a sack in all but name, and
            // it should cost him the ground he lost getting there.
            let msg = format!("{name} is hauled down before he can get rid of it.");
            self.finish(PlayOutcome::Sack, spot, msg);
        }
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

        // Taking off does not end the play: a quarterback on the move can still
        // let it go downfield, which is half of what the scramble is for.
        let can_throw = self.phase == Phase::Dropback
            || (self.phase == Phase::Scramble && self.carrier == Some(i));
        if ctl.throw && can_throw && self.t > SNAP_DELAY + 0.25 {
            if let Some(target) = ctl.target {
                if !self.players[target].down {
                    self.throw_to_with(i, Some(target), ctl.power, ctl.aim);
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
        self.throw_to_with(qb, target, 0.8, Vec2::ZERO);
    }

    /// What a throw of this weight, bent this far off a receiver, would do.
    ///
    /// The thrower's arm sets how far the ball can carry: the meter is a range,
    /// so a soft throw dies in front of the man and a hard one carries over his
    /// head. Leading him is automatic, and `aim` is on top of that, which is what
    /// lets a player throw somebody open.
    pub fn throw_plan(&self, qb: usize, target: usize, power: f32, aim: Vec2) -> ThrowPlan {
        let qb_pos = self.players[qb].pos;
        let power = power.clamp(0.0, 1.0);
        let arm = self.attrs(qb).arm_strength();
        let receiver = self.players[target].pos;
        let speed = 17.0 + arm * 0.5;
        let rough = (receiver - qb_pos).length().max(0.1);
        let lead = self.players[target].vel * (rough / speed).clamp(0.25, 2.4) * 0.75;
        let wanted = receiver + lead + aim;
        let offset = wanted - qb_pos;
        let distance = offset.length();
        let reach = 9.0 + (arm * 0.42 + 8.0) * power;
        let landing = qb_pos + offset.normalize_or_zero() * distance.min(reach);
        let landing = Vec2::new(
            clamp(landing.x, -field::ENDZONE_DEPTH, FIELD_LENGTH + field::ENDZONE_DEPTH),
            clamp(landing.y, -FIELD_HALF_WIDTH, FIELD_HALF_WIDTH),
        );
        let air_yards = ((landing.x - qb_pos.x) * self.dirf()).max(0.0);
        let power_needed = ((distance - 9.0) / (arm * 0.42 + 8.0)).clamp(0.0, 1.0);
        ThrowPlan {
            landing,
            air_yards,
            distance,
            reach,
            power_needed,
        }
    }

    fn throw_to_with(&mut self, qb: usize, target: Option<usize>, power: f32, aim: Vec2) {
        // A new throw is a new chance for somebody to get a hand on it.
        self.pass_contacted = false;
        let qb_pos = self.players[qb].pos;
        let power = power.clamp(0.0, 1.0);

        let (to, air, receiver) = match target {
            Some(t) => {
                let plan = self.throw_plan(qb, t, power, aim);
                let accuracy = self.attrs(qb).accuracy_factor();
                let pressure = self.qb_pressure();
                // A soft throw hangs and drifts; a rope arrives flat and can run
                // past the man. Either way the ball goes where the arm sent it.
                let sigma = 0.20
                    + plan.air_yards * 0.024
                    + (1.0 - accuracy) * 1.4
                    + (2.5 - pressure).max(0.0) * 0.30
                    + (1.0 - power * power) * 0.6;
                let err = Vec2::new(self.rng.gauss() * sigma, self.rng.gauss() * sigma * 0.85);
                (plan.landing + err, plan.air_yards, Some(t))
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

        let arm = self.attrs(qb).arm_strength();
        // The meter is a range of passes, not one pass thrown harder. A tap is a
        // floater: slow enough to watch, high enough to come down over a linebacker,
        // and it hangs long enough that underthrowing a man is a real mistake. Held
        // all the way down it is a rope. Squaring the meter keeps the soft end soft,
        // which is the half of the range a hand actually plays in, and keeps the arm
        // from turning every throw into a fastball — the old formula added the whole
        // arm rating to the speed, so a strong quarterback could not throw softly at
        // all.
        let touch = power * power;
        let speed = 7.0 + 22.0 * touch + arm * 0.10;
        // Time the flight on the grass the ball actually has to cover. Timing it on
        // the downfield component alone made a swing pass to the flat, which covers
        // twelve yards sideways and two downfield, arrive like a fastball -- and a
        // floater over the middle is the whole point of the soft end of the meter.
        let carry = (to - qb_pos).length().max(1.0);
        let dur = (carry / speed).clamp(0.35, 3.0);
        let peak = 0.45 + carry * 0.12 + (1.0 - touch) * 1.9;
        self.ball.flight = Some(Flight {
            from: qb_pos,
            to: Vec2::new(to.x, clamp(to.y, -FIELD_HALF_WIDTH, FIELD_HALF_WIDTH)),
            t: 0.0,
            duration: dur,
            peak,
            target: receiver,
            thrower: qb,
            air_yards: air,
            speed,
        });
        self.ball.holder = None;
        self.ball.pos = qb_pos;
        self.ball.z = 1.5;
        self.carrier = None;
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

    /// What the carrier is in the middle of doing, for the client's overlay.
    pub fn carrier_move(&self) -> CarrierMove {
        self.carrier_moves
    }

    /// True when the man with the ball cannot throw it any more, which frees the
    /// face buttons to be his moves. A quarterback behind the line still passes;
    /// a runner, a receiver after the catch, and a quarterback past the line have
    /// nothing to do with the ball but run with it.
    ///
    /// This asks the ball, not the sim's own idea of who the carrier is: the
    /// carrier field is promoted a frame after a handoff or a scramble, and while
    /// it lagged, a man was visibly running with the ball while the buttons were
    /// still showing him his receivers.
    pub fn face_buttons_are_moves(&self) -> bool {
        let Some(holder) = self.ball.holder else { return false };
        if self.ball.flight.is_some() || self.result.is_some() || self.players[holder].down {
            return false;
        }
        // Only a quarterback behind the line still has a pass to throw.
        holder != self.qb || (self.players[holder].pos.x - self.los) * self.dirf() > 0.0
    }

    /// The human runner asks for a move. Ignored unless he actually has the ball
    /// in his hands, so a mash of the face buttons during the mesh, in the pocket
    /// or after the throw cannot do anything.
    pub fn request_move(&mut self, kind: Kind) {
        // The man with the ball is the man the buttons belong to. Anything else
        // lets a live-looking button do nothing, which is how a dead button feels.
        let Some(holder) = self.ball.holder else { return };
        if self.ball.flight.is_some() || self.result.is_some() || !self.face_buttons_are_moves() {
            return;
        }
        let vel = self.players[holder].vel;
        let facing = if vel.length() > 0.4 { vel } else { Vec2::new(self.dirf(), 0.0) };
        if !self.carrier_moves.start(kind, facing) {
            return;
        }
        // A move is a run, so the sim has to be treating him as the runner too.
        self.carrier = Some(holder);
        let name = self.name_of(holder);
        let text = match kind {
            Kind::Spin => format!("{name} spins out"),
            Kind::Hurdle => format!("{name} goes over the top"),
            Kind::Dive => format!("{name} dives forward"),
            Kind::Slide => format!("{name} slides down"),
        };
        self.events.push(SimEvent { t: self.t, text });
    }

    fn carry_step(&mut self, i: usize, dt: f32, target: Vec2, speed: f32) {
        // A move takes the stick away for as long as it runs: a dive goes where it
        // was aimed, not where the stick is a frame later.
        let mut target = target;
        let mut speed = speed;
        if let Some(kind) = self.carrier_moves.kind() {
            speed *= self.carrier_moves.speed_scale();
            if !kind.steerable() {
                target = self.players[i].pos + self.carrier_moves.dir() * 2.6;
            }
        }
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
            return;
        }

        // The end of a dive or a slide is the end of the play.
        if let Some(kind) = self.carrier_moves.step(dt) {
            if kind.commits() {
                let spot = self.players[i].pos.x;
                let name = self.name_of(i);
                let desc = match kind {
                    Kind::Slide => format!("{name} slides down and gives himself up."),
                    _ => format!("{name} dives forward and is down."),
                };
                self.finish(PlayOutcome::Run, spot, desc);
            }
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
            // The defender on the pad answers the stick; the rest of the eleven
            // are the coordinator's. With the stick at rest he keeps working to
            // the ball rather than standing still, so handing the pad around
            // never freezes a man on the field.
            if Some(i) == self.human_defender {
                if let Some(ctl) = self.def_control {
                    let mag = ctl.move_dir.length().min(1.0);
                    if mag > 0.18 {
                        let target = self.players[i].pos + ctl.move_dir.normalize_or_zero() * 4.0;
                        let spd = self.speed_cap(i) * (0.6 + 0.4 * mag);
                        self.steer(i, target, spd, dt);
                    } else {
                        let aim = if let Some(f) = self.ball.flight.as_ref() {
                            f.to
                        } else if let Some(c) = carrier {
                            self.players[c].pos
                        } else {
                            self.players[self.qb].pos
                        };
                        self.steer(i, aim, self.speed_cap(i) * 0.4, dt);
                    }
                    continue;
                }
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
                }
            }

            // Illegal block in the back on run blocks (roughly one every few games).
            if self.play.family.is_run() && self.rng.chance(0.0035 * dt) {
                let mut call = PenaltyCall::new(PenaltyKind::IllegalBlockInTheBack, self.players[i].pos.x);
                call.player = Some(self.roster_index(i));
                self.penalty(call);
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
        // Only defenders near the ball carrier can attempt a tackle. The man on
        // the pad reaches a little further, and when he presses the button he gets
        // the first shot: a tackle the player asked for cannot be taken off him by
        // a defender the coordinator happens to like more.
        let mut attempts: Vec<(usize, f32)> = Vec::new();
        for d in 7..14 {
            if self.players[d].down || self.players[d].stun > 0.0 || self.players[d].tackle_cooldown > 0.0 {
                continue;
            }
            let reach = if Some(d) == self.human_defender {
                TACKLE_RADIUS * 1.3
            } else {
                TACKLE_RADIUS
            };
            let dist = self.players[d].pos.dist(self.players[carrier].pos);
            if dist <= reach {
                attempts.push((d, dist));
            }
        }
        attempts.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        if let (Some(h), Some(ctl)) = (self.human_defender, self.def_control) {
            if ctl.tackle {
                if let Some(pos) = attempts.iter().position(|(d, _)| *d == h) {
                    let him = attempts.remove(pos);
                    attempts.insert(0, him);
                }
            }
        }
        for (d, _) in attempts {
            let carrier_attrs = self.attrs(carrier);
            let defender_attrs = self.attrs(d);
            // Wrap tackling: the A7FL requires an actual attempt to wrap up.
            let closing = (self.players[d].vel - self.players[carrier].vel).length();
            let mut p = 0.30
                + (defender_attrs.tackle_factor() - carrier_attrs.break_tackle_factor()) * 0.55
                + (closing * 0.02).min(0.15);
            // A man in the air cannot be wrapped up: the attempt goes under him and
            // he is left grasping at the turf.
            if self.carrier_moves.is_hurdling() {
                self.players[d].stun = 0.55;
                self.players[d].tackle_cooldown = 0.30;
                self.events.push(SimEvent {
                    t: self.t,
                    text: format!(
                        "{} hurdles {}!",
                        self.name_of(carrier),
                        self.name_of(d)
                    ),
                });
                continue;
            }
            // A spin is a smaller window to hit, and a dive gives up some of the
            // body to the hit in exchange for the yard.
            p *= self.carrier_moves.tackle_scale();
            p = clamp(p, 0.08, 0.93);
            self.players[d].tackle_cooldown = 0.28;

            if !self.rng.chance(p) {
                // Missed tackle: the defender is left grasping.
                self.players[d].stun = 0.65;
                self.players[carrier].vel *= 0.9;
                let text = if self.carrier_moves.kind() == Some(Kind::Spin) {
                    format!(
                        "{} spins out of {}'s tackle!",
                        self.name_of(carrier),
                        self.name_of(d)
                    )
                } else {
                    format!("{} breaks the tackle!", self.name_of(carrier))
                };
                self.events.push(SimEvent { t: self.t, text });
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

            if high_tackle {
                let mut call = PenaltyCall::new(PenaltyKind::Unsportsmanlike, spot);
                call.player = Some(tackler_roster);
                self.penalty(call);
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
                _ if yards < -0.5 => format!(
                    "{} drops {} {} yards behind the line.",
                    self.name_of(d),
                    self.name_of(carrier),
                    (-yards).round() as i32
                ),
                _ => format!(
                    "{} stops {} after {} yards.",
                    self.name_of(d),
                    self.name_of(carrier),
                    yards.round() as i32
                ),
            };
            // The hit is a wrap, not a teleport: the defender has to get his arms
            // around the man and ride him down, and the ball is not dead until the
            // two of them are on the turf.
            self.start_wrap(carrier, d, outcome, spot, desc, tackler_roster, fumbled);
            return;
        }
    }

    /// The first wrap of a tackle.
    ///
    /// Everything the play will become is decided here — the spot, the sack, the
    /// lost fumble — and then held: contact is not the end of the play, and a card
    /// that comes up on the frame the defender arrives is a card over a man still
    /// standing. The wrap runs for [`WRAP_SECONDS`] and the ball dies on the turf.
    #[allow(clippy::too_many_arguments)]
    fn start_wrap(
        &mut self,
        carrier: usize,
        tackler: usize,
        outcome: PlayOutcome,
        spot: f32,
        description: String,
        tackler_roster: usize,
        fumbled: bool,
    ) {
        self.players[carrier].vel = Vec2::ZERO;
        self.players[carrier].tackled = 0.0;
        self.players[tackler].wrap = 0.0;
        // He goes down away from the hit, not through it. A man hit square in the
        // chest turns off the contact; the figure falls along its own facing, so
        // the facing is what has to be turned. Hit from behind or the side, he
        // keeps running into the fall, which is also what it looks like.
        let away = self.players[carrier].pos - self.players[tackler].pos;
        if away.length() > 0.05 {
            let facing = Vec2::new(
                self.players[carrier].heading.cos(),
                self.players[carrier].heading.sin(),
            );
            if facing.dot(away.normalize_or_zero()) > 0.3 {
                self.players[carrier].heading = away.y.atan2(away.x);
            }
        }
        self.wrap = Some(Wrap {
            carrier,
            tackler,
            t: 0.0,
            outcome,
            spot,
            description,
            elapsed: (self.t - SNAP_DELAY).max(0.0),
            tackler_roster,
            fumbled,
        });
        let text = format!(
            "{} wraps up {}!",
            self.name_of(tackler),
            self.name_of(carrier)
        );
        self.events.push(SimEvent { t: self.t, text });
    }

    /// Drive the tackle home: the defender closes and wraps, the carrier goes over
    /// the top of it, and the ball is dead when they are both on the turf.
    fn advance_wrap(&mut self, dt: f32) {
        let Some(mut w) = self.wrap.clone() else {
            return;
        };
        w.t += dt;
        let p = (w.t / WRAP_SECONDS).clamp(0.0, 1.0);
        self.players[w.carrier].tackled = p;
        self.players[w.carrier].vel = Vec2::ZERO;
        self.players[w.tackler].wrap = p;
        let spot = self.players[w.carrier].pos;
        // He closes to arm's length and stays there. Driving him to the carrier's
        // exact spot put two bodies inside each other, which renders as one man
        // made of both of them.
        let away = (spot - self.players[w.tackler].pos).normalize_or_zero();
        let contact = spot - away * 0.44;
        let d = &mut self.players[w.tackler];
        d.pos = d.pos.lerp(contact, (dt * 7.0).min(1.0));
        if away.length() > 0.01 {
            d.heading = away.y.atan2(away.x);
        }
        if p < 1.0 {
            self.wrap = Some(w);
            return;
        }
        self.wrap = None;
        self.players[w.tackler].wrap = 0.0;
        self.players[w.tackler].tackle_cooldown = 0.40;
        self.finish_tackle(w.carrier, Some(w.tackler));
        // The clock stopped on contact; the wrap was picture, not time played.
        self.elapsed_override = Some(w.elapsed);
        if w.fumbled {
            self.fumble(w.carrier, w.tackler);
            return;
        }
        let mut r = self.finish(w.outcome, w.spot, w.description);
        r.tackler = Some(w.tackler_roster);
        self.result = Some(r);
    }

    /// The defender closest to a ball in the air, if he is close enough to reach
    /// it. A man cannot get a hand on one over his head, which is what the caller
    /// checks before it asks.
    fn in_the_way(&self) -> Option<(usize, f32)> {
        let mut best: Option<(usize, f32)> = None;
        for d in 7..14 {
            let p = &self.players[d];
            if p.down || p.stun > 0.0 {
                continue;
            }
            let dist = p.pos.dist(self.ball.pos);
            if dist > 1.1 {
                continue;
            }
            if best.map(|(_, b)| dist < b).unwrap_or(true) {
                best = Some((d, dist));
            }
        }
        best
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
        // The man on the pad gets his own shot at a ball in the air. A defensive
        // back who reads it and presses pick or swat does not have to hope the
        // automatic contest rolls his way; he has to be near the ball and he has
        // to be there at the right time, and he gets one look at it.
        if let (Some(d), Some(ctl)) = (self.human_defender, self.def_control) {
            if (ctl.intercept || ctl.swat)
                && !self.pass_contacted
                && !self.players[d].down
                && progress > 0.10
                && progress < 0.99
            {
                let dist = self.players[d].pos.dist(self.ball.pos);
                if dist < 2.1 && self.ball.z < 3.4 {
                    self.pass_contacted = true;
                    let cov = self.attrs(d).coverage_factor();
                    let hands = self.attrs(d).catching as f32 / 100.0;
                    // Right on it is a play; a lunging reach is a half chance.
                    let close = if dist < 0.85 { 1.0 } else { 0.5 };
                    let spot = self.ball.pos.x;
                    if ctl.intercept {
                        let pick = (0.06 + cov * 0.14 + hands * 0.10) * close;
                        if self.rng.chance(pick) {
                            let desc =
                                format!("{} jumps in front and picks it off!", self.name_of(d));
                            let mut r = self.finish(PlayOutcome::Interception, spot, desc);
                            r.turnover = true;
                            r.tackler = Some(self.roster_index(d));
                            r.air_yards = flight.air_yards;
                            r.passer = Some(self.roster_index(flight.thrower));
                            r.receiver = flight.target.map(|t| self.roster_index(t));
                            r.highlight = true;
                            self.result = Some(r);
                            self.ball.flight = None;
                            self.catch_spot = spot;
                            return;
                        }
                    }
                    if ctl.swat {
                        let bat = (0.16 + cov * 0.20 + hands * 0.10) * close;
                        if self.rng.chance(bat) {
                            let desc =
                                format!("{} gets a hand in and bats it down!", self.name_of(d));
                            let mut r = self.finish(PlayOutcome::PassIncomplete, self.los, desc);
                            r.batted_down = true;
                            r.air_yards = flight.air_yards;
                            r.passer = Some(self.roster_index(flight.thrower));
                            r.receiver = flight.target.map(|t| self.roster_index(t));
                            self.result = Some(r);
                            self.ball.flight = None;
                            self.catch_spot = spot;
                            return;
                        }
                    }
                }
            }
        }
        // A defender standing in the way of the ball gets a hand on it before it
        // arrives. This is not the landing-spot contest: it is the safety drifting
        // over the top and the corner who undercut the route. He either knocks it
        // down or takes it away -- he is not allowed to stand there and do nothing.
        if progress > 0.15 && progress < 0.97 && self.ball.z < 3.0 && !self.pass_contacted {
            if let Some((d, dist)) = self.in_the_way() {
                self.pass_contacted = true;
                let cov = self.attrs(d).coverage_factor();
                let hands = self.attrs(d).catching as f32 / 100.0;
                // Right in front of him is a play; a lunge at the last inch is not.
                let reached = if dist < 0.7 { 1.0 } else { 0.45 };
                let pick = (0.02 + cov * 0.09 + hands * 0.05) * reached;
                let bat = (0.12 + cov * 0.16 + hands * 0.08) * reached;
                let spot = self.ball.pos.x;
                if self.rng.chance(pick) {
                    let desc = format!("{} jumps the route and picks it off!", self.name_of(d));
                    let mut r = self.finish(PlayOutcome::Interception, spot, desc);
                    r.turnover = true;
                    r.tackler = Some(self.roster_index(d));
                    r.air_yards = flight.air_yards;
                    r.passer = Some(self.roster_index(flight.thrower));
                    r.receiver = flight.target.map(|t| self.roster_index(t));
                    r.highlight = true;
                    self.result = Some(r);
                    self.ball.flight = None;
                    self.catch_spot = spot;
                    return;
                }
                if self.rng.chance(bat) {
                    let desc = format!("{} gets a hand in and bats it down!", self.name_of(d));
                    let mut r = self.finish(PlayOutcome::PassIncomplete, self.los, desc);
                    r.batted_down = true;
                    r.air_yards = flight.air_yards;
                    r.passer = Some(self.roster_index(flight.thrower));
                    r.receiver = flight.target.map(|t| self.roster_index(t));
                    self.result = Some(r);
                    self.ball.flight = None;
                    self.catch_spot = spot;
                    return;
                }
            }
        }
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
        // A ball that came down nowhere near the man it was meant for is an
        // overthrow, not a fifty-fifty: there is nothing there for him to catch.
        let stray = target
            .map(|t| self.players[t].pos.dist(landing) > CATCH_RADIUS * 1.8)
            .unwrap_or(false);
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
        if let Some(t) = target.filter(|_| !stray) {
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
        } else if stray {
            (
                PlayOutcome::PassIncomplete,
                "Overthrown. Nobody was close to it.".to_string(),
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

    /// A flag on the field.
    ///
    /// The play keeps going. A7FL fouls are walked off after the ball is dead, and
    /// stopping the snap on the whistle turned every hold into a play that never
    /// happened: the runner had nowhere to go, the pass was never thrown, and the
    /// yardage screen came up over a frozen picture. The flag stays on the field and
    /// is attached to whatever the play turns into.
    fn penalty(&mut self, call: PenaltyCall) {
        if self.flag.is_some() {
            // One flag per play: a second foul is not worth a second walk-off.
            return;
        }
        self.events.push(SimEvent {
            t: self.t,
            text: format!(
                "Yellow flag on the field: {}.",
                call.kind.label().to_lowercase()
            ),
        });
        self.flag = Some(call);
        self.flag_at = self.t;
    }

    fn finish(&mut self, outcome: PlayOutcome, end_spot: f32, description: String) -> PlayResult {
        let yards = (end_spot - self.los) * self.dirf();
        let score_play = outcome == PlayOutcome::Touchdown;
        let safety = outcome == PlayOutcome::Safety;
        // The clock stops on contact, not on the turf: a wrap that takes four
        // tenths of a second is picture, not time played.
        let elapsed = self
            .elapsed_override
            .take()
            .unwrap_or_else(|| (self.t - SNAP_DELAY).max(0.0));
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
            batted_down: false,
            air_yards: 0.0,
            penalty: self.flag.take(),
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

    /// Hand a defender to the pad. `None` gives him back to the coordinator.
    pub fn set_def_control(&mut self, control: Option<DefControl>) {
        self.def_control = control;
    }

    pub fn set_human_defender(&mut self, who: Option<usize>) {
        self.human_defender = who.filter(|i| (7..14).contains(i));
    }

    /// Defenders, nearest the ball first: the order the switch button walks.
    pub fn defender_order(&self) -> Vec<usize> {
        let mut out: Vec<usize> = (7..14)
            .filter(|&i| !self.players[i].down && self.players[i].stun <= 0.0)
            .collect();
        out.sort_by(|&a, &b| {
            let da = self.players[a].pos.dist(self.ball.pos);
            let db = self.players[b].pos.dist(self.ball.pos);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    }

    /// The man the pad should start on: the defender closest to the ball.
    pub fn nearest_defender(&self) -> Option<usize> {
        self.defender_order().first().copied()
    }

    /// Move the pad to the next defender, wrapping at the end of the list.
    pub fn cycle_defender(&mut self) -> Option<usize> {
        let order = self.defender_order();
        if order.is_empty() {
            self.human_defender = None;
            return None;
        }
        let next = match self.human_defender.and_then(|h| order.iter().position(|&i| i == h)) {
            Some(pos) => order[(pos + 1) % order.len()],
            None => order[0],
        };
        self.human_defender = Some(next);
        Some(next)
    }

    /// The defense role the pad is on, for the overlay.
    pub fn human_defender_role(&self) -> Option<DefRole> {
        self.human_defender.map(|i| DefRole::ALL[self.players[i].role_index])
    }

    /// Stop the snap waiting on the button and run it now.
    pub fn request_snap(&mut self) {
        self.awaiting_snap = false;
        if self.phase == Phase::PreSnap {
            self.t = self.t.max(SNAP_DELAY);
            self.snap();
        }
    }

    /// Make the snap wait for [`PlaySim::request_snap`] instead of going on a
    /// timer. The AI league does not call this, so headless games are untouched.
    pub fn await_snap(&mut self, waiting: bool) {
        self.awaiting_snap = waiting;
    }

    pub fn snap_is_waiting(&self) -> bool {
        self.awaiting_snap && self.phase == Phase::PreSnap
    }

    /// Change the play at the line, the way an audible does. Only legal before
    /// the snap, and only through a full re-line-up, so nobody keeps a route from
    /// the play that was called a moment ago.
    pub fn call_play(&mut self, play: Play) {
        if self.phase != Phase::PreSnap {
            return;
        }
        self.play = play;
        self.line_up();
        self.build_routes();
        self.assign_coverage();
        self.capture_presnap();
        self.carrier = None;
        self.intended = None;
        self.handoff = None;
        self.ball.holder = Some(self.qb);
        self.ball.flight = None;
        self.ball.pos = self.players[self.qb].pos;
        self.ball.z = 1.15;
        self.pass_contacted = false;
        self.throw_away = false;
        self.pass_caught = false;
        self.scramble_started = false;
        self.setup_handoff();
        self.events.push(SimEvent {
            t: self.t,
            text: format!("Audible: {}", self.play.name),
        });
    }

    /// Flip the called play to the other side of the formation.
    pub fn flip_play(&mut self) {
        let flipped = self.play.flipped();
        self.call_play(flipped);
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

    #[test]
    fn a_flag_does_not_stop_the_play_and_rides_on_the_result() {
        let (off, def_) = teams();
        let book = playbook();
        let d = defensive_playbook();
        let setup = setup(40.0, 1, 10.0, 11);
        let mut sim = PlaySim::new(&off, &def_, &book[0], &d[0], &setup);
        for _ in 0..30 {
            sim.step(1.0 / 60.0);
        }

        sim.penalty(PenaltyCall::new(PenaltyKind::Holding, 42.0));
        assert!(sim.result.is_none(), "the flag killed the play on the spot");
        assert_eq!(
            sim.flag.as_ref().map(|c| c.kind),
            Some(PenaltyKind::Holding),
            "the flag never went down"
        );

        // A second flag does not replace the first one.
        sim.penalty(PenaltyCall::new(PenaltyKind::Clipping, 44.0));
        assert_eq!(
            sim.flag.as_ref().map(|c| c.kind),
            Some(PenaltyKind::Holding)
        );

        let r = sim.run_to_completion();
        assert_eq!(
            r.penalty.as_ref().map(|c| c.kind),
            Some(PenaltyKind::Holding),
            "the flag never reached the dead-ball result"
        );
        assert_ne!(r.outcome, PlayOutcome::Penalty, "the play was cut short");
        assert!(sim.flag.is_none(), "the flag stayed down after the play");
    }

    #[test]
    fn a_flagged_play_still_moves_the_ball_and_finishes() {
        // Nothing in the sim may hang waiting for a play that a flag used to end.
        let (off, def_) = teams();
        let book = playbook();
        let d = defensive_playbook();
        for seed in 0..40u64 {
            let play = &book[(seed as usize) % book.len()];
            let dcall = &d[(seed as usize * 3) % d.len()];
            let setup = setup(45.0, 2, 6.0, seed);
            let mut sim = PlaySim::new(&off, &def_, play, dcall, &setup);
            for _ in 0..20 {
                sim.step(1.0 / 60.0);
            }
            sim.penalty(PenaltyCall::new(PenaltyKind::Holding, 46.0));
            let r = sim.run_to_completion();
            assert!(r.penalty.is_some());
            assert!(r.end_spot.is_finite() && r.elapsed.is_finite());
            assert!(r.end_spot >= -10.0 && r.end_spot <= 110.0, "spot {}", r.end_spot);
        }
    }

    /// Contact is not the end of the play: the defender wraps, the carrier goes
    /// over the top of it, and only then is the ball dead. A card one frame after
    /// the hit is a card over two men still on their feet.
    #[test]
    fn a_run_that_keeps_gaining_ground_survives_the_thirteen_second_wall() {
        let (off, def) = teams();
        let book = playbook();
        let play = book
            .iter()
            .find(|p| p.family.is_run())
            .expect("no run play");
        let dcall = defensive_playbook()[0].clone();
        let s = setup(20.0, 1, 10.0, 31);
        let mut sim = PlaySim::new(&off, &def, play, &dcall, &s);
        sim.request_snap();
        for _ in 0..90 {
            sim.step(FIXED_DT);
            if sim.carrier.is_some() {
                break;
            }
        }
        let carrier = sim.carrier.expect("nobody took the handoff");
        // Nobody gets to him, and he keeps moving the ball downfield at a jog.
        let mut t = 0.0f32;
        while t < MAX_PLAY_TIME + 3.0 {
            for d in 7..14 {
                sim.players[d].pos = Vec2::new(-40.0, 39.0);
                sim.players[d].stun = 1.0;
            }
            let spot = s.los + t * 1.2;
            sim.players[carrier].pos.x = spot;
            sim.step(FIXED_DT);
            t += FIXED_DT;
            if sim.result.is_some() {
                break;
            }
        }
        assert!(
            sim.result.is_none(),
            "the run was cut off at the old wall: {:?}",
            sim.result.as_ref().map(|r| r.description.clone())
        );
    }

    #[test]
    fn a_run_that_stops_gaining_ground_is_booked_where_it_stands() {
        let (off, def) = teams();
        let book = playbook();
        let play = book
            .iter()
            .find(|p| p.family.is_run())
            .expect("no run play");
        let dcall = defensive_playbook()[0].clone();
        let s = setup(30.0, 1, 10.0, 37);
        let mut sim = PlaySim::new(&off, &def, play, &dcall, &s);
        sim.request_snap();
        for _ in 0..90 {
            sim.step(FIXED_DT);
            if sim.carrier.is_some() {
                break;
            }
        }
        let carrier = sim.carrier.expect("nobody took the handoff");
        // He breaks one, gets to the second level, and is caught: the ball stops
        // moving well short of the end zone and the pile sits on him.
        let spot = s.los + 22.0;
        let mut t = 0.0f32;
        while t < STALL_SECONDS + 2.0 && sim.result.is_none() {
            for d in 7..14 {
                sim.players[d].pos = Vec2::new(-40.0, 39.0);
                sim.players[d].stun = 1.0;
            }
            sim.players[carrier].pos = Vec2::new(spot, 0.0);
            sim.step(FIXED_DT);
            t += FIXED_DT;
        }
        let result = sim.result.clone().expect("the stalled run was never ended");
        assert!(!result.incomplete, "a run was booked as an incompletion: {}", result.description);
        assert!(result.yards > 15.0, "the whole gain was thrown away: {}", result.description);
        assert!(
            (result.end_spot - spot).abs() < 2.0,
            "the ball was spotted at {} instead of {}: {}",
            result.end_spot,
            spot,
            result.description
        );
    }

    #[test]
    fn a_passer_who_never_throws_still_runs_out_of_time() {
        let (off, def) = teams();
        let book = playbook();
        let play = book
            .iter()
            .find(|p| p.family == PlayFamily::DropbackPass)
            .expect("no dropback play");
        let dcall = defensive_playbook()[0].clone();
        let s = setup(30.0, 1, 10.0, 41);
        let mut sim = PlaySim::new(&off, &def, play, &dcall, &s);
        sim.request_snap();
        // A human holding the ball and never throwing it.
        sim.set_control(Some(QbControl::default()));
        let mut t = 0.0f32;
        while t < MAX_PLAY_TIME + 6.0 && sim.result.is_none() {
            sim.step(FIXED_DT);
            t += FIXED_DT;
        }
        assert!(sim.result.is_some(), "the pocket never ended");
    }

    #[test]
    fn the_snap_waits_for_the_button() {
        let (off, def) = teams();
        let book = playbook();
        let dcall = defensive_playbook()[0].clone();
        let s = setup(35.0, 1, 10.0, 11);
        let mut sim = PlaySim::new(&off, &def, &book[0], &dcall, &s);
        sim.await_snap(true);
        // Four seconds of looking at the defense: no snap, and no drift.
        let lined = (0..14).map(|i| sim.players[i].pos).collect::<Vec<_>>();
        for _ in 0..240 {
            sim.step(FIXED_DT);
        }
        assert!(!sim.is_snapped(), "the ball snapped itself");
        assert_eq!(sim.t, SNAP_DELAY, "the clock ran on at the line");
        for (i, was) in lined.iter().enumerate() {
            let now = sim.players[i].pos;
            // Only the pre-snap shuffle, which is two hundredths of a yard.
            assert!((now - *was).length() < 0.05, "man {i} walked off: {now:?} from {was:?}");
        }
        sim.request_snap();
        assert!(sim.is_snapped(), "the ball never went");
        assert_eq!(sim.t, SNAP_DELAY, "the snap moved the clock");
    }

    #[test]
    fn an_audible_changes_the_play_and_re_lines_everyone() {
        let (off, def) = teams();
        let book = playbook();
        let first = book
            .iter()
            .find(|p| p.formation == crate::playbook::Formation::Pistol)
            .expect("no pistol play");
        let other = book
            .iter()
            .find(|p| p.name == "FOUR VERTS")
            .expect("no four verts");
        let dcall = defensive_playbook()[0].clone();
        let s = setup(35.0, 1, 10.0, 3);
        let mut sim = PlaySim::new(&off, &def, first, &dcall, &s);
        let was = sim.players[6].pos;
        sim.call_play(other.clone());
        assert_eq!(sim.play.name, "FOUR VERTS", "the audible did not stick");
        assert_ne!(sim.players[6].pos, was, "nobody moved for the audible");
        for i in 0..7 {
            let role = OffRole::ALL[sim.players[i].role_index];
            let want = other.alignment(role);
            let got = sim.players[i].pos;
            let along = (got.x - s.los) * s.direction as f32;
            let across = got.y - s.ball_y;
            assert!(
                (along - want.x).abs() < 1e-3 && (across - want.y).abs() < 1e-3,
                "{role:?} lined up at ({along}, {across}) not {want:?}"
            );
            // And he is running the new play's route, not the old one's.
            let assign = other.assignment(role).expect("missing assignment");
            let wants_route = matches!(
                assign,
                Assign::Route { .. } | Assign::Fake { .. } | Assign::Carry { .. } | Assign::QbCarry { .. }
            );
            assert_eq!(!sim.players[i].route.is_empty(), wants_route, "{role:?} route out of step");
        }
    }

    #[test]
    fn a_flipped_call_runs_to_the_other_side() {
        let (off, def) = teams();
        let book = playbook();
        let play = book.iter().find(|p| p.name == "MESH").expect("no mesh");
        let dcall = defensive_playbook()[0].clone();
        let s = setup(35.0, 1, 10.0, 5);
        let mut sim = PlaySim::new(&off, &def, play, &dcall, &s);
        let before: Vec<Vec2> = (0..7).map(|i| sim.players[i].pos).collect();
        sim.flip_play();
        assert!(sim.play.flip, "the flip was not kept");
        for (i, was) in before.iter().enumerate() {
            let now = sim.players[i].pos;
            assert!((now.x - was.x).abs() < 1e-3, "man {i} moved upfield on a flip");
            assert!((now.y + was.y).abs() < 1e-3, "man {i} did not mirror: {now:?}");
        }
        // The routes are mirrored too, not just the alignments.
        assert!(
            sim.players[5].route.iter().all(|p| p.y < s.ball_y),
            "the wide left route did not mirror: {:?}",
            sim.players[5].route
        );
    }

    #[test]
    fn the_pad_walks_round_all_seven_defenders() {
        let (off, def) = teams();
        let book = playbook();
        let dcall = defensive_playbook()[0].clone();
        let s = setup(35.0, 1, 10.0, 9);
        let mut sim = PlaySim::new(&off, &def, &book[0], &dcall, &s);
        let first = sim.cycle_defender().expect("no defender to control");
        assert!((7..14).contains(&first));
        let mut seen = vec![first];
        for _ in 0..6 {
            seen.push(sim.cycle_defender().expect("switch ran out"));
        }
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 7, "the switch skipped a man: {seen:?}");
        assert_eq!(sim.cycle_defender(), Some(first), "the switch does not wrap");
    }

    #[test]
    fn a_human_defender_makes_the_tackle_he_asks_for() {
        let (off, def) = teams();
        let book = playbook();
        let play = book
            .iter()
            .find(|p| p.family.is_run())
            .expect("no run play");
        let dcall = defensive_playbook()[0].clone();
        let s = setup(35.0, 1, 10.0, 17);
        let mut sim = PlaySim::new(&off, &def, play, &dcall, &s);
        sim.request_snap();
        for _ in 0..90 {
            sim.step(FIXED_DT);
            if sim.carrier.is_some() {
                break;
            }
        }
        let carrier = sim.carrier.expect("nobody took the handoff");
        sim.set_human_defender(Some(7));
        let mut down = false;
        for _ in 0..80 {
            if sim.result.is_some() {
                break;
            }
            sim.players[7].pos = sim.players[carrier].pos;
            sim.players[7].stun = 0.0;
            sim.players[7].tackle_cooldown = 0.0;
            sim.set_def_control(Some(DefControl {
                tackle: true,
                ..Default::default()
            }));
            sim.step(FIXED_DT);
            if sim.wrap.is_some() || sim.result.is_some() {
                down = true;
                break;
            }
        }
        assert!(down, "pressing tackle never produced a tackle");
    }

    #[test]
    fn a_tackle_takes_a_beat_before_the_ball_is_dead() {
        let (off, def_) = teams();
        let book = playbook();
        let dbook = defensive_playbook();
        let play = book.iter().find(|p| p.family.is_run()).expect("a run play");
        let mut sim = PlaySim::new(&off, &def_, play, &dbook[0], &setup(40.0, 1, 10.0, 3));
        let mut guard = 0;
        while sim.carrier.is_none() && sim.result.is_none() && guard < 900 {
            sim.step(1.0 / 60.0);
            guard += 1;
        }
        let carrier = sim.carrier.expect("the handoff");

        // Put the nearest defender on him every frame until the arms go on.
        let mut guard = 0;
        while sim.wrap.is_none() && sim.result.is_none() && guard < 300 {
            sim.players[7].pos = sim.players[carrier].pos + Vec2::new(0.25, 0.0);
            sim.players[7].stun = 0.0;
            sim.players[7].down = false;
            sim.players[7].tackle_cooldown = 0.0;
            sim.step(1.0 / 60.0);
            guard += 1;
        }
        assert!(sim.wrap.is_some(), "no tackle was ever attempted");
        assert!(
            sim.result.is_none(),
            "the hit ended the play on the spot: {:?}",
            sim.result.as_ref().map(|r| &r.description)
        );
        assert!(
            !sim.players[carrier].down,
            "he is on the turf before the wrap has even started"
        );

        // And then it finishes, on the ground, a good part of a second later.
        let mut t = 0.0;
        while sim.result.is_none() && t < 3.0 {
            sim.step(1.0 / 60.0);
            t += 1.0 / 60.0;
        }
        let r = sim.result.clone().expect("the tackle has to end the play");
        assert!(
            t > 0.30,
            "the wrap resolved in {t:.2}s: there is nothing to watch"
        );
        assert!(sim.players[carrier].down, "nobody ends up on the ground");
        assert!(r.tackler.is_some(), "nobody gets credit for the tackle");
    }

    /// A defender in the air's path is not scenery. He either knocks it down or
    /// takes it away, and the picture says which.
    #[test]
    fn a_defender_in_the_way_gets_a_hand_on_the_ball() {
        let (off, def_) = teams();
        let book = playbook();
        let dbook = defensive_playbook();
        let play = book.iter().find(|p| p.family.is_pass()).expect("a pass play");
        let mut stops = 0;
        for seed in 0..60u64 {
            let mut sim = PlaySim::new(
                &off,
                &def_,
                play,
                &dbook[seed as usize % dbook.len()],
                &setup(40.0, 1, 10.0, seed),
            );
            let mut guard = 0;
            // Stand a defender directly in the flight path once it is up.
            while sim.result.is_none() && guard < 1500 {
                if sim.ball.flight.is_some() {
                    let ball = sim.ball.pos;
                    sim.players[7].pos = ball;
                    sim.players[7].down = false;
                    sim.players[7].stun = 0.0;
                }
                sim.step(1.0 / 60.0);
                guard += 1;
            }
            if let Some(r) = &sim.result {
                if r.batted_down {
                    stops += 1;
                    assert!(r.incomplete, "a batted ball is not a completion");
                    assert_eq!(r.headline(), Some("PASS BATTED DOWN"));
                } else if r.outcome == PlayOutcome::Interception {
                    stops += 1;
                    assert_eq!(r.headline(), Some("INTERCEPTION"));
                }
            }
        }
        assert!(stops > 0, "a man standing under the ball never touched it");
    }

    /// The word over the dead ball has to be the thing that just happened.
    #[test]
    fn the_picture_names_the_hit_it_just_saw() {
        let (off, def_) = teams();
        let book = playbook();
        let dbook = defensive_playbook();
        let mut r = simulate_play(&off, &def_, &book[0], &dbook[0], &setup(40.0, 1, 10.0, 9));
        r.sack = false;
        r.touchdown = false;
        r.safety = false;
        r.turnover = false;
        r.incomplete = false;
        r.batted_down = false;
        r.outcome = PlayOutcome::Run;
        r.yards = -3.0;
        assert_eq!(r.headline(), Some("TACKLE FOR LOSS"));
        r.yards = 4.0;
        assert_eq!(r.headline(), None);
        r.sack = true;
        assert_eq!(r.headline(), Some("SACK"));
        r.sack = false;
        r.batted_down = true;
        assert_eq!(r.headline(), Some("PASS BATTED DOWN"));
    }
}

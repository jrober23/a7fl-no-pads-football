//! Continuous-time 7 on 7 scrimmage play simulation.
//!
//! Every snap is advanced in fixed timesteps so the broadcast renderer can draw any
//! frame of it, and so a whole game can be simulated headlessly at high speed. The
//! simulation owns all of the 7 on 7 mechanics that matter on a snap:
//!
//! * two down linemen who may release up to five yards, four eligibles,
//! * no blitz restrictions (a defense may rush as many as it likes, which leaves
//!   receivers uncovered),
//! * wrap-tackle resolution with break tackles, plus the rulebook's high-tackle and
//!   block-in-the-back infractions,
//! * pass interference, holding while the quarterback is in the pocket, roughing
//!   the passer, intentional grounding,
//! * sideline / end line / goal line geometry including safeties.
use crate::difficulty::Difficulty;
use crate::field::{self, FIELD_HALF_WIDTH, FIELD_LENGTH};
use crate::math::{approach_angle, clamp, lerp, Rng, Vec2};
use crate::playbook::Handoff;
use crate::playbook::{
    defensive_alignment, Assign, Coverage, DefCall, DefRole, OffRole, Play, PlayFamily,
};
use crate::moves::{CarrierMove, Kind};
use crate::rules::{FoulTeam, PenaltyCall, PenaltyKind};
use crate::team::{Attributes, Team};

/// How close a defender's hands have to be to a man to get hold of him. It was
/// nearly a yard of daylight, and the picture showed the gap: a runner was stopped
/// by a man he was plainly not touching yet.
/// How wide a man is across the shoulders. Two of these is where two men touch,
/// and that is what contact means here: the separation pass used to hold every
/// pair half a yard apart at their centres whatever they were doing, which reads
/// as a force field between them rather than as a shoulder.
pub const BODY_RADIUS: f32 = 0.31;

/// How close a defender has to be to lay a hand on the carrier.
///
/// This is body to body: the men are held 0.55 apart by the separation pass, so
/// anything at or under that never fires at all, and the whole offense stops
/// ending its plays. 0.60 is the floor the running game is tuned against, and it is a contact
/// distance: at that gap the two men are touching, so the wrap is made by hands
/// that are actually on him rather than by a defender standing in his air.
pub const TACKLE_RADIUS: f32 = 0.74;

/// The ball in the air is decided by the receiver's hands against the coverage
/// ratings of the men around the landing spot, not by a flat completion roll.
pub const CATCH_BASE: f32 = 0.14;
/// How much a point of hands-minus-coverage is worth on the catch.
pub const CATCH_WEIGHT: f32 = 0.55;
pub const CATCH_FLOOR: f32 = 0.06;
/// Yards from the landing spot inside which a defender's coverage rating bites.
pub const COVERAGE_AREA: f32 = 3.0;
/// How much further a lunge reaches than a man on his feet, because his arms are
/// already out in front of him when he arrives.
pub const ARM_REACH: f32 = 0.44;

/// How close the two men in a wrap are held: shoulders inside a hand's width,
/// hands on the chest. It was [`ARM_REACH`], which put one body inside the other
/// -- one man made of two -- and then the separation pass pushed them back apart
/// every frame, which is the judder the wrap had.
pub const HOLD_DISTANCE: f32 = BODY_RADIUS * 2.0 + 0.08;
/// How far a diving defender throws himself at the man with the ball.
pub const DIVE_LUNGE: f32 = 2.20;
/// Hands up inside this many yards of the carrier, hands at his sides past
/// [`PRESS_FAR`]: a defender working to get to the ball is reaching for him.
pub const PRESS_AT: f32 = 1.2;
pub const PRESS_FAR: f32 = 3.2;
/// How long a dive keeps the longer reach, in seconds.
pub const DIVE_WINDOW: f32 = 0.30;

/// How long he is on the ground after a lunge. A dive is a commitment: he does
/// not land on his hands and immediately run again, and without this he kept the
/// speed of the lunge and slid away across the field.
pub const DIVE_RECOVERY: f32 = 0.30;

/// How long the wrap takes: the arms go on him, the feet drive, and the two of
/// them go to the turf. Contact is not the end of the play. A tackle that
/// resolved on the frame the defender arrived was a teleport with a sound effect:
/// there was nothing on the field to see, and the next screen was already coming.
pub const WRAP_SECONDS: f32 = 0.42;
/// How long a loose ball rolls before anybody may fall on it: long enough for the
/// men around it to turn and get a hand on it, short enough that a fumble is a
/// scramble rather than a pile that sits there.
pub const LOOSE_GRACE: f32 = 0.35;
/// How close a man has to get to a loose ball to come up with it.
pub const RECOVER_RADIUS: f32 = 1.15;

/// How far a blocker's hands reach. A man with his arms out in front is wider than
/// a man standing in the way, and the pocket depends on the reach. What was wrong
/// was not the distance he could start from but that he never closed it: see the
/// step-in in `resolve_blocks`.
pub const BLOCK_REACH: f32 = 1.90;
pub const CATCH_RADIUS: f32 = 1.25;
pub const SNAP_DELAY: f32 = 0.85;
pub const MAX_PLAY_TIME: f32 = 13.0;
/// How long the quarterback sells a play action: the ball is shown to the man
/// coming across his face and pulled back before he turns to throw.
const FAKE_SELL: f32 = 0.45;
/// How hard the men behind the front lean toward a fake while it is being sold.
const PA_LEAN: f32 = 2.6;
/// How long a runner may go without gaining a yard before the play is dead. A
/// man who is still moving the ball is playing; a man who is not is stuck in a
/// pile, and the clock is the wrong thing to measure him by.
pub const STALL_SECONDS: f32 = 3.6;
/// The absolute backstop for a run, long enough for a run the length of the field
/// at a jog. Only a physics bug should ever reach it.
pub const RUN_PLAY_LIMIT: f32 = 40.0;
pub const FIXED_DT: f32 = 1.0 / 60.0;
/// How long a ball nobody caught takes to drop the last of the way to the turf.
/// The flight ends at the catch height, so without this the whistle went while
/// the ball was still up in the air and the incompletion never landed.
pub const BALL_FALL_SECONDS: f32 = 0.42;
/// How fast the men who have to change sides jog to their new spots on a flipped
/// call. A flip used to slide everybody across on an eased lerp, which read as a
/// teleport with a smear on it; a flip is a man turning, jogging, and squaring up.
pub const FLIP_JOG_SPEED: f32 = 2.9;
/// How fast a man can turn on the way to his new spot, radians a second.
pub const FLIP_TURN_RATE: f32 = 9.0;
/// How close two bodies have to be before it is a touch and not a lean. The
/// wrap-up radius is nearly a yard, which had a defender standing at the line
/// flagged the instant he leaned across it: this is shoulder to shoulder.
pub const PRESNAP_TOUCH: f32 = 0.62;
/// How far beyond the line a man has to be when the ball goes to be offside:
/// a step over, not a shoulder.
pub const SNAP_OFFSIDE: f32 = 0.3;
/// How long the quarterback's arm is out on a pump fake before he can use it
/// again.
pub const PUMP_TIME: f32 = 0.55;

/// How long an interception may be run back before the returner is done.
pub const RETURN_SECONDS: f32 = 9.0;
/// How long the chase takes to reach full speed. They have to turn and run; the
/// man with the ball does not. Without it the nearest two were on him as soon as
/// the grace ran out and every interception came back three yards.
pub const CHASE_WARMUP: f32 = 1.4;

/// The beat of daylight the man who just picked it off gets before anybody can
/// lay a hand on him. The catch happens at the landing spot, which is where the
/// man the ball was thrown to is standing, so without it the nearest "tackler"
/// was the receiver he had just taken it from and every interception was booked
/// dead on the frame the return started.
pub const RETURN_GRACE: f32 = 1.7;

/// How much room a body is allowed past the paint. The advertising boards stand
/// 0.55 of a yard beyond the sideline; a man used to be allowed a yard and a
/// half of run-off, which walked him through the boards and into the front row.
pub const BODY_APRON: f32 = 0.2;
/// How long a CPU offense waits at the line when a human is on defense: long
/// enough to walk a man up, check to another call, and get set.
pub const PRESNAP_WAIT: f32 = 2.6;
/// How long a dummy count takes to sell at the line.
pub const DUMMY_TIME: f32 = 1.1;
/// How far past the ball a man is drawn when he bites at the count: further
/// than `SNAP_OFFSIDE`, so the whistle he earns is one the picture shows.
pub const JUMP_PAST: f32 = 0.5;
/// How often the count catches your own line leaning instead of theirs.
pub const DUMMY_FALSE_START_CHANCE: f32 = 0.07;
/// How far a defender may lean into the neutral zone before the ball goes. Any
/// further and he is across the line, which is a foul the moment the snap comes.

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

/// Which play a defender has decided on under a ball in the air. The two are
/// different pictures -- two hands up to take it away, one hand up to knock it
/// down -- and the passer reads the same thing the player does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BallHands {
    /// Going for the interception.
    Pick,
    /// Going for the knock-down.
    Swat,
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
    /// A drop drawn on the call sheet: he follows the path he was given instead
    /// of one the coverage picked for him.
    Drop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    PreSnap,
    Run,
    Dropback,
    Scramble,
    BallInAir,
    /// The ball was picked off and the man who has it is running the other way.
    Return,
    /// The ball is on the ground and nobody has it. A fumble, or a backward pass
    /// that hit the turf: the play is still live and both teams are after it.
    Loose,
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
    /// How long a dive still has its hands out, in seconds.
    pub dive_t: f32,
    /// The line he left his feet on. The lunge travels this way for the window.
    pub dive_aim: Vec2,
    /// How much of the lunge he has already covered this dive.
    pub dive_fly: f32,
    /// Whether the dive button has been let go since his last dive.
    /// Ground already covered by the lunges chained into this one. A held
    /// button keeps him diving; this is what makes the chain continuous instead
    /// of restarting the curve at zero every window.
    pub dive_base: f32,
    /// Whether the button has been let go since his last lunge. A dive is one
    /// press and one lunge; held down it chained, and a defender chaining lunges
    /// at a runner who has already moved is a body careening about the field.
    pub dive_armed: bool,
    /// 0..1: how much his hands are up, working to get to the man with the ball.
    pub tackle_arms: f32,
    pub engaged: Option<usize>,
    /// Where his hands are, in radians across his body: the hand on the side the
    /// rusher goes to follows him no more than 45 degrees either way.
    pub hands: f32,
    /// 0..1, hands up and driving the man in front of him back upfield. Run
    /// blocking, as against holding a pocket.
    pub drive: f32,
    /// Hands out in front, posed as a blocker. The line every play, the backs and
    /// receivers on a run, and never the man carrying the ball.
    pub blocking: bool,
    /// 0..1, how far the ball is being offered to a runner. A give is made with
    /// the ball away from the chest, held out to the man coming for it, and the
    /// picture reaches the same distance with the arms.
    pub give: f32,
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

/// What the computer did with the option it was handed.
///
/// The read is one defender -- the end on the play side -- and the answer is one
/// of three things, which is the whole of the triple option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionChoice {
    /// The end stayed wide, so the ball went back to the first man: the dive.
    Handoff,
    /// The end crashed down inside, so the quarterback kept it.
    Keep,
    /// He kept it, and then gave it on to the man trailing him when they came
    /// for him.
    Pitch,
}

/// A ball on the ground that nobody has yet.
///
/// It rolls, it slows, and both sides go and get it. The play is not dead until a
/// man comes up with it and is put on the ground, which is the whole difference
/// between a fumble and a tackle.
#[derive(Clone, Debug)]
struct LooseBall {
    at: Vec2,
    vel: Vec2,
    /// Seconds since the ball came out.
    t: f32,
}

/// A ball on its way to the ground that nobody is going to catch. The play is
/// not over until it lands: the result waits here while the ball falls.
#[derive(Clone, Debug)]
struct Fall {
    t: f32,
    from_z: f32,
    outcome: PlayOutcome,
    spot: f32,
    description: String,
    receiver: Option<usize>,
    passer: Option<usize>,
    air_yards: f32,
    batted_down: bool,
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
}

/// Live human input for a defender, set by an interactive client.
///
/// Movement is the stick; the buttons are the only way a player can make a tackle,
/// a pass breakup or a pick happen on his own man rather than waiting for the
/// coordinator's dice. Absent during ordinary AI play, so `sim` and `film` behave
/// exactly as they always have.
/// A tackle thrown with the right stick behind it: up over the nameplate, down
/// under it. Both go for the ball rather than for the man alone, and a high hit
/// is the one that can be flagged, because in this game hitting above the
/// nameplate is unnecessary roughness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    High,
    Low,
}

impl Hit {
    /// How much more of the ball this hit is after, on top of the ordinary
    /// fumble chance. Strength and tackling together decide how much of the man
    /// a defender can put behind it, so the strongest tackler in the league is
    /// worth roughly three times what the softest one is.
    pub fn fumble_bonus(self, attrs: &crate::team::Attributes) -> f32 {
        let strength = attrs.strength as f32 / 100.0;
        let tackling = attrs.tackle_factor();
        let force = 0.40 + strength * 0.50 + tackling * 0.50;
        let base = match self {
            Hit::High => 0.060,
            Hit::Low => 0.040,
        };
        base * force
    }

    pub fn label(self) -> &'static str {
        match self {
            Hit::High => "HIGH HIT",
            Hit::Low => "LOW HIT",
        }
    }
}

/// The chance a wrap-up knocks the ball out, before the carrier's move scales
/// it. A hit stick tackle is worth more of it, in proportion to how much man the
/// defender can put behind the hit.
fn fumble_chance(
    carrier: &crate::team::Attributes,
    defender: &crate::team::Attributes,
    hit: Option<Hit>,
    difficulty: f32,
) -> f32 {
    0.012
        + (1.0 - carrier.break_tackle_factor()) * 0.022
        + (1.0 - carrier.awareness_factor()) * 0.012
        // A hard defense goes for the ball on the way in.
        + difficulty
        + hit.map(|h| h.fumble_bonus(defender)).unwrap_or(0.0)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DefControl {
    /// Stick direction in world space, `x` down the field and `y` across it.
    pub move_dir: Vec2,
    /// One frame of the tackle button.
    pub tackle: bool,
    /// One frame of the swat button: knock the ball down.
    pub swat: bool,
    /// One frame of the dive button: a lunging tackle from further out.
    pub dive: bool,
    /// One frame of the pick button: go and take it away.
    pub intercept: bool,
    /// The right stick behind the tackle: up over the nameplate, down under it.
    /// An ordinary tackle is `None`.
    pub hit: Option<Hit>,
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

#[derive(Clone)]
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
    /// The man a play action is sold to, and the seconds of sell still to go.
    /// The clock on it starts when the ball goes, not when the play is set up:
    /// the fake is the first thing the quarterback does with the snap. `None` on
    /// every play that is not a play action.
    pa_fake: Option<(usize, f32)>,
    /// The end the computer read on an option, and what it did about it. `None`
    /// until the read is made, and on every play that is not an option.
    pub option_key: Option<usize>,
    pub option_choice: Option<OptionChoice>,
    pub result: Option<PlayResult>,
    /// `Some` when a human is driving the quarterback.
    pub control: Option<QbControl>,
    /// `Some` when a human is driving a defender.
    pub def_control: Option<DefControl>,
    /// Which defender the pad is on, as an index into `players`.
    pub human_defender: Option<usize>,
    /// How hard the other sideline is playing. Set by the game that owns the
    /// snap, not by the snap itself.
    pub difficulty: Difficulty,
    /// The snap waits for the button while this is set. An offense at the line
    /// decides when the ball goes, so sitting in the audible cannot run the play
    /// out from under him.
    awaiting_snap: bool,
    /// Where every man stood when the formation was set. The pre-snap shuffle is
    /// an offset from these, so a long look at the defense cannot walk the whole
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
    /// A timeout asked for during the play. The sim cannot take one — the clock
    /// and the allowance are the game's — so the press rides out with the result
    /// and is spent at the whistle, which is when a timeout is actually called.
    pub timeout_asked: bool,
    /// Seconds left on the fumble plate, and what it says. Set the frame the ball
    /// comes out, on any kind of play, so every client raises the same alert.
    pub fumble_alert: f32,
    pub fumble_note: String,
    /// The play clock at the line: twenty-five seconds to snap it. It runs while
    /// the ball waits on the centre, and running it out is a five-yard foul.
    pub play_clock: f32,
    /// True while the player is holding the preview button at the line.
    preview: bool,
    /// Time left of a dummy count being sold at the line.
    dummy_t: f32,
    /// A tackle in progress: the hit is on and the two men are going down.
    pub wrap: Option<Wrap>,
    /// A ball on the turf with both teams chasing it.
    loose: Option<LooseBall>,
    /// Which side came up with a loose ball, once somebody has it. The tackle
    /// that ends the play is booked as a fumble rather than as a run.
    loose_recovery: Option<Side>,
    /// The man running it back got the ball off the ground, not out of the air.
    return_is_fumble: bool,
    /// Whether a defender has already had his chance at a ball in the air, so one
    /// throw is not swatted at on every frame of its flight.
    pass_contacted: bool,
    /// The defender playing the ball in the air and what he is playing it for,
    /// while the ball is up. Nothing here once the pass is over.
    pub ball_hands: Option<(usize, BallHands)>,
    /// What the game clock stopped at, when the ball died before the wrap finished.
    elapsed_override: Option<f32>,
    /// A ball dropping to the turf with nobody on it. The result is booked when
    /// it lands, not when the flight that carried it ends.
    falling: Option<Fall>,
    /// Time left on a pump fake, in seconds.
    pump_t: f32,
    /// The man who picked it off and the spot he caught it, while he runs it back.
    returner: Option<usize>,
    return_spot: f32,
    /// Seconds since the pick.
    return_t: f32,
    /// The throw that was intercepted: which arm, and how far it had travelled.
    return_throw: Option<(usize, f32)>,
    /// Where everybody stood when a flip was called. The men who change sides walk
    /// across from here rather than being placed on their new spot.
    shift: Option<Vec<Vec2>>,
    shift_t: f32,
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
                dive_t: 0.0,
                dive_aim: Vec2::ZERO,
                dive_fly: 0.0,
                dive_base: 0.0,
                dive_armed: true,
                tackle_arms: 0.0,
                engaged: None,
                hands: 0.0,
                drive: 0.0,
                blocking: false, give: 0.0,
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
            let a = defensive_alignment(role, dcall, setup.los, setup.direction, play.run_side);
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
                dive_t: 0.0,
                dive_aim: Vec2::ZERO,
                dive_fly: 0.0,
                dive_base: 0.0,
                dive_armed: true,
                tackle_arms: 0.0,
                engaged: None,
                hands: 0.0,
                drive: 0.0,
                blocking: false, give: 0.0,
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
            pa_fake: None,
            option_key: None,
            option_choice: None,
            result: None,
            events: Vec::new(),
            read_timer: 0.0,
            catch_spot: setup.los,
            scramble_started: false,
            control: None,
            def_control: None,
            human_defender: None,
            difficulty: Difficulty::Easy,
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
            timeout_asked: false,
            fumble_alert: 0.0,
            fumble_note: String::new(),
            play_clock: crate::rules::PLAY_CLOCK_SECONDS,
            preview: false,
            dummy_t: 0.0,
            wrap: None,
            loose: None,
            loose_recovery: None,
            return_is_fumble: false,
            pass_contacted: false,
            ball_hands: None,
            elapsed_override: None,
            falling: None,
            pump_t: 0.0,
            returner: None,
            return_spot: setup.los,
            return_t: 0.0,
            return_throw: None,
            shift: None,
            shift_t: 0.0,
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

    /// Who a man is, for the things that have to say his name out loud.
    pub fn name_of(&self, i: usize) -> String {
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
                &self.def,
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
        // A drop drawn on the call sheet is where that man goes. The front stands
        // where it was drawn and the path is his, so the coverage behind the front
        // is the coverage the player drew rather than the one the call implies.
        let mut covered: Vec<usize> = Vec::new();
        for i in 7..14 {
            let role = DefRole::ALL[self.players[i].role_index];
            let drawn = self
                .def
                .drops
                .iter()
                .find(|(r, path)| *r == role && !path.is_empty())
                .map(|(_, path)| path.clone());
            if let Some(path) = drawn {
                let (los, ball_y, dirf) = (self.los, self.setup.ball_y, self.dirf());
                self.players[i].route = path
                    .iter()
                    .map(|q| Vec2::new(los + dirf * q.x, ball_y + q.y))
                    .collect();
                self.players[i].waypoint = 0;
                self.players[i].job = CoverageJob::Drop;
                covered.push(i);
            }
        }
        let mut available: Vec<usize> = (7..14)
            .filter(|i| !rushers.contains(i) && !covered.contains(i))
            .collect();
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

        // Press puts the corners up on the man across from them whether the men
        // behind them are in man or zone: that is the technique, and it is the
        // thing the trigger at the line changes.
        if self.def.press {
            for i in 7..14 {
                let role = DefRole::ALL[self.players[i].role_index];
                if !matches!(role, DefRole::CbL | DefRole::CbR) {
                    continue;
                }
                if self.players[i].job == CoverageJob::Drop {
                    continue;
                }
                let mine = self.players[i].pos.y;
                let across = self
                    .eligible_indices()
                    .into_iter()
                    .min_by(|&a, &b| {
                        let da = (self.players[a].pos.y - mine).abs();
                        let db = (self.players[b].pos.y - mine).abs();
                        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                    });
                let y = across.map(|e| self.players[e].pos.y).unwrap_or(mine);
                self.players[i].pos = Vec2::new(
                    self.los + self.dirf() * 1.1,
                    clamp(y, -FIELD_HALF_WIDTH + 0.8, FIELD_HALF_WIDTH - 0.8),
                );
            }
        }
    }

    fn play_route_depth(&self, sim_index: usize) -> u8 {
        let role = OffRole::ALL[self.players[sim_index].role_index];
        self.play
            .assignment(role)
            .and_then(|a| a.route_depth())
            .unwrap_or(0)
    }

    /// Which way the ball is going, taken off the runner's own drawn path.
    ///
    /// Negative or positive is a sideline, zero is dead straight at the line.
    /// The quarterback uses it to take his side of the runner before giving the
    /// ball up: a give runs the runner's way, never across him.
    fn runner_side(&self, car: usize) -> f32 {
        // Where the run ends up, not where it starts from: a man lined up at the
        // ball and sent straight has no side, one drawn away from the ball has
        // the side he was drawn to, whether his path is one point or several.
        let drift = match self.players[car].route.last() {
            Some(last) => last.y - self.setup.ball_y,
            None => 0.0,
        };
        if drift.abs() < 1.0 {
            0.0
        } else {
            drift.signum()
        }
    }

    /// Who a play action is sold to: the man nearest the quarterback who has a
    /// path to run, which is the back coming across his face.
    fn fake_target(&self) -> Option<usize> {
        (0..7)
            .filter(|&i| i != self.qb && !self.players[i].route.is_empty())
            .min_by(|&a, &b| {
                let da = self.players[a].pos.dist(self.players[self.qb].pos);
                let db = self.players[b].pos.dist(self.players[self.qb].pos);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// True while the quarterback has a play action to sell.
    fn selling_fake(&self) -> bool {
        self.pa_fake.map(|(_, left)| left > 0.0).unwrap_or(false)
    }

    /// The step of the fake: he goes to the man he is selling it to, at a
    /// runner's pace, so the two of them meet the way a give would.
    fn fake_step(&mut self, i: usize, dt: f32) {
        let Some((man, _)) = self.pa_fake else {
            return;
        };
        // He goes to the man, not to a spot on the ground: the ball is put where
        // the runner is, the way a give is, and the runner runs through it.
        let him = self.players[man].pos;
        self.steer(i, him, self.speed_cap(i) * 0.85, dt);
    }

    fn setup_handoff(&mut self) {
        let carrier_role = self.play.carrier_role();
        self.possession_start = Some(0);
        self.pa_fake = None;
        match self.play.family {
            PlayFamily::RunInside | PlayFamily::RunOutside | PlayFamily::QbRun => {
                if let Some(role) = carrier_role {
                    let idx = role.index();
                    // A fake is sold, not made: the man runs his path as a decoy
                    // and the quarterback keeps it. A real handoff is left
                    // pending, with no time on it, until the two men meet --
                    // where they meet is what the quarterback's own path
                    // decides.
                    if role == OffRole::Qb || !self.play.handoff.gives_it() {
                        self.carrier = Some(self.qb);
                        self.ball.holder = Some(self.qb);
                    } else {
                        self.handoff = Some((idx, SNAP_DELAY + 0.25));
                    }
                }
            }
            PlayFamily::PlayAction => {
                // The fake is sold with the ball in the quarterback's hands: he
                // steps to the man coming across his face, shows him the ball
                // and pulls it back before he drops. Nothing changes hands, so
                // there is no handoff to schedule -- only a man to sell it to.
                self.pa_fake = self.fake_target().map(|man| (man, FAKE_SELL));
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
            self.fumble_alert = (self.fumble_alert - dt.max(0.0)).max(0.0);
            return self.result.as_ref();
        }
        let dt = dt.min(0.05);
        self.t += dt;
        self.fumble_alert = (self.fumble_alert - dt).max(0.0);
        // Who stands like a blocker this frame. Decided from the play, so a man is
        // posed as one even on a frame where he has nobody's hands on him yet, and
        // the mesh cannot leave a stale flag on the runner.
        self.pose_blockers();
        self.pump_t = (self.pump_t - dt).max(0.0);
        // How close a defender is to putting hands on the man with the ball. The
        // figure carries its arms up while he works to get there: a defender
        // closing is not a man jogging with his arms at his sides.
        let carried = self.carrier.map(|c| (c, self.players[c].pos));
        for (i, p) in self.players.iter_mut().enumerate() {
            p.tackle_cooldown = (p.tackle_cooldown - dt).max(0.0);
            if p.dive_t > 0.0 {
                p.dive_t = (p.dive_t - dt).max(0.0);
                if p.dive_t <= 0.0 {
                    // He has landed. A lunge is the whole of the movement: keep
                    // the speed it was carrying and he skates on across the
                    // field after his hands are already on the ground, so he is
                    // put on it and stays there for a moment.
                    p.vel = Vec2::ZERO;
                    p.stun = p.stun.max(DIVE_RECOVERY);
                    p.dive_base = 0.0;
                    p.dive_fly = 0.0;
                }
            }
            p.tackle_arms = match carried {
                Some((c, at)) if i >= 7 && i != c && !p.down && p.stun <= 0.0 => {
                    let far = (PRESS_FAR - p.pos.dist(at)) / (PRESS_FAR - PRESS_AT);
                    let want = far.clamp(0.0, 1.0);
                    p.tackle_arms + (want - p.tackle_arms) * (dt * 12.0).min(1.0)
                }
                _ => 0.0,
            };
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
            // The clock at the line runs on the game, not on the player's nerves:
            // standing over the ball until the count is gone is a foul.
            self.play_clock = (self.play_clock - dt).max(0.0);
            if self.awaiting_snap && self.play_clock <= 0.0 {
                self.call_delay_of_game();
                return self.result.as_ref();
            }
            // Quick pre-snap shuffle so the broadcast shot has life. It is an
            // offset from where the formation was set, not a nudge applied every
            // frame: with the snap waiting on a button the old version walked the
            // whole offense sideways while the player read the defense.
            self.advance_shift(dt);
            self.dummy_t = (self.dummy_t - dt).max(0.0);
            for i in 0..14 {
                // The defender on the pad is not shuffled: he has legs of his own
                // before the snap, and writing the formation over him every frame
                // is what kept a player from walking up to the line.
                if Some(i) == self.human_defender {
                    continue;
                }
                // A man mid-jog is being moved by the walk itself; the settle of
                // the formation must not drag him along behind it.
                if self.shift.is_some() {
                    continue;
                }
                let jitter = Vec2::new(0.0, (self.t * 1.7 + i as f32).sin() * 0.02);
                let base = self.presnap.get(i).copied().unwrap_or(self.players[i].pos);
                self.players[i].pos = base + jitter;
            }
            self.walk_presnap_defender(dt);
            self.check_presnap_encroachment();
            if self.result.is_some() {
                return self.result.as_ref();
            }
            if self.awaiting_snap {
                // Hold at the line: every timing constant in the play is measured
                // from SNAP_DELAY, so the clock stops here and starts again when
                // the player asks for the ball.
                self.t = self.t.min(SNAP_DELAY);
            } else if self.t >= SNAP_DELAY + self.presnap_wait() {
                self.snap();
                // The snap can be the foul itself: hand the flag back with it.
                return self.result.as_ref();
            }
            return None;
        }

        // A tackle in flight: the two men are still going to the ground, so
        // nothing else on the field moves and the ball is not dead until they are.
        if self.wrap.is_some() {
            self.advance_wrap(dt);
            return self.result.as_ref();
        }

        // A loose ball is the play. Nobody has it, everybody is after it, and it
        // is not dead until a man comes up with it and is put on the ground.
        if self.phase == Phase::Loose {
            self.update_loose(dt);
            return self.result.as_ref();
        }

        // An interception is not the end of the play: the man who picked it off
        // has the ball, and the men who threw it are the ones chasing now.
        if self.phase == Phase::Return {
            self.update_return(dt);
            self.update_ball(dt);
            return self.result.as_ref();
        }

        // The sell runs on the play's own clock, which starts here.
        if let Some((_, left)) = self.pa_fake.as_mut() {
            *left = (*left - dt).max(0.0);
        }
        self.update_offense(dt);
        self.update_defense(dt);
        self.play_action_bite(dt);
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
        if self.result.is_none() && self.falling.is_none() {
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
            let outcome = if self.loose_recovery == Some(Side::Offense) {
                // He fell on a loose ball earlier in the play, so this is a
                // fumble however far he got after picking it up.
                PlayOutcome::FumbleRecovered
            } else if self.pass_caught {
                PlayOutcome::PassComplete
            } else if self.scramble_started && h == self.qb {
                PlayOutcome::QbScramble
            } else {
                PlayOutcome::Run
            };
            let msg = if self.loose_recovery == Some(Side::Offense) {
                format!(
                    "{name} comes up with the loose ball and is run down {} yards past it.",
                    gain.round().abs() as i32
                )
            } else if gain < -0.5 {
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

    /// Run the last of a dead play out.
    ///
    /// The result is booked the frame the ball is dead, but the men on the field
    /// are still moving: the tackle is still going to the ground, the runner is
    /// still sliding, the ball is still settling. Freezing the picture there is
    /// what made a dead ball read as a cut to the next screen. The client calls
    /// this during the dead-ball beat, so the play comes to rest before the word
    /// over it is read.
    pub fn settle(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.05);
        self.pump_t = (self.pump_t - dt).max(0.0);
        for p in self.players.iter_mut() {
            p.stun = (p.stun - dt).max(0.0);
            p.tackle_cooldown = (p.tackle_cooldown - dt).max(0.0);
            p.beaten = (p.beaten - dt).max(0.0);
            if p.down {
                continue;
            }
            // Ground friction, not a brake: they coast to a stop.
            p.vel *= (1.0 - 4.5 * dt).clamp(0.0, 1.0);
            if p.vel.length() < 0.05 {
                p.vel = Vec2::ZERO;
            }
            p.pos += p.vel * dt;
            p.run_phase = (p.run_phase + p.vel.length() * dt / STRIDE_YARDS).rem_euclid(1.0);
        }
        if let Some(h) = self.ball.holder {
            self.ball.pos = self.players[h].pos;
            self.ball.z = 1.0;
        }
    }

    /// The quarterback pumps the arm without letting go of the ball.
    ///
    /// Nothing is thrown, so no pass is charged to him and the play does not
    /// change. The men in front of him buy it: a defender whose feet are already
    /// committed takes a step he cannot get back, which is the reason to pump.
    /// It costs him the half second the fake takes.
    pub fn pump_fake(&mut self) {
        let Some(holder) = self.ball.holder else { return };
        if holder != self.qb
            || self.ball.flight.is_some()
            || self.result.is_some()
            || self.phase == Phase::PreSnap
            || self.pump_t > 0.0
        {
            return;
        }
        self.pump_t = PUMP_TIME;
        let qb_pos = self.players[self.qb].pos;
        let mut bit = 0;
        for d in 7..14 {
            if self.players[d].down {
                continue;
            }
            if self.players[d].pos.dist(qb_pos) < 7.0 {
                self.players[d].beaten = self.players[d].beaten.max(0.30);
                bit += 1;
            }
        }
        let text = if bit > 0 {
            format!("{} pumps the arm and {} bite", self.name_of(self.qb), bit)
        } else {
            format!("{} pumps the arm", self.name_of(self.qb))
        };
        self.events.push(SimEvent { t: self.t, text });
    }

    /// How long the CPU offense waits at the line before it snaps it.
    fn presnap_wait(&self) -> f32 {
        if self.human_defender.is_some() {
            PRESNAP_WAIT
        } else {
            0.0
        }
    }

    /// The defender on the pad answers the stick before the snap, inside the
    /// neutral zone. He may lean across the line; what he may not do is still be
    /// there when the ball goes, or run into a man on the other side of it.
    fn walk_presnap_defender(&mut self, dt: f32) {
        let Some(d) = self.human_defender else { return };
        if self.players[d].down || self.players[d].stun > 0.0 {
            return;
        }
        let ctl = self.def_control;
        let mag = ctl.map(|c| c.move_dir.length().min(1.0)).unwrap_or(0.0);
        if mag > 0.18 {
            let dir = ctl.map(|c| c.move_dir).unwrap_or(Vec2::ZERO).normalize_or_zero();
            let target = self.players[d].pos + dir * 3.0;
            let spd = self.speed_cap(d) * (0.30 + 0.30 * mag);
            self.steer(d, target, spd, dt);
        } else {
            self.players[d].vel *= (1.0 - 9.0 * dt).clamp(0.0, 1.0);
            if self.players[d].vel.length() < 0.03 {
                self.players[d].vel = Vec2::ZERO;
            }
        }
        // He may go where he likes before the snap, including across the line.
        // What makes it a foul is putting a hand on somebody, not the depth of
        // his alignment: pinning him to the line was being read as being stuck.
    }

    /// Contact across the line before the snap is encroachment, and it is
    /// whistled the moment it happens rather than at the snap.
    fn check_presnap_encroachment(&mut self) {
        let Some(d) = self.human_defender else { return };
        // You cannot foul from your own side of the ball, and the defense lines
        // up beyond the line: standing over it on his side is not being across.
        if (self.players[d].pos.x - self.los) * self.dirf() >= -0.12 {
            return;
        }
        let touched = (0..7).any(|i| {
            !self.players[i].down && self.players[i].pos.dist(self.players[d].pos) < PRESNAP_TOUCH
        });
        if touched {
            self.call_encroachment(d);
        }
    }

    /// The play clock ran out before the ball was snapped. Five yards on the
    /// offense and the down is played again: nothing happened, so nothing after
    /// the count counts.
    fn call_delay_of_game(&mut self) {
        if self.result.is_some() {
            return;
        }
        let mut call = PenaltyCall::new(PenaltyKind::DelayOfGame, self.los);
        call.against = FoulTeam::Offense;
        self.flag = Some(call);
        self.events.push(SimEvent {
            t: self.t,
            text: "The play clock runs out before the snap".to_string(),
        });
        self.finish(
            PlayOutcome::Penalty,
            self.los,
            "Delay of game: five yards, and the down is played again.".to_string(),
        );
    }

    /// A man was past the line when the ball went: five yards and the down again.
    fn call_offsides(&mut self, i: usize, side: FoulTeam) {
        let name = self.name_of(i);
        let spot = self.players[i].pos.x;
        let kind = match side {
            FoulTeam::Offense => PenaltyKind::FalseStart,
            FoulTeam::Defense => PenaltyKind::OffsideDefense,
        };
        let mut call = PenaltyCall::new(kind, spot).with_player(self.roster_index(i));
        call.against = side;
        self.flag = Some(call);
        self.events.push(SimEvent {
            t: self.t,
            text: format!("{name} is past the line when the ball goes"),
        });
        self.finish(
            PlayOutcome::Penalty,
            self.los,
            format!("{name} is across at the snap: offside, five yards."),
        );
    }

    /// Whistle the man who leaned, whatever shirt he is wearing. An offensive
    /// man across before the snap is a false start and a defensive one is
    /// encroachment, so the side comes from the man the flag names rather than
    /// from the side the count was aimed at.
    fn whistle_jump(&mut self, i: usize) {
        match self.players[i].side {
            Side::Offense => self.call_false_start(i),
            Side::Defense => self.call_encroachment(i),
        }
    }

    /// One of ours flinched at his own count: five yards on the offense, the
    /// down played again, and the ball never leaves the ground.
    fn call_false_start(&mut self, i: usize) {
        let name = self.name_of(i);
        let spot = self.players[i].pos.x;
        let mut call = PenaltyCall::new(PenaltyKind::FalseStart, spot)
            .with_player(self.roster_index(i));
        call.against = FoulTeam::Offense;
        self.flag = Some(call);
        self.events.push(SimEvent {
            t: self.t,
            text: format!("{name} moves before the snap"),
        });
        self.finish(
            PlayOutcome::Penalty,
            self.los,
            format!("{name} jumps the count: false start, five yards."),
        );
    }

    /// Book the foul: five yards, replay the down, and no snap. The defense is
    /// the offending side even though the rulebook's default for the word is the
    /// offense jumping early, so the side is set here rather than read off it.
    fn call_encroachment(&mut self, d: usize) {
        let name = self.name_of(d);
        let spot = self.players[d].pos.x;
        let mut call = PenaltyCall::new(PenaltyKind::Encroachment, spot)
            .with_player(self.roster_index(d));
        call.against = FoulTeam::Defense;
        self.flag = Some(call);
        self.events.push(SimEvent {
            t: self.t,
            text: format!("{name} is across the line before the snap"),
        });
        self.finish(
            PlayOutcome::Penalty,
            self.los,
            format!("{name} jumps the count: encroachment, five yards."),
        );
    }

    fn snap(&mut self) {
        // The ball is hiked, so being past the line is a foul now. Depth was legal
        // while the men were set, and a hand on somebody is the encroachment that
        // is checked as he walks; the snap is what makes standing across it offside.
        let mut across = None;
        for i in 0..14 {
            let p = &self.players[i];
            if p.down {
                continue;
            }
            // The defense lines up beyond the line as a matter of course, so it is
            // offside when it is on the offense's side of it. The offense is the
            // other way round.
            let along = (p.pos.x - self.los) * self.dirf();
            let over = if i < 7 { along > SNAP_OFFSIDE } else { along < -SNAP_OFFSIDE };
            if over {
                across = Some((i, if i < 7 { FoulTeam::Offense } else { FoulTeam::Defense }));
                break;
            }
        }
        if let Some((i, side)) = across {
            self.call_offsides(i, side);
            return;
        }
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
        // The computer's own option, made before anyone moves: the read is of the
        // men in front of it, so it belongs on the frame the mesh would happen.
        if self.control.is_none() {
            self.ai_option();
        }
        // The mesh point. The handoff is scheduled after the snap, so it has to be
        // completed here rather than at the snap itself: a run play gives the ball
        // to the back on its own beat, while a play-action fake schedules one too
        // and the quarterback keeps it.
        // A play action is not a handoff: the fake is sold and the ball is
        // thrown, so only the run families mesh here.
        let a_run = matches!(
            self.play.family,
            PlayFamily::RunInside | PlayFamily::RunOutside | PlayFamily::QbRun
        );
        // Cleared every frame and set again by the mesh below: a give is only
        // being offered while the exchange is still to be made.
        self.players[self.qb].give = 0.0;
        if self.carrier.is_none() && a_run {
            if let Some((idx, at)) = self.handoff {
                let kind = self.play.handoff;
                let hand = self.players[self.qb].pos;
                let man = self.players[idx].pos;
                // A handoff is two men meeting, not the ball moving on a timer:
                // the quarterback's path brings him to the runner, and how
                // close they have to get is what kind of give it is. A lateral
                // is a toss, so it can be caught from further away and with the
                // runner already level with the ball; a handoff is an exchange
                // behind the line.
                // A handoff is made at arm's length, where two men meet at the
                // mesh; a toss is out in the air long enough to be caught from
                // a little further away.
                let reach = if kind == Handoff::Lateral { 3.5 } else { 3.0 };
                let legal = kind == Handoff::Lateral
                    || (man.x - self.los) * self.dirf() <= 0.0;
                // The ball goes the runner's way: a quarterback standing on the
                // far side of him would be handing it back across him, against
                // the run. He steps round to the runner's inside and gives it
                // from there, so the ball travels the way the play does.
                let side = self.runner_side(idx);
                let across_him = side != 0.0 && (hand.y - man.y) * side > 0.8;
                let there = man.dist(hand) <= reach && !across_him;
                // How much of the ball he is showing: against his chest at arm's
                // length, out in front of him at the mesh, so the ball is
                // travelling the runner's way before it is his.
                self.players[self.qb].give =
                    ((reach - man.dist(hand)) / (reach * 0.7)).clamp(0.0, 1.0);
                // The ball is shown before it is taken: the exchange waits for
                // it to be out, so a handoff is made at arm's length instead of
                // being a pass made at three yards. A toss is the exception --
                // it is out in the air, so the runner can be level with it.
                let offered = kind == Handoff::Lateral || self.players[self.qb].give >= 0.55;
                if self.t >= at && !self.players[idx].down && legal && there && offered {
                    self.carrier = Some(idx);
                    self.ball.holder = Some(idx);
                    self.handoff = None;
                    let name = self.name_of(idx);
                    self.events.push(SimEvent {
                        t: self.t,
                        text: match kind {
                            Handoff::Lateral => format!("{name} takes the toss"),
                            Handoff::Inside => format!("{name} takes the inside handoff"),
                            Handoff::Forward => format!("{name} takes the handoff"),
                            _ => format!("{name} takes the handoff"),
                        },
                    });
                } else if self.t >= at + 3.0 {
                    // He never came for it: the quarterback keeps the ball rather
                    // than standing there holding a handoff nobody took.
                    self.handoff = None;
                    self.carrier = Some(self.qb);
                    let name = self.name_of(idx);
                    let qb_name = self.name_of(self.qb);
                    self.events.push(SimEvent {
                        t: self.t,
                        text: format!("{qb_name} keeps it: {name} never came for the handoff"),
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
                // He runs the start-and-run pattern the play was drawn with, and
                // the handoff is made where it brings him to the runner: the
                // drawing decides where they meet, not a clock. With no path
                // drawn he stands his ground and meets the runner at the mesh,
                // stepping toward him only once he is close enough to get there.
                let (car, _) = self.handoff.unwrap();
                let path = self.players[i].route.clone();
                if self.players[i].waypoint < path.len() {
                    let target = path[self.players[i].waypoint];
                    self.steer(i, target, self.speed_cap(i) * 0.95, dt);
                    if self.players[i].pos.dist(target) < 0.6 {
                        self.players[i].waypoint += 1;
                    }
                } else if self.players[i].pos.dist(self.players[car].pos) < 8.0 {
                    // The meeting point is on the runner's inside, not straight
                    // behind him: a back sweeping left is handed the ball from
                    // his right, so the give runs left with him.
                    let side = self.runner_side(car);
                    let target = self.players[car].pos
                        + Vec2::new(-self.dirf() * 0.9, -side * 0.7);
                    self.steer(i, target, self.speed_cap(i) * 0.9, dt);
                }
                // His shoulders turn to the man he is giving it to, whatever way
                // his feet are going. The give is made with the body: turned to
                // the runner, the ball is already going his way when it leaves.
                let to = self.players[car].pos - self.players[i].pos;
                let near = to.length();
                if near > 0.2 && near < 4.0 {
                    self.players[i].heading = turn_towards(
                        self.players[i].heading,
                        to.y.atan2(to.x),
                        9.0 * dt,
                    );
                }
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
                // The sell comes before the drop: the ball is shown to the man
                // coming across and pulled back before he turns away, so a play
                // action is a run sold with the quarterback's body.
                if self.selling_fake() {
                    self.fake_step(i, dt);
                    return;
                }
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
        // The fake is not the player's to skip: the ball has to be shown before
        // it can be thrown, so the stick and the button wait for the sell.
        let selling = self.selling_fake();
        let mag = ctl.move_dir.length().min(1.0);
        if selling {
            self.fake_step(i, dt);
        } else if mag > 0.18 {
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
        if !selling && past_los && self.phase == Phase::Dropback && self.t > SNAP_DELAY + 0.4 {
            self.start_scramble(i);
            return;
        }

        // Taking off does not end the play: a quarterback on the move can still
        // let it go downfield, which is half of what the scramble is for.
        let can_throw = self.phase == Phase::Dropback
            || (self.phase == Phase::Scramble && self.carrier == Some(i));
        if ctl.throw && can_throw && !selling && self.t > SNAP_DELAY + 0.25 {
            if let Some(target) = ctl.target {
                if !self.players[target].down {
                    // The pad's arc means nothing to an AI arm: it throws a
                    // normal pass, so the button's lob is the human's alone.
                    self.throw_to_with(i, Some(target), 0.45);
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
        // The button names the man; the meter is the player's, not the sim's.
        self.throw_to_with(qb, target, 0.8);
    }

    /// What a throw of this weight at this man would do.
    ///
    /// The ball goes to the receiver the button named, and leading him is the
    /// sim's job, not the player's: a throw at a moving man is aimed where he
    /// will be, not where he is. How far it carries is the arm and nothing else
    /// -- the button is the arc now -- so a weak arm physically cannot reach a
    /// man downfield and a strong one can, whatever the button is doing.
    pub fn throw_plan(&self, qb: usize, target: usize, _touch: f32) -> ThrowPlan {
        let qb_pos = self.players[qb].pos;
        let arm = self.attrs(qb).arm_strength();
        let receiver = self.players[target].pos;
        let speed = 16.0 + arm * 0.34;
        let rough = (receiver - qb_pos).length().max(0.1);
        let lead = self.players[target].vel * (rough / speed).clamp(0.25, 2.4) * 0.75;
        let wanted = receiver + lead;
        let offset = wanted - qb_pos;
        let distance = offset.length();
        let reach = 9.0 + arm * 0.62;
        let landing = qb_pos + offset.normalize_or_zero() * distance.min(reach);
        let landing = Vec2::new(
            clamp(landing.x, -field::ENDZONE_DEPTH, FIELD_LENGTH + field::ENDZONE_DEPTH),
            clamp(landing.y, -FIELD_HALF_WIDTH, FIELD_HALF_WIDTH),
        );
        let air_yards = ((landing.x - qb_pos.x) * self.dirf()).max(0.0);
        let power_needed = (distance / reach).clamp(0.0, 1.0);
        ThrowPlan {
            landing,
            air_yards,
            distance,
            reach,
            power_needed,
        }
    }

    /// Send the ball to a man at a given arc. Public so the tests can measure
    /// the flight without going through a pad.
    pub fn throw_to_with(&mut self, qb: usize, target: Option<usize>, power: f32) {
        // A new throw is a new chance for somebody to get a hand on it.
        self.pass_contacted = false;
        self.ball_hands = None;
        let qb_pos = self.players[qb].pos;
        // The button is the arc, not the arm. It arrives here called power, but
        // nothing below uses it for power any more.
        let touch = power.clamp(0.0, 1.0);

        let (to, air, receiver) = match target {
            Some(t) => {
                let plan = self.throw_plan(qb, t, touch);
                let accuracy = self.attrs(qb).accuracy_factor();
                let pressure = self.qb_pressure();
                // A soft throw hangs and drifts; a rope arrives flat and can run
                // past the man. Either way the ball goes where the arm sent it.
                let sigma = 0.20
                    + plan.air_yards * 0.024
                    + (1.0 - accuracy) * 1.4
                    + (2.5 - pressure).max(0.0) * 0.30
                    + touch * 0.35;
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
        // The button shapes the flight; the arm gives it its speed. A tap puts
        // the ball on a line between the receiver's hips and knees, held down it
        // is the high one that comes down over a linebacker's hands, and what
        // leaves his hand is his arm either way -- so a weak arm's lob hangs long
        // enough to be played on, and a strong arm thrown flat is a rope.
        let speed = (14.0 + arm * 0.34 - 5.0 * touch).max(9.0);
        // Time the flight on the grass the ball actually has to cover. Timing it on
        // the downfield component alone made a swing pass to the flat, which covers
        // twelve yards sideways and two downfield, arrive like a fastball -- and a
        // floater over the middle is the whole point of the soft end of the meter.
        let carry = (to - qb_pos).length().max(1.0);
        let dur = (carry / speed).clamp(0.35, 3.0);
        let peak = 0.5 + carry * 0.02 + touch * (2.0 + carry * 0.22);
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
    ///
    /// A man the drawing pointed at is the exception, because his run is the
    /// play: he goes on to the next point of his own path, and once the path is
    /// behind him he carries on the way he was going. A sweep then comes round
    /// the corner without the player holding the stick, instead of stalling at
    /// the mesh until he is steered. The stick still owns him the moment it
    /// moves: this is only what he does with nobody's hand on him.
    fn update_carrier_idle(&mut self, i: usize, dt: f32) {
        if self.is_drawn_carrier(i) && !self.players[i].route.is_empty() {
            let mut wp = self.players[i].waypoint;
            while wp < self.players[i].route.len() {
                // A point he is standing on is behind him, whatever the drawing
                // said: the last leg of a path is followed by its direction.
                if self.players[i].pos.dist(self.players[i].route[wp]) > 1.0 {
                    break;
                }
                wp += 1;
            }
            self.players[i].waypoint = wp;
            let target = if wp < self.players[i].route.len() {
                self.players[i].route[wp]
            } else {
                let ahead = if self.players[i].vel.length() > 0.4 {
                    self.players[i].vel.normalize_or_zero()
                } else {
                    Vec2::new(self.dirf(), 0.0)
                };
                self.players[i].pos + ahead * 2.0
            };
            let speed = self.speed_cap(i) * 0.9;
            self.carry_step(i, dt, target, speed);
            return;
        }
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
        let Some(holder) = self.ball.holder else { return };
        let vel = self.players[holder].vel;
        let facing = if vel.length() > 0.4 { vel } else { Vec2::new(self.dirf(), 0.0) };
        self.start_move(kind, facing);
    }

    /// A move aimed across the runner's own line: a cut and a juke go to a side
    /// rather than down the field, so the caller names the side and the sim turns
    /// it into a direction he can actually break toward.
    pub fn request_move_dir(&mut self, kind: Kind, side: f32) {
        let Some(holder) = self.ball.holder else { return };
        let vel = self.players[holder].vel;
        let facing = if vel.length() > 0.4 { vel } else { Vec2::new(self.dirf(), 0.0) };
        let f = facing.normalize_or_zero();
        let across = Vec2::new(-f.y, f.x) * if side < 0.0 { -1.0 } else { 1.0 };
        self.start_move(kind, across);
    }

    fn start_move(&mut self, kind: Kind, dir: Vec2) {
        // The man with the ball is the man the buttons belong to. Anything else
        // lets a live-looking button do nothing, which is how a dead button feels.
        let Some(holder) = self.ball.holder else { return };
        if self.ball.flight.is_some() || self.result.is_some() || !self.face_buttons_are_moves() {
            return;
        }
        // A quarterback does not dive. The same button gives himself up instead,
        // and it is the same button whether he is in the pocket or past the line.
        let kind = if kind == Kind::Dive && holder == self.qb {
            Kind::Slide
        } else {
            kind
        };
        if !self.carrier_moves.start(kind, dir) {
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
            Kind::StiffArm => format!("{name} puts a hand out"),
            Kind::Cut => format!("{name} plants and cuts"),
            Kind::Juke => format!("{name} jumps outside"),
            Kind::Stop => format!("{name} stops dead"),
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
        // Only a point that is ahead of him and not already under his feet is
        // worth running to. A route that starts behind the man who just caught it
        // used to turn the carrier round and send him back toward the line of
        // scrimmage, which is the one direction a ball carrier never runs.
        let mut wp = p.waypoint;
        while wp < p.route.len() {
            let pt = p.route[wp];
            if (pt.x - p.pos.x) * dirf > -0.35 && pt.dist(p.pos) > 0.6 {
                break;
            }
            wp += 1;
        }
        let mut base = if wp < p.route.len() {
            p.route[wp]
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
        // A man the play is drawn for on a run runs that path before the ball is
        // his: the handoff finds him on the move, so a jet sweep comes across the
        // quarterback's face instead of drifting upfield waiting to be handed a
        // ball he never went to get.
        let assignment = match self.play.assignment(role) {
            // Only on runs: on a play action the man with the ball drawn for him
            // is running the fake, and how deep he goes is the play's business.
            Some(Assign::Carry { .. })
                if matches!(
                    self.play.family,
                    PlayFamily::RunInside | PlayFamily::RunOutside | PlayFamily::QbRun
                ) =>
            {
                Some(Assign::Route {
                    points: Vec::new(),
                    depth: 1,
                })
            }
            other => other.cloned(),
        };
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

    /// Is this man in the block, rather than running a route or holding the ball?
    fn blocker_on_this_play(&self, i: usize) -> bool {
        let role = OffRole::ALL[self.players[i].role_index];
        matches!(
            self.play.assignment(role),
            Some(Assign::PassBlock { .. })
                | Some(Assign::RunBlock { .. })
                | Some(Assign::ScreenBlock { .. })
        ) || (role != OffRole::Qb && self.carrier.is_some() && self.play.family.is_run())
    }

    /// Blockers mirror the threat to their aim point.
    ///
    /// On Easy it is the nearest body, which is what a line of men who have never
    /// spoken to each other manages. On Normal and Hard the line counts inside
    /// out: the man nearest the ball picks first, and every blocker after him
    /// takes the most inside rusher nobody else has. Without that, two linemen
    /// doubled one rusher while a third came through the gap they left.
    fn block_target(&self, i: usize, aim: Vec2) -> Vec2 {
        match self.block_threat(i, aim) {
            Some(d) => {
                let threat = self.players[d].pos;
                let to_threat = (threat - self.players[i].pos).normalize_or_zero();
                // Take the defender on the upfield shoulder.
                threat - to_threat * 0.35
            }
            None => aim,
        }
    }

    /// Which rusher this blocker takes. On Easy that is simply the closest; on
    /// the harder levels the line works inside out, so every man has his own.
    fn block_threat(&self, i: usize, _aim: Vec2) -> Option<usize> {
        // The closest man to him, but the man in his path first: the nearest body
        // is only a blocker's man if he is between the blocker and what he is
        // protecting. Without that the line picks up a rusher out wide while the
        // one in the quarterback's face runs through untouched.
        let ahead = Vec2::new(self.dirf(), 0.0);
        let mut men: Vec<(usize, f32, bool)> = (7..14)
            .filter(|&d| {
                !self.players[d].down
                    && self.players[d].stun <= 0.0
                    && self.players[d].beaten <= 0.0
                    && !(self.players[d].engaged.is_some() && self.players[d].engaged != Some(i))
            })
            .map(|d| {
                let rel = self.players[d].pos - self.players[i].pos;
                let front = rel.normalize_or_zero().dot(ahead) > 0.15;
                (d, rel.length(), front)
            })
            .filter(|(_, dist, _)| *dist < 4.5)
            .collect();
        men.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        });
        men.first().map(|(d, _, _)| *d)
    }

    // -----------------------------------------------------------------
    // Defense
    // -----------------------------------------------------------------

    /// Bodies cannot occupy the same ground. Two men come apart at the
    /// shoulders (twice the body radius), and a pair in a wrap is held outside
    /// this by `HOLD_DISTANCE`. Only two men both on the ground may share it:
    /// a standing defender used to be allowed inside the man who had just been
    /// tackled, which is the overlap that made a pile read as one body.
    fn separate_bodies(&mut self) {
        let touch = BODY_RADIUS * 2.0;
        for a in 0..14 {
            for b in (a + 1)..14 {
                if self.players[a].down && self.players[b].down {
                    continue;
                }
                let delta = self.players[b].pos - self.players[a].pos;
                let d = delta.length();
                if d < touch && d > 1e-4 {
                    let push = delta.normalize_or_zero() * (touch - d) * 0.5;
                    self.players[a].pos -= push;
                    self.players[b].pos += push;
                }
            }
        }
    }

    /// A play action is a lie told with the ball. While it is being sold the men
    /// behind the front lean toward it, and the step they take is the daylight
    /// the throw is made into.
    fn play_action_bite(&mut self, dt: f32) {
        if !self.selling_fake() {
            return;
        }
        let Some(holder) = self.ball.holder else {
            return;
        };
        let at = self.players[holder].pos;
        let human = self.human_defender;
        for i in 7..14 {
            if Some(i) == human {
                continue;
            }
            let p = &mut self.players[i];
            if p.down || p.stun > 0.0 || p.wrap > 0.0 || matches!(p.job, CoverageJob::Rush) {
                continue;
            }
            // Only the men close enough to see it: a corner on the far side of
            // the field does not come off his man for a handoff.
            let to = at - p.pos;
            let d = to.length();
            if d > 12.0 || d < 0.3 {
                continue;
            }
            let step = (PA_LEAN * dt).min(d);
            p.pos += to / d * step;
        }
    }

    fn update_defense(&mut self, dt: f32) {
        let carrier = self.carrier;
        let ball_landing = self.ball.flight.as_ref().map(|f| f.to);
        for i in 7..14 {
            if self.players[i].down || self.players[i].stun > 0.0 {
                continue;
            }
            // A man whose hands are on the carrier in a wrap holds his side of
            // the body. Steering him at the ball every frame walked him back
            // through the carrier and they merged into one figure.
            if self.wrap.is_some() && self.players[i].wrap > 0.0 {
                continue;
            }
            // The defender on the pad answers the stick; the rest of the eleven
            // are the coordinator's. With the stick at rest he keeps working to
            // the ball rather than standing still, so handing the pad around
            // never freezes a man on the field.
            if Some(i) == self.human_defender {
                if let Some(ctl) = self.def_control {
                    let mag = ctl.move_dir.length().min(1.0);
                    // A dive throws the body at the man with the ball: a yard of
                    // ground his legs could not cover, and a longer reach for the
                    // tackle worked out below.
                    // A dive is a press, not a hold. Held down it re-armed the
                    // moment the window closed, and every re-arm put the lunge
                    // back to its first sliver: chasing a runner with the button
                    // down left him piling up at the line instead of going
                    // anywhere. Letting go is what loads the next one.
                    // He may chain them, and holding the button while he chases
                    // does exactly that. What broke before was the seam: every
                    // restart reset the ground already flown to zero, so the step
                    // was measured from the wrong end of the curve and he stalled
                    // at the end of each lunge -- press it near the line and he
                    // got there, hold it in the open field and he stopped dead.
                    // Letting go loads the next lunge. It only reloads once he is
                    // back on his feet, though: this used to run on every frame the
                    // button was not held -- which is every frame of a dive, since
                    // the press is one frame and the lunge is a third of a second.
                    // Zeroing the ground already flown meant each frame re-applied
                    // the whole lunge from zero: one press carried him about
                    // twenty-six yards across the field, finishing with a
                    // two-and-a-fifth yard jump on the frame he landed. That is the
                    // flight, and with the old aim it went straight at the carrier.
                    if !ctl.dive && self.players[i].dive_t <= 0.0 {
                        // Letting go lets the lungs fill: the next press is a
                        // fresh lunge from a standstill, not a continuation.
                        self.players[i].dive_armed = true;
                        self.players[i].dive_base = 0.0;
                        self.players[i].dive_fly = 0.0;
                    }
                    if ctl.dive && self.players[i].dive_armed && self.players[i].dive_t <= 0.0 {
                        self.players[i].dive_armed = false;
                        self.players[i].dive_base = self.players[i].dive_fly;
                        // Start the window one frame in. On its first frame a
                        // lunge has flown nothing, so starting at the full window
                        // gave every press -- and every chained lunge -- a frame
                        // in which he did not move at all. That dead frame is the
                        // stall the report felt.
                        self.players[i].dive_t =
                            (DIVE_WINDOW - dt.max(0.0)).max(DIVE_WINDOW * 0.5);
                        // Where the dive goes: at the man with the ball when he is
                        // close enough to be lunged at, and otherwise where the
                        // player is pointing. Aiming at the carrier from anywhere
                        // on the field made every dive a leap at the ball -- a
                        // defender on the far side of the field threw himself
                        // across it -- when a dive is a body thrown from where it
                        // stands, and the ground it covers is the ground under him.
                        let stick = ctl.move_dir;
                        let facing = if self.players[i].vel.length() > 0.4 {
                            self.players[i].vel.normalize_or_zero()
                        } else {
                            Vec2::new(
                                self.players[i].heading.cos(),
                                self.players[i].heading.sin(),
                            )
                        };
                        // The dive is the stick, and nothing else. It is never
                        // aimed at the ball carrier, however close he is: the
                        // ground a dive covers is covered where the man stands and
                        // in the direction he is pointing, and the tackle it makes
                        // is whatever his reach finds from there. Homing on the
                        // carrier -- from across the field, or from three yards --
                        // is the dive the report kept seeing.
                        let aim_at = if stick.length() > 0.18 {
                            self.players[i].pos + stick.normalize_or_zero()
                        } else {
                            // No stick: he dives the way he is facing.
                            self.players[i].pos + facing
                        };
                        let to = aim_at - self.players[i].pos;
                        // The line he leaves his feet on. Nothing moves here: the
                        // ground a dive buys is covered over the next third of a
                        // second, in the movement below. Adding it to his position
                        // in one frame is what made a dive look like a teleport.
                        self.players[i].dive_aim = to.normalize_or_zero();
                    }
                    if self.players[i].dive_t > 0.0 {
                        // In the air his legs are not taking orders. The body is
                        // committed along the line it left on and the stick is
                        // dead until the dive ends.
                        //
                        // The ground is covered the way a body actually covers
                        // it: out hard on the first frames and dying away as he
                        // lands. Spread flat over the window instead, the lunge
                        // was slower than a walk and pressing dive read as the
                        // defender stopping dead.
                        let done = 1.0 - self.players[i].dive_t / DIVE_WINDOW;
                        let flown = self.players[i].dive_base
                            + DIVE_LUNGE * (1.0 - (1.0 - done) * (1.0 - done));
                        let step = (flown - self.players[i].dive_fly).max(0.0);
                        self.players[i].dive_fly = flown;
                        let aim = self.players[i].dive_aim;
                        self.players[i].vel = aim * (step / dt.max(0.0001));
                        self.players[i].pos += aim * step;
                    } else if mag > 0.18 {
                        let target = self.players[i].pos + ctl.move_dir.normalize_or_zero() * 4.0;
                        let spd = self.speed_cap(i) * (0.6 + 0.4 * mag);
                        self.steer(i, target, spd, dt);
                    } else {
                        // Stick at rest: the man on the pad stands his ground.
                        // He used to keep working to the ball on his own, so the
                        // one defender the player was steering walked himself
                        // onto the carrier every time the stick was let go --
                        // the defender stuck to the ball that outlived the aim
                        // fix. A dive starts where he stands; nothing but the
                        // stick moves him. The coordinator's men still pursue.
                        self.steer(i, self.players[i].pos, 0.0, dt);
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
                CoverageJob::Drop => {
                    if let Some(landing) = ball_landing {
                        let spd = self.speed_cap(i);
                        self.steer(i, landing, spd, dt);
                    } else {
                        let next = self.players[i]
                            .route
                            .get(self.players[i].waypoint)
                            .copied();
                        match next {
                            Some(t) => {
                                let spd = self.speed_cap(i) * 0.9;
                                self.steer(i, t, spd, dt);
                                if self.players[i].pos.dist(t) < 0.55 {
                                    self.players[i].waypoint += 1;
                                }
                            }
                            None => {
                                // At the end of the drawn drop, he plays the ball:
                                // a drawn path is where a man goes, not a leash.
                                if let Some(c) = carrier {
                                    let spd = self.speed_cap(i);
                                    let t = self.players[c].pos;
                                    self.steer(i, t, spd, dt);
                                }
                            }
                        }
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
        // A man on the pad is never frozen by a block. He is still being blocked --
        // he does not get his full speed -- but the stick always moves him, which
        // is what was wrong with the defensive lineman who could not get off the
        // line once the computer hiked it.
        let mine = Some(i) == self.human_defender;
        let p = &mut self.players[i];
        if p.engaged.is_some() && !mine {
            // Blocked players shuffle rather than run.
            p.vel *= 0.55;
            p.pos += p.vel * dt;
            return;
        }
        let max_speed = if p.engaged.is_some() { max_speed * 0.7 } else { max_speed };
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
            p.pos.y = clamp(
                p.pos.y,
                -FIELD_HALF_WIDTH - BODY_APRON,
                FIELD_HALF_WIDTH + BODY_APRON,
            );
        }
        self.separate_bodies();
        self.separate_bodies();
        // A shove runs after the wall, so enforce it once more: nobody ends a
        // frame standing in the advertising boards.
        for p in self.players.iter_mut() {
            p.pos.y = clamp(
                p.pos.y,
                -FIELD_HALF_WIDTH - BODY_APRON,
                FIELD_HALF_WIDTH + BODY_APRON,
            );
        }
    }

    /// Who is posed with his hands out, as a blocker: the two linemen on every
    /// play, and the backs and receivers on a run. Never the man carrying the ball,
    /// whoever he is and however the play was called, and never a defender, who is
    /// the one being blocked. Set from the play each frame rather than from the
    /// blocking itself, so a blocker whose hands have not found anybody yet still
    /// stands like one, and the mesh cannot leave a stale flag on the runner.
    fn pose_blockers(&mut self) {
        let snap = !matches!(self.phase, Phase::PreSnap) && self.t > SNAP_DELAY;
        let run = self.play.family.is_run();
        for i in 0..7 {
            let role = OffRole::ALL[self.players[i].role_index];
            let lineman = matches!(role, OffRole::Lg | OffRole::Rg);
            self.players[i].blocking = snap
                && !self.players[i].down
                && self.carrier != Some(i)
                && (lineman || (run && role != OffRole::Qb));
        }
    }

    fn resolve_blocks(&mut self, dt: f32) {
        // Hands out in front belong to blockers: the two linemen on every play, and
        // the backs and receivers on a run -- not the man carrying it, who is
        // running, and not the defense, who are the ones being blocked.
        let run = self.play.family.is_run();
        for i in 0..7 {
            if self.players[i].down || self.players[i].stun > 0.0 {
                continue;
            }
            if !self.blocker_on_this_play(i) {
                continue;
            }
            // On the harder levels nobody doubles up: a defender already leaning
            // on a teammate is somebody else's man.
            let coordinated = self.difficulty != Difficulty::Easy;
            // Find the closest defender to him, inside the reach his hands give
            // him. Arms out in front are wider than a body standing in the way, so
            // a man who is technically a step away is still being blocked.
            let _ = coordinated;
            let target = self
                .block_threat(i, self.players[i].pos)
                .filter(|&d| self.players[d].pos.dist(self.players[i].pos) <= BLOCK_REACH);
            let Some(d) = target else {
                self.players[i].engaged = None;
                self.players[i].hands *= 0.86;
                self.players[i].drive *= 0.86;
                continue;
            };
            self.players[i].engaged = Some(d);
            self.players[d].engaged = Some(i);
            // And he steps into him. Engagement used to start from a body's width
            // away and never close the gap, so a lineman could be blocking a man
            // nobody could see him touch. Only the last of it, though: dragging a
            // man a full yard off his spot every frame took the pocket with it.
            let gap = self.players[d].pos.dist(self.players[i].pos);
            // Hands on him. The old step-in stopped at 0.9 and only started at
            // 1.5, so a blocker past that held off at the end of an invisible
            // rope; now he keeps walking in until his hands are on the man's
            // chest and stays there, which is what a block looks like.
            let hands_on = BODY_RADIUS * 2.0 + 0.18;
            if gap > hands_on {
                let to = (self.players[d].pos - self.players[i].pos).normalize_or_zero();
                let close = (gap - hands_on).min(0.55) * (dt * 9.0).min(1.0);
                self.players[i].pos += to * close;
            }

            // His hands go where the rusher goes: the hand on that side follows
            // him, never more than 45 degrees, and the body comes round after the
            // hands instead of the hands craning off a body facing the wrong way.
            // That is the pocket: a wall that bends with the man in front of it.
            let to_d = self.players[d].pos - self.players[i].pos;
            let mut off = to_d.angle() - self.players[i].heading;
            while off > std::f32::consts::PI {
                off -= std::f32::consts::TAU;
            }
            while off < -std::f32::consts::PI {
                off += std::f32::consts::TAU;
            }
            let want = off.clamp(-std::f32::consts::FRAC_PI_4, std::f32::consts::FRAC_PI_4);
            let k = (dt * 9.0).min(1.0);
            self.players[i].hands += (want - self.players[i].hands) * k;
            self.players[i].heading = approach_angle(self.players[i].heading, to_d.angle(), 3.0 * dt);
            // Run blocking is a shove and pass blocking is a hold, so the hands
            // come up only on runs, when the man in front is being walked back.
            self.players[i].drive += ((run as i32) as f32 - self.players[i].drive) * (dt * 4.0).min(1.0);

            let blocker = self.attrs(i);
            let defender = self.attrs(d);
            let pass_rush = (defender.pass_rush as f32 - blocker.pass_block as f32) / 100.0;
            let run_stuff = (defender.strength as f32 - blocker.run_block as f32) / 100.0;
            let advantage = pass_rush.max(run_stuff);
            let noise = self.rng.gauss() * 0.5;
            self.players[d].pressure += dt * (0.55 + advantage * 1.8) + noise * dt * 0.4;

            // Grind along the line: the defender is steered toward the QB, the
            // blocker mirrors him.
            if run {
                // Hands up and driving: both men go upfield together, the defender
                // because he is being pushed and the blocker because he is pushing.
                let push = self.dirf() * (0.35 + advantage * 1.2) * dt;
                self.players[d].pos.x += push;
                self.players[i].pos.x += push * 0.92;
            } else {
                let qb = self.players[self.qb].pos;
                let toward = (qb - self.players[d].pos).normalize_or_zero();
                let slide = toward.perp();
                let drift = slide * (advantage * 0.9 + 0.15) * dt;
                self.players[d].pos += drift;
                self.players[i].pos -= drift * 0.9;
            }

            if self.players[d].pressure > 1.0 {
                self.players[d].pressure = 0.0;
                self.players[d].beaten = 0.0;
                self.players[d].engaged = None;
                self.players[i].engaged = None;
                self.players[i].stun = 0.45;
                // A hold while getting beaten is the classic 7 on 7 10-yard foul.
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
            // A diving man has his arms out in front of him, so a lunge reaches
            // further than a wrap; a man on his feet wraps what his shoulder can.
            let reach = TACKLE_RADIUS
                + if self.players[d].dive_t > 0.0 { ARM_REACH } else { 0.0 };
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
            // Wrap tackling: 7 on 7 requires an actual attempt to wrap up.
            let closing = (self.players[d].vel - self.players[carrier].vel).length();
            let mut p = 0.45
                + (defender_attrs.tackle_factor() - carrier_attrs.break_tackle_factor()) * 0.55
                + (closing * 0.02).min(0.15);
            // A dive reaches further and holds on less: it is a lunge, not a wrap.
            if Some(d) == self.human_defender && self.players[d].dive_t > 0.0 {
                p = (p - 0.12).max(0.05);
            }
            // A stiff arm is a shove, not a tackle: the free hand goes into the
            // first man's chest and pushes him off his line. It does not end the
            // play, and it does not stop the runner.
            if self.carrier_moves.kind() == Some(Kind::StiffArm) {
                let away = (self.players[d].pos - self.players[carrier].pos).normalize_or_zero();
                self.players[d].pos += away * 0.70;
                self.players[d].vel += away * 1.4;
                self.players[d].stun = 0.45;
                self.players[d].tackle_cooldown = 0.35;
                self.players[carrier].vel *= 0.95;
                self.events.push(SimEvent {
                    t: self.t,
                    text: format!("{} stiff-arms {}!", self.name_of(carrier), self.name_of(d)),
                });
                continue;
            }
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

            // The right stick behind the tackle goes for the ball, and aimed high
            // it goes over the nameplate: that is the hit that gets flagged.
            let hit = if Some(d) == self.human_defender {
                self.def_control.and_then(|c| c.hit)
            } else {
                None
            };
            // No-pads rules: a tackle above the nameplate is unnecessary roughness.
            let mut roughness = 0.02 * (1.2 - defender_attrs.discipline as f32 / 100.0);
            if hit == Some(Hit::High) {
                roughness *= 3.0;
            }
            let high_tackle = self.rng.chance(roughness);
            let spot = clamp(
                self.players[carrier].pos.x,
                -field::ENDZONE_DEPTH,
                FIELD_LENGTH + field::ENDZONE_DEPTH,
            );
            let fumbled = self.rng.chance(fumble_chance(
                &carrier_attrs,
                &defender_attrs,
                hit,
                self.difficulty.fumble_bonus(),
            ));
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

            let outcome = if self.loose_recovery == Some(Side::Offense) {
                // The ball came out earlier in this play and he fell on it: the
                // play is a fumble however far he ran after picking it up.
                PlayOutcome::FumbleRecovered
            } else if sack {
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
                PlayOutcome::FumbleRecovered => format!(
                    "{} comes up with the loose ball and is stopped by {} {} yards from the spot.",
                    self.name_of(carrier),
                    self.name_of(d),
                    yards.round().abs() as i32
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
        // The tackler is on him the moment the wrap starts. The radius the roll
        // is made inside is contact range, but the wrap used to begin wherever he
        // happened to be and then slide him in, which read as the runner being
        // held in place by nothing. Close the gap on the frame of the hit instead:
        // the reach is the arm's, and it is spent arriving, not drifting.
        // Unless he is in the air. A dive reaches further than a man on his feet,
        // so snapping him to arm's length would be a teleport across a yard: the
        // lunge is his arrival, and the wrap closes from wherever it puts him.
        let diving = self.players[tackler].dive_t > 0.0;
        let gap = self.players[tackler].pos.dist(self.players[carrier].pos);
        if !diving && gap > HOLD_DISTANCE {
            let toward = (self.players[tackler].pos - self.players[carrier].pos)
                .normalize_or_zero();
            self.players[tackler].pos =
                self.players[carrier].pos + toward * HOLD_DISTANCE;
        }
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
        let contact = spot - away * HOLD_DISTANCE;
        let d = &mut self.players[w.tackler];
        d.pos = d.pos.lerp(contact, (dt * 7.0).min(1.0));
        if away.length() > 0.01 {
            d.heading = away.y.atan2(away.x);
        }
        // Whoever else has arrived gets a hand on him too. Three men driving at
        // one point end up inside each other and read as a blob; each takes his
        // own side of the body instead, so a pile looks like men holding a man.
        let front = away.angle();
        let mut helpers: Vec<usize> = (7..14)
            .filter(|&q| {
                q != w.tackler
                    && q != w.carrier
                    && !self.players[q].down
                    && self.players[q].stun <= 0.0
                    && self.players[q].pos.dist(spot) < 3.0
            })
            .collect();
        helpers.sort_by(|a, b| {
            let da = self.players[*a].pos.dist(spot);
            let db = self.players[*b].pos.dist(spot);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        helpers.truncate(3);
        let count = (helpers.len() + 1) as f32;
        for (k, q) in helpers.iter().enumerate() {
            let side = if k % 2 == 0 { 1.0 } else { -1.0 };
            let ring = (k / 2 + 1) as f32;
            let angle = front + side * ring * (std::f32::consts::TAU / count);
            let want = spot + Vec2::new(angle.cos(), angle.sin()) * HOLD_DISTANCE;
            let him = &mut self.players[*q];
            him.pos = him.pos.lerp(want, (dt * 6.0).min(1.0));
            // A body has a width: if the lerp or the carrier's own movement put
            // him inside it, he is seated back out at hands length.
            let off = him.pos - spot;
            if off.length() < HOLD_DISTANCE {
                let dir = if off.length() > 1e-3 {
                    off.normalize_or_zero()
                } else {
                    Vec2::new(angle.cos(), angle.sin())
                };
                him.pos = spot + dir * HOLD_DISTANCE;
            }
            him.heading = (spot.y - him.pos.y).atan2(spot.x - him.pos.x);
            him.wrap = p;
            him.tackle_arms = 1.0;
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
        // He is the one going to the ground with the ball, so he keeps it. Clear
        // the holder and the ball is drawn loose at his feet, which reads as the
        // tackler standing over the pile holding it.
        self.ball.holder = Some(carrier);
        self.ball.pos = self.players[carrier].pos;
        self.ball.z = 0.55;
    }

    /// The ball comes out. Nothing is decided here: it goes on the ground, both
    /// teams go and get it, and whoever comes up with it runs with it until he is
    /// tackled. A strip is the start of a play, not the end of one.
    fn fumble(&mut self, carrier: usize, defender: usize) {
        let spot = self.players[carrier].pos;
        let desc = format!(
            "{} knocks it out of {}'s hands -- the ball is loose!",
            self.name_of(defender),
            self.name_of(carrier)
        );
        self.fumble_alert = 2.4;
        self.fumble_note = desc.clone();
        self.start_loose(spot, desc);
    }

    /// Put the ball on the turf and let the play run.
    fn start_loose(&mut self, at: Vec2, description: String) {
        if self.result.is_some() {
            return;
        }
        self.carrier = None;
        self.intended = None;
        self.ball.holder = None;
        self.ball.flight = None;
        self.ball.pos = at;
        self.ball.z = 0.22;
        self.phase = Phase::Loose;
        // It does not stop where it came out: it squirts backwards and to one
        // side, which is what turns a fumble into a scramble.
        let vel = Vec2::new(
            -self.dirf() * self.rng.range(1.2, 4.5),
            self.rng.range(-4.5, 4.5),
        );
        self.loose = Some(LooseBall { at, vel, t: 0.0 });
        self.events.push(SimEvent { t: self.t, text: description });
    }

    fn update_loose(&mut self, dt: f32) {
        let Some(mut loose) = self.loose.clone() else {
            return;
        };
        loose.t += dt;
        loose.at += loose.vel * dt;
        loose.vel *= 0.90f32.powf(dt * 60.0);
        let edge = FIELD_HALF_WIDTH - 0.35;
        if loose.at.y.abs() > edge {
            loose.at.y = loose.at.y.clamp(-edge, edge);
            loose.vel.y = -loose.vel.y * 0.35;
        }
        self.ball.pos = loose.at;
        self.ball.z = 0.22;
        // Both teams go and get it. There is nothing else on the field to do.
        for i in 0..14 {
            if self.players[i].down {
                continue;
            }
            let speed = self.speed_cap(i) * 0.95;
            self.steer(i, loose.at, speed, dt);
        }
        self.integrate(dt);
        if loose.t >= LOOSE_GRACE {
            let mut closest: Option<(usize, f32)> = None;
            for i in 0..14 {
                if self.players[i].down {
                    continue;
                }
                let dist = self.players[i].pos.dist(loose.at);
                if dist <= RECOVER_RADIUS && closest.map(|(_, b)| dist < b).unwrap_or(true) {
                    closest = Some((i, dist));
                }
            }
            if let Some((i, _)) = closest {
                self.loose = None;
                self.recover_loose(i, loose.at);
                return;
            }
        }
        self.loose = Some(loose);
    }

    /// Somebody came up with it.
    ///
    /// A ball recovered in the end zone a side is defending is two points the
    /// other way, whoever falls on it. Otherwise the offense keeps it and runs,
    /// and a defender starts a return of his own.
    fn recover_loose(&mut self, i: usize, at: Vec2) {
        let spot = self.players[i].pos;
        if field::is_in_own_endzone(spot.x, self.dir) {
            let desc = format!(
                "{} falls on it in his own end zone -- that is a safety!",
                self.name_of(i)
            );
            self.finish(PlayOutcome::Safety, spot.x, desc);
            return;
        }
        if self.players[i].side == Side::Defense {
            self.start_fumble_return(i, at.x);
            return;
        }
        self.loose_recovery = Some(Side::Offense);
        self.carrier = Some(i);
        self.ball.holder = Some(i);
        self.ball.pos = self.players[i].pos;
        self.ball.z = 1.0;
        self.phase = Phase::Run;
        self.events.push(SimEvent {
            t: self.t,
            text: format!("{} comes up with it and runs!", self.name_of(i)),
        });
    }

    /// A defender recovered a fumble. The play is not over: he can take it back
    /// for six, and the men who lost it are the ones chasing now.
    fn start_fumble_return(&mut self, d: usize, spot: f32) {
        // Same on a recovery: a fumbled try is a dead try, not a scoop and score.
        if self.setup.is_try {
            let dead = clamp(spot, 0.0, FIELD_LENGTH);
            let text = format!("{} falls on it and the try is dead there.", self.name_of(d));
            self.finish(PlayOutcome::FumbleLost, dead, text);
            return;
        }
        self.ball.flight = None;
        self.ball.z = 1.0;
        self.ball.pos = self.players[d].pos;
        self.ball.holder = Some(d);
        self.carrier = Some(d);
        self.returner = Some(d);
        self.return_spot = spot;
        self.return_t = 0.0;
        self.return_throw = None;
        self.return_is_fumble = true;
        self.carrier_moves.reset();
        self.phase = Phase::Return;
        self.intended = None;
        self.pass_caught = true;
        // The pad goes to the man who came up with it.
        if self.human_defender.is_some() {
            self.human_defender = Some(d);
        }
        self.events.push(SimEvent {
            t: self.t,
            text: format!(
                "{} recovers the fumble and is running the other way!",
                self.name_of(d)
            ),
        });
    }

    // -----------------------------------------------------------------
    // Ball flight
    // -----------------------------------------------------------------

    fn update_ball(&mut self, dt: f32) {
        // A ball that is on its way down is not a ball in the air any more: it
        // falls the last of the way, and the result follows it to the turf.
        if let Some(mut fall) = self.falling.take() {
            fall.t += dt;
            let k = (fall.t / BALL_FALL_SECONDS).clamp(0.0, 1.0);
            self.ball.z = lerp(fall.from_z, 0.12, k * k);
            if k >= 1.0 {
                let mut r = self.finish(fall.outcome, fall.spot, fall.description.clone());
                r.receiver = fall.receiver;
                r.passer = fall.passer;
                r.air_yards = fall.air_yards;
                r.batted_down = fall.batted_down;
                self.result = Some(r);
            } else {
                self.falling = Some(fall);
            }
            return;
        }
        let Some(mut flight) = self.ball.flight.clone() else {
            // Ball in somebody's hands.
            if let Some(h) = self.ball.holder {
                if !self.players[h].down {
                    // A give is made with the ball out in front of the body: it
                    // is offered to the runner, so it travels his way before it
                    // changes hands.
                    let mut at = self.players[h].pos;
                    let give = self.players[h].give.clamp(0.0, 1.0);
                    if give > 0.0 {
                        if let Some((man, _)) = self.handoff {
                            let to = self.players[man].pos - at;
                            at += to.normalize_or_zero() * (0.55 * give);
                        }
                    }
                    self.ball.pos = at;
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
            if (ctl.intercept || ctl.swat || ctl.dive)
                && !self.pass_contacted
                && !self.players[d].down
                && progress > 0.10
                && progress < 0.99
            {
                let dist = self.players[d].pos.dist(self.ball.pos);
                if dist < 2.1 && self.ball.z < 3.4 {
                    self.pass_contacted = true;
                    // Committed before the roll: the figure shows what he is
                    // trying to do, not what he managed to do.
                    self.ball_hands = Some((
                        d,
                        if ctl.intercept { BallHands::Pick } else { BallHands::Swat },
                    ));
                    let cov = self.attrs(d).coverage_factor();
                    let hands = self.attrs(d).catching as f32 / 100.0;
                    // Right on it is a play; a lunging reach is a half chance.
                    let close = if dist < 0.85 { 1.0 } else { 0.5 };
                    let spot = self.ball.pos.x;
                    if ctl.intercept {
                        let pick = (0.06 + cov * 0.14 + hands * 0.10) * close;
                        if self.rng.chance(pick) {
                            self.catch_spot = spot;
                            self.start_return(d, spot, flight.thrower, flight.air_yards);
                            return;
                        }
                    }
                    if ctl.swat || ctl.dive {
                        let bat = (0.16 + cov * 0.20 + hands * 0.10) * close;
                        if self.rng.chance(bat) {
                            let desc =
                                format!("{} gets a hand in and bats it down!", self.name_of(d));
                            self.let_fall(PlayOutcome::PassIncomplete, spot, desc, flight.thrower, flight.air_yards, true);
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
                    self.ball_hands = Some((d, BallHands::Pick));
                    self.catch_spot = spot;
                    self.start_return(d, spot, flight.thrower, flight.air_yards);
                    return;
                }
                self.ball_hands = Some((d, BallHands::Swat));
                if self.rng.chance(bat) {
                    let desc = format!("{} gets a hand in and bats it down!", self.name_of(d));
                    self.let_fall(PlayOutcome::PassIncomplete, spot, desc, flight.thrower, flight.air_yards, true);
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

    /// The chance this receiver comes down with the ball at `landing`.
    ///
    /// His hands against the coverage by him: a lockdown corner and a fill-in
    /// used to defend a pass identically, because the only rating the catch roll
    /// ever read was the receiver's. Separation stays physical -- a man two yards
    /// clear catches it against anybody -- but a covered man catches it only if
    /// his hands are better than the man covering him.
    pub fn catch_chance(&self, receiver: usize, landing: Vec2) -> f32 {
        let hands = self.attrs(receiver).catch_factor();
        let coverage = self.coverage_in_the_area(landing);
        let separation = (7..14)
            .filter(|d| !self.players[*d].down)
            .map(|d| self.players[d].pos.dist(landing))
            .fold(COVERAGE_AREA * 2.0, f32::min);
        let space = ((separation - 0.9) * 0.11).clamp(-0.18, 0.22);
        let accuracy_penalty =
            ((self.players[receiver].pos.dist(landing) - 1.0).max(0.0) * 0.35).min(0.5);
        // A short throw is on him before the defense can close.
        let short_bonus = if (self.los - landing.x).abs() < 10.0 { 0.10 } else { 0.0 };
        clamp(
            CATCH_BASE + (hands - coverage) * CATCH_WEIGHT + space - accuracy_penalty
                + short_bonus,
            CATCH_FLOOR,
            0.97,
        )
    }

    /// How well the men in the area cover the landing spot, 0..1.
    ///
    /// The nearest man's rating is the grip, and a second man converging on the
    /// ball tightens it. This is coverage in the area, not man-to-man: the safety
    /// driving downhill counts, and a defender three yards away with a 40 rating
    /// is not mistaken for a blanket.
    pub fn coverage_in_the_area(&self, landing: Vec2) -> f32 {
        let mut grips: Vec<f32> = (7..14)
            .filter(|d| !self.players[*d].down)
            .map(|d| {
                let dist = self.players[d].pos.dist(landing);
                let near = (1.0 - dist / COVERAGE_AREA).max(0.0);
                self.attrs(d).coverage_factor() * near
            })
            .collect();
        grips.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        let first = grips.first().copied().unwrap_or(0.0);
        let second = grips.get(1).copied().unwrap_or(0.0);
        (first + second * 0.25).min(1.0)
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
                let pick_chance = (0.035 + cov * 0.11 + catching * 0.06 + self.difficulty.pick_bonus())
                    * if target.is_some() { 1.0 } else { 2.2 };
                if self.rng.chance(pick_chance) {
                    self.ball_hands = Some((d, BallHands::Pick));
                    let thrower = flight.thrower;
                    let air = flight.air_yards;
                    self.start_return(d, landing.x, thrower, air);
                    return;
                }
            }
        }

        // Catch or incompletion.
        // Catch or incompletion. This is the contest the ratings exist for: the
        // receiver's hands against the coverage of the men around the spot, with
        // his separation as the head start the coverage has to make up.
        if let Some(t) = target.filter(|_| !stray) {
            let p = self.catch_chance(t, landing);
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

        // A pass that goes backwards is a lateral, and a lateral on the turf is
        // not an incompletion: it is a live ball, exactly as if the ball had been
        // fumbled. Whoever gets there first has it.
        let forward = (flight.to.x - flight.from.x) * self.dirf() >= 0.0;
        if !forward && !self.throw_away {
            let desc = format!(
                "{}'s backward pass hits the turf -- still live!",
                self.name_of(flight.thrower)
            );
            self.start_loose(landing, desc);
            return;
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
        self.let_fall(outcome, self.los, desc, flight.thrower, flight.air_yards, false);
    }

    // -----------------------------------------------------------------
    // Finishing
    // -----------------------------------------------------------------

    /// A flag on the field.
    ///
    /// The play keeps going. 7 on 7 fouls are walked off after the ball is dead, and
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

    /// Which way the picture faces. An interception turns the play round, so the
    /// camera and the stick follow the man with the ball rather than the team that
    /// snapped it.
    pub fn facing(&self) -> i8 {
        if self.phase == Phase::Return {
            -self.setup.direction
        } else {
            self.setup.direction
        }
    }

    /// True while the preview button is held at the line.
    pub fn previewing(&self) -> bool {
        self.preview && self.phase == Phase::PreSnap
    }

    pub fn set_preview(&mut self, on: bool) {
        self.preview = on;
    }

    /// The man running an interception back, for the client to put the pad on.
    pub fn returner_index(&self) -> Option<usize> {
        if self.phase == Phase::Return {
            self.returner
        } else {
            None
        }
    }

    /// Bark a hard count at the line to draw the defense across it.
    ///
    /// The gamble is the whole point, and it runs both ways: a defender who bites
    /// is five yards and a replay of the down, a defense that sits still has seen
    /// the count for nothing, and a count that catches your own line leaning is a
    /// false start on you.
    pub fn dummy_audible(&mut self) {
        if self.phase != Phase::PreSnap || self.dummy_t > 0.0 {
            return;
        }
        self.dummy_t = DUMMY_TIME;
        let name = self.name_of(self.qb);
        self.events.push(SimEvent {
            t: self.t,
            text: format!("{name} barks the count"),
        });
        // The count is a gamble in both directions. Your own line can be the one
        // that leans, and that is the offense's own foul: the man is named, the
        // ball never goes, and the five yards come off his side.
        if self.rng.chance(DUMMY_FALSE_START_CHANCE) {
            let leaning = (0..7)
                .filter(|&i| !self.players[i].down && self.players[i].stun <= 0.0)
                .min_by(|&a, &b| {
                    let da = self.players[a].pos.dist(self.ball.pos);
                    let db = self.players[b].pos.dist(self.ball.pos);
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                });
            if let Some(i) = leaning {
                self.players[i].pos.x = self.los + self.dirf() * JUMP_PAST;
                self.players[i].vel = Vec2::new(self.dirf() * 0.9, 0.0);
                self.whistle_jump(i);
                self.dummy_t = 0.0;
                return;
            }
        }
        // Otherwise whoever is closest to the ball and close enough to the line
        // to be tempted. Only one man has to jump.
        let bait = (7..14)
            .filter(|&d| {
                !self.players[d].down
                    && self.players[d].stun <= 0.0
                    && ((self.players[d].pos.x - self.los) * self.dirf()).abs() < 2.2
            })
            .min_by(|&a, &b| {
                let da = self.players[a].pos.dist(self.ball.pos);
                let db = self.players[b].pos.dist(self.ball.pos);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            });
        let Some(d) = bait else { return };
        // A hard count draws a jump about a third of the time; the rest of the
        // time the defense just looks back at him.
        if !self.rng.chance(0.34) {
            return;
        }
        // Drawn across the ball, not away from it. A defensive man who jumps at
        // the count is over the line, which is what makes the whistle his; the
        // old nudge moved him a yard the other way and flagged him anyway.
        self.players[d].pos.x = self.los - self.dirf() * JUMP_PAST;
        self.players[d].vel = Vec2::new(-self.dirf() * 1.4, 0.0);
        self.whistle_jump(d);
        self.dummy_t = 0.0;
    }

    /// Check the defense to another call at the line. Only legal before the snap.
    pub fn call_defense(&mut self, def: DefCall) {
        if self.phase != Phase::PreSnap {
            return;
        }
        // The offense keeps the ground it is standing on: it is the defense that
        // is being re-lined, and moving the men across the ball would be a jump.
        let off: Vec<Vec2> = self.players[..7].iter().map(|p| p.pos).collect();
        let name = def.name;
        self.def = def;
        self.line_up();
        for (i, was) in off.iter().enumerate() {
            self.players[i].pos = *was;
            self.players[i].vel = Vec2::ZERO;
        }
        self.assign_coverage();
        self.capture_presnap();
        self.events.push(SimEvent {
            t: self.t,
            text: format!("Defense checks to {name}"),
        });
    }

    /// Press or off the receivers: the trigger at the line.
    ///
    /// The corners are lined up across from the man in front of them with a
    /// hand's worth of cushion, and the defense is re-lined in place: the offense
    /// keeps the ground it is standing on, because it is the defense that is
    /// changing its mind. Only before the snap.
    pub fn set_press(&mut self, press: bool) {
        if self.phase != Phase::PreSnap || self.def.press == press {
            return;
        }
        self.def.press = press;
        self.assign_coverage();
        self.capture_presnap();
        self.events.push(SimEvent {
            t: self.t,
            text: format!(
                "Corners {}",
                if press {
                    "press at the line"
                } else {
                    "off the receivers"
                }
            ),
        });
    }

    /// Whether the corners are up at the line.
    pub fn press(&self) -> bool {
        self.def.press
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

    /// The man the flag is on, as an index into `players`.
    ///
    /// The call records a roster number and a side, which is what the box score
    /// wants; the picture needs the body on the field, so that a replay can point
    /// at him wherever he has run to.
    pub fn flagged_player(&self) -> Option<usize> {
        let call = self.flag.as_ref()?;
        let roster = call.player?;
        let side = match call.against {
            FoulTeam::Offense => Side::Offense,
            FoulTeam::Defense => Side::Defense,
        };
        (0..self.players.len())
            .find(|&i| self.players[i].side == side && self.roster_index(i) == roster)
    }

    /// The man the pad should start on: the defender closest to the ball.
    pub fn nearest_defender(&self) -> Option<usize> {
        self.defender_order().first().copied()
    }

    /// The defender closest to the man with the ball.
    ///
    /// The pad was set once, at the snap, on the man nearest the line, and stayed
    /// on him while the carrier ran away from him. The one man on the field the
    /// player could not steer was the one who could make the tackle.
    pub fn nearest_defender_to_carrier(&self) -> Option<usize> {
        let carrier = self.carrier?;
        // Only the offense carrying the ball: after a turnover the ball is the
        // defense's, and the pad belongs to the man running it back.
        if self.players[carrier].side != Side::Offense {
            return None;
        }
        let there = self.players[carrier].pos;
        self.defender_order().into_iter().min_by(|&a, &b| {
            let da = self.players[a].pos.dist(there);
            let db = self.players[b].pos.dist(there);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
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
        // The ball cannot go while the formation is still walking into place.
        self.finish_shift();
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
        // An audible changes what the offense is doing. The defense is standing
        // where it is standing: re-drawing their alignment turned every defender
        // round on his mark, which read as the other team resetting at the line.
        let def_was: Vec<(Vec2, f32)> = (7..14)
            .map(|d| (self.players[d].pos, self.players[d].heading))
            .collect();
        self.line_up();
        self.build_routes();
        self.assign_coverage();
        for (k, (pos, heading)) in def_was.into_iter().enumerate() {
            let d = 7 + k;
            self.players[d].pos = pos;
            self.players[d].heading = heading;
            self.players[d].vel = Vec2::ZERO;
        }
        self.capture_presnap();
        self.carrier = None;
        self.intended = None;
        self.handoff = None;
        self.option_key = None;
        self.option_choice = None;
        self.ball.holder = Some(self.qb);
        self.ball.flight = None;
        self.ball.pos = self.players[self.qb].pos;
        self.ball.z = 1.15;
        self.pass_contacted = false;
        self.ball_hands = None;
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
    ///
    /// The formation is rebuilt, but the men are not placed on their new spots:
    /// they keep the ground they were standing on and walk across to the flipped
    /// alignment, which is what a flip looks like from the sideline. The guards do
    /// not move because a flip does not change their side, and neither do the two
    /// wide men: they take each other's job from the sideline they are already on.
    pub fn flip_play(&mut self) {
        if self.phase != Phase::PreSnap {
            return;
        }
        let from: Vec<Vec2> = self.players.iter().map(|p| p.pos).collect();
        let flipped = self.play.flipped();
        self.call_play(flipped);
        // The re-line-up has just placed everybody on his new spot. Put them all
        // back where they were standing: the walk is what moves them from here.
        for (i, was) in from.iter().enumerate() {
            self.players[i].pos = *was;
            self.players[i].vel = Vec2::ZERO;
        }
        self.shift = Some(from);
        self.shift_t = 0.0;
    }

    /// Put everybody where the current call lines them up, cutting short any
    /// shift that is still in progress.
    pub fn finish_shift(&mut self) {
        // Nothing to finish unless a flip is being walked: the ground a man
        // walked to before the snap is where he snaps from, and re-drawing the
        // formation here put every defender back on his mark as the ball went.
        if self.shift.is_none() {
            return;
        }
        self.shift = None;
        self.shift_t = 0.0;
        let upfield = self.dirf().atan2(0.0);
        for i in 0..7 {
            let base = self.presnap.get(i).copied().unwrap_or(self.players[i].pos);
            self.players[i].pos = base;
            self.players[i].vel = Vec2::ZERO;
            self.players[i].heading = upfield;
        }
    }

    /// The walk to a flipped alignment, one leg at a time: turn to face the way
    /// he is going, jog to the spot at a speed a person runs, then square his
    /// shoulders to the line. Men whose spot did not move do not move at all.
    fn advance_shift(&mut self, dt: f32) {
        let Some(from) = self.shift.clone() else {
            return;
        };
        self.shift_t += dt;
        let upfield = Vec2::new(self.dirf(), 0.0);
        let up_angle = upfield.y.atan2(upfield.x);
        let mut done = true;
        // Only the offense walks. The defense keeps its ground, its footing and
        // its eyes: turning seven defenders round on the spot every time the
        // player flipped a call read as the other team resetting at the line.
        for i in 0..7 {
            let to = self.presnap.get(i).copied().unwrap_or(self.players[i].pos);
            let start = from.get(i).copied().unwrap_or(to);
            let delta = to - start;
            let dist = delta.length();
            let travel = if dist > 0.01 { delta / dist } else { upfield };
            let jogged = (FLIP_JOG_SPEED * self.shift_t).min(dist);
            let arrived = jogged >= dist - 0.01;
            let want = if arrived { upfield } else { travel };
            let p = &mut self.players[i];
            p.heading = turn_towards(p.heading, want.y.atan2(want.x), FLIP_TURN_RATE * dt);
            p.pos = if arrived { to } else { start + travel * jogged };
            p.vel = if arrived { Vec2::ZERO } else { travel * FLIP_JOG_SPEED };
            if !arrived || angle_gap(p.heading, up_angle) > 0.01 {
                done = false;
            }
        }
        if done {
            self.shift = None;
        }
    }

    /// Book a ball that is coming down with nobody under it. Nothing is decided
    /// until it lands: the clock, the card and the next snap all wait for the
    /// ball to hit the turf.
    fn let_fall(
        &mut self,
        outcome: PlayOutcome,
        spot: f32,
        description: String,
        thrower: usize,
        air_yards: f32,
        batted_down: bool,
    ) {
        self.falling = Some(Fall {
            t: 0.0,
            from_z: self.ball.z.max(1.0),
            outcome,
            spot,
            description,
            receiver: self.intended.map(|i| self.roster_index(i)),
            passer: Some(self.roster_index(thrower)),
            air_yards,
            batted_down,
        });
        self.ball.flight = None;
    }

    /// A defender took the ball away. He has it, and the play is not over: the
    /// offense becomes the chase, and the defense can run it back for six.
    fn start_return(&mut self, d: usize, spot: f32, thrower: usize, air_yards: f32) {
        // A try is over the moment the defense has it. There is no return to run:
        // the ball changed hands, the try failed, and the play is dead where he
        // took it rather than run back for points the try never counts.
        if self.setup.is_try {
            let dead = clamp(spot, 0.0, FIELD_LENGTH);
            let text = format!("{} picks it off and the try is dead there.", self.name_of(d));
            self.finish(PlayOutcome::Interception, dead, text);
            return;
        }
        self.ball.flight = None;
        self.ball.z = 1.0;
        self.ball.pos = self.players[d].pos;
        self.ball.holder = Some(d);
        self.carrier = Some(d);
        self.returner = Some(d);
        self.return_spot = spot;
        self.return_t = 0.0;
        self.return_throw = Some((thrower, air_yards));
        self.return_is_fumble = false;
        self.carrier_moves.reset();
        self.phase = Phase::Return;
        self.intended = None;
        self.pass_caught = true;
        // The pad goes to the man with the ball: an interception a player made
        // himself should not then be run back by the coordinator.
        if self.human_defender.is_some() {
            self.human_defender = Some(d);
        }
        let text = format!("{} picks it off and is running the other way!", self.name_of(d));
        self.events.push(SimEvent { t: self.t, text });
    }

    fn update_return(&mut self, dt: f32) {
        let Some(r) = self.returner else { return };
        self.return_t += dt;
        let goal = field::opponent_goal_side(-self.dir);
        // The man with the ball answers the pad if it is on him; otherwise he
        // runs for the far corner the way a returner does.
        let human = self.human_defender == Some(r);
        let stick = if human {
            self.def_control.map(|c| c.move_dir).unwrap_or(Vec2::ZERO)
        } else {
            Vec2::ZERO
        };
        let mut target;
        let mut spd = self.speed_cap(r);
        if stick.length() > 0.18 {
            target = self.players[r].pos + stick.normalize_or_zero() * 4.0;
        } else {
            target = Vec2::new(goal, (self.players[r].pos.y * 0.35).clamp(-9.0, 9.0));
        }
        // The man running it back is a runner, and a runner has the four moves.
        // A pick or a fumble return used to be the one carry in the game where the
        // buttons did nothing and the only way past the last man was to outrun him.
        if let Some(kind) = self.carrier_moves.kind() {
            spd *= self.carrier_moves.speed_scale();
            if !kind.steerable() {
                target = self.players[r].pos + self.carrier_moves.dir() * 2.6;
            }
        }
        self.steer(r, target, spd, dt);
        self.ball.pos = self.players[r].pos;
        self.ball.holder = Some(r);

        // The chase. The three men nearest the ball when it was picked are the
        // ones with a shot at him; everybody else is behind the play.
        let mut chasers: Vec<(usize, f32)> = (0..7)
            .filter(|&i| {
                !self.players[i].down
                    && self.players[i].stun <= 0.0
                    && self.players[i].tackle_cooldown <= 0.0
            })
            .map(|i| (i, self.players[i].pos.dist(self.players[r].pos)))
            .collect();
        chasers.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        chasers.truncate(3);
        // The chase starts behind the play: they have to turn and run, and the
        // man with the ball does not, so the first stride or two are slower.
        let warm = (self.return_t / CHASE_WARMUP).clamp(0.0, 1.0);
        let chase_pace = 0.55 + 0.42 * warm;
        for (n, (i, _)) in chasers.iter().enumerate() {
            let lead = 0.22 + 0.12 * n as f32;
            let aim = self.players[r].pos + self.players[r].vel * lead;
            let spd = self.speed_cap(*i) * chase_pace;
            self.steer(*i, aim, spd, dt);
        }
        for i in 0..14 {
            if i == r || chasers.iter().any(|(c, _)| *c == i) {
                continue;
            }
            self.players[i].vel *= 0.85;
        }
        self.integrate(dt);

        let pos = self.players[r].pos;
        let spot = clamp(pos.x, 0.0, FIELD_LENGTH);
        let scored = if self.dir > 0 { pos.x <= 0.0 } else { pos.x >= FIELD_LENGTH };
        if scored {
            self.finish_return(goal, true);
            return;
        }
        // Out at the sideline: the return dies where he stepped out.
        if pos.y.abs() >= FIELD_HALF_WIDTH {
            self.finish_return(spot, false);
            return;
        }
        // The end of a dive is the end of the return, the same as it is on offense.
        if let Some(kind) = self.carrier_moves.step(dt) {
            if kind.commits() {
                let name = self.name_of(r);
                let text = match kind {
                    Kind::Slide => format!("{name} slides down and gives himself up."),
                    _ => format!("{name} dives down and is tackled there."),
                };
                self.events.push(SimEvent { t: self.t, text });
                self.finish_return(spot, false);
                return;
            }
        }
        // A hit. Any of the men in the chase close enough to wrap him up ends it,
        // and a move makes him a smaller target: a spin or a stiff arm late on the
        // wrap, and a man in the air over the tackle cannot be wrapped up at all.
        let wrap = self.carrier_moves.tackle_scale();
        let hit = chasers
            .iter()
            .find(|(i, _)| {
                self.return_t >= RETURN_GRACE
                    && wrap > 0.0
                    && self.players[*i].pos.dist(self.players[r].pos) <= TACKLE_RADIUS * wrap
            })
            .map(|(i, _)| *i);
        if let Some(i) = hit {
            self.players[i].tackle_cooldown = 0.8;
            self.finish_return(spot, false);
            return;
        }
        if self.t > SNAP_DELAY + RETURN_SECONDS {
            self.finish_return(spot, false);
        }
    }

    /// End the return: an interception with whatever he gained, or six points.
    fn finish_return(&mut self, spot: f32, touchdown: bool) {
        let Some(r) = self.returner else { return };
        let ret = ((spot - self.return_spot) * -self.dirf()).max(0.0);
        let name = self.name_of(r);
        let fumble = self.return_is_fumble;
        let desc = match (touchdown, fumble) {
            (true, true) => format!("{name} scoops up the fumble and takes it all the way back!"),
            (true, false) => format!("{name} takes the interception all the way back!"),
            (false, true) => format!("{name} returns the fumble {} yards.", ret.round() as i32),
            (false, false) => format!("{name} returns the interception {} yards.", ret.round() as i32),
        };
        let outcome = if touchdown {
            PlayOutcome::Touchdown
        } else if fumble {
            PlayOutcome::FumbleLost
        } else {
            PlayOutcome::Interception
        };
        let mut res = self.finish(outcome, spot, desc);
        res.turnover = true;
        res.return_yards = ret;
        res.tackler = Some(self.roster_index(r));
        // The man carrying it is a defender: he is not a receiver in the box score.
        res.carrier = None;
        if let Some((thrower, air)) = self.return_throw {
            res.passer = Some(self.roster_index(thrower));
            res.air_yards = air;
        }
        res.highlight = true;
        self.result = Some(res);
    }

    /// Eligible receivers with the index to drive [`QbControl::target`].
    pub fn eligible_targets(&self) -> Vec<(OffRole, usize)> {
        self.eligible_indices()
            .into_iter()
            .map(|i| (OffRole::ALL[self.players[i].role_index], i))
            .collect()
    }

    /// The men a pitch can go to on a run: everybody running a path who is not
    /// the man with the ball. They come up on the face buttons in the order they
    /// are numbered, so the button a man is standing over is the button that
    /// pitches to him.
    pub fn pitch_targets(&self) -> Vec<(OffRole, usize)> {
        if !self.play.family.is_run() || self.ball.flight.is_some() || self.result.is_some() {
            return Vec::new();
        }
        let Some(holder) = self.ball.holder else {
            return Vec::new();
        };
        if self.players[holder].down {
            return Vec::new();
        }
        // The option is live behind the line of scrimmage. Once a runner is past
        // it he is a runner, and the buttons are his moves again.
        if !self.behind_the_line() {
            return Vec::new();
        }
        let carrier = self.play.carrier_role().map(|r| r.index());
        let mut out: Vec<(OffRole, usize)> = Vec::new();
        // The man the play was drawn for is not on a button any more: the ball
        // is given to him by the two men meeting, not by the player pressing for
        // it. The buttons are the men behind the ball, which is the pitch.
        for i in 0..7 {
            if i == holder || Some(i) == carrier || self.players[i].down {
                continue;
            }
            if self.players[i].route.is_empty() {
                continue;
            }
            let role = OffRole::ALL[self.players[i].role_index];
            if role == OffRole::Qb || role.is_line() {
                continue;
            }
            out.push((role, i));
        }
        out
    }

    /// True when this man is the one the play was drawn to hand the ball to.
    pub fn is_drawn_carrier(&self, index: usize) -> bool {
        self.play.carrier_role().map(|r| r.index()) == Some(index)
    }

    /// Pitch it back to a runner.
    ///
    /// The face buttons on a run are not receivers. A pitch is a backward pass,
    /// so a man ahead of the ball is not a target and asking for him does nothing
    /// at all: the man carrying it keeps it. What makes pitching a decision
    /// rather than a free upgrade is the man catching it -- with a defender on
    /// his back the ball can come out of his hands, and then it is on the ground
    /// for whoever wants it.
    ///
    /// Returns true when the pitch was made (even if it came out loose).
    pub fn pitch(&mut self, target: usize) -> bool {
        // Far enough for a sweep coming round the formation, short of a throw.
        const REACH: f32 = 18.0;
        // Inside this the defender on the catcher is on him, not near him.
        const TRAFFIC: f32 = 2.4;
        let Some(holder) = self.ball.holder else {
            return false;
        };
        if holder == target
            || self.result.is_some()
            || self.ball.flight.is_some()
            || self.players[target].down
            || self.players[holder].down
            || !self.play.family.is_run()
            || matches!(self.phase, Phase::PreSnap | Phase::Loose | Phase::Return)
        {
            return false;
        }
        let dirf = self.dirf();
        let hand = self.players[holder].pos;
        let mouth = self.players[target].pos;
        // A pitch that goes forward is a throw, and the button says pitch.
        if (mouth.x - hand.x) * dirf > 0.35 {
            return false;
        }
        if hand.dist(mouth) > REACH {
            return false;
        }
        let pitcher = self.name_of(holder);
        let catcher = self.name_of(target);
        // A man the play was drawn to give the ball to is handed it, not tossed
        // it: at the quarterback that is a clean exchange nobody can knock away.
        // Anybody else is a pitch, and a pitch into traffic can come out.
        let handoff = self.is_drawn_carrier(target) && hand.dist(mouth) <= 3.5;
        let pressure = self.nearest_defender_dist(target);
        if !handoff && pressure < TRAFFIC && self.rng.chance((1.0 - pressure / TRAFFIC) * 0.34) {
            self.fumble_alert = 2.4;
            self.fumble_note = format!("{pitcher} pitches it into traffic - the ball is loose!");
            self.start_loose(
                mouth,
                format!("{pitcher}'s pitch is knocked away from {catcher}!"),
            );
            return true;
        }
        self.carrier = Some(target);
        self.ball.holder = Some(target);
        self.ball.pos = mouth;
        self.ball.z = 1.0;
        self.events.push(SimEvent {
            t: self.t,
            text: if handoff {
                format!("{pitcher} hands it to {catcher}")
            } else {
                format!("{pitcher} pitches back to {catcher}")
            },
        });
        true
    }

    /// The defender the option reads: the end on the side the ball is going.
    ///
    /// One read is the whole scheme. Everything else is blocked or accounted
    /// for, so the man whose decision makes the play is the end the dive runs at.
    pub fn option_read(&self) -> Option<usize> {
        let side = self.play.run_side;
        let lane = self.setup.ball_y + side as f32 * 4.0;
        let mut best: Option<(usize, f32)> = None;
        for i in 7..14 {
            if self.players[i].down {
                continue;
            }
            let role = DefRole::ALL[self.players[i].role_index];
            if !matches!(role, DefRole::EdgeL | DefRole::EdgeR) {
                continue;
            }
            let score = if side == 0 {
                (self.players[i].pos.y - self.setup.ball_y).abs()
            } else {
                (self.players[i].pos.y - lane).abs()
            };
            if best.map(|(_, b)| score < b).unwrap_or(true) {
                best = Some((i, score));
            }
        }
        best.map(|(i, _)| i)
    }

    /// True when the man being read has come down inside: he is playing the
    /// dive, so the quarterback pulls it and keeps.
    pub fn key_crashed(&self, key: usize) -> bool {
        let p = &self.players[key];
        if (p.pos.x - self.los) * self.dirf() < -0.4 {
            return true;
        }
        let across = p.pos.y - self.setup.ball_y;
        if across.abs() < 2.5 {
            return true;
        }
        p.vel.y != 0.0 && p.vel.y.signum() != across.signum() && p.vel.y.abs() > 1.2
    }

    /// True when `target` is at or behind `hand`: the only place a pitch goes.
    fn behind(&self, hand: usize, target: usize) -> bool {
        (self.players[target].pos.x - self.players[hand].pos.x) * self.dirf() <= 0.35
    }

    /// The first man back: the dive, the deepest man behind the ball.
    fn option_dive(&self) -> Option<usize> {
        let hand = self.players[self.qb].pos.x;
        let dirf = self.dirf();
        self.pitch_targets()
            .into_iter()
            .filter(|(_, i)| self.behind(self.qb, *i))
            .max_by(|a, b| {
                let da = (hand - self.players[a.1].pos.x) * dirf;
                let db = (hand - self.players[b.1].pos.x) * dirf;
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(_, i)| i)
    }

    /// The computer's option: read the end at the mesh, then give it on or keep
    /// it, and give it again if they come for the keeper.
    ///
    /// It is one read and it is made once. Only on a run, only with nobody on the
    /// pad, and only while the quarterback actually has the ball.
    fn ai_option(&mut self) {
        if !self.play.family.is_run() || self.result.is_some() || self.carrier != Some(self.qb) {
            return;
        }
        match self.option_choice {
            None => {
                // The mesh: the end has had his first two steps by now.
                if self.t < SNAP_DELAY + 0.45 {
                    return;
                }
                let Some(key) = self.option_read() else { return };
                self.option_key = Some(key);
                let dive = self.option_dive();
                let crashed = self.key_crashed(key);
                let give = !crashed && dive.is_some();
                let text = if give {
                    format!(
                        "{} reads {} staying wide and gives it on",
                        self.name_of(self.qb),
                        self.name_of(key)
                    )
                } else if crashed {
                    format!(
                        "{} reads {} crashing down and keeps it",
                        self.name_of(self.qb),
                        self.name_of(key)
                    )
                } else {
                    format!("{} keeps it", self.name_of(self.qb))
                };
                self.events.push(SimEvent { t: self.t, text });
                self.option_choice = Some(OptionChoice::Keep);
                if give {
                    if let Some(dive) = dive {
                        if self.pitch(dive) {
                            self.option_choice = Some(OptionChoice::Handoff);
                        }
                    }
                }
            }
            Some(OptionChoice::Keep) => {
                // They have come for the keeper: give it on to the man trailing
                // him, if there is one behind the ball with room to take it.
                if self.t < SNAP_DELAY + 0.5 || self.nearest_defender_dist(self.qb) > 2.6 {
                    return;
                }
                let qb_x = self.players[self.qb].pos.x;
                let dirf = self.dirf();
                let trailing = self
                    .pitch_targets()
                    .into_iter()
                    .filter(|(_, i)| self.behind(self.qb, *i))
                    .filter(|(_, i)| self.nearest_defender_dist(*i) > 2.6)
                    .max_by(|a, b| {
                        let da = (qb_x - self.players[a.1].pos.x) * dirf;
                        let db = (qb_x - self.players[b.1].pos.x) * dirf;
                        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|(_, i)| i);
                if let Some(trailing) = trailing {
                    if self.pitch(trailing) {
                        self.option_choice = Some(OptionChoice::Pitch);
                    }
                }
            }
            Some(_) => {}
        }
    }

    /// True while the ball is still behind the line of scrimmage, which is the
    /// ground a pitch is live on and the ground a runner still has the option to
    /// get rid of it.
    pub fn behind_the_line(&self) -> bool {
        // A quarterback still holding the ball is not a runner yet: he meshes on
        // the line with the ball out in front of him, and the men behind him are
        // still his to give it to. Once the ball is in a runner's hands the line
        // is the line.
        if self.carrier.is_none() && self.ball.holder == Some(self.qb) {
            return true;
        }
        (self.ball.pos.x - self.los) * self.dirf() <= 0.0
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

/// Turn a heading toward another by at most `step` radians, the short way round.
fn turn_towards(heading: f32, want: f32, step: f32) -> f32 {
    let gap = angle_gap(heading, want);
    if gap.abs() <= step {
        want
    } else {
        heading + gap.clamp(-step, step)
    }
}

/// The signed difference between two headings, -pi..pi.
fn angle_gap(a: f32, b: f32) -> f32 {
    (b - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
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
    fn a_catch_is_decided_by_hands_against_coverage_ratings() {
        let (off, def_) = teams();
        let book = playbook();
        let d = defensive_playbook();
        let s = setup(45.0, 1, 10.0, 31);
        let mut sim = PlaySim::new(&off, &def_, &book[0], &d[0], &s);
        let receiver = 3;
        let landing = sim.players[receiver].pos;
        // Nobody near the spot: the ball is the receiver's to drop.
        for i in 7..14 {
            sim.players[i].pos = Vec2::new(landing.x - 20.0, landing.y);
        }
        let open = sim.catch_chance(receiver, landing);
        // The best cover man in the league at his hip.
        let best = (7..14)
            .max_by_key(|i| sim.attrs(*i).coverage)
            .expect("a defense");
        sim.players[best].pos = Vec2::new(landing.x + 0.4, landing.y);
        let covered = sim.catch_chance(receiver, landing);
        assert!(
            covered < open,
            "coverage took nothing away: {covered} at his hip, {open} uncovered"
        );
        // The same hip, covered by the worst man out there: the rating is what
        // took the ball away, not the fact that somebody was standing there.
        let worst = (7..14)
            .min_by_key(|i| sim.attrs(*i).coverage)
            .expect("a defense");
        sim.players[worst].pos = Vec2::new(landing.x + 0.4, landing.y);
        sim.players[best].pos = Vec2::new(landing.x - 20.0, landing.y);
        let by_scrub = sim.catch_chance(receiver, landing);
        assert!(
            by_scrub > covered,
            "the cover rating never entered the catch: {by_scrub} vs {covered}"
        );
        assert!(
            (CATCH_FLOOR..=0.97).contains(&covered) && (CATCH_FLOOR..=0.97).contains(&by_scrub),
            "chance outside its bounds: {covered}, {by_scrub}"
        );
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
        // The ceiling used to be twelve, when a carrier could be sent back toward
        // the line of scrimmage by a route point behind him. Now that he runs
        // forward, and the line drives its men back on runs, a play gains more.
        assert!(
            (-3.0..16.0).contains(&avg),
            "average yards implausible: {avg}"
        );
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
        let routes_before: Vec<Vec<Vec2>> = (0..7).map(|i| sim.players[i].route.clone()).collect();
        sim.flip_play();
        assert!(sim.play.flip, "the flip was not kept");
        // Nobody is placed on his new spot: everybody is still standing where he
        // lined up, and the button that flipped the call has moved nobody.
        for i in 0..7 {
            let now = sim.players[i].pos;
            assert!(
                (now - before[i]).length() < 0.05,
                "man {i} was teleported across on the flip: {now:?} from {:?}",
                before[i]
            );
        }
        // Then they run there. The guards do not go anywhere at all.
        sim.await_snap(true);
        // A receiver's mirrored spot can be the width of the formation away, so
        // the jog is given all the time it needs rather than a fixed count.
        for _ in 0..1200 {
            sim.step(FIXED_DT);
            if sim.shift.is_none() {
                break;
            }
        }
        assert!(sim.shift.is_none(), "the jog never finished");
        for i in 0..7 {
            let role = OffRole::ALL[sim.players[i].role_index];
            let now = sim.players[i].pos;
            if role.keeps_side() {
                assert!(
                    (now - before[i]).length() < 0.05,
                    "{role:?} moved on a flip: {now:?} from {:?}",
                    before[i]
                );
                continue;
            }
            assert!((now.x - before[i].x).abs() < 0.02, "{role:?} moved upfield on a flip");
            assert!(
                (now.y + before[i].y).abs() < 0.05,
                "{role:?} did not walk to the mirrored spot: {now:?} from {:?}",
                before[i]
            );
        }
        // The routes are mirrored, and the outside men swapped: what the wide
        // left runs now is the mirror of the job the wide right had, taken from
        // his own feet, and the wide right has the other one. Neither of them
        // moved a step to do it.
        let find = |role: OffRole| {
            (0..7)
                .find(|&i| OffRole::ALL[sim.players[i].role_index] == role)
                .expect("a wide man is missing")
        };
        let shape = |route: &[Vec2]| -> Vec<Vec2> {
            if route.is_empty() {
                return Vec::new();
            }
            route.iter().map(|p| *p - route[0]).collect()
        };
        let (l, r) = (find(OffRole::WideL), find(OffRole::WideR));
        let left_now = shape(&sim.players[l].route);
        let right_was = shape(&routes_before[r]);
        assert_eq!(left_now.len(), right_was.len(), "the wide jobs did not swap");
        for (now, was) in left_now.iter().zip(right_was.iter()) {
            assert!(
                (now.x - was.x).abs() < 1e-3 && (now.y + was.y).abs() < 1e-3,
                "the wide left did not take the other job: {now:?} from {was:?}"
            );
        }
    }

    #[test]
    fn an_incompletion_lands_before_the_play_is_over() {
        let (off, def) = teams();
        let book = playbook();
        let dcall = defensive_playbook()[0].clone();
        let s = setup(35.0, 1, 10.0, 21);
        let mut sim = PlaySim::new(&off, &def, &book[0], &dcall, &s);
        sim.request_snap();
        for _ in 0..6 {
            sim.step(FIXED_DT);
        }
        let los = sim.los;
        sim.let_fall(PlayOutcome::PassIncomplete, los, "Incomplete.".to_string(), 0, 12.0, false);
        // The word does not come while the ball is still up there.
        assert!(sim.result.is_none(), "the play died in the air");
        let z0 = sim.ball_position().1;
        for _ in 0..4 {
            sim.step(FIXED_DT);
        }
        assert!(sim.ball_position().1 < z0, "the ball never started down");
        assert!(sim.result.is_none(), "the play died before the ball landed");
        for _ in 0..40 {
            sim.step(FIXED_DT);
        }
        let r = sim.result.clone().expect("the incompletion never came down");
        assert_eq!(r.outcome, PlayOutcome::PassIncomplete);
        assert!(sim.ball_position().1 < 0.3, "it was booked above the turf");
    }

    #[test]
    fn an_interception_is_run_back_instead_of_ending_the_snap() {
        let (off, def) = teams();
        let book = playbook();
        let play = book
            .iter()
            .find(|p| p.family.is_pass())
            .expect("no pass play");
        let dcall = defensive_playbook()[0].clone();
        let s = setup(45.0, 1, 10.0, 31);
        let mut sim = PlaySim::new(&off, &def, play, &dcall, &s);
        sim.request_snap();
        sim.start_return(7, 45.0, 0, 14.0);
        assert_eq!(sim.phase, Phase::Return, "the pick did not turn the play round");
        assert_eq!(sim.ball.holder, Some(7), "the wrong man has the ball");
        sim.step(FIXED_DT);
        assert!(sim.result.is_none(), "the pick ended the play on the catch");
        // Nobody gets there for a while: he is away with it.
        let mut guard = 0;
        while sim.result.is_none() && guard < 700 {
            sim.step(FIXED_DT);
            guard += 1;
        }
        let r = sim.result.expect("the return never ended");
        assert!(
            r.outcome == PlayOutcome::Interception || r.touchdown,
            "the return came back as {:?}",
            r.outcome
        );
        assert!(r.return_yards >= 0.0, "a return with negative yards");
        assert!(r.turnover, "an interception is not a turnover");
    }

    /// A return is worth more than the catch: the cover men were standing on the
    /// spot when he took it, and used to end it after three yards.
    #[test]
    fn an_interception_return_is_longer_than_the_catch() {
        let (off, def) = teams();
        let book = playbook();
        let play = book
            .iter()
            .find(|p| p.family.is_pass())
            .expect("no pass play");
        let dcall = defensive_playbook()[0].clone();
        let s = setup(45.0, 1, 10.0, 39);
        let mut sim = PlaySim::new(&off, &def, play, &dcall, &s);
        sim.request_snap();
        sim.start_return(7, 45.0, 0, 14.0);
        // The receiver he took it from is on top of him at the catch.
        let spot = sim.players[7].pos;
        sim.players[4].pos = spot;
        let mut guard = 0;
        while sim.result.is_none() && guard < 900 {
            sim.step(FIXED_DT);
            guard += 1;
        }
        let r = sim.result.expect("the return never ended");
        assert!(
            r.return_yards >= 8.0,
            "the return was only {} yards",
            r.return_yards
        );
    }

    /// The harder levels give the line one man each instead of letting two men
    /// stand on the same rusher while a third walks through.
    #[test]
    fn the_line_blocks_the_closest_man_in_front_of_each_of_them() {
        let (off, def) = teams();
        let book = playbook();
        let play = book
            .iter()
            .find(|p| p.family.is_pass())
            .expect("no pass play");
        let aim = Vec2::new(45.0, 0.0);
        for level in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard] {
            let dcall = defensive_playbook()[0].clone();
            let s = setup(45.0, 1, 10.0, 93);
            let mut sim = PlaySim::new(&off, &def, play, &dcall, &s);
            sim.difficulty = level;
            let picks: Vec<(usize, usize)> = (0..7)
                .filter(|&i| sim.blocker_on_this_play(i))
                .filter_map(|i| sim.block_threat(i, aim).map(|d| (i, d)))
                .collect();
            assert!(!picks.is_empty(), "{level:?}: nobody had a man to block");
            for (i, d) in picks {
                let mine = sim.players[d].pos.dist(sim.players[i].pos);
                assert!(mine < 4.5, "{level:?}: {i} took a man {mine:.2} away");
                // Nobody in his path is closer than the man he took.
                let ahead = Vec2::new(sim.dirf(), 0.0);
                let closer = (7..14)
                    .filter(|&x| x != d && !sim.players[x].down && sim.players[x].stun <= 0.0)
                    .any(|x| {
                        let rel = sim.players[x].pos - sim.players[i].pos;
                        rel.normalize_or_zero().dot(ahead) > 0.15 && rel.length() + 0.01 < mine
                    });
                assert!(
                    !closer,
                    "{level:?}: blocker {i} ignored a closer man in his path"
                );
            }
        }
    }

    #[test]
    fn the_man_who_just_picked_it_off_gets_a_head_start() {
        let (off, def) = teams();
        let book = playbook();
        let play = book
            .iter()
            .find(|p| p.family.is_pass())
            .expect("no pass play");
        let dcall = defensive_playbook()[0].clone();
        let s = setup(45.0, 1, 10.0, 37);
        let mut sim = PlaySim::new(&off, &def, play, &dcall, &s);
        sim.request_snap();
        sim.start_return(7, 45.0, 0, 14.0);
        // The man the ball was thrown to is standing on top of him: that used to
        // be an instant tackle and the return never happened.
        let spot = sim.players[7].pos;
        sim.players[4].pos = spot;
        sim.players[4].stun = 0.0;
        for _ in 0..12 {
            sim.step(FIXED_DT);
        }
        assert!(sim.result.is_none(), "the return was over before it started");
    }

    /// A defender going up for a ball tells the picture which play he is on:
    /// one hand up is the swat, two hands up is the interception. The state is
    /// set when he commits, so a miss still reads as the play he tried.
    #[test]
    fn a_defender_shows_whether_he_is_swatting_or_picking() {
        for (swat, want) in [(true, BallHands::Swat), (false, BallHands::Pick)] {
            let (off, def) = teams();
            let book = playbook();
            let play = book
                .iter()
                .find(|p| p.family.is_pass())
                .expect("no pass play");
            let dcall = defensive_playbook()[0].clone();
            let s = setup(45.0, 1, 10.0, 91);
            let mut sim = PlaySim::new(&off, &def, play, &dcall, &s);
            sim.set_human_defender(Some(7));
            let at = sim.players[7].pos;
            sim.ball.holder = None;
            sim.phase = Phase::BallInAir;
            sim.pass_contacted = false;
            sim.ball.flight = Some(Flight {
                from: at,
                to: at,
                t: 1.0,
                duration: 2.0,
                peak: 0.3,
                target: None,
                thrower: sim.qb,
                air_yards: 8.0,
                speed: 12.0,
            });
            sim.set_def_control(Some(DefControl {
                swat,
                intercept: !swat,
                ..Default::default()
            }));
            sim.step(FIXED_DT);
            assert_eq!(
                sim.ball_hands,
                Some((7, want)),
                "the man on the pad showed the wrong hands"
            );
        }
    }

    #[test]
    fn the_defender_on_the_pad_can_walk_before_the_snap() {
        let (off, def) = teams();
        let book = playbook();
        let dcall = defensive_playbook()[0].clone();
        let s = setup(40.0, 1, 10.0, 71);
        let mut sim = PlaySim::new(&off, &def, &book[0], &dcall, &s);
        sim.set_human_defender(Some(7));
        let was = sim.players[7].pos;
        sim.set_def_control(Some(DefControl {
            move_dir: Vec2::new(0.0, 1.0),
            tackle: false,
            dive: false,
            swat: false,
            intercept: false,
            hit: None,
        }));
        for _ in 0..90 {
            sim.step(FIXED_DT);
        }
        assert!(!sim.is_snapped(), "the ball went while he was still walking");
        assert!(
            (sim.players[7].pos - was).length() > 0.3,
            "the defender never moved: {:?} from {was:?}",
            sim.players[7].pos
        );
    }

    #[test]
    fn a_man_past_the_line_at_the_snap_is_offside() {
        let (off, def) = teams();
        let book = playbook();
        let dcall = defensive_playbook()[0].clone();
        let s = setup(40.0, 1, 10.0, 73);
        let mut sim = PlaySim::new(&off, &def, &book[0], &dcall, &s);
        // A receiver steps a yard past the ball before it is hiked.
        sim.players[4].pos.x = s.los + s.direction as f32;
        sim.request_snap();
        let r = sim.result.clone().expect("a man past the line snapped with no flag");
        let call = r.penalty.clone().expect("a penalty with no call");
        assert_eq!(call.kind, crate::rules::PenaltyKind::FalseStart);
        assert_eq!(call.against, crate::rules::FoulTeam::Offense);
        assert_eq!(r.yards, 0.0, "an offside is not a play");
        assert_eq!(sim.phase, Phase::Dead, "the ball went anyway");
    }

    #[test]
    fn leaning_across_the_line_is_legal_until_the_snap() {
        let (off, def) = teams();
        let book = playbook();
        let dcall = defensive_playbook()[0].clone();
        let s = setup(40.0, 1, 10.0, 72);

        // A yard into the backfield, with nobody near him: depth is legal while
        // the men are set, and it is the snap that makes it offside.
        let mut sim = PlaySim::new(&off, &def, &book[0], &dcall, &s);
        sim.set_human_defender(Some(7));
        sim.players[7].pos = Vec2::new(s.los - s.direction as f32, s.ball_y + 20.0);
        for _ in 0..6 {
            sim.step(FIXED_DT);
        }
        assert!(sim.result.is_none(), "leaning across the line drew a flag");
        sim.request_snap();
        let r = sim.result.clone().expect("across at the snap was not offside");
        let call = r.penalty.clone().expect("a penalty with no call");
        assert_eq!(call.kind, crate::rules::PenaltyKind::OffsideDefense);
        assert_eq!(call.against, crate::rules::FoulTeam::Defense);
        assert_eq!(sim.phase, Phase::Dead, "the ball went with a man offside");
    }

    #[test]
    fn a_hand_on_a_man_across_the_line_is_encroachment() {
        let (off, def) = teams();
        let book = playbook();
        let dcall = defensive_playbook()[0].clone();
        let s = setup(40.0, 1, 10.0, 74);
        let mut sim = PlaySim::new(&off, &def, &book[0], &dcall, &s);
        sim.set_human_defender(Some(7));
        let mut sim = PlaySim::new(&off, &def, &book[0], &dcall, &s);
        sim.set_human_defender(Some(7));
        // On top of the man over the ball, a step across the line.
        let man = sim.players[1].pos;
        sim.players[7].pos = Vec2::new(s.los - s.direction as f32 * 0.35, man.y);
        sim.step(FIXED_DT);
        let r = sim
            .result
            .clone()
            .expect("a touch across the line was not flagged");
        let call = r.penalty.clone().expect("a penalty with no call");
        assert_eq!(call.kind, crate::rules::PenaltyKind::Encroachment);
        assert_eq!(call.against, crate::rules::FoulTeam::Defense);
        assert_eq!(r.yards, 0.0, "an encroachment is not a play");
        assert_eq!(sim.phase, Phase::Dead, "the ball went on a dead-ball foul");
    }

    /// A count that draws a defender is encroachment on the defense, and the
    /// man is left across the ball rather than backing away from it.
    #[test]
    fn a_dummy_count_can_draw_the_defence_offside() {
        let book = playbook();
        let dcall = defensive_playbook()[0].clone();
        let mut flagged = 0;
        let mut quiet = 0;
        for seed in 0..60u64 {
            let (off, def) = teams();
            let s = setup(40.0, 1, 10.0, 100 + seed);
            let mut sim = PlaySim::new(&off, &def, &book[0], &dcall, &s);
            sim.dummy_audible();
            let Some(r) = sim.result.clone() else {
                quiet += 1;
                continue;
            };
            let call = r.penalty.expect("a jump with no flag");
            if call.against != crate::rules::FoulTeam::Defense {
                quiet += 1;
                continue;
            }
            assert_eq!(call.kind, crate::rules::PenaltyKind::Encroachment);
            // The spot of the foul is where the man was when it was whistled, so
            // this is the picture's claim as well as the flag's.
            let across = (call.spot_of_foul - s.los) * s.direction as f32;
            assert!(
                across < -SNAP_OFFSIDE,
                "the flag names a man who never crossed: {across:.2} yards from the ball"
            );
            flagged += 1;
        }
        assert!(
            flagged > 0 && quiet > 0,
            "the count is a sure thing or a waste of breath: {flagged} jumps, {quiet} quiet"
        );
    }

    /// The count can catch your own line leaning, and then it is the offense's
    /// own foul: five yards his way, the down again, the ball still on the turf.
    #[test]
    fn a_count_that_catches_your_own_line_is_a_false_start() {
        let book = playbook();
        let dcall = defensive_playbook()[0].clone();
        let mut seen = false;
        for seed in 0..200u64 {
            let (off, def) = teams();
            let s = setup(40.0, 1, 10.0, 400 + seed);
            let mut sim = PlaySim::new(&off, &def, &book[0], &dcall, &s);
            sim.dummy_audible();
            let Some(call) = sim.result.as_ref().and_then(|r| r.penalty.clone()) else {
                continue;
            };
            if call.kind != crate::rules::PenaltyKind::FalseStart {
                continue;
            }
            let r = sim.result.clone().expect("a flag with no play");
            assert_eq!(call.against, crate::rules::FoulTeam::Offense);
            assert!(call.player.is_some(), "a false start with nobody on it");
            assert_eq!(r.yards, 0.0, "a false start is not a play");
            assert_eq!(sim.phase, Phase::Dead, "the ball went on a dead-ball foul");
            let across = (call.spot_of_foul - s.los) * s.direction as f32;
            assert!(
                across > SNAP_OFFSIDE,
                "the flag names a man who never crossed: {across:.2} yards from the ball"
            );
            seen = true;
            break;
        }
        assert!(seen, "the count never caught the offense in 200 seeds");
    }

    #[test]
    fn the_trigger_flips_the_corners_between_press_and_off() {
        let (off, def) = teams();
        let book = playbook();
        let call = defensive_playbook()
            .into_iter()
            .find(|d| !d.press && d.coverage.is_man())
            .expect("no off man call");
        let s = setup(40.0, 1, 10.0, 9);
        let mut sim = PlaySim::new(&off, &def, &book[0], &call, &s);
        let corner = |sim: &PlaySim| {
            (7..14)
                .find(|&i| DefRole::ALL[sim.players[i].role_index] == DefRole::CbL)
                .map(|i| sim.players[i].pos.x)
                .expect("no left corner")
        };
        let off_first = corner(&sim);
        assert!(!sim.press());
        sim.set_press(true);
        assert!(sim.press());
        let pressed = corner(&sim);
        assert!(
            pressed < off_first - 1.0,
            "press should be up at the line: {pressed} against {off_first}"
        );
        sim.set_press(false);
        assert!(!sim.press());
        assert!((corner(&sim) - off_first).abs() < 0.01, "the corner did not back off again");
        // After the snap the corners are committed.
        sim.request_snap();
        sim.set_press(true);
        assert!(!sim.press(), "a snap should end the technique change");
    }

    #[test]
    fn a_drop_drawn_on_the_call_sheet_is_where_the_defender_goes() {
        let (off, def) = teams();
        let book = playbook();
        let mut call = defensive_playbook()
            .into_iter()
            .find(|d| d.coverage == Coverage::Cover2)
            .expect("no cover two");
        call.drops = vec![(DefRole::Safety, vec![Vec2::new(9.0, -6.0)])];
        let s = setup(40.0, 1, 10.0, 21);
        let sim = PlaySim::new(&off, &def, &book[0], &call, &s);
        let safety = (7..14)
            .find(|&i| DefRole::ALL[sim.players[i].role_index] == DefRole::Safety)
            .expect("no safety");
        assert_eq!(sim.players[safety].job, CoverageJob::Drop);
        let path = sim.players[safety].route.clone();
        assert_eq!(path.len(), 1, "the drawn drop was not laid out: {path:?}");
        assert!(
            (path[0].x - (s.los + 9.0)).abs() < 0.01,
            "the drop is not nine yards past the line: {path:?}"
        );
    }

    #[test]
    fn a_defence_audible_moves_the_defence_and_not_the_offence() {
        let (off, def) = teams();
        let book = playbook();
        let book_def = defensive_playbook();
        let first = book_def[0].clone();
        let other = book_def
            .iter()
            .find(|d| d.name != first.name)
            .cloned()
            .expect("only one defensive call");
        let s = setup(40.0, 1, 10.0, 73);
        let mut sim = PlaySim::new(&off, &def, &book[0], &first, &s);
        let off_before: Vec<Vec2> = (0..7).map(|i| sim.players[i].pos).collect();
        let def_before: Vec<Vec2> = (7..14).map(|i| sim.players[i].pos).collect();
        sim.call_defense(other.clone());
        assert_eq!(sim.def.name, other.name);
        for i in 0..7 {
            assert!(
                (sim.players[i].pos - off_before[i]).length() < 0.05,
                "the offense was re-lined by a defensive check"
            );
        }
        assert!(
            (7..14).any(|i| sim.players[i].pos.dist(def_before[i - 7]) > 0.2),
            "the defense did not move to the new call"
        );
    }

    #[test]
    fn an_interception_faces_the_other_way() {
        let (off, def) = teams();
        let book = playbook();
        let play = book.iter().find(|p| p.family.is_pass()).expect("no pass play");
        let dcall = defensive_playbook()[0].clone();
        let s = setup(45.0, 1, 10.0, 74);
        let mut sim = PlaySim::new(&off, &def, play, &dcall, &s);
        sim.request_snap();
        assert_eq!(sim.facing(), s.direction);
        sim.start_return(7, 45.0, 0, 14.0);
        assert_eq!(sim.facing(), -s.direction, "the picture did not turn round");
        assert_eq!(sim.returner_index(), Some(7));
    }

    #[test]
    fn a_return_that_reaches_the_goal_line_is_six() {
        let (off, def) = teams();
        let book = playbook();
        let play = book
            .iter()
            .find(|p| p.family.is_pass())
            .expect("no pass play");
        let dcall = defensive_playbook()[0].clone();
        let s = setup(45.0, 1, 10.0, 33);
        let mut sim = PlaySim::new(&off, &def, play, &dcall, &s);
        sim.request_snap();
        // He picks it near the goal he is running at, with nobody between him and
        // the paint.
        sim.players[7].pos = Vec2::new(1.4, 0.0);
        sim.start_return(7, 45.0, 0, 14.0);
        let mut guard = 0;
        while sim.result.is_none() && guard < 200 {
            sim.step(FIXED_DT);
            guard += 1;
        }
        let r = sim.result.expect("the return never ended");
        assert_eq!(r.outcome, PlayOutcome::Touchdown, "the goal line was not six");
        assert!(r.touchdown);
        assert!(r.turnover, "the score did not come off a turnover");
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
    fn a_dive_reaches_a_man_a_wrap_up_cannot() {
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
        // Past the end of a wrap-up, inside a dive's reach once it lunges.
        // Just past a wrap-up, and inside what a dive's lunge actually covers.
        let gap = DIVE_LUNGE + TACKLE_RADIUS - 0.05;
        sim.players[carrier].pos = Vec2::new(20.0, 0.0);
        sim.players[carrier].vel = Vec2::ZERO;
        sim.players[7].pos = Vec2::new(20.0 + gap, 0.0);
        sim.players[7].stun = 0.0;
        sim.players[7].tackle_cooldown = 0.0;
        sim.players[7].dive_t = 0.0;
        sim.set_def_control(Some(DefControl { tackle: true, ..Default::default() }));
        sim.step(FIXED_DT);
        assert!(
            sim.wrap.is_none() && sim.players[7].stun == 0.0,
            "a wrap-up reached {gap} yards"
        );
        let mut reached = false;
        // One press, then the body is in the air: the lunge covers the ground
        // over the window, so the defender is not reset between frames.
        sim.players[7].pos = Vec2::new(20.0 + gap, 0.0);
        sim.players[7].dive_t = 0.0;
        // The stick points at him: a dive goes where it is aimed, so this is
        // the press a player makes when he wants to lunge at a man.
        sim.set_def_control(Some(DefControl {
            move_dir: Vec2::new(-1.0, 0.0),
            dive: true,
            ..Default::default()
        }));
        for _ in 0..40 {
            if sim.result.is_some() {
                break;
            }
            sim.players[carrier].pos = Vec2::new(20.0, 0.0);
            sim.players[carrier].vel = Vec2::ZERO;
            sim.players[7].stun = 0.0;
            sim.players[7].tackle_cooldown = 0.0;
            sim.step(FIXED_DT);
            if sim.wrap.is_some() || sim.players[7].stun > 0.0 || sim.result.is_some() {
                reached = true;
                break;
            }
        }
        assert!(reached, "a dive from {gap} yards got nowhere");
    }




    #[test]
    fn a_defender_coming_for_the_carrier_puts_his_hands_up() {
        let (off, def_) = teams();
        let book = playbook();
        let dbook = defensive_playbook();
        let play = book.iter().find(|p| p.family.is_run()).expect("a run play");
        let mut sim = PlaySim::new(&off, &def_, play, &dbook[0], &setup(40.0, 1, 10.0, 11));
        let mut guard = 0;
        while sim.carrier.is_none() && sim.result.is_none() && guard < 900 {
            sim.step(1.0 / 60.0);
            guard += 1;
        }
        let carrier = sim.carrier.expect("the handoff");

        // Well away from him: hands stay down.
        for _ in 0..20 {
            sim.players[7].pos = sim.players[carrier].pos + Vec2::new(2.9, 0.0);
            sim.players[7].stun = 0.0;
            sim.step(1.0 / 60.0);
        }
        assert!(
            sim.players[7].tackle_arms < 0.4,
            "his hands were up from 2.9 yards: {:.2}",
            sim.players[7].tackle_arms
        );

        // Inside a yard, outside tackle range: reaching, not yet wrapped.
        for _ in 0..20 {
            sim.players[carrier].pos = Vec2::new(20.0, 0.0);
            sim.players[carrier].vel = Vec2::ZERO;
            sim.players[7].pos = Vec2::new(21.0, 0.0);
            sim.players[7].stun = 0.0;
            sim.players[7].tackle_cooldown = 0.0;
            sim.step(1.0 / 60.0);
        }
        assert!(
            sim.players[7].tackle_arms > 0.6,
            "his hands stayed down a yard away: {:.2}",
            sim.players[7].tackle_arms
        );
        assert!(sim.wrap.is_none(), "that was a tackle, not a reach");
    }


    #[test]
    fn a_quarterback_slides_instead_of_diving() {
        let (off, def_) = teams();
        let book = playbook();
        let dbook = defensive_playbook();
        let play = book.iter().find(|p| !p.family.is_run()).expect("a pass play");
        let mut sim = PlaySim::new(&off, &def_, play, &dbook[0], &setup(40.0, 1, 10.0, 11));
        let qb = sim.qb;
        let mut guard = 0;
        while sim.ball.holder != Some(qb) && sim.result.is_none() && guard < 90 {
            sim.step(1.0 / 60.0);
            guard += 1;
        }
        assert_eq!(sim.ball.holder, Some(qb), "the snap never reached him");
        // Past the line he is a runner, so the runner's buttons are his.
        let past = sim.los + 2.0 * sim.dirf();
        sim.players[qb].pos = Vec2::new(past, 0.0);
        assert!(sim.face_buttons_are_moves(), "he was still in the pocket");
        sim.request_move(Kind::Dive);
        assert_eq!(
            sim.carrier_move().kind(),
            Some(Kind::Slide),
            "the quarterback went head first"
        );
    }

    #[test]
    fn a_cut_breaks_across_his_own_line() {
        let (off, def_) = teams();
        let book = playbook();
        let dbook = defensive_playbook();
        let play = book.iter().find(|p| p.family.is_run()).expect("a run play");
        let mut sim = PlaySim::new(&off, &def_, play, &dbook[0], &setup(40.0, 1, 10.0, 11));
        let mut guard = 0;
        while sim.carrier.is_none() && sim.result.is_none() && guard < 900 {
            sim.step(1.0 / 60.0);
            guard += 1;
        }
        let c = sim.carrier.expect("the handoff");
        let down_field = Vec2::new(sim.dirf(), 0.0);
        sim.players[c].vel = down_field * 5.0;
        sim.request_move_dir(Kind::Cut, 1.0);
        assert_eq!(sim.carrier_move().kind(), Some(Kind::Cut));
        let broke = sim.carrier_move().dir();
        assert!(
            broke.dot(down_field).abs() < 0.1,
            "the cut went down the field instead of across it: {broke:?}"
        );
    }

    /// One press is one lunge, and the lunge is 2.2 yards.
    ///
    /// The reload ran on every frame the dive button was up -- which is every
    /// frame of the dive, because the press is a frame and the lunge is a third of
    /// a second. Each of those frames set the ground already flown back to zero and
    /// then added the whole curve again, so a single press walked the defender
    /// about twenty-six yards, with a two-yard jump on the last frame. Measured
    /// here frame by frame, which is how it was found.
    #[test]
    fn one_press_is_one_lunge_and_no_more() {
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
        sim.set_human_defender(Some(7));
        // Twelve yards off the ball: nothing he does can reach it in a lunge, so
        // the only thing moving him is the dive.
        let start = sim.players[carrier].pos + Vec2::new(0.0, 12.0);
        sim.players[7].pos = start;
        sim.players[7].vel = Vec2::ZERO;
        sim.players[7].stun = 0.0;
        sim.players[7].dive_t = 0.0;
        sim.players[7].dive_armed = true;
        sim.players[7].tackle_cooldown = 0.0;
        let stick = Vec2::new(1.0, 0.0);

        // The press lasts one frame; the button is up for the whole lunge after it.
        sim.set_def_control(Some(DefControl { move_dir: stick, dive: true, ..Default::default() }));
        sim.step(1.0 / 60.0);
        let mut worst: f32 = 0.0;
        let mut last = sim.players[7].pos;
        let mut frames = 0;
        while sim.players[7].dive_t > 0.0 && frames < 90 && sim.result.is_none() {
            sim.set_def_control(Some(DefControl { move_dir: stick, ..Default::default() }));
            sim.step(1.0 / 60.0);
            worst = worst.max(sim.players[7].pos.dist(last));
            last = sim.players[7].pos;
            frames += 1;
        }
        let travelled = start.dist(sim.players[7].pos);
        assert!(
            travelled <= DIVE_LUNGE + 0.05,
            "one press carried him {travelled:.2} yards; a lunge is {DIVE_LUNGE:.2}"
        );
        assert!(
            travelled > 1.0,
            "the lunge barely moved him: {travelled:.2} yards over {frames} frames"
        );
        assert!(
            worst < DIVE_LUNGE * 0.5,
            "one frame of the dive jumped {worst:.2} yards"
        );
        assert!(
            (sim.players[7].pos - start).dot(stick) > 0.9 * travelled,
            "the lunge did not go along the stick"
        );
    }

    /// Every man on defense, every frame: nobody crosses the field in one tick.
    ///
    /// The dive has been reported three times, so this walks all seven defenders
    /// through a live run -- the press, the lunge, the wrap and the pile that
    /// follows it -- and measures the ground each one covers between frames.
    /// A body at full speed covers about a seventh of a yard in a frame. Anything
    /// over half a yard is a mover that is not the man's legs: a homing aim, a
    /// snap onto the carrier, or a reload that re-applies a lunge.
    #[test]
    fn no_defender_crosses_the_field_in_a_frame() {
        let (off, def_) = teams();
        let book = playbook();
        let dbook = defensive_playbook();
        let play = book.iter().find(|p| p.family.is_run()).expect("a run play");
        let mut sim = PlaySim::new(&off, &def_, play, &dbook[0], &setup(40.0, 1, 10.0, 21));
        let mut guard = 0;
        while sim.carrier.is_none() && sim.result.is_none() && guard < 900 {
            sim.step(1.0 / 60.0);
            guard += 1;
        }
        let carrier = sim.carrier.expect("the handoff");
        sim.set_human_defender(sim.nearest_defender());
        let me = sim.human_defender.expect("no defender on the pad");
        let mut pressed = false;
        let mut prev: Vec<Vec2> = sim.players.iter().map(|p| p.pos).collect();
        // (defender, frame, ground covered that frame)
        let mut worst = (0usize, 0usize, 0.0f32);
        for frame in 0..360 {
            if sim.result.is_some() {
                break;
            }
            let gap = sim.players[me].pos.dist(sim.players[carrier].pos);
            // One press, when the ball is close enough for a lunge to mean
            // something. Everything after it is the button being up, which is
            // what the arcade sends for a tap.
            let dive = !pressed && gap < 3.5;
            if dive {
                pressed = true;
            }
            let stick = (sim.players[carrier].pos - sim.players[me].pos).normalize_or_zero();
            sim.set_def_control(Some(DefControl {
                move_dir: stick,
                dive,
                ..Default::default()
            }));
            sim.step(1.0 / 60.0);
            for i in 7..14 {
                let step = prev[i].dist(sim.players[i].pos);
                if step > worst.2 {
                    worst = (i, frame, step);
                }
            }
            prev = sim.players.iter().map(|p| p.pos).collect();
        }
        assert!(pressed, "he never got close enough to dive");
        assert!(
            worst.2 < 0.6,
            "defender {} covered {:.2} yards in frame {}: that is not his legs",
            worst.0,
            worst.2,
            worst.1
        );
    }

    /// The pad's man stays where he is put.
    ///
    /// With the stick at rest he used to keep working to the ball on his own, so
    /// the one defender the player was steering walked himself onto the carrier
    /// every time the stick was let go: the "attached to the ball" that outlived
    /// the dive's aim. Nothing but the stick moves him now.
    #[test]
    fn the_pad_stays_where_it_is_put() {
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
        sim.set_human_defender(Some(7));
        let start = sim.players[carrier].pos + Vec2::new(0.0, 12.0);
        sim.players[7].pos = start;
        sim.players[7].vel = Vec2::ZERO;
        sim.players[7].stun = 0.0;
        sim.players[7].dive_t = 0.0;
        sim.players[7].dive_armed = true;
        sim.players[7].tackle_cooldown = 0.0;
        for _ in 0..40 {
            sim.set_def_control(Some(DefControl::default()));
            sim.step(1.0 / 60.0);
        }
        let moved = start.dist(sim.players[7].pos);
        assert!(
            moved < 0.25,
            "he walked {moved:.2} yards across the field with no stick on him"
        );
    }

    /// A dive is thrown from where the man stands. With the ball twenty yards
    /// away, the press goes where the stick points and never at the carrier:
    /// homing on the ball from across the field is what read as being sucked to
    /// him the moment the button went down.
    #[test]
    fn a_dive_from_across_the_field_goes_where_he_is_pointing() {
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
        let downfield = Vec2::new(sim.dirf(), 0.0);

        sim.set_human_defender(Some(7));
        // Fifteen yards across the field from the ball, on his feet, pointing
        // downfield: the two directions are nothing like each other, so an aim
        // that turns at the carrier cannot pass for one that goes where he points.
        let side = if sim.players[carrier].pos.y > 0.0 { -1.0 } else { 1.0 };
        sim.players[7].pos = sim.players[carrier].pos + Vec2::new(0.0, 15.0 * side);
        sim.players[7].vel = Vec2::ZERO;
        sim.players[7].stun = 0.0;
        sim.players[7].dive_t = 0.0;
        sim.players[7].tackle_cooldown = 0.0;
        let away = sim.players[7].pos.dist(sim.players[carrier].pos);

        sim.set_def_control(Some(DefControl {
            move_dir: downfield,
            dive: true,
            ..Default::default()
        }));
        sim.step(1.0 / 60.0);
        let aim = sim.players[7].dive_aim;
        assert!(
            aim.dot(downfield) > 0.85,
            "the dive did not go where he was pointing: {aim:?}"
        );
        assert!(
            aim
                .dot((sim.players[carrier].pos - sim.players[7].pos).normalize_or_zero())
                .abs()
                < 0.4,
            "the dive turned at the ball {away:.1} yards away: {aim:?}"
        );

        // And close enough to put a hand on him: two yards across, still pointing
        // downfield. A dive with the ball on his shoulder still goes where he is
        // pointed, because the dive is a body movement and not a tackle.
        sim.players[7].pos = sim.players[carrier].pos + Vec2::new(0.0, 2.0 * side);
        sim.players[7].vel = Vec2::ZERO;
        sim.players[7].stun = 0.0;
        sim.players[7].dive_t = 0.0;
        sim.players[7].dive_armed = true;
        sim.players[7].tackle_cooldown = 0.0;
        sim.set_def_control(Some(DefControl {
            move_dir: downfield,
            dive: true,
            ..Default::default()
        }));
        sim.step(1.0 / 60.0);
        let close = sim.players[7].dive_aim;
        assert!(
            close.dot(downfield) > 0.85,
            "the dive still turned at a man two yards away: {close:?}"
        );
    }

    #[test]
    fn a_dive_is_never_teleported_onto_the_carrier() {
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
        let spot = sim.players[carrier].pos.x;

        // In the air, a yard short of him: the lunge is his arrival, and nothing
        // may move him onto the carrier.
        sim.players[7].pos = sim.players[carrier].pos + Vec2::new(1.0, 0.0);
        sim.players[7].dive_t = 0.2;
        let before = sim.players[7].pos;
        sim.start_wrap(carrier, 7, PlayOutcome::Run, spot, "a dive".to_string(), 0, false);
        sim.wrap = None;
        assert!(
            sim.players[7].pos.dist(before) < 0.01,
            "a dive teleported {:.2} yards onto the carrier",
            sim.players[7].pos.dist(before)
        );

        // The press is travel too. In the frame he leaves his feet he may cover a
        // frame of the window and no more, however far away the carrier is.
        sim.set_human_defender(Some(7));
        sim.players[7].pos = sim.players[carrier].pos + Vec2::new(2.0, 0.0);
        sim.players[7].dive_t = 0.0;
        sim.players[7].stun = 0.0;
        sim.players[7].tackle_cooldown = 0.0;
        let before = sim.players[7].pos;
        sim.set_def_control(Some(DefControl { dive: true, ..Default::default() }));
        sim.step(1.0 / 60.0);
        let moved = sim.players[7].pos.dist(before);
        // The burst is front-loaded, so a frame may move twice the average.
        let one_frame = 2.0 * DIVE_LUNGE / DIVE_WINDOW / 60.0;
        assert!(
            moved <= one_frame + 0.06,
            "the dive jumped {moved:.2} yards in one frame, a frame of it is {one_frame:.2}"
        );

        // On his feet inside the radius, the reach is spent arriving: he closes
        // to arm's length and no further.
        sim.players[7].dive_t = 0.0;
        sim.players[7].pos = sim.players[carrier].pos + Vec2::new(TACKLE_RADIUS - 0.02, 0.0);
        sim.start_wrap(carrier, 7, PlayOutcome::Run, spot, "a wrap".to_string(), 0, false);
        let gap = sim.players[7].pos.dist(sim.players[carrier].pos);
        // Hands length: outside the man's body and inside his arms. Snapped to
        // 0.44 it was inside the carrier and the separation pass shoved him back
        // out every frame, which is what made the pair judder.
        assert!(
            (BODY_RADIUS * 2.0..=HOLD_DISTANCE + 0.01).contains(&gap),
            "the wrap started {gap:.2} yards away: inside him, or a glide in"
        );
    }

    #[test]
    fn a_wrap_starts_with_the_tackler_on_him() {
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
        let mut guard = 0;
        while sim.wrap.is_none() && sim.result.is_none() && guard < 400 {
            // Just inside the tackle radius, and well outside an arm's length. On
            // his back side, so the runner's own stride does not carry him into
            // the tackler between the hit and the frame it is measured on.
            sim.players[7].pos = sim.players[carrier].pos - Vec2::new(TACKLE_RADIUS - 0.02, 0.0);
            sim.players[7].stun = 0.0;
            sim.players[7].down = false;
            sim.players[7].tackle_cooldown = 0.0;
            sim.step(1.0 / 60.0);
            guard += 1;
        }
        assert!(sim.wrap.is_some(), "no tackle was attempted from the radius");
        let gap = sim.players[carrier].pos.dist(sim.players[7].pos);
        // The runner is moving: between the hit and this frame he covers ground,
        // so the bound allows a stride and no more. A snap to inside the man's
        // body would still land under it.
        assert!(
            (BODY_RADIUS * 2.0..=HOLD_DISTANCE + 0.2).contains(&gap),
            "the wrap started with the tackler {gap:.2} yards away: inside him, or a glide in"
        );
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
    /// Run a handoff, then park a defender on the carrier's hip until the arms
    /// go on. Every test below is about what the picture shows from there.
    /// Take a run play to the moment the arms go on the carrier. The sim is the
    /// caller's, so the teams and the play call outlive it.
    fn to_the_wrap(sim: &mut PlaySim<'_>) -> usize {
        let mut guard = 0;
        while sim.carrier.is_none() && sim.result.is_none() && guard < 900 {
            sim.step(1.0 / 60.0);
            guard += 1;
        }
        let carrier = sim.carrier.expect("the handoff");
        guard = 0;
        while sim.wrap.is_none() && sim.result.is_none() && guard < 300 {
            sim.players[7].pos = sim.players[carrier].pos + Vec2::new(0.25, 0.0);
            sim.players[7].stun = 0.0;
            sim.players[7].down = false;
            sim.players[7].tackle_cooldown = 0.0;
            sim.step(1.0 / 60.0);
            guard += 1;
        }
        assert!(sim.wrap.is_some(), "no tackle was ever attempted");
        carrier
    }

    /// Held at hands length. The tackler used to be snapped to 0.44 yards, which
    /// is inside the carrier's body, and the separation pass shoved him back out
    /// again on the next frame: the pair juddered and read as two men in one
    /// shirt. A hold is a shoulder's width apart with the hands on the chest.
    #[test]
    fn a_tackle_holds_his_man_at_hands_length() {
        let (off, def_) = teams();
        let book = playbook();
        let dbook = defensive_playbook();
        let play = book.iter().find(|p| p.family.is_run()).expect("a run play");
        let mut sim = PlaySim::new(&off, &def_, play, &dbook[0], &setup(40.0, 1, 10.0, 3));
        let carrier = to_the_wrap(&mut sim);
        for _ in 0..12 {
            sim.step(1.0 / 60.0);
            let gap = sim.players[7].pos.dist(sim.players[carrier].pos);
            assert!(
                (0.60..=0.85).contains(&gap),
                "the tackler held him at {gap:.2} yards"
            );
        }
    }

    /// A second and a third man on the same body take their own side of it.
    /// Converging on one point put them inside each other, which is the blob the
    /// report called telekinesis: nobody could see a hand on anybody.
    #[test]
    fn the_second_and_third_man_get_their_own_side_of_him() {
        let (off, def_) = teams();
        let book = playbook();
        let dbook = defensive_playbook();
        let play = book.iter().find(|p| p.family.is_run()).expect("a run play");
        let mut sim = PlaySim::new(&off, &def_, play, &dbook[0], &setup(40.0, 1, 10.0, 5));
        let carrier = to_the_wrap(&mut sim);
        // Both of them arriving from the same side is the ordinary case: a
        // pursuit converges, it does not space itself out.
        sim.players[8].pos = sim.players[carrier].pos + Vec2::new(0.4, 1.4);
        sim.players[9].pos = sim.players[carrier].pos + Vec2::new(1.2, 1.2);
        for _ in 0..20 {
            if sim.result.is_some() {
                break;
            }
            sim.players[8].stun = 0.0;
            sim.players[9].stun = 0.0;
            sim.step(1.0 / 60.0);
        }
        let a = sim.players[8].pos - sim.players[carrier].pos;
        let b = sim.players[9].pos - sim.players[carrier].pos;
        assert!(
            a.length() < 1.05 && b.length() < 1.05,
            "they never got to him: {:.2} and {:.2} yards off",
            a.length(),
            b.length()
        );
        assert!(
            a.length() > BODY_RADIUS * 2.0 && b.length() > BODY_RADIUS * 2.0,
            "a helper was standing inside him: {:.2} and {:.2} yards",
            a.length(),
            b.length()
        );
        assert!(
            a.normalize_or_zero().dot(b.normalize_or_zero()) < 0.4,
            "both piled on the same side of him"
        );
    }

    /// A blocker blocks with his hands on the man, not from the end of a zone.
    /// He used to engage at 1.9 yards and only close from inside 1.5, so a lineman
    /// could be holding a man off and never touch him.
    #[test]
    fn a_blocker_walks_in_until_his_hands_are_on_the_man() {
        let (off, def_) = teams();
        let book = playbook();
        let dbook = defensive_playbook();
        let play = book.iter().find(|p| p.family.is_run()).expect("a run play");
        let mut sim = PlaySim::new(&off, &def_, play, &dbook[0], &setup(40.0, 1, 10.0, 7));
        let mut pair = None;
        for _ in 0..240 {
            sim.step(1.0 / 60.0);
            for i in 0..7 {
                if let Some(d) = sim.players[i].engaged {
                    pair = Some((i, d));
                    break;
                }
            }
            if pair.is_some() {
                break;
            }
        }
        let (i, d) = pair.expect("nobody ever got a hand on a rusher");
        // Park everyone else off the field so the blocker has one man to deal
        // with, then set that man a yard and a half away: inside the reach his
        // hands give him, and outside the 1.5 yards the old step-in started at.
        for q in 7..14 {
            if q != d {
                sim.players[q].pos = Vec2::new(sim.players[q].pos.x, 70.0);
            }
        }
        let away = (sim.players[d].pos - sim.players[i].pos).normalize_or_zero();
        sim.players[d].pos = sim.players[i].pos + away * 1.7;
        let mut closest: f32 = 1.7;
        for _ in 0..40 {
            if sim.result.is_some() {
                break;
            }
            sim.step(1.0 / 60.0);
            closest = closest.min(sim.players[i].pos.dist(sim.players[d].pos));
        }
        // Hands on the chest, a shoulder's width apart. He used to hold off at
        // the end of a zone: engaged from 1.9 yards and never closing from 1.7.
        assert!(
            closest < 1.05,
            "a blocker held a man at {closest:.2} yards with nobody touching anybody"
        );
    }

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
    /// A lateral that hits the turf is not an incompletion: the ball is live.
    #[test]
    fn a_backward_pass_that_hits_the_turf_is_still_live() {
        let (off, def_) = teams();
        let book = playbook();
        let d = defensive_playbook();
        let play = book.iter().find(|p| p.family.is_pass()).expect("a pass play");
        let mut sim = PlaySim::new(&off, &def_, play, &d[0], &setup(45.0, 1, 10.0, 3));
        let from = sim.players[sim.qb].pos;
        let flight = Flight {
            from,
            to: Vec2::new(from.x - 4.0 * sim.dirf(), from.y + 2.0),
            t: 1.0,
            duration: 1.0,
            peak: 0.4,
            target: None,
            thrower: sim.qb,
            air_yards: 0.0,
            speed: 12.0,
        };
        sim.phase = Phase::BallInAir;
        sim.ball.holder = None;
        sim.resolve_catch(&flight);
        assert!(sim.result.is_none(), "a backward pass was whistled dead");
        assert_eq!(sim.phase, Phase::Loose, "the ball did not come loose");
        assert!(sim.ball.holder.is_none(), "somebody had it already");
        let mut guard = 0;
        while sim.result.is_none() && guard < 2400 {
            sim.step(FIXED_DT);
            guard += 1;
        }
        let r = sim.result.clone().expect("the loose ball never resolved");
        assert!(
            matches!(
                r.outcome,
                PlayOutcome::FumbleRecovered
                    | PlayOutcome::FumbleLost
                    | PlayOutcome::Touchdown
                    | PlayOutcome::Safety
            ),
            "a live ball was booked as {:?}: {}",
            r.outcome,
            r.description
        );
    }

    /// A returner is a runner: he gets the four moves the back gets.
    #[test]
    fn a_returner_has_the_moves_a_back_has() {
        let (off, def_) = teams();
        let play = &playbook()[0];
        let d = defensive_playbook();
        let mut sim = PlaySim::new(&off, &def_, play, &d[0], &setup(45.0, 1, 10.0, 1));
        sim.await_snap(false);
        let qb = sim.qb;
        sim.start_return(7, 45.0, qb, 8.0);
        assert_eq!(sim.phase, Phase::Return);
        assert_eq!(sim.carrier, Some(7));
        sim.request_move(Kind::Spin);
        assert_eq!(
            sim.carrier_move().kind(),
            Some(Kind::Spin),
            "the returner could not spin"
        );
        // And a dive ends the return where he lands, like it ends a run.
        for _ in 0..40 {
            sim.step(FIXED_DT);
        }
        sim.request_move(Kind::Dive);
        assert_eq!(sim.carrier_move().kind(), Some(Kind::Dive));
        let mut guard = 0;
        while sim.result.is_none() && guard < 200 {
            sim.step(FIXED_DT);
            guard += 1;
        }
        assert!(sim.result.is_some(), "a dive on a return never ended the play");
        assert_eq!(sim.phase, Phase::Dead);
    }

    /// A try is over the moment a defender has the ball: no return, no six.
    #[test]
    fn a_try_dies_the_moment_the_defence_has_it() {
        let (off, def_) = teams();
        let play = &playbook()[0];
        let d = defensive_playbook();
        let mut setup_try = setup(45.0, 1, 10.0, 1);
        setup_try.is_try = true;

        let mut picked = PlaySim::new(&off, &def_, play, &d[0], &setup_try);
        picked.await_snap(false);
        let qb = picked.qb;
        picked.start_return(7, 45.0, qb, 8.0);
        let r = picked.result.clone().expect("the pick did not end the try");
        assert_eq!(r.outcome, PlayOutcome::Interception);
        assert_eq!(picked.phase, Phase::Dead);
        assert!(!r.touchdown, "a pick on a try was run back for six");
        assert_eq!(r.return_yards, 0.0, "a pick on a try was run back at all");

        let mut fumbled = PlaySim::new(&off, &def_, play, &d[0], &setup_try);
        fumbled.await_snap(false);
        fumbled.start_fumble_return(8, 44.0);
        let r = fumbled.result.clone().expect("the recovery did not end the try");
        assert_eq!(r.outcome, PlayOutcome::FumbleLost);
        assert_eq!(fumbled.phase, Phase::Dead);
        assert!(!r.touchdown, "a recovered fumble on a try was run back for six");
        assert_eq!(r.return_yards, 0.0);
    }

    /// The pad follows the man with the ball, not the man nearest the line.
    #[test]
    fn the_nearest_defender_is_the_one_on_the_carrier() {
        let (off, def_) = teams();
        let play = &playbook()[0];
        let d = defensive_playbook();
        let mut sim = PlaySim::new(&off, &def_, play, &d[0], &setup(45.0, 1, 10.0, 1));
        sim.await_snap(false);
        for _ in 0..30 {
            sim.step(FIXED_DT);
        }
        // Put the ball in the quarterback's hands wherever the play has left him:
        // who ends up with it is not what this is testing.
        let qb = sim.qb;
        sim.carrier = Some(qb);
        let nearest = sim
            .nearest_defender_to_carrier()
            .expect("no defender was offered for the carrier");
        assert_eq!(sim.players[nearest].side, Side::Defense);
        let there = sim.players[qb].pos;
        let best = sim.players[nearest].pos.dist(there);
        for i in 7..14 {
            if sim.players[i].down || sim.players[i].stun > 0.0 {
                continue;
            }
            let away = sim.players[i].pos.dist(there);
            assert!(
                away + 1e-3 >= best,
                "a defender {away:.2} yards away was passed over for one {best:.2} away"
            );
        }
    }

    /// Standing over the ball until the count runs out is a five-yard foul, and
    /// the ball never goes: nothing happened, so nothing after the count counts.
    #[test]
    fn running_out_the_play_clock_is_a_delay_of_game() {
        let (off, def_) = teams();
        let play = &playbook()[0];
        let d = defensive_playbook();
        let mut sim = PlaySim::new(&off, &def_, play, &d[0], &setup(45.0, 1, 10.0, 1));
        sim.await_snap(true);
        assert!(sim.play_clock > 20.0, "the clock started at {}", sim.play_clock);
        let mut guard = 0;
        while sim.result.is_none() && guard < 2400 {
            sim.step(FIXED_DT);
            guard += 1;
        }
        let r = sim.result.clone().expect("the play clock never ran out");
        assert_eq!(r.outcome, PlayOutcome::Penalty, "{:?}", r.outcome);
        let call = r.penalty.clone().expect("no foul was booked");
        assert_eq!(call.kind, crate::rules::PenaltyKind::DelayOfGame);
        assert_eq!(call.against, FoulTeam::Offense);
        assert_eq!(sim.phase, Phase::Dead, "the ball went anyway");
        assert_eq!(sim.play_clock, 0.0);
        assert_eq!(r.yards, 0.0, "a foul before the snap is not a play");
    }

    /// A fumble puts the plate up the moment the ball comes out, with the words of
    /// it, and the plate comes down on its own: it is the only way a player learns
    /// the ball is on the ground behind him.
    #[test]
    fn a_fumble_raises_the_alert() {
        let (off, def_) = teams();
        let book = playbook();
        let d = defensive_playbook();
        let play = &book[0];
        let mut sim = PlaySim::new(&off, &def_, play, &d[0], &setup(38.0, 1, 10.0, 3));
        let mut guard = 0;
        while sim.carrier.is_none() && sim.result.is_none() && guard < 600 {
            sim.step(FIXED_DT);
            guard += 1;
        }
        let Some(carrier) = sim.carrier else {
            return;
        };
        assert_eq!(sim.fumble_alert, 0.0, "the plate was up before the ball came out");
        sim.fumble(carrier, 7);
        assert!(sim.fumble_alert > 0.0, "no alert on a fumble");
        assert!(
            sim.fumble_note.contains("loose"),
            "the alert says nothing: {}",
            sim.fumble_note
        );
        let started = sim.fumble_alert;
        for _ in 0..40 {
            if sim.result.is_some() {
                break;
            }
            sim.step(FIXED_DT);
        }
        assert!(
            sim.fumble_alert < started || sim.result.is_some(),
            "the fumble plate never came down"
        );
    }

    /// A strip does not end the play: the ball is on the ground, both teams go
    /// after it, and whoever comes up with it runs until he is tackled.
    #[test]
    fn a_fumble_is_a_live_ball_until_somebody_is_tackled() {
        let (off, def_) = teams();
        let book = playbook();
        let d = defensive_playbook();
        let mut strips = 0;
        for seed in 0..24u64 {
            let play = &book[seed as usize % book.len()];
            let mut sim = PlaySim::new(
                &off,
                &def_,
                play,
                &d[seed as usize % d.len()],
                &setup(38.0, 1, 10.0, seed),
            );
            let mut guard = 0;
            while sim.carrier.is_none() && sim.result.is_none() && guard < 600 {
                sim.step(FIXED_DT);
                guard += 1;
            }
            let Some(carrier) = sim.carrier else { continue };
            sim.fumble(carrier, 7);
            assert!(sim.result.is_none(), "the strip killed the play on the spot");
            assert_eq!(sim.phase, Phase::Loose, "the ball was not on the ground");
            strips += 1;
            let mut guard = 0;
            while sim.result.is_none() && guard < 3600 {
                sim.step(FIXED_DT);
                guard += 1;
            }
            let r = sim.result.clone().expect("nobody came up with the loose ball");
            assert!(
                matches!(
                    r.outcome,
                    PlayOutcome::FumbleRecovered
                        | PlayOutcome::FumbleLost
                        | PlayOutcome::Touchdown
                        | PlayOutcome::Safety
                ),
                "a recovered fumble was booked as {:?}: {}",
                r.outcome,
                r.description
            );
        }
        assert!(strips > 0, "no snap produced a runner to strip");
    }
    /// A carrier never runs back at the line of scrimmage: a route point behind
    /// him is not a target.
    #[test]
    fn a_carrier_never_turns_back_toward_the_line_of_scrimmage() {
        let (off, def_) = teams();
        let book = playbook();
        let d = defensive_playbook();
        let play = book.iter().find(|p| p.family.is_pass()).expect("a pass play");
        let mut sim = PlaySim::new(&off, &def_, play, &d[0], &setup(45.0, 1, 10.0, 5));
        let receiver = 3usize;
        let here = sim.players[receiver].pos;
        sim.players[receiver].route = vec![
            Vec2::new(here.x - 12.0, here.y),
            Vec2::new(here.x + 15.0, here.y + 3.0),
        ];
        sim.players[receiver].waypoint = 0;
        sim.carrier = Some(receiver);
        sim.ball.holder = Some(receiver);
        let target = sim.carry_target(receiver);
        assert!(
            target.x > here.x,
            "the carrier was aimed back at {:?} from {:?}",
            target,
            here
        );
    }

    /// Blockers get their hands on the nearest rusher, and the hand on the side
    /// he goes to never follows him more than 45 degrees across the body.
    #[test]
    fn a_blocker_puts_his_hands_on_the_nearest_rusher() {
        let (off, def_) = teams();
        let book = playbook();
        let d = defensive_playbook();
        let play = book.iter().find(|p| p.family.is_pass()).expect("a pass play");
        let mut blocked = 0;
        for seed in 0..16u64 {
            let mut sim = PlaySim::new(
                &off,
                &def_,
                play,
                &d[seed as usize % d.len()],
                &setup(45.0, 1, 10.0, seed),
            );
            for _ in 0..200 {
                sim.step(FIXED_DT);
                if sim.result.is_some() {
                    break;
                }
                for i in 0..7 {
                    if sim.players[i].engaged.is_some() {
                        blocked += 1;
                        assert!(
                            sim.players[i].hands.abs() <= std::f32::consts::FRAC_PI_4 + 0.05,
                            "hands {:.2} rad across the body",
                            sim.players[i].hands
                        );
                        let d_i = sim.players[i].engaged.unwrap();
                        let clash = sim.players[d_i].pos.dist(sim.players[i].pos);
                        assert!(clash <= 1.9 + 0.05, "hands on a man {clash:.2} away");
                    }
                }
            }
        }
        assert!(blocked > 0, "nobody ever got his hands on a rusher");
    }

    /// Run blocking is a shove: the hands come up and the man in front is walked
    /// back upfield.
    #[test]
    fn run_blockers_drive_the_man_in_front_of_them_back() {
        let (off, def_) = teams();
        let book = playbook();
        let d = defensive_playbook();
        let play = book.iter().find(|p| p.family.is_run()).expect("a run play");
        let mut best = 0.0f32;
        for seed in 0..16u64 {
            let mut sim = PlaySim::new(
                &off,
                &def_,
                play,
                &d[seed as usize % d.len()],
                &setup(45.0, 1, 10.0, seed),
            );
            for _ in 0..200 {
                sim.step(FIXED_DT);
                if sim.result.is_some() {
                    break;
                }
                for i in 0..7 {
                    if sim.players[i].engaged.is_some() {
                        best = best.max(sim.players[i].drive);
                    }
                }
            }
        }
        assert!(best > 0.25, "run blockers never got their hands up: {best}");
    }

    /// Hands out in front are for blockers only: the line on every play, the backs
    /// and receivers on a run, and never the man carrying the ball -- and never a
    /// defender, who is the one being blocked.
    #[test]
    fn only_blockers_are_posed_with_their_hands_out() {
        let (off, def_) = teams();
        let book = playbook();
        let d = defensive_playbook();
        let mut checked = 0;
        for play in book.iter() {
            for seed in 0..6u64 {
                let mut sim = PlaySim::new(
                    &off,
                    &def_,
                    play,
                    &d[seed as usize % d.len()],
                    &setup(45.0, 1, 10.0, seed),
                );
                // A second and a half in the ball is out and the carrier has settled,
                // which is where the picture has to be right.
                while sim.t < SNAP_DELAY + 1.5 && sim.result.is_none() {
                    sim.step(FIXED_DT);
                }
                if sim.result.is_some() {
                    continue;
                }
                checked += 1;
                let carried = sim.carrier;
                for i in 0..7 {
                    let role = OffRole::ALL[sim.players[i].role_index];
                    let lineman = matches!(role, OffRole::Lg | OffRole::Rg);
                    let want = !sim.players[i].down
                        && carried != Some(i)
                        && (lineman || (play.family.is_run() && role != OffRole::Qb));
                    assert_eq!(
                        sim.players[i].blocking,
                        want,
                        "role {:?} on play {} with carrier {carried:?}",
                        role,
                        play.name
                    );
                }
                for p in sim.players.iter().skip(7) {
                    assert!(!p.blocking, "a defender of all people was posed blocking");
                }
            }
        }
        assert!(checked > 6, "hardly any play got far enough to check: {checked}");
    }

    /// The hit stick is worth more of the ball the stronger the tackler is: a
    /// high hit over the nameplate is the best of it, a low hit is still a hit,
    /// and a man with no stick behind the tackle gets the ordinary roll.
    #[test]
    fn the_hit_stick_goes_for_the_ball() {
        use crate::team::Attributes;
        let carrier = Attributes {
            break_tackle: 60,
            awareness: 60,
            ..Default::default()
        };
        let strong = Attributes {
            strength: 90,
            tackling: 90,
            ..Default::default()
        };
        let weak = Attributes {
            strength: 45,
            tackling: 45,
            ..Default::default()
        };
        let plain = fumble_chance(&carrier, &strong, None, 0.0);
        let low = fumble_chance(&carrier, &strong, Some(Hit::Low), 0.0);
        let high = fumble_chance(&carrier, &strong, Some(Hit::High), 0.0);
        assert!(high > low && low > plain, "{plain} {low} {high}");
        assert!(
            high - plain > 0.03,
            "the hit barely moved the odds: {}",
            high - plain
        );
        assert!(high < 0.5, "a high hit is a coin flip: {high}");
        // The ratings are what the hit is made of.
        assert!(
            high > fumble_chance(&carrier, &weak, Some(Hit::High), 0.0) + 0.02,
            "strength and tackling did not decide the hit"
        );
    }
}

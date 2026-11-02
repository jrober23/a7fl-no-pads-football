//! The A7FL rulebook as data.
//!
//! Everything here comes from the published "Official Rules of American 7s Football
//! League": 4 x 15 minute quarters, a 30 second play clock, throw-offs instead of
//! kickoffs, no kicking at all, 1- or 2-point conversion by scrimmage play, overtime
//! possessions from the 25, and the mercy rule (five touchdowns in the first
//! quarter, seven after it).
//!
//! Penalty distances, dead/live status, "loss of down" and automatic first downs are
//! transcribed from the A7FL penalty summary. Enforcement uses `BasicSpot`,
//! `PreviousSpot`, `SpotOfFoul` and `EndOfRun` semantics like the rulebook describes,
//! including the "no penalty exceeds half the distance to the goal" cap.

use crate::field::{self, FIELD_LENGTH};
use crate::math::{clamp, Rgb};

pub const QUARTER_SECONDS: f32 = 15.0 * 60.0;
pub const PLAY_CLOCK_SECONDS: f32 = 30.0;
pub const OVERTIME_PLAY_CLOCK_SECONDS: f32 = 30.0;
pub const DOWNS: u8 = 4;
pub const YARDS_TO_GO: f32 = 10.0;
pub const TIMEOUTS_PER_HALF: u8 = 3;
pub const TIMEOUT_SECONDS: f32 = 30.0;
pub const PLAYERS_ON_FIELD: usize = 7;
pub const TWO_MINUTE_WARNING: f32 = 120.0;

/// Between snaps the game clock keeps running in the A7FL (running clock). This is
/// the huddle / play-clock time we burn per snap so a simulated game lasts about
/// as long as a real one.
pub const TIME_BETWEEN_SNAPS: f32 = 22.0;

/// Throw-offs replace kickoffs: thrown from the throwing team's own 35 and must
/// travel 40 yards in the air.
pub const THROW_OFF_SPOT_YARD_LINE: f32 = 35.0;
pub const THROW_OFF_MIN_AIR_YARDS: f32 = 40.0;
/// The far goal line is 65 yards away from the 35. A throw-off tops out twenty
/// yards short of it, so the return has somewhere to go: 45 air yards.
pub const THROW_OFF_MAX_AIR_YARDS: f32 = 45.0;
pub const THROW_OFF_TOUCHBACK_TO: f32 = 20.0;
pub const SAFETY_THROW_OFF_YARD_LINE: f32 = 25.0;
/// Kicking team sends 3 players, receiving team sends 1 returner.
pub const THROW_OFF_GUNNERS: usize = 3;

/// No kicking: a failed 4th down is replaced by a throw-off from the line of
/// scrimmage with no minimum distance.
pub const OVERTIME_SPOT_YARD_LINE: f32 = 25.0;
pub const OVERTIME_FIRST_DOWNS_ALLOWED: u8 = 2;
pub const MERCY_LEAD_TOUCHDOWNS: u32 = 7;
/// The first quarter is called early: five touchdowns is a game that has
/// stopped being a game, and the league does not make anybody play out the rest
/// of a 30-point half. From the second quarter on it takes seven.
pub const MERCY_LEAD_TOUCHDOWNS_EARLY: u32 = 5;

/// Conversion spots (from the scoring team's perspective, yards to goal).
pub const CONVERSION_ONE_POINT_YARDS: f32 = 5.0;
pub const CONVERSION_TWO_POINT_YARDS: f32 = 10.0;

/// Visual accent shared by the broadcast package.
pub const A7FL_RED: Rgb = Rgb::hex(0xE4572E);
pub const A7FL_INK: Rgb = Rgb::hex(0x0E1116);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum FoulTeam {
    Offense,
    Defense,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpotKind {
    /// Repeat the down from where it started.
    PreviousSpot,
    /// Walk off from where the foul happened.
    SpotOfFoul,
    /// Walk off from where the run ended.
    EndOfRun,
    /// Rulebook "basic spot": previous spot for our purposes.
    BasicSpot,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum PenaltyKind {
    FalseStart,
    Encroachment,
    OffsideDefense,
    NeutralZoneInfraction,
    DelayOfGame,
    IllegalFormation,
    IllegalShift,
    IllegalMotion,
    TooManyPlayers,
    IllegalSubstitution,
    Holding,
    IllegalBlockInTheBack,
    IllegalUseOfHands,
    Clipping,
    BlockingBelowWaist,
    ChopBlock,
    Tripping,
    IllegalForwardPass,
    IntentionalGrounding,
    IneligibleReceiverDownfield,
    OffensivePassInterference,
    DefensivePassInterference,
    DefensiveHolding,
    IllegalContact,
    RoughingThePasser,
    LateHit,
    Unsportsmanlike,
    Targeting,
    HorseCollar,
    BlindsideBlock,
    IllegallyBattingBall,
    FairCatchInterference,
    IllegalKick,
}

impl PenaltyKind {
    pub fn yards(self) -> f32 {
        use PenaltyKind::*;
        match self {
            FalseStart | Encroachment | OffsideDefense | NeutralZoneInfraction | DelayOfGame
            | IllegalFormation | IllegalShift | IllegalMotion | TooManyPlayers
            | IllegalSubstitution | IllegalForwardPass | IntentionalGrounding
            | IneligibleReceiverDownfield | DefensiveHolding | IllegalContact => 5.0,
            Holding | IllegalBlockInTheBack | IllegalUseOfHands | Tripping
            | IllegallyBattingBall | IllegalKick => 10.0,
            Clipping | BlockingBelowWaist | ChopBlock | OffensivePassInterference
            | RoughingThePasser | LateHit | Unsportsmanlike | Targeting | HorseCollar
            | BlindsideBlock | FairCatchInterference => 15.0,
            DefensivePassInterference => 15.0,
        }
    }

    pub fn against(self) -> FoulTeam {
        use PenaltyKind::*;
        match self {
            DefensivePassInterference | DefensiveHolding | IllegalContact | RoughingThePasser
            | OffsideDefense | NeutralZoneInfraction | FairCatchInterference | Targeting
            | HorseCollar | IllegallyBattingBall | IllegalKick => FoulTeam::Defense,
            LateHit | Unsportsmanlike => FoulTeam::Defense,
            _ => FoulTeam::Offense,
        }
    }

    pub fn label(self) -> &'static str {
        use PenaltyKind::*;
        match self {
            FalseStart => "FALSE START",
            Encroachment => "ENCROACHMENT",
            OffsideDefense => "OFFSIDE",
            NeutralZoneInfraction => "NEUTRAL ZONE INFRACTION",
            DelayOfGame => "DELAY OF GAME",
            IllegalFormation => "ILLEGAL FORMATION",
            IllegalShift => "ILLEGAL SHIFT",
            IllegalMotion => "ILLEGAL MOTION",
            TooManyPlayers => "TOO MANY MEN ON THE FIELD",
            IllegalSubstitution => "ILLEGAL SUBSTITUTION",
            Holding => "HOLDING",
            IllegalBlockInTheBack => "BLOCK IN THE BACK",
            IllegalUseOfHands => "ILLEGAL USE OF HANDS",
            Clipping => "CLIPPING",
            BlockingBelowWaist => "BLOCK BELOW THE WAIST",
            ChopBlock => "CHOP BLOCK",
            Tripping => "TRIPPING",
            IllegalForwardPass => "ILLEGAL FORWARD PASS",
            IntentionalGrounding => "INTENTIONAL GROUNDING",
            IneligibleReceiverDownfield => "INELIGIBLE MAN DOWN FIELD",
            OffensivePassInterference => "OFFENSIVE PASS INTERFERENCE",
            DefensivePassInterference => "DEFENSIVE PASS INTERFERENCE",
            DefensiveHolding => "DEFENSIVE HOLDING",
            IllegalContact => "ILLEGAL CONTACT",
            RoughingThePasser => "ROUGHING THE PASSER",
            LateHit => "LATE HIT",
            Unsportsmanlike => "UNSPORTSMANLIKE CONDUCT",
            Targeting => "TARGETING",
            HorseCollar => "HORSE COLLAR TACKLE",
            BlindsideBlock => "BLINDSIDE BLOCK",
            IllegallyBattingBall => "ILLEGALLY BATTED BALL",
            FairCatchInterference => "FAIR CATCH INTERFERENCE",
            IllegalKick => "ILLEGAL KICKING",
        }
    }

    pub fn spot_kind(self) -> SpotKind {
        use PenaltyKind::*;
        match self {
            DefensivePassInterference | Targeting | HorseCollar | Clipping | BlindsideBlock => {
                SpotKind::SpotOfFoul
            }
            LateHit => SpotKind::EndOfRun,
            Holding | IllegalBlockInTheBack | IllegalUseOfHands | BlockingBelowWaist
            | ChopBlock | OffensivePassInterference => SpotKind::BasicSpot,
            _ => SpotKind::PreviousSpot,
        }
    }

    /// Dead ball fouls are whistled before the play can develop.
    pub fn dead_ball(self) -> bool {
        use PenaltyKind::*;
        matches!(
            self,
            FalseStart
                | Encroachment
                | NeutralZoneInfraction
                | DelayOfGame
                | IllegalSubstitution
                | Unsportsmanlike
                | IllegalFormation
        )
    }

    pub fn auto_first_down(self) -> bool {
        use PenaltyKind::*;
        matches!(
            self,
            DefensivePassInterference
                | DefensiveHolding
                | IllegalContact
                | RoughingThePasser
                | Targeting
        )
    }

    pub fn loss_of_down(self) -> bool {
        use PenaltyKind::*;
        matches!(self, IntentionalGrounding | IllegalForwardPass | IllegallyBattingBall)
    }

    pub fn replay_down(self) -> bool {
        !self.loss_of_down() && !self.auto_first_down()
    }

    /// Infractions serious enough for the referee to consider an ejection.
    pub fn ejection_risk(self) -> bool {
        matches!(self, PenaltyKind::Targeting | PenaltyKind::Unsportsmanlike)
    }
}

/// A called foul. `spot_of_foul` is a canonical field coordinate.
#[derive(Clone, Debug)]
pub struct PenaltyCall {
    pub kind: PenaltyKind,
    pub against: FoulTeam,
    pub spot_of_foul: f32,
    pub player: Option<usize>,
}

impl PenaltyCall {
    pub fn new(kind: PenaltyKind, spot_of_foul: f32) -> Self {
        PenaltyCall { kind, against: kind.against(), spot_of_foul, player: None }
    }

    pub fn with_player(mut self, player: usize) -> Self {
        self.player = Some(player);
        self
    }

    pub fn label(&self) -> &'static str {
        self.kind.label()
    }
}

/// Outcome of walking a foul off, expressed in canonical field coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Enforcement {
    pub spot: f32,
    pub down: u8,
    pub to_go: f32,
    pub replay_down: bool,
    pub auto_first_down: bool,
    pub loss_of_down: bool,
    /// Yards walked off, always positive.
    pub yards: f32,
    /// True when the half-the-distance rule shortened the walk-off.
    pub half_the_distance: bool,
}

/// Apply a penalty to the current series.
///
/// * `prev_spot` — the ball's spot at the snap.
/// * `direction` — the offense's attack direction (`+1` toward `x = 100`).
/// * `end_of_run` — where the play ended, used by end-of-run fouls.
pub fn enforce(
    call: &PenaltyCall,
    prev_spot: f32,
    direction: i8,
    end_of_run: f32,
    down: u8,
    to_go: f32,
) -> Enforcement {
    let kind = call.kind;
    let prev_ytg = field::yards_to_goal(prev_spot, direction);
    let base = match kind.spot_kind() {
        SpotKind::PreviousSpot => prev_spot,
        // Rulebook "basic spot": walk it off from the foul, but never from behind
        // the line of scrimmage.
        SpotKind::BasicSpot => {
            let foul = clamp(call.spot_of_foul, 0.0, FIELD_LENGTH);
            if field::yards_to_goal(foul, direction) >= prev_ytg {
                prev_spot
            } else {
                foul
            }
        }
        SpotKind::SpotOfFoul => clamp(call.spot_of_foul, -field::ENDZONE_DEPTH, 110.0),
        SpotKind::EndOfRun => end_of_run,
    };
    let base = clamp(base, 0.0, FIELD_LENGTH);
    let target_ytg = (prev_ytg - to_go).max(0.0);

    let mut half_the_distance = false;
    let mut walked = kind.yards();
    let mut spot;

    if kind == PenaltyKind::DefensivePassInterference {
        // Spot foul with an automatic first down. A foul in the end zone puts the
        // ball on the 1.
        let foul_ytg = field::yards_to_goal(clamp(call.spot_of_foul, 0.0, FIELD_LENGTH), direction);
        let new_ytg = if foul_ytg <= 1.0 { 1.0 } else { foul_ytg };
        spot = field::spot_from_yards_to_goal(new_ytg, direction);
        walked = prev_ytg - new_ytg;
    } else {
        // Distance available in the direction the walk-off travels before the goal line.
        let available = match call.against {
            FoulTeam::Offense => {
                if direction > 0 {
                    base
                } else {
                    FIELD_LENGTH - base
                }
            }
            FoulTeam::Defense => field::yards_to_goal(base, direction),
        };
        let half = (available * 0.5).max(0.5);
        if walked > half {
            walked = half;
            half_the_distance = true;
        }
        let signed = match call.against {
            FoulTeam::Offense => {
                if direction > 0 {
                    -walked
                } else {
                    walked
                }
            }
            FoulTeam::Defense => {
                if direction > 0 {
                    walked
                } else {
                    -walked
                }
            }
        };
        spot = clamp(base + signed, 0.0, FIELD_LENGTH);
    }

    // Never spot the ball in an end zone (except the DPI case handled above).
    let mut ytg = field::yards_to_goal(spot, direction);
    if !(0.5..=99.5).contains(&ytg) {
        ytg = clamp(ytg, 0.5, 99.5);
        spot = field::spot_from_yards_to_goal(ytg, direction);
    }

    let new_ytg = ytg;
    let mut down_out = down;
    let mut replay = kind.replay_down();
    let mut auto_first = kind.auto_first_down();
    let loss = kind.loss_of_down();
    let to_go_out;

    if loss {
        down_out = (down + 1).min(DOWNS);
        replay = false;
        // Same target line, recalculated distance to gain.
        to_go_out = (new_ytg - target_ytg).max(1.0);
    } else if auto_first || new_ytg <= target_ytg {
        auto_first = true;
        down_out = 1;
        replay = false;
        to_go_out = new_ytg.min(YARDS_TO_GO).max(1.0);
    } else {
        to_go_out = (new_ytg - target_ytg).max(1.0);
    }

    Enforcement {
        spot,
        down: down_out,
        to_go: to_go_out,
        replay_down: replay,
        auto_first_down: auto_first,
        loss_of_down: loss,
        yards: walked.max(0.0),
        half_the_distance,
    }
}

/// Broadcast phrasing, e.g. "Holding, offense, 10 yards. Repeat second down."
pub fn penalty_sentence(call: &PenaltyCall, enf: &Enforcement) -> String {
    let team = match call.against {
        FoulTeam::Offense => "offense",
        FoulTeam::Defense => "defense",
    };
    let mut s = format!("{} on the {}, {} yards", call.kind.label(), team, enf.yards.round() as i32);
    if enf.half_the_distance {
        s.push_str(" (half the distance)");
    }
    if enf.auto_first_down {
        s.push_str(", automatic first down");
    } else if enf.loss_of_down {
        s.push_str(", loss of down");
    } else if enf.replay_down {
        s.push_str(", down repeats");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holding_walks_ten_and_repeats_the_down() {
        let call = PenaltyCall::new(PenaltyKind::Holding, 40.0);
        let e = enforce(&call, 40.0, 1, 44.0, 3, 6.0);
        assert!((e.spot - 30.0).abs() < 1e-3, "spot was {}", e.spot);
        assert_eq!(e.down, 3);
        assert!(e.replay_down);
        assert!((e.to_go - 16.0).abs() < 1e-3);
    }

    #[test]
    fn defensive_offside_shrinks_the_distance_to_gain() {
        let call = PenaltyCall::new(PenaltyKind::OffsideDefense, 40.0);
        let e = enforce(&call, 40.0, 1, 45.0, 3, 10.0);
        assert!((e.spot - 45.0).abs() < 1e-3);
        assert_eq!(e.down, 3, "no automatic first on offside");
        assert!((e.to_go - 5.0).abs() < 1e-3);
    }

    #[test]
    fn defensive_pass_interference_is_a_spot_foul_with_auto_first() {
        let call = PenaltyCall::new(PenaltyKind::DefensivePassInterference, 88.0);
        let e = enforce(&call, 60.0, 1, 60.0, 1, 10.0);
        assert!((e.spot - 88.0).abs() < 1e-3);
        assert_eq!(e.down, 1);
        assert!(e.auto_first_down);
        assert!((e.to_go - 10.0).abs() < 1e-3);
    }

    #[test]
    fn dpi_in_the_endzone_goes_to_the_one() {
        let call = PenaltyCall::new(PenaltyKind::DefensivePassInterference, 104.0);
        let e = enforce(&call, 95.0, 1, 95.0, 2, 5.0);
        assert!((field::yards_to_goal(e.spot, 1) - 1.0).abs() < 1e-3);
        assert_eq!(e.down, 1);
    }

    #[test]
    fn half_the_distance_is_capped_near_the_goal() {
        let call = PenaltyCall::new(PenaltyKind::Holding, 4.0);
        let e = enforce(&call, 4.0, 1, 6.0, 3, 8.0);
        assert!(e.half_the_distance);
        assert!((e.spot - 2.0).abs() < 1e-3, "spot was {}", e.spot);
        assert!(e.spot > 0.0);
    }

    #[test]
    fn intentional_grounding_loses_a_down() {
        let call = PenaltyCall::new(PenaltyKind::IntentionalGrounding, 30.0);
        let e = enforce(&call, 30.0, -1, 28.0, 2, 9.0);
        assert_eq!(e.down, 3);
        assert!(e.loss_of_down);
        assert!(!e.replay_down);
    }

    #[test]
    fn enforcement_can_still_move_the_sticks() {
        // 4th and 1 from the 30: a defensive offside walks the ball past the line
        // to gain, so the offense is awarded a new series instead of repeating 4th.
        let call = PenaltyCall::new(PenaltyKind::OffsideDefense, 30.0);
        let e = enforce(&call, 30.0, 1, 31.0, 4, 1.0);
        assert_eq!(e.down, 1);
        assert!((e.to_go - 10.0).abs() < 1e-3);
    }

    #[test]
    fn basic_spot_walks_from_the_foul_when_downfield() {
        // Holding at the 45 with the ball snapped at the 35: 10 yards back from the
        // foul spot, not from the snap.
        let call = PenaltyCall::new(PenaltyKind::Holding, 45.0);
        let e = enforce(&call, 35.0, 1, 50.0, 2, 10.0);
        assert!((e.spot - 35.0).abs() < 1e-3, "spot was {}", e.spot);
        assert_eq!(e.down, 2);
    }

    #[test]
    fn basic_spot_never_goes_behind_the_line_of_scrimmage() {
        let call = PenaltyCall::new(PenaltyKind::Holding, 30.0);
        let e = enforce(&call, 35.0, 1, 40.0, 2, 10.0);
        assert!((e.spot - 25.0).abs() < 1e-3, "spot was {}", e.spot);
    }

    #[test]
    fn penalty_metadata_matches_the_rulebook_table() {
        assert_eq!(PenaltyKind::Holding.yards(), 10.0);
        assert_eq!(PenaltyKind::OffensivePassInterference.yards(), 15.0);
        assert!(PenaltyKind::FalseStart.dead_ball());
        assert!(PenaltyKind::DefensiveHolding.auto_first_down());
        assert!(PenaltyKind::DefensivePassInterference.auto_first_down());
        assert!(!PenaltyKind::Holding.auto_first_down());
        assert_eq!(PenaltyKind::Targeting.against(), FoulTeam::Defense);
        assert_eq!(PenaltyKind::BlindsideBlock.against(), FoulTeam::Offense);
    }
}

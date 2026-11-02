//! Head-coach AI: play calling, fourth-down decisions, two-point decisions and
//! clock management.

use std::sync::OnceLock;

use crate::difficulty::Difficulty;
use crate::math::Rng;
use crate::playbook::{defensive_playbook, playbook, DefCall, Play, PlayFamily};
use crate::sim::PlayOutcome;

use crate::game::{GamePhase, GameState};

static OFFENSE_BOOK: OnceLock<Vec<Play>> = OnceLock::new();
static DEFENSE_BOOK: OnceLock<Vec<DefCall>> = OnceLock::new();

pub fn offense_book() -> &'static [Play] {
    OFFENSE_BOOK.get_or_init(playbook)
}

pub fn defense_book() -> &'static [DefCall] {
    DEFENSE_BOOK.get_or_init(defensive_playbook)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FourthDownChoice {
    GoForIt,
    ThrowOff,
}

/// Down, distance and situation, summarised for the play caller.
#[derive(Clone, Copy, Debug)]
pub struct Situation {
    pub down: u8,
    pub to_go: f32,
    pub yards_to_goal: f32,
    pub quarter: u8,
    pub clock: f32,
    pub score_diff: i32,
    pub is_try: bool,
    pub overtime: bool,
}

impl Situation {
    pub fn from_game(g: &GameState) -> Self {
        Situation {
            down: g.down,
            to_go: g.to_go,
            yards_to_goal: g.yards_to_goal(),
            quarter: g.quarter,
            clock: g.clock,
            score_diff: g.score[g.offense()] as i32 - g.score[g.defense()] as i32,
            is_try: matches!(g.phase, GamePhase::Conversion { .. }),
            overtime: g.is_overtime(),
        }
    }

    pub fn late_and_close(&self) -> bool {
        (self.quarter == 2 || self.quarter == 4 || self.overtime) && self.clock < 120.0
    }

    pub fn desperate(&self) -> bool {
        self.late_and_close() && self.score_diff < 0
    }
}

/// Weighted pick of a play matching a family, falling back to anything similar.
fn pick_family(rng: &mut Rng, families: &[PlayFamily]) -> &'static Play {
    let book = offense_book();
    let candidates: Vec<&'static Play> = book
        .iter()
        .filter(|p| families.contains(&p.family))
        .collect();
    if candidates.is_empty() {
        return &book[rng.index(book.len())];
    }
    // Slight preference for plays with routes matching the situation is handled by
    // the caller passing narrower families.
    candidates[rng.index(candidates.len())]
}

pub fn choose_offense(g: &GameState, seed: u64) -> Play {
    let mut rng = Rng::new(seed ^ 0x0FF5_EE11);
    let s = Situation::from_game(g);

    // Conversions: pick a concept that fits the distance.
    if s.is_try {
        let families: &[PlayFamily] = if s.yards_to_goal <= 6.0 {
            &[PlayFamily::QuickPass, PlayFamily::RunInside, PlayFamily::RunOutside]
        } else {
            &[PlayFamily::QuickPass, PlayFamily::DropbackPass]
        };
        return pick_family(&mut rng, families).clone();
    }

    // Short-yardage and goal line.
    if s.to_go <= 2.0 && (s.down >= 3 || s.yards_to_goal < 4.0) {
        if rng.chance(0.62) {
            return pick_family(&mut rng, &[PlayFamily::RunInside, PlayFamily::QbRun]).clone();
        }
        return pick_family(&mut rng, &[PlayFamily::QuickPass, PlayFamily::PlayAction]).clone();
    }

    if s.down == 4 {
        // Call something with a high completion rate.
        return pick_family(
            &mut rng,
            &[PlayFamily::QuickPass, PlayFamily::DropbackPass, PlayFamily::Screen],
        )
        .clone();
    }

    if s.down == 3 {
        if s.to_go > 8.0 {
            return pick_family(
                &mut rng,
                &[PlayFamily::DropbackPass, PlayFamily::Screen, PlayFamily::QuickPass],
            )
            .clone();
        }
        return pick_family(
            &mut rng,
            &[PlayFamily::QuickPass, PlayFamily::DropbackPass, PlayFamily::RunOutside],
        )
        .clone();
    }

    // Clock-killing mode: run the ball, stay in bounds.
    if s.late_and_close() && s.score_diff > 0 {
        if rng.chance(0.7) {
            return pick_family(&mut rng, &[PlayFamily::RunInside, PlayFamily::RunOutside]).clone();
        }
        return pick_family(&mut rng, &[PlayFamily::QuickPass]).clone();
    }

    // Two-minute offense: get out of bounds, throw it downfield.
    if s.desperate() {
        return pick_family(
            &mut rng,
            &[PlayFamily::DropbackPass, PlayFamily::PlayAction, PlayFamily::QuickPass],
        )
        .clone();
    }

    // A normal or hard coordinator is chasing the end zone, not just the sticks:
    // it throws on early downs rather than grinding the ball for four yards.
    let push = g.difficulty.aggression();
    if push > 0.0 && s.down <= 2 && s.to_go >= 5.0 && rng.chance(0.30 + 0.30 * push) {
        return pick_family(
            &mut rng,
            &[PlayFamily::DropbackPass, PlayFamily::PlayAction, PlayFamily::QuickPass],
        )
        .clone();
    }

    // Normal down and distance: establish the run on early downs.
    if s.down <= 2 && s.to_go <= 6.0 && rng.chance(0.55) {
        return pick_family(
            &mut rng,
            &[PlayFamily::RunInside, PlayFamily::RunOutside, PlayFamily::QbRun],
        )
        .clone();
    }
    if s.to_go >= 8.0 {
        return pick_family(
            &mut rng,
            &[
                PlayFamily::DropbackPass,
                PlayFamily::PlayAction,
                PlayFamily::QuickPass,
                PlayFamily::Screen,
            ],
        )
        .clone();
    }
    pick_family(
        &mut rng,
        &[
            PlayFamily::RunInside,
            PlayFamily::RunOutside,
            PlayFamily::QuickPass,
            PlayFamily::PlayAction,
            PlayFamily::Screen,
            PlayFamily::QbRun,
        ],
    )
    .clone()
}

pub fn choose_defense(g: &GameState, seed: u64) -> DefCall {
    let mut rng = Rng::new(seed ^ 0xDEFE_5511);
    let s = Situation::from_game(g);
    let book = defense_book();
    let by_name = |name: &str| -> DefCall {
        book.iter().find(|d| d.name == name).cloned().unwrap_or_else(|| book[0].clone())
    };

    // Goal line: crowd the box.
    if s.yards_to_goal <= 5.0 {
        return if rng.chance(0.55) { by_name("GOAL LINE BEAR") } else { by_name("COVER 1 MAN") };
    }
    // Short yardage: bring pressure.
    if s.to_go <= 2.5 && s.down >= 3 {
        return if rng.chance(0.5) { by_name("ALL OUT BLITZ") } else { by_name("MIKE BLITZ") };
    }
    // Long yardage / two-minute: keep everything in front.
    if s.to_go >= 9.0 || (s.late_and_close() && s.score_diff > 0) {
        return if rng.chance(0.5) { by_name("COVER 3 ZONE") } else { by_name("COVER 4 ZONE") };
    }
    // A hard side hunts the ball: it presses and plays man rather than keeping
    // everything in front and waiting for the throw to be incomplete.
    if g.difficulty == Difficulty::Hard {
        if s.down == 3 {
            return if rng.chance(0.60) { by_name("PRESS BAIL") } else { by_name("COVER 1 MAN") };
        }
        if s.to_go < 12.0 && rng.chance(0.35) {
            return by_name("COVER 1 MAN");
        }
    }
    // Third down: mix.
    if s.down == 3 {
        return if rng.chance(0.45) {
            by_name("COVER 2 ZONE")
        } else if rng.chance(0.5) {
            by_name("PRESS BAIL")
        } else {
            by_name("WILL BLITZ")
        };
    }
    if rng.chance(0.35) {
        by_name("COVER 1 MAN")
    } else if rng.chance(0.5) {
        by_name("COVER 3 ZONE")
    } else {
        by_name("COVER 2 ZONE")
    }
}

/// Should the offense throw off on fourth down instead of snapping it?
pub fn choose_fourth_down(g: &GameState, seed: u64) -> FourthDownChoice {
    let s = Situation::from_game(g);
    // Going for it is attractive inside the opponent's 45 or with very little to go.
    let go_chance = if s.yards_to_goal <= 45.0 && s.to_go <= 4.0 {
        0.85
    } else if s.yards_to_goal <= 50.0 && s.to_go <= 2.0 {
        0.7
    } else if s.yards_to_goal <= 35.0 && s.to_go <= 8.0 {
        0.55
    } else if s.desperate() {
        0.85
    } else {
        0.12
    };
    let mut rng = Rng::new(seed ^ 0x4D_0000);
    // A hard side backs itself on fourth down.
    let go_chance = if g.difficulty == Difficulty::Hard { go_chance + 0.08 } else { go_chance };
    if rng.chance(go_chance) {
        FourthDownChoice::GoForIt
    } else {
        FourthDownChoice::ThrowOff
    }
}

/// Two-point decision. The rulebook lets a team choose 1-from-the-5 or 2-from-the-10
/// once, and the choice is locked in for that try.
pub fn choose_two_point(g: &GameState, seed: u64) -> bool {
    let s = Situation::from_game(g);
    let mut rng = Rng::new(seed ^ 0x2B_0000);
    if s.overtime && s.quarter >= 7 {
        // After the second overtime period, everyone is forced to go for two.
        return true;
    }
    if s.late_and_close() {
        let diff = s.score_diff;
        if (1..=2).contains(&-diff) || diff == 2 {
            return true;
        }
        if diff <= -5 && s.clock < 300.0 {
            return true;
        }
    }
    // Aggressive programs go for two more often.
    rng.chance(0.18)
}

/// Timeout policy: burn them when trailing late and the clock is moving.
pub fn wants_timeout(g: &GameState) -> bool {
    let side = g.defense();
    if g.timeouts[side] == 0 {
        return false;
    }
    if g.no_timeouts_for_leader && g.score[side] > g.score[g.offense()] {
        return false;
    }
    let diff = g.score[side] as i32 - g.score[g.offense()] as i32;
    (g.quarter == 2 || g.quarter == 4) && g.clock < 100.0 && diff < 0 && !g.clock_stopped
}

/// How a broadcaster would describe the down and distance.
pub fn down_and_distance(g: &GameState) -> String {
    let ord = match g.down {
        1 => "1st",
        2 => "2nd",
        3 => "3rd",
        _ => "4th",
    };
    let ytg = g.offense_yards_to_goal();
    if ytg <= g.to_go {
        format!("{ord} & GOAL")
    } else if (g.to_go - 10.0).abs() < 0.6 {
        format!("{ord} & 10")
    } else {
        format!("{ord} & {}", g.to_go.round() as i32)
    }
}

/// Human-readable read of the defensive call for the pre-snap overlay.
pub fn defense_read(def: &DefCall) -> String {
    format!(
        "{} · {} · {} rush",
        def.front.label(),
        def.coverage.label(),
        def.rushers()
    )
}

/// Convenience for the broadcast ticker: what just happened.
pub fn outcome_verb(outcome: PlayOutcome) -> &'static str {
    match outcome {
        PlayOutcome::Run => "RUN",
        PlayOutcome::QbScramble => "SCRAMBLE",
        PlayOutcome::PassComplete => "COMPLETE",
        PlayOutcome::PassIncomplete => "INCOMPLETE",
        PlayOutcome::Sack => "SACK",
        PlayOutcome::Interception => "INTERCEPTION",
        PlayOutcome::FumbleLost => "FUMBLE",
        PlayOutcome::FumbleRecovered => "FUMBLE",
        PlayOutcome::Touchdown => "TOUCHDOWN",
        PlayOutcome::Safety => "SAFETY",
        PlayOutcome::Penalty => "PENALTY",
        PlayOutcome::ThrowAway => "THROWN AWAY",
    }
}

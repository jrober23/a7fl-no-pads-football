//! Head-coach AI: play calling, fourth-down decisions, two-point decisions and
//! clock management.

use std::sync::OnceLock;

use crate::difficulty::Difficulty;
use crate::math::Rng;
use crate::playbook::{
    defensive_playbook, defensive_playbook_for, playbook, playbook_for, Book, DefCall, Play,
    PlayFamily,
};
use crate::sim::PlayOutcome;

use crate::game::{GamePhase, GameState};

static OFFENSE_BOOK: OnceLock<Vec<Play>> = OnceLock::new();
static DEFENSE_BOOK: OnceLock<Vec<DefCall>> = OnceLock::new();
/// Every book, laid out once: a coordinator asking for its club's book on every
/// snap should not be building it every snap.
static OFF_BOOKS: OnceLock<Vec<Vec<Play>>> = OnceLock::new();
static DEF_BOOKS: OnceLock<Vec<Vec<DefCall>>> = OnceLock::new();

pub fn offense_book() -> &'static [Play] {
    OFFENSE_BOOK.get_or_init(playbook)
}

pub fn defense_book() -> &'static [DefCall] {
    DEFENSE_BOOK.get_or_init(defensive_playbook)
}

/// The offense a club runs: its own book, not the league's whole catalog.
pub fn offense_book_for(book: Book) -> &'static [Play] {
    if let Book::Custom(slot) = book {
        return crate::design::cached_offense(slot as usize);
    }
    let all = OFF_BOOKS.get_or_init(|| Book::ALL.iter().map(|b| playbook_for(*b)).collect());
    &all[book.index()]
}

/// The defense a club plays.
pub fn defense_book_for(book: Book) -> &'static [DefCall] {
    if let Book::Custom(slot) = book {
        return crate::design::cached_defense(slot as usize);
    }
    let all =
        DEF_BOOKS.get_or_init(|| Book::ALL.iter().map(|b| defensive_playbook_for(*b)).collect());
    &all[book.index()]
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
fn pick_family(rng: &mut Rng, book: &'static [Play], families: &[PlayFamily]) -> &'static Play {
    let candidates: Vec<&'static Play> = book
        .iter()
        .filter(|p| families.contains(&p.family))
        .collect();
    if candidates.is_empty() {
        // The club's book does not carry that concept -- a power side has no
        // screen, an air raid side no boot. Keep the intent rather than the
        // concept: a down that called for a throw gets a throw, a down that
        // called for a run gets a run. Falling through to any play at all turned
        // every one of those downs into a coin toss, and a game of coin tosses
        // is a game with no clock left in it.
        let want_pass = families.iter().any(|f| f.is_pass());
        let near: Vec<&'static Play> = book
            .iter()
            .filter(|p| p.family.is_pass() == want_pass)
            .collect();
        if !near.is_empty() {
            return near[rng.index(near.len())];
        }
        return &book[rng.index(book.len())];
    }
    // Slight preference for plays with routes matching the situation is handled by
    // the caller passing narrower families.
    candidates[rng.index(candidates.len())]
}

pub fn choose_offense(g: &GameState, seed: u64) -> Play {
    let mut rng = Rng::new(seed ^ 0x0FF5_EE11);
    let s = Situation::from_game(g);
    // The book belongs to the club with the ball: a power side calls power and a
    // spread side calls the option on the same down and distance.
    let book = offense_book_for(g.teams[g.offense()].book);

    // Conversions: pick a concept that fits the distance.
    if s.is_try {
        let families: &[PlayFamily] = if s.yards_to_goal <= 6.0 {
            &[PlayFamily::QuickPass, PlayFamily::RunInside, PlayFamily::RunOutside]
        } else {
            &[PlayFamily::QuickPass, PlayFamily::DropbackPass]
        };
        return pick_family(&mut rng, book, families).clone();
    }

    // Short-yardage and goal line.
    if s.to_go <= 2.0 && (s.down >= 3 || s.yards_to_goal < 4.0) {
        if rng.chance(0.62) {
            return pick_family(&mut rng, book, &[PlayFamily::RunInside, PlayFamily::QbRun]).clone();
        }
        return pick_family(&mut rng, book, &[PlayFamily::QuickPass, PlayFamily::PlayAction]).clone();
    }

    if s.down == 4 {
        // Call something with a high completion rate.
        return pick_family(&mut rng, book, &[PlayFamily::QuickPass, PlayFamily::DropbackPass, PlayFamily::Screen],
        )
        .clone();
    }

    if s.down == 3 {
        if s.to_go > 8.0 {
            return pick_family(&mut rng, book, &[PlayFamily::DropbackPass, PlayFamily::Screen, PlayFamily::QuickPass],
            )
            .clone();
        }
        return pick_family(&mut rng, book, &[PlayFamily::QuickPass, PlayFamily::DropbackPass, PlayFamily::RunOutside],
        )
        .clone();
    }

    // Clock-killing mode: run the ball, stay in bounds.
    if s.late_and_close() && s.score_diff > 0 {
        if rng.chance(0.7) {
            return pick_family(&mut rng, book, &[PlayFamily::RunInside, PlayFamily::RunOutside]).clone();
        }
        return pick_family(&mut rng, book, &[PlayFamily::QuickPass]).clone();
    }

    // Two-minute offense: get out of bounds, throw it downfield.
    if s.desperate() {
        return pick_family(&mut rng, book, &[PlayFamily::DropbackPass, PlayFamily::PlayAction, PlayFamily::QuickPass],
        )
        .clone();
    }

    // A normal or hard coordinator is chasing the end zone, not just the sticks:
    // it throws on early downs rather than grinding the ball for four yards.
    let push = g.difficulty.aggression();
    if push > 0.0 && s.down <= 2 && s.to_go >= 5.0 && rng.chance(0.30 + 0.30 * push) {
        return pick_family(&mut rng, book, &[PlayFamily::DropbackPass, PlayFamily::PlayAction, PlayFamily::QuickPass],
        )
        .clone();
    }

    // Normal down and distance: establish the run on early downs.
    if s.down <= 2 && s.to_go <= 6.0 && rng.chance(0.55) {
        return pick_family(&mut rng, book, &[PlayFamily::RunInside, PlayFamily::RunOutside, PlayFamily::QbRun],
        )
        .clone();
    }
    if s.to_go >= 8.0 {
        return pick_family(&mut rng, book, &[
                PlayFamily::DropbackPass,
                PlayFamily::PlayAction,
                PlayFamily::QuickPass,
                PlayFamily::Screen,
            ],
        )
        .clone();
    }
    pick_family(&mut rng, book, &[
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
    let book = defense_book_for(g.teams[g.defense()].book);
    // A call this club's book does not carry is replaced by the nearest thing it
    // does: the same coverage off a different front, then the front itself, then
    // whatever it opens with. A book is a book, not a script with holes in it.
    let by_name = |name: &str| -> DefCall {
        if let Some(d) = book.iter().find(|d| d.name == name) {
            return d.clone();
        }
        if let Some(want) = defensive_playbook().into_iter().find(|d| d.name == name) {
            if let Some(d) = book.iter().find(|d| d.coverage == want.coverage) {
                return d.clone();
            }
            if let Some(d) = book.iter().find(|d| d.front == want.front) {
                return d.clone();
            }
        }
        book[0].clone()
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
///
/// Deep in its own half a computer side takes the throw-off almost every time:
/// there is nothing to gain by snapping it on its own 25. Easy and Normal do not
/// gamble there at all. Hard reads the scoreboard instead -- it will snap it from
/// its own side when it has to have the ball, and otherwise takes its chances on
/// down and distance.
pub fn choose_fourth_down(g: &GameState, seed: u64) -> FourthDownChoice {
    let s = Situation::from_game(g);
    // Still on its own side of the field: fifty yards or more from the goal.
    let backed_up = s.yards_to_goal > 50.0;
    if backed_up && g.difficulty != Difficulty::Hard {
        return FourthDownChoice::ThrowOff;
    }
    // Going for it is attractive inside the opponent's 45 or with very little to go.
    let go_chance = if s.desperate() {
        // Behind late: the throw-off hands the ball straight back, so a side that
        // needs a score snaps it, backed up or not.
        0.85
    } else if s.yards_to_goal <= 45.0 && s.to_go <= 4.0 {
        0.85
    } else if s.yards_to_goal <= 50.0 && s.to_go <= 2.0 {
        0.7
    } else if s.yards_to_goal <= 35.0 && s.to_go <= 8.0 {
        0.55
    } else {
        0.12
    };
    let mut rng = Rng::new(seed ^ 0x4D_0000);
    // A hard side backs itself on fourth down, including from its own half.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::team::league_teams;

    fn fourth_down_on_our_own_28(difficulty: Difficulty) -> GameState {
        let t = league_teams();
        let mut g = GameState::new(t[0].clone(), t[1].clone(), 77);
        g.set_difficulty(difficulty);
        g.spot = 28.0;
        g.direction = 1;
        g.down = 4;
        g.to_go = 12.0;
        g
    }

    /// Backed up on its own side, a computer side takes the throw-off: there is
    /// nothing to snap a fourth and twelve for from its own 28.
    #[test]
    fn a_backed_up_computer_throws_it_off_on_easy_and_normal() {
        for difficulty in [Difficulty::Easy, Difficulty::Normal] {
            let g = fourth_down_on_our_own_28(difficulty);
            for seed in 0..64u64 {
                assert_eq!(
                    choose_fourth_down(&g, seed),
                    FourthDownChoice::ThrowOff,
                    "{difficulty:?} went for it from its own 28 on seed {seed}"
                );
            }
        }
    }

    /// Hard backs itself sometimes from there, but not blindly.
    #[test]
    fn a_hard_side_mixes_going_for_it_from_its_own_end() {
        let g = fourth_down_on_our_own_28(Difficulty::Hard);
        let went = (0..256u64)
            .filter(|&seed| choose_fourth_down(&g, seed) == FourthDownChoice::GoForIt)
            .count();
        assert!(went > 0, "a hard side never backed itself from its own 28");
        assert!(
            went < 128,
            "a hard side went for it from its own 28 {went} times in 256"
        );
    }

    /// Behind late, a hard side snaps it even backed up: the throw-off hands the
    /// ball straight back, and it needs the ball.
    #[test]
    fn a_desperate_hard_side_goes_for_it_from_its_own_end() {
        let mut g = fourth_down_on_our_own_28(Difficulty::Hard);
        g.quarter = 4;
        g.clock = 45.0;
        g.score = [0, 7];
        g.possession = 0;
        let went = (0..256u64)
            .filter(|&seed| choose_fourth_down(&g, seed) == FourthDownChoice::GoForIt)
            .count();
        assert!(
            went > 150,
            "a desperate side only went for it {went} times in 256"
        );
    }
}

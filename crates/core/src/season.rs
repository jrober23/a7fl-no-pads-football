//! A season: a schedule, a table, and a bracket.
//!
//! Everything is deterministic from one seed, so a season can be put down and
//! picked up again, and the same seed always plays out the same way. The league
//! is thirty-two clubs in four divisions; a season is some number of weeks of
//! round-robin play, and then the top two in each division go into an eight-club
//! bracket that ends at the championship.

use crate::game::GameState;
use crate::math::Rng;
use crate::playbook::Book;
use crate::team::{clubs, team_from_spec, Division};

/// The longest season the round-robin can give: every club meets every other.
pub const MAX_WEEKS: usize = 31;

/// How many clubs reach the bracket: the top two in each of the four divisions.
pub const PLAYOFF_TEAMS: usize = 8;

/// One club's season to date.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub wins: u16,
    pub losses: u16,
    pub ties: u16,
    pub scored: u32,
    pub allowed: u32,
}

impl Record {
    pub fn played(&self) -> u16 {
        self.wins + self.losses + self.ties
    }

    /// Two for a win, one for a tie: the order the table is sorted in.
    pub fn points(&self) -> i32 {
        self.wins as i32 * 2 + self.ties as i32
    }

    pub fn diff(&self) -> i32 {
        self.scored as i32 - self.allowed as i32
    }

    /// `9-2`, or `9-1-1` when there was a tie in it.
    pub fn line(&self) -> String {
        if self.ties > 0 {
            format!("{}-{}-{}", self.wins, self.losses, self.ties)
        } else {
            format!("{}-{}", self.wins, self.losses)
        }
    }
}

/// One game on the schedule.
#[derive(Clone, Copy, Debug)]
pub struct Fixture {
    pub home: usize,
    pub away: usize,
    pub score: Option<(u32, u32)>,
}

impl Fixture {
    pub fn played(&self) -> bool {
        self.score.is_some()
    }

    pub fn winner(&self) -> Option<usize> {
        let (h, a) = self.score?;
        if h > a {
            Some(self.home)
        } else if a > h {
            Some(self.away)
        } else {
            None
        }
    }

    pub fn line(&self) -> String {
        match self.score {
            Some((h, a)) => format!("{h}-{a}"),
            None => "-".to_string(),
        }
    }
}

/// One round of the bracket.
#[derive(Clone, Debug)]
pub struct Round {
    pub name: &'static str,
    pub games: Vec<Fixture>,
}

/// Where the season is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Regular,
    Playoffs(usize),
    Done,
}

pub struct Season {
    pub seed: u64,
    /// The club the player runs.
    pub you: usize,
    /// The schedule, one entry per week.
    pub weeks: Vec<Vec<Fixture>>,
    /// The next week to play, counted from zero.
    pub week: usize,
    /// The bracket, empty until the regular season is done.
    pub rounds: Vec<Round>,
    pub champion: Option<usize>,
    pub phase: Phase,
    /// The book the player runs in his own games: his club's own unless he picked
    /// another one when the season was set up.
    pub book: Book,
}

impl Season {
    /// A season of `weeks` weeks for club `you`.
    ///
    /// The schedule is the circle method: club 0 stays put and the rest rotate
    /// one place a week, so every club plays every week and no two clubs meet
    /// twice until the whole league has been round. The order is shuffled first,
    /// so two seasons with different seeds are not the same fixture list.
    pub fn new(you: usize, weeks: usize, seed: u64) -> Season {
        let n = clubs().len().max(2);
        let mut order: Vec<usize> = (0..n).collect();
        let mut rng = Rng::new(seed ^ 0x5EA5_0F00);
        for i in (1..n).rev() {
            let j = rng.index(i + 1);
            order.swap(i, j);
        }
        let mut schedule = Vec::new();
        for r in 0..n - 1 {
            let mut games = Vec::new();
            for i in 0..n / 2 {
                let (a, b) = (order[i], order[n - 1 - i]);
                let (home, away) = if (r + i) % 2 == 0 { (a, b) } else { (b, a) };
                games.push(Fixture { home, away, score: None });
            }
            schedule.push(games);
            let last = order.pop().expect("a club to rotate");
            order.insert(1, last);
        }
        schedule.truncate(weeks.clamp(1, MAX_WEEKS));
        let you = you.min(n - 1);
        Season {
            seed,
            you,
            weeks: schedule,
            week: 0,
            rounds: Vec::new(),
            champion: None,
            phase: Phase::Regular,
            book: clubs()[you].book,
        }
    }

    /// How many weeks this season runs.
    pub fn length(&self) -> usize {
        self.weeks.len()
    }

    /// The player's game in the current week or round, if he has one.
    pub fn your_fixture(&self) -> Option<(usize, Fixture)> {
        match self.phase {
            Phase::Regular => {
                let week = self.weeks.get(self.week)?;
                week.iter()
                    .position(|f| f.home == self.you || f.away == self.you)
                    .map(|i| (i, week[i]))
            }
            Phase::Playoffs(r) => {
                let round = self.rounds.get(r)?;
                round
                    .games
                    .iter()
                    .position(|f| f.home == self.you || f.away == self.you)
                    .map(|i| (i, round.games[i]))
            }
            Phase::Done => None,
        }
    }

    /// Record the score of the player's game and finish everything else that is
    /// due this week (or this round).
    pub fn record(&mut self, score: (u32, u32)) {
        if let Some((index, _)) = self.your_fixture() {
            match self.phase {
                Phase::Regular => {
                    if let Some(week) = self.weeks.get_mut(self.week) {
                        week[index].score = Some(score);
                    }
                }
                Phase::Playoffs(r) => {
                    if let Some(round) = self.rounds.get_mut(r) {
                        round.games[index].score = Some(score);
                    }
                }
                Phase::Done => {}
            }
        }
        self.sim_rest();
    }

    /// Play out everything still unplayed this week or round, the player's game
    /// included, and move on.
    pub fn sim_rest(&mut self) {
        match self.phase {
            Phase::Regular => {
                let week = self.week;
                let len = self.weeks.get(week).map(|w| w.len()).unwrap_or(0);
                for i in 0..len {
                    let salt = self.seed ^ (week as u64).wrapping_mul(0x9E37_79B9) ^ i as u64;
                    let mut fixture = self.weeks[week][i];
                    sim_fixture(&mut fixture, salt);
                    self.weeks[week][i] = fixture;
                }
                self.week += 1;
            }
            Phase::Playoffs(r) => {
                let len = self.rounds.get(r).map(|x| x.games.len()).unwrap_or(0);
                for i in 0..len {
                    let salt = self.seed ^ 0xBEEF ^ (r as u64).wrapping_mul(0x85EB_CA6B) ^ i as u64;
                    let mut fixture = self.rounds[r].games[i];
                    sim_fixture(&mut fixture, salt);
                    self.rounds[r].games[i] = fixture;
                }
                self.finish_round(r);
            }
            Phase::Done => {}
        }
    }

    /// The player's game only, so the rest of the week can be simulated before
    /// he decides what to do with his own.
    pub fn sim_other_games(&mut self) {
        match self.phase {
            Phase::Regular => {
                let week = self.week;
                let len = self.weeks.get(week).map(|w| w.len()).unwrap_or(0);
                for i in 0..len {
                    let fixture = self.weeks[week][i];
                    if fixture.played() || fixture.home == self.you || fixture.away == self.you {
                        continue;
                    }
                    let salt = self.seed ^ (week as u64).wrapping_mul(0x9E37_79B9) ^ i as u64;
                    let mut fixture = fixture;
                    sim_fixture(&mut fixture, salt);
                    self.weeks[week][i] = fixture;
                }
            }
            Phase::Playoffs(r) => {
                let len = self.rounds.get(r).map(|x| x.games.len()).unwrap_or(0);
                for i in 0..len {
                    let fixture = self.rounds[r].games[i];
                    if fixture.played() || fixture.home == self.you || fixture.away == self.you {
                        continue;
                    }
                    let salt = self.seed ^ 0xBEEF ^ (r as u64).wrapping_mul(0x85EB_CA6B) ^ i as u64;
                    let mut fixture = fixture;
                    sim_fixture(&mut fixture, salt);
                    self.rounds[r].games[i] = fixture;
                }
            }
            Phase::Done => {}
        }
    }

    /// Move on from a completed week when the player has recorded his own game.
    pub fn advance_week(&mut self) {
        if self.phase == Phase::Regular && self.week < self.weeks.len() {
            let done = self
                .weeks
                .get(self.week)
                .map(|w| w.iter().all(|f| f.played()))
                .unwrap_or(true);
            if done {
                self.week += 1;
            }
        }
    }

    /// True once every week has been played.
    pub fn regular_done(&self) -> bool {
        self.phase != Phase::Regular || self.week >= self.weeks.len()
    }

    /// One club's record across the regular season.
    pub fn record_of(&self, club: usize) -> Record {
        let mut r = Record::default();
        for week in &self.weeks {
            for f in week {
                if f.home != club && f.away != club {
                    continue;
                }
                let Some((h, a)) = f.score else { continue };
                let (mine, theirs) = if f.home == club { (h, a) } else { (a, h) };
                r.scored += mine;
                r.allowed += theirs;
                if mine > theirs {
                    r.wins += 1;
                } else if theirs > mine {
                    r.losses += 1;
                } else {
                    r.ties += 1;
                }
            }
        }
        r
    }

    /// A division's table, best first.
    pub fn table(&self, division: Division) -> Vec<(usize, Record)> {
        let all = clubs();
        let mut rows: Vec<(usize, Record)> = (0..all.len())
            .filter(|&i| all[i].division == division)
            .map(|i| (i, self.record_of(i)))
            .collect();
        rows.sort_by(|a, b| {
            b.1.points()
                .cmp(&a.1.points())
                .then(b.1.diff().cmp(&a.1.diff()))
                .then(b.1.scored.cmp(&a.1.scored))
                .then(a.0.cmp(&b.0))
        });
        rows
    }

    /// The eight clubs in the bracket: the top two of each division, seeded by
    /// record. One from each division is guaranteed a home game by construction;
    /// the seed order is what decides who meets whom.
    pub fn seeds(&self) -> Vec<usize> {
        let mut field: Vec<(usize, Record)> = Vec::new();
        for div in Division::ALL {
            field.extend(self.table(div).into_iter().take(2));
        }
        field.sort_by(|a, b| {
            b.1.points()
                .cmp(&a.1.points())
                .then(b.1.diff().cmp(&a.1.diff()))
                .then(b.1.scored.cmp(&a.1.scored))
                .then(a.0.cmp(&b.0))
        });
        field.into_iter().map(|(i, _)| i).collect()
    }

    /// Build the quarterfinals from the table.
    pub fn start_playoffs(&mut self) {
        if self.phase != Phase::Regular || !self.regular_done() {
            return;
        }
        let s = self.seeds();
        if s.len() < PLAYOFF_TEAMS {
            self.phase = Phase::Done;
            return;
        }
        let pair = |a: usize, b: usize| Fixture {
            home: s[a],
            away: s[b],
            score: None,
        };
        // 1 v 8, 4 v 5 on the top half; 2 v 7, 3 v 6 on the bottom.
        self.rounds = vec![Round {
            name: "QUARTERFINAL",
            games: vec![pair(0, 7), pair(3, 4), pair(1, 6), pair(2, 5)],
        }];
        self.phase = Phase::Playoffs(0);
    }

    /// The name of the round being played, for the header.
    pub fn round_name(&self) -> &'static str {
        match self.phase {
            Phase::Regular => "REGULAR SEASON",
            Phase::Playoffs(r) => self.rounds.get(r).map(|x| x.name).unwrap_or("PLAYOFFS"),
            Phase::Done => "SEASON OVER",
        }
    }

    /// Promote the winners of a finished round, and crown a champion when the
    /// final is in the books.
    fn finish_round(&mut self, r: usize) {
        let Some(round) = self.rounds.get(r) else { return };
        if round.games.iter().any(|g| !g.played()) {
            return;
        }
        // A bracket game has to send somebody through: the game can end tied
        // after overtime, and the home side is always the better seed here, so a
        // tie goes to the club that was already ahead of the other in the table.
        let winners: Vec<usize> = round
            .games
            .iter()
            .map(|g| g.winner().unwrap_or(g.home))
            .collect();
        if round.name == "CHAMPIONSHIP" || winners.len() == 1 {
            self.champion = winners.first().copied();
            self.phase = Phase::Done;
            return;
        }
        if winners.len() < 2 {
            self.phase = Phase::Done;
            return;
        }
        let name = if winners.len() == 2 {
            "CHAMPIONSHIP"
        } else {
            "SEMIFINAL"
        };
        let mut games = Vec::new();
        for pair in winners.chunks(2) {
            if pair.len() == 2 {
                games.push(Fixture {
                    home: pair[0],
                    away: pair[1],
                    score: None,
                });
            }
        }
        self.rounds.push(Round { name, games });
        self.phase = Phase::Playoffs(r + 1);
    }
}

/// Play one game out and take the score off the simulation.
fn sim_fixture(fixture: &mut Fixture, seed: u64) {
    if fixture.played() {
        return;
    }
    let all = clubs();
    let Some(home) = all.get(fixture.home) else { return };
    let Some(away) = all.get(fixture.away) else { return };
    let mut game = GameState::new(team_from_spec(home.clone()), team_from_spec(away.clone()), seed);
    game.simulate_game();
    fixture.score = Some((game.score[0], game.score[1]));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_season_opens_on_the_club_book_and_can_run_another() {
        // The book the player's own games are played from: his club's until he
        // picks another on the way in, and then that one every week of the year.
        let mut season = Season::new(3, 8, 12);
        assert_eq!(season.book, clubs()[3].book, "his club's book by default");
        season.book = Book::Custom(1);
        assert_eq!(season.book, Book::Custom(1), "and the one he picked after");
    }

    #[test]
    fn every_club_plays_once_a_week_and_never_itself() {
        let season = Season::new(0, 6, 11);
        assert_eq!(season.weeks.len(), 6);
        for week in &season.weeks {
            assert_eq!(week.len(), clubs().len() / 2, "a full slate");
            let mut seen = vec![false; clubs().len()];
            for f in week {
                assert_ne!(f.home, f.away, "a club played itself");
                assert!(!seen[f.home] && !seen[f.away], "a club played twice in a week");
                seen[f.home] = true;
                seen[f.away] = true;
            }
            assert!(seen.iter().all(|&s| s), "a club had the week off");
        }
        // No fixture repeated across the season.
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for week in &season.weeks {
            for f in week {
                let key = (f.home.min(f.away), f.home.max(f.away));
                assert!(!pairs.contains(&key), "the same two clubs met twice");
                pairs.push(key);
            }
        }
    }

    #[test]
    fn the_same_seed_plays_the_same_season() {
        let mut a = Season::new(3, 3, 99);
        let mut b = Season::new(3, 3, 99);
        a.sim_rest();
        b.sim_rest();
        for (x, y) in a.weeks.iter().flatten().zip(b.weeks.iter().flatten()) {
            assert_eq!(x.score, y.score);
        }
    }

    #[test]
    fn the_table_adds_up() {
        let mut season = Season::new(0, 4, 7);
        for _ in 0..4 {
            season.sim_rest();
        }
        assert!(season.regular_done());
        for i in 0..clubs().len() {
            let r = season.record_of(i);
            assert_eq!(r.played(), 4, "every club plays every week");
            assert_eq!(r.wins + r.losses + r.ties, r.played());
        }
    }

    #[test]
    fn the_top_two_of_each_division_make_the_field() {
        let mut season = Season::new(0, 3, 5);
        for _ in 0..3 {
            season.sim_rest();
        }
        let seeds = season.seeds();
        assert_eq!(seeds.len(), PLAYOFF_TEAMS);
        let all = clubs();
        for div in Division::ALL {
            let table = season.table(div);
            for (club, _) in table.iter().take(2) {
                assert!(seeds.contains(club), "a top-two club missed the field");
            }
            for (club, _) in table.iter().skip(2) {
                assert!(!seeds.contains(club), "a club outside the top two got in");
            }
        }
    }

    #[test]
    fn a_tied_playoff_game_still_sends_a_side_through() {
        let mut season = Season::new(0, 2, 33);
        for _ in 0..2 {
            season.sim_rest();
        }
        season.start_playoffs();
        for game in season.rounds[0].games.iter_mut() {
            game.score = Some((10, 10));
        }
        season.sim_rest();
        assert_eq!(season.rounds.len(), 2, "a tied quarterfinal built no semifinal");
        assert_eq!(season.rounds[1].games.len(), 2);
        assert_eq!(season.rounds[1].name, "SEMIFINAL");
    }

    #[test]
    fn a_bracket_always_produces_a_champion() {
        let mut season = Season::new(0, 2, 21);
        for _ in 0..2 {
            season.sim_rest();
        }
        season.start_playoffs();
        assert_eq!(season.rounds.len(), 1);
        assert_eq!(season.rounds[0].games.len(), 4);
        let mut rounds = 0;
        while season.phase != Phase::Done && rounds < 5 {
            season.sim_rest();
            rounds += 1;
        }
        assert_eq!(season.rounds.len(), 3, "quarterfinal, semifinal, final");
        assert!(season.champion.is_some(), "no champion was crowned");
    }
}

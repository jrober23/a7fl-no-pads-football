//! Box score and drive tracking.

use crate::sim::{PlayOutcome, PlayResult};
use crate::team::Team;

#[derive(Clone, Debug, Default)]
pub struct TeamStats {
    pub points: u32,
    pub first_downs: u32,
    pub plays: u32,
    pub total_yards: f32,
    pub pass_att: u32,
    pub pass_comp: u32,
    pub pass_yards: f32,
    pub pass_td: u32,
    pub interceptions_lost: u32,
    pub sacks_allowed: u32,
    pub sack_yards: f32,
    pub rush_att: u32,
    pub rush_yards: f32,
    pub rush_td: u32,
    pub fumbles_lost: u32,
    pub turnovers: u32,
    pub penalties: u32,
    pub penalty_yards: u32,
    pub third_down_att: u32,
    pub third_down_conv: u32,
    pub fourth_down_att: u32,
    pub fourth_down_conv: u32,
    pub time_of_possession: f32,
    pub points_by_quarter: [u32; 5],
    pub touchdowns: u32,
    pub conversions_made: u32,
    pub conversions_att: u32,
}

impl TeamStats {
    pub fn pass_rating_ish(&self) -> f32 {
        if self.pass_att == 0 {
            0.0
        } else {
            let comp = self.pass_comp as f32 / self.pass_att as f32;
            let ypa = self.pass_yards / self.pass_att as f32;
            (comp * 55.0 + ypa * 5.0 + self.pass_td as f32 * 2.0 - self.interceptions_lost as f32 * 3.0)
                .clamp(0.0, 120.0)
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PlayerStats {
    pub roster_index: usize,
    pub pass_att: u32,
    pub pass_comp: u32,
    pub pass_yards: f32,
    pub pass_td: u32,
    pub interceptions: u32,
    pub rush_att: u32,
    pub rush_yards: f32,
    pub rush_td: u32,
    pub receptions: u32,
    pub rec_yards: f32,
    pub rec_td: u32,
    pub tackles: u32,
    pub sacks: u32,
    pub picks: u32,
    pub pass_breakups: u32,
    pub penalties: u32,
    pub targets: u32,
}

#[derive(Clone, Debug)]
pub struct DriveSummary {
    pub offense: usize,
    pub start_spot: f32,
    pub start_yards_to_goal: f32,
    pub plays: u32,
    pub yards: f32,
    pub result: String,
    pub points: u32,
    pub time: f32,
}

#[derive(Clone, Debug)]
pub struct BoxScore {
    pub teams: [TeamStats; 2],
    pub players: [Vec<PlayerStats>; 2],
    pub drives: Vec<DriveSummary>,
    pub quarter_scores: [[u32; 5]; 2],
}

impl BoxScore {
    pub fn new(home: &Team, away: &Team) -> Self {
        let players = [
            home.roster.iter().map(|p| PlayerStats { roster_index: p.id as usize, ..Default::default() }).collect(),
            away.roster.iter().map(|p| PlayerStats { roster_index: p.id as usize, ..Default::default() }).collect(),
        ];
        BoxScore {
            teams: [TeamStats::default(), TeamStats::default()],
            players,
            drives: Vec::new(),
            quarter_scores: [[0; 5]; 2],
        }
    }

    fn player(&mut self, side: usize, roster: usize) -> Option<&mut PlayerStats> {
        self.players[side].iter_mut().find(|p| p.roster_index == roster)
    }

    /// Record a completed snap. `offense_side` is 0 (home) or 1 (away).
    pub fn record(&mut self, offense_side: usize, result: &PlayResult, first_down: bool, quarter: usize) {
        let defense_side = 1 - offense_side;
        let passer = result.passer;
        let receiver = result.receiver;
        let carrier = result.carrier;
        let tackler = result.tackler;
        let yards = result.yards;
        let touchdown = result.touchdown;
        let sack = result.sack;
        let interception = result.outcome == PlayOutcome::Interception;
        let fumble_lost = result.outcome == PlayOutcome::FumbleLost;
        let pass_attempt = result.outcome.is_pass_attempt();
        // A touchdown is a completion when a receiver is credited, and a run
        // otherwise (including quarterback scrambles).
        let completed = result.outcome == PlayOutcome::PassComplete
            || (touchdown && receiver.is_some());
        let running = matches!(
            result.outcome,
            PlayOutcome::Run | PlayOutcome::QbScramble | PlayOutcome::FumbleRecovered
        ) || (touchdown && receiver.is_none());
        let passing_td = touchdown && receiver.is_some();
        let rushing_td = touchdown && receiver.is_none() && carrier.is_some();

        // --- individual stats -------------------------------------------------
        if let Some(p) = passer {
            if let Some(ps) = self.player(offense_side, p) {
                if pass_attempt {
                    ps.pass_att += 1;
                }
                if completed {
                    ps.pass_comp += 1;
                    ps.pass_yards += yards;
                }
                if passing_td {
                    ps.pass_td += 1;
                }
                if interception {
                    ps.interceptions += 1;
                }
            }
        }
        if running {
            if let Some(c) = carrier {
                if let Some(ps) = self.player(offense_side, c) {
                    ps.rush_att += 1;
                    ps.rush_yards += yards;
                    if rushing_td {
                        ps.rush_td += 1;
                    }
                }
            }
        }
        if let Some(r) = receiver {
            if let Some(ps) = self.player(offense_side, r) {
                ps.targets += 1;
                if completed {
                    ps.receptions += 1;
                    ps.rec_yards += yards;
                    if passing_td {
                        ps.rec_td += 1;
                    }
                }
            }
        }
        if let Some(d) = tackler {
            if let Some(ps) = self.player(defense_side, d) {
                ps.tackles += 1;
                if sack {
                    ps.sacks += 1;
                }
                if interception {
                    ps.picks += 1;
                }
            }
        }
        if let Some(pen) = &result.penalty {
            if let Some(p) = pen.player {
                let side = match pen.against {
                    crate::rules::FoulTeam::Offense => offense_side,
                    crate::rules::FoulTeam::Defense => defense_side,
                };
                if let Some(ps) = self.player(side, p) {
                    ps.penalties += 1;
                }
            }
        }

        // --- team stats -------------------------------------------------------
        let t = &mut self.teams[offense_side];
        t.plays += 1;
        t.total_yards += yards;
        t.time_of_possession += result.elapsed;
        if pass_attempt {
            t.pass_att += 1;
            if completed {
                t.pass_comp += 1;
                t.pass_yards += yards;
                if passing_td {
                    t.pass_td += 1;
                }
            }
            if interception {
                t.interceptions_lost += 1;
            }
        }
        if sack {
            t.sacks_allowed += 1;
            t.sack_yards += yards;
        }
        if running {
            t.rush_att += 1;
            t.rush_yards += yards;
            if rushing_td {
                t.rush_td += 1;
            }
        }
        if result.turnover {
            if fumble_lost {
                t.fumbles_lost += 1;
            }
            t.turnovers += 1;
        }
        if touchdown {
            t.touchdowns += 1;
        }
        if first_down {
            self.teams[offense_side].first_downs += 1;
        }
        let _ = quarter;
    }

    pub fn add_points(&mut self, side: usize, points: u32, quarter: usize) {
        self.teams[side].points += points;
        let q = quarter.min(4);
        self.teams[side].points_by_quarter[q] += points;
        self.quarter_scores[side][q] += points;
    }

    pub fn add_penalty(&mut self, side: usize, yards: u32) {
        self.teams[side].penalties += 1;
        self.teams[side].penalty_yards += yards;
    }

    pub fn add_conversion(&mut self, side: usize, made: bool) {
        self.teams[side].conversions_att += 1;
        if made {
            self.teams[side].conversions_made += 1;
        }
    }

    pub fn player_stats(&self, side: usize, roster: usize) -> Option<&PlayerStats> {
        self.players[side].iter().find(|p| p.roster_index == roster)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playbook::{defensive_playbook, playbook};
    use crate::sim::{simulate_play, SnapSetup};
    use crate::team::league_teams;

    #[test]
    fn box_score_records_a_pass() {
        let teams = league_teams();
        let off = &teams[0];
        let def = &teams[1];
        let book = playbook();
        let d = defensive_playbook();
        let mut box_score = BoxScore::new(off, def);
        let mut recorded = 0;
        for seed in 0..50u64 {
            let play = &book[(seed as usize) % book.len()];
            let r = simulate_play(
                off,
                def,
                play,
                &d[seed as usize % d.len()],
                &SnapSetup {
                    los: 30.0,
                    ball_y: 0.0,
                    direction: 1,
                    down: 1,
                    to_go: 10.0,
                    seed,
                    offense_team: 0,
                    defense_team: 1,
                    is_try: false,
                },
            );
            box_score.record(0, &r, r.yards >= 10.0, 0);
            recorded += 1;
        }
        assert_eq!(recorded, 50);
        assert_eq!(box_score.teams[0].plays, 50);
        assert!(box_score.teams[0].total_yards.is_finite());
    }
}

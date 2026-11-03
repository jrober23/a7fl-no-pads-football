//! The game: quarters, clock, downs, throw-offs, conversions, overtime and the
//! 7 on 7 mercy rule.
//!
//! This state machine consumes snapshot results from [`crate::sim`] and
//! [`crate::throwoff`] and applies the rulebook. Every executed snap is kept in a
//! [`PlayRecord`] so the broadcast renderer can replay it frame-for-frame.

use crate::ai::{self, FourthDownChoice};
use crate::difficulty::Difficulty;
use crate::field::{self, FIELD_LENGTH};
use crate::math::{clamp, Rng};
use crate::playbook::{DefCall, Play};
use crate::rules::{
    enforce, penalty_sentence, CONVERSION_ONE_POINT_YARDS, CONVERSION_TWO_POINT_YARDS, DOWNS,
    MERCY_LEAD_TOUCHDOWNS, MERCY_LEAD_TOUCHDOWNS_EARLY, OVERTIME_FIRST_DOWNS_ALLOWED,
    OVERTIME_SPOT_YARD_LINE,
    PLAY_CLOCK_SECONDS, QUARTER_SECONDS, SAFETY_THROW_OFF_YARD_LINE, THROW_OFF_SPOT_YARD_LINE,
    TIME_BETWEEN_SNAPS, TIMEOUTS_PER_HALF, TIMEOUT_SECONDS, TWO_MINUTE_WARNING, YARDS_TO_GO,
};
use crate::sim::{simulate_play, PlayOutcome, PlayResult, SnapSetup};
use crate::stats::{BoxScore, DriveSummary};
use crate::team::Team;
use crate::throwoff::{simulate_throw_off, ThrowOffResult, ThrowOffSetup};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GamePhase {
    /// Waiting on a throw-off. `receiving` is the team catching it, `from_own` is
    /// the throwing team's distance from its own goal line.
    ThrowOff { receiving: usize, from_own: f32 },
    Scrimmage,
    ConversionChoice { offense: usize },
    Conversion { offense: usize, points: u32 },
    GameOver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ThrowOff,
    ChooseConversion,
    CallPlay { fourth_down: bool },
    GameOver,
}

#[derive(Clone, Debug)]
pub struct ScrimmageRecord {
    pub quarter: u8,
    pub clock: f32,
    pub down: u8,
    pub to_go: f32,
    pub setup: SnapSetup,
    pub play: Play,
    pub defense: DefCall,
    pub result: PlayResult,
    pub score_before: [u32; 2],
    pub score_after: [u32; 2],
    pub is_try: bool,
    pub penalty_yards: u32,
}

#[derive(Clone, Debug)]
pub struct ThrowOffRecord {
    pub quarter: u8,
    pub clock: f32,
    pub setup: ThrowOffSetup,
    pub result: ThrowOffResult,
    pub score_before: [u32; 2],
    pub score_after: [u32; 2],
    pub is_fourth_down_throw: bool,
}

#[derive(Clone, Debug)]
pub enum PlayRecord {
    Scrimmage(ScrimmageRecord),
    ThrowOff(ThrowOffRecord),
}

impl PlayRecord {
    pub fn description(&self) -> &str {
        match self {
            PlayRecord::Scrimmage(r) => &r.result.description,
            PlayRecord::ThrowOff(r) => &r.result.description,
        }
    }

    pub fn quarter(&self) -> u8 {
        match self {
            PlayRecord::Scrimmage(r) => r.quarter,
            PlayRecord::ThrowOff(r) => r.quarter,
        }
    }

    pub fn clock(&self) -> f32 {
        match self {
            PlayRecord::Scrimmage(r) => r.clock,
            PlayRecord::ThrowOff(r) => r.clock,
        }
    }

    pub fn score_after(&self) -> [u32; 2] {
        match self {
            PlayRecord::Scrimmage(r) => r.score_after,
            PlayRecord::ThrowOff(r) => r.score_after,
        }
    }

    pub fn is_score(&self) -> bool {
        match self {
            PlayRecord::Scrimmage(r) => {
                r.result.touchdown || r.result.safety || r.result.penalty.is_some()
            }
            PlayRecord::ThrowOff(r) => {
                r.result.touchdown || r.result.safety || r.result.touchback || r.result.fair_catch
            }
        }
    }

    pub fn highlight(&self) -> bool {
        match self {
            PlayRecord::Scrimmage(r) => r.result.highlight,
            PlayRecord::ThrowOff(r) => r.result.highlight,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DriveState {
    pub offense: usize,
    pub start_spot: f32,
    pub plays: u32,
    pub yards: f32,
    pub points: u32,
    pub time: f32,
}

#[derive(Clone, Debug)]
pub struct OtState {
    pub round: u8,
    pub possession_index: u8,
    pub scores: [u32; 2],
    pub first_downs: u8,
    pub forced_two: bool,
}

pub struct GameState {
    pub teams: [Team; 2],
    pub score: [u32; 2],
    pub quarter: u8,
    pub clock: f32,
    /// How long one quarter runs, in seconds. The league plays fifteen; a
    /// player setting a shorter game sets this before the opening throw-off.
    pub quarter_seconds: f32,
    pub possession: usize,
    pub spot: f32,
    pub ball_y: f32,
    pub direction: i8,
    pub down: u8,
    pub to_go: f32,
    pub play_clock: f32,
    pub timeouts: [u8; 2],
    pub phase: GamePhase,
    pub box_score: BoxScore,
    pub log: Vec<PlayRecord>,
    pub commentary: Vec<String>,
    pub over: bool,
    pub winner: Option<usize>,
    pub seed: u64,
    pub ai_counter: u64,
    pub two_minute_warned: bool,
    pub first_half_receiver: usize,
    pub drive: DriveState,
    pub ot: Option<OtState>,
    pub last_record: Option<PlayRecord>,
    pub clock_stopped: bool,
    pub no_timeouts_for_leader: bool,
    pub home_attack_dir: i8,
    /// How the other sideline plays: the coordinator's calls and how hard the
    /// defence goes for the ball.
    pub difficulty: Difficulty,
}

/// Who gets the six points on a touchdown.
///
/// Normally the side with the ball. On a return -- an interception or a fumble
/// recovered by the defense -- the men who scored are the men who took it away,
/// and the play is booked as a touchdown rather than as the takeaway, so the
/// turnover is what says which is which.
fn touchdown_scorer(turnover: bool, offense: usize, defense: usize) -> usize {
    if turnover {
        defense
    } else {
        offense
    }
}

impl GameState {
    /// Set how hard the other sideline plays, from the settings screen.
    /// Set the length of a quarter, in seconds. It has to be set before the
    /// opening throw-off: a clock already running is not rewound under the
    /// players, but every quarter after this one uses the new length.
    pub fn set_quarter_seconds(&mut self, seconds: f32) {
        let seconds = seconds.clamp(60.0, 3600.0);
        self.quarter_seconds = seconds;
        if self.log.is_empty() && self.quarter == 1 && self.clock > 0.0 {
            self.clock = seconds;
        }
    }

    pub fn set_difficulty(&mut self, difficulty: Difficulty) {
        self.difficulty = difficulty;
    }

    /// Set the side that takes the opening throw-off. The toss happens on the
    /// front end now, so the game is told the answer rather than drawing it.
    pub fn set_opening_receiver(&mut self, team: usize) {
        let receiving = team.min(1);
        self.first_half_receiver = receiving;
        self.possession = receiving;
        self.phase = GamePhase::ThrowOff { receiving, from_own: THROW_OFF_SPOT_YARD_LINE };
        self.commentary.clear();
        self.commentary.push(format!(
            "{} takes the opening throw-off.",
            self.teams[receiving].full_name()
        ));
        self.refresh_direction();
    }

    pub fn new(home: Team, away: Team, seed: u64) -> Self {
        let box_score = BoxScore::new(&home, &away);
        let mut rng = Rng::new(seed);
        let first_half_receiver = rng.index(2);
        let teams = [home, away];
        let mut game = GameState {
            teams,
            score: [0, 0],
            quarter: 1,
            clock: QUARTER_SECONDS,
            quarter_seconds: QUARTER_SECONDS,
            possession: first_half_receiver,
            spot: field::spot_from_yards_to_goal(65.0, 1),
            ball_y: 0.0,
            direction: 1,
            down: 1,
            to_go: 10.0,
            play_clock: PLAY_CLOCK_SECONDS,
            timeouts: [TIMEOUTS_PER_HALF, TIMEOUTS_PER_HALF],
            phase: GamePhase::ThrowOff { receiving: first_half_receiver, from_own: THROW_OFF_SPOT_YARD_LINE },
            box_score,
            log: Vec::new(),
            commentary: Vec::new(),
            over: false,
            winner: None,
            seed,
            ai_counter: 0,
            two_minute_warned: false,
            first_half_receiver,
            drive: DriveState::default(),
            ot: None,
            last_record: None,
            clock_stopped: true,
            no_timeouts_for_leader: false,
            home_attack_dir: 1,
            difficulty: Difficulty::Easy,
        };
        game.refresh_direction();
        game.commentary.push(format!(
            "{} wins the toss and will receive the opening throw-off.",
            game.teams[first_half_receiver].full_name()
        ));
        game
    }

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    pub fn offense(&self) -> usize {
        self.possession
    }

    pub fn defense(&self) -> usize {
        1 - self.possession
    }

    pub fn yards_to_goal(&self) -> f32 {
        field::yards_to_goal(self.spot, self.direction)
    }

    pub fn offense_yards_to_goal(&self) -> f32 {
        self.yards_to_goal()
    }

    pub fn is_overtime(&self) -> bool {
        self.quarter > 4
    }

    pub fn is_over(&self) -> bool {
        self.over
    }

    pub fn score(&self) -> [u32; 2] {
        self.score
    }

    /// Distance from the offense's own goal line, in yards.
    pub fn own_yards_at_spot(&self) -> f32 {
        FIELD_LENGTH - self.yards_to_goal()
    }

    pub fn direction_for(&self, team: usize) -> i8 {
        if team == 0 {
            self.home_attack_dir
        } else {
            -self.home_attack_dir
        }
    }

    fn refresh_direction(&mut self) {
        // Teams switch ends after halftime and in overtime rounds.
        let flip = matches!(self.quarter, 3 | 4) || (self.quarter >= 5 && self.quarter % 2 == 0);
        self.home_attack_dir = if flip { -1 } else { 1 };
        self.direction = self.direction_for(self.possession);
    }

    pub fn next_seed(&mut self) -> u64 {
        self.ai_counter = self.ai_counter.wrapping_add(1);
        self.seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(self.ai_counter.wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }

    pub fn action(&self) -> Action {
        if self.over {
            return Action::GameOver;
        }
        match self.phase {
            GamePhase::ThrowOff { .. } => Action::ThrowOff,
            GamePhase::ConversionChoice { .. } => Action::ChooseConversion,
            GamePhase::Conversion { .. } => Action::CallPlay { fourth_down: false },
            GamePhase::Scrimmage => Action::CallPlay { fourth_down: self.down == 4 },
            GamePhase::GameOver => Action::GameOver,
        }
    }

    pub fn down_and_distance(&self) -> String {
        ai::down_and_distance(self)
    }

    pub fn clock_text(&self) -> String {
        let c = self.clock.max(0.0);
        let m = (c / 60.0).floor() as u32;
        let s = (c - m as f32 * 60.0).floor() as u32;
        format!("{m}:{s:02}")
    }

    pub fn quarter_text(&self) -> String {
        match self.quarter {
            1 => "1ST".to_string(),
            2 => "2ND".to_string(),
            3 => "3RD".to_string(),
            4 => "4TH".to_string(),
            q => format!("OT{}", q - 4),
        }
    }

    pub fn is_under_two(&self) -> bool {
        (self.quarter == 2 || self.quarter == 4) && self.clock <= TWO_MINUTE_WARNING
    }

    // ------------------------------------------------------------------
    // Drives
    // ------------------------------------------------------------------

    fn start_drive(&mut self) {
        self.drive = DriveState {
            offense: self.possession,
            start_spot: self.spot,
            plays: 0,
            yards: 0.0,
            points: 0,
            time: 0.0,
        };
    }

    fn end_drive(&mut self, result: &str) {
        if self.drive.plays == 0 {
            return;
        }
        let summary = DriveSummary {
            offense: self.drive.offense,
            start_spot: self.drive.start_spot,
            start_yards_to_goal: field::yards_to_goal(self.drive.start_spot, self.direction),
            plays: self.drive.plays,
            yards: self.drive.yards,
            result: result.to_string(),
            points: self.drive.points,
            time: self.drive.time,
        };
        self.box_score.drives.push(summary);
        self.drive = DriveState::default();
    }

    // ------------------------------------------------------------------
    // Public driving API
    // ------------------------------------------------------------------

    pub fn simulate_next(&mut self) -> Option<PlayRecord> {
        if self.over {
            return None;
        }
        match self.phase {
            GamePhase::ThrowOff { receiving, from_own } => {
                Some(self.run_throw_off(receiving, from_own, false))
            }
            GamePhase::ConversionChoice { .. } => {
                let seed = self.next_seed();
                let two = ai::choose_two_point(self, seed);
                self.choose_conversion(two)
            }
            GamePhase::Conversion { .. } => {
                let seed = self.next_seed();
                let play = ai::choose_offense(self, seed);
                let seed = self.next_seed();
                let def = ai::choose_defense(self, seed);
                Some(self.snap(play, def))
            }
            GamePhase::Scrimmage => {
                if self.down == 4 {
                    let seed = self.next_seed();
                    let choice = ai::choose_fourth_down(self, seed);
                    if choice == FourthDownChoice::ThrowOff {
                        let from_own = self.own_yards_at_spot();
                        let receiving = self.defense();
                        return Some(self.run_throw_off(receiving, from_own, true));
                    }
                }
                let seed = self.next_seed();
                let play = ai::choose_offense(self, seed);
                let seed = self.next_seed();
                let def = ai::choose_defense(self, seed);
                Some(self.snap(play, def))
            }
            GamePhase::GameOver => None,
        }
    }

    /// Human play call for a scrimmage down or conversion attempt.
    pub fn call_play(&mut self, play: Play, def: DefCall) -> Option<PlayRecord> {
        match self.phase {
            GamePhase::Scrimmage | GamePhase::Conversion { .. } => Some(self.snap(play, def)),
            _ => None,
        }
    }

    /// Fourth-down throw-off (the 7 on 7 replacement for a punt).
    pub fn fourth_down_throw_off(&mut self) -> Option<PlayRecord> {
        if self.phase != GamePhase::Scrimmage || self.down != 4 {
            return None;
        }
        let from_own = self.own_yards_at_spot();
        Some(self.run_throw_off(self.defense(), from_own, true))
    }

    /// Set up a conversion attempt without running it, so a human can call the
    /// try and play the snap out themselves.
    pub fn set_conversion(&mut self, two_point: bool) {
        let offense = match self.phase {
            GamePhase::ConversionChoice { offense } => offense,
            GamePhase::Conversion { offense, .. } => offense,
            _ => self.possession,
        };
        self.possession = offense;
        self.refresh_direction();
        let points = if two_point { 2 } else { 1 };
        let yards = if two_point { CONVERSION_TWO_POINT_YARDS } else { CONVERSION_ONE_POINT_YARDS };
        self.spot = field::spot_from_yards_to_goal(yards, self.direction);
        self.down = 1;
        self.to_go = yards;
        self.phase = GamePhase::Conversion { offense, points };
    }

    pub fn choose_conversion(&mut self, two_point: bool) -> Option<PlayRecord> {
        self.set_conversion(two_point);
        let seed = self.next_seed();
        let play = ai::choose_offense(self, seed);
        let seed = self.next_seed();
        let def = ai::choose_defense(self, seed);
        Some(self.snap(play, def))
    }

    pub fn use_timeout(&mut self, side: usize) -> bool {
        if self.timeouts[side] == 0 {
            return false;
        }
        if self.no_timeouts_for_leader && self.score[side] > self.score[1 - side] {
            return false;
        }
        self.timeouts[side] -= 1;
        self.clock_stopped = true;
        self.commentary.push(format!(
            "Timeout, {}. {} remaining.",
            self.teams[side].abbr, self.timeouts[side]
        ));
        let _ = TIMEOUT_SECONDS;
        true
    }

    pub fn simulate_game(&mut self) -> &[PlayRecord] {
        let mut guard = 0;
        while !self.over && guard < 3000 {
            if self.simulate_next().is_none() {
                break;
            }
            guard += 1;
        }
        &self.log
    }

    // ------------------------------------------------------------------
    // Snaps
    // ------------------------------------------------------------------

    fn snap(&mut self, play: Play, def: DefCall) -> PlayRecord {
        let setup = self.snap_setup();
        let result = simulate_play(
            &self.teams[setup.offense_team],
            &self.teams[setup.defense_team],
            &play,
            &def,
            &setup,
        );
        self.finish_snap(play, def, setup, result)
    }

    /// Like [`GameState::snap`], but hands the live simulation to `drive` so an
    /// interactive client can steer the quarterback and watch the play unfold
    /// before the result is booked. If the driver returns without a result the
    /// play is run to completion.
    pub fn snap_with<F>(&mut self, play: Play, def: DefCall, mut drive: F) -> Option<PlayRecord>
    where
        F: for<'s> FnMut(&mut crate::sim::PlaySim<'s>),
    {
        if !matches!(self.phase, GamePhase::Scrimmage | GamePhase::Conversion { .. }) {
            return None;
        }
        let setup = self.snap_setup();
        let result = {
            let mut sim = crate::sim::PlaySim::new(
                &self.teams[setup.offense_team],
                &self.teams[setup.defense_team],
                &play,
                &def,
                &setup,
            );
            sim.difficulty = self.difficulty;
            drive(&mut sim);
            match sim.result.clone() {
                Some(r) => r,
                None => sim.run_to_completion(),
            }
        };
        Some(self.finish_snap(play, def, setup, result))
    }

    fn snap_setup(&mut self) -> SnapSetup {
        let is_try = matches!(self.phase, GamePhase::Conversion { .. });
        let seed = self.next_seed();
        let offense = self.possession;
        SnapSetup {
            los: self.spot,
            ball_y: self.ball_y,
            direction: self.direction,
            down: self.down,
            to_go: self.to_go,
            seed,
            offense_team: offense,
            defense_team: 1 - offense,
            is_try,
        }
    }

    fn finish_snap(
        &mut self,
        play: Play,
        def: DefCall,
        setup: SnapSetup,
        result: crate::sim::PlayResult,
    ) -> PlayRecord {
        let is_try = matches!(self.phase, GamePhase::Conversion { .. });
        let score_before = self.score;
        let mut record = ScrimmageRecord {
            quarter: self.quarter,
            clock: self.clock,
            down: self.down,
            to_go: self.to_go,
            setup,
            play,
            defense: def,
            result,
            score_before,
            score_after: self.score,
            is_try,
            penalty_yards: 0,
        };
        if is_try {
            self.apply_try(&mut record);
        } else {
            self.apply_scrimmage(&mut record);
        }
        record.score_after = self.score;
        let rec = PlayRecord::Scrimmage(record);
        self.log.push(rec.clone());
        self.last_record = Some(rec.clone());
        self.play_clock = PLAY_CLOCK_SECONDS;
        rec
    }

    fn apply_scrimmage(&mut self, record: &mut ScrimmageRecord) {
        let offense = record.setup.offense_team;
        let defense = record.setup.defense_team;
        let dir = record.setup.direction;
        let result = record.result.clone();
        let prev_spot = record.setup.los;
        let prev_down = record.down;
        let prev_to_go = record.to_go;
        let quarter = record.quarter;

        self.drive.plays += 1;
        self.drive.time += result.elapsed;
        // Time of possession includes the huddle: the running clock keeps moving
        // between snaps unless the play stopped it.
        let between = if self.is_overtime() { 0.0 } else { TIME_BETWEEN_SNAPS };
        self.box_score.teams[offense].time_of_possession += between;
        self.drive.time += between;

        if let Some(call) = result.penalty.clone() {
            if result.touchdown || result.safety {
                // The play already put points on the board and a flag does not take
                // them off again: the foul is noted and declined.
                self.commentary.push(format!(
                    "{} on the play, declined - the score stands.",
                    call.kind.label()
                ));
            } else {
                let enf = enforce(&call, prev_spot, dir, result.end_spot, prev_down, prev_to_go);
                self.box_score.add_penalty(offense, enf.yards.round() as u32);
                record.penalty_yards = enf.yards.round() as u32;
                self.commentary.push(penalty_sentence(&call, &enf));
                self.spot = enf.spot;
                self.down = enf.down;
                self.to_go = enf.to_go;
                self.clock_stopped = true;
                self.box_score.record(offense, &result, false, quarter as usize);
                if (prev_down == DOWNS && enf.loss_of_down) || self.down > DOWNS {
                    let spot = self.spot;
                    self.end_drive("Turnover on downs");
                    self.start_series(defense, spot);
                }
                self.advance_clock(result.elapsed, true);
                return;
            }
        }

        let new_spot = if result.touchdown {
            field::opponent_goal_side(dir)
        } else if result.safety {
            field::opponent_goal_side(-dir)
        } else {
            clamp(result.end_spot, 0.0, FIELD_LENGTH)
        };

        let prev_ytg = field::yards_to_goal(prev_spot, dir);
        let target = prev_ytg - prev_to_go;
        let new_ytg = field::yards_to_goal(new_spot, dir);
        let first_down = result.touchdown || new_ytg <= target + 0.01;

        // Score-keeping first: a touchdown overrides normal down logic.
        if result.touchdown {
            // A return is the defense's six points, not the offense's: the men who
            // scored are the men who were defending. A return that reaches the end
            // zone is booked as a touchdown, so the play's outcome alone cannot
            // tell them apart -- the turnover can.
            let scoring = touchdown_scorer(result.turnover, offense, defense);
            self.apply_score(scoring, 6);
            self.drive.points += 6;
            self.drive.yards += result.yards;
            self.commentary.push(format!(
                "TOUCHDOWN {}! {}",
                self.teams[scoring].abbr, result.description
            ));
            self.box_score.record(offense, &result, true, quarter as usize);
            self.spot = new_spot;
            self.end_drive("Touchdown");
            self.after_touchdown(scoring);
            return;
        }

        if result.safety {
            self.apply_score(defense, 2);
            self.commentary.push(format!("SAFETY! {}", result.description));
            self.box_score.record(offense, &result, false, quarter as usize);
            self.end_drive("Safety");
            // The team that was on offense throws off from its own 25.
            self.possession = offense;
            self.refresh_direction();
            self.phase = GamePhase::ThrowOff {
                receiving: defense,
                from_own: SAFETY_THROW_OFF_YARD_LINE,
            };
            self.clock_stopped = true;
            if self.ot.is_some() {
                self.end_ot_possession();
            }
            return;
        }

        self.drive.yards += result.yards;
        self.box_score.record(offense, &result, first_down, quarter as usize);

        let under_two = self.is_under_two();
        let stopped = result.turnover
            || (under_two && (result.incomplete || result.out_of_bounds));

        if result.turnover {
            self.commentary.push(format!("Turnover. {}", result.description));
            self.end_drive("Turnover");
            self.start_series(defense, new_spot);
            self.clock_stopped = true;
            if self.ot.is_some() {
                self.end_ot_possession();
            }
            self.advance_clock(result.elapsed, true);
            return;
        }

        // Normal continuation.
        self.spot = clamp(new_spot, 0.0, FIELD_LENGTH);
        if first_down {
            self.box_score.teams[offense].first_downs += 0;
            self.down = 1;
            self.to_go = self.yards_to_goal().min(YARDS_TO_GO).max(1.0);
            self.ball_y = 0.0;
            if self.ot.is_some() {
                if let Some(ot) = self.ot.as_mut() {
                    ot.first_downs += 1;
                    if ot.first_downs >= OVERTIME_FIRST_DOWNS_ALLOWED {
                        self.commentary
                            .push("That's the second first down — the overtime series is over.".to_string());
                        self.end_drive("Overtime series over");
                        self.end_ot_possession();
                        return;
                    }
                }
            }
        } else {
            self.down += 1;
            self.to_go = self.yards_to_goal().min(self.to_go).max(0.5);
            if self.down > DOWNS {
                self.commentary
                    .push(format!("{} turns it over on downs.", self.teams[offense].abbr));
                let spot = self.spot;
                self.end_drive("Turnover on downs");
                self.start_series(defense, spot);
                self.clock_stopped = true;
                if self.ot.is_some() {
                    self.end_ot_possession();
                }
                self.advance_clock(result.elapsed, true);
                return;
            }
        }

        self.clock_stopped = stopped;
        self.advance_clock(result.elapsed, stopped);
    }

    fn apply_try(&mut self, record: &mut ScrimmageRecord) {
        let offense = record.setup.offense_team;
        let points = match self.phase {
            GamePhase::Conversion { points, .. } => points,
            _ => 1,
        };
        let result = record.result.clone();
        self.box_score.record(offense, &result, false, self.quarter as usize);

        if let Some(call) = result.penalty.clone() {
            if !result.touchdown {
                // The 1-or-2 decision is locked in even if a penalty is called.
                let enf =
                    enforce(&call, record.setup.los, self.direction, result.end_spot, 1, self.to_go);
                self.box_score.add_penalty(offense, enf.yards.round() as u32);
                self.commentary.push(penalty_sentence(&call, &enf));
                self.spot = enf.spot;
                self.down = 1;
                self.to_go = enf.to_go;
                self.phase = GamePhase::Conversion { offense, points };
                self.clock_stopped = true;
                return;
            }
            self.commentary.push(format!(
                "{} on the play, declined - the conversion stands.",
                call.kind.label()
            ));
        }

        let converted = result.touchdown;
        self.box_score.add_conversion(offense, converted);
        if converted {
            self.apply_score(offense, points);
            self.commentary.push(format!(
                "{} {} the {} point conversion.",
                self.teams[offense].abbr,
                if converted { "gets" } else { "misses" },
                points
            ));
            self.drive.points += points;
        }

        if self.ot.is_some() {
            self.end_drive(if converted { "Conversion" } else { "Stopped" });
            self.end_ot_possession();
            return;
        }

        // After any score the scoring team throws off.
        self.possession = offense;
        self.refresh_direction();
        self.phase = GamePhase::ThrowOff {
            receiving: 1 - offense,
            from_own: THROW_OFF_SPOT_YARD_LINE,
        };
        self.clock_stopped = true;
        self.end_drive(if converted { "Touchdown" } else { "Stopped" });
        self.advance_clock(result.elapsed, true);
    }

    fn after_touchdown(&mut self, offense: usize) {
        self.possession = offense;
        self.refresh_direction();
        self.phase = GamePhase::ConversionChoice { offense };
        self.clock_stopped = true;
        self.play_clock = PLAY_CLOCK_SECONDS;
    }

    fn apply_score(&mut self, team: usize, points: u32) {
        self.score[team] += points;
        let q_index = (self.quarter.min(6) as usize).saturating_sub(1);
        self.box_score.add_points(team, points, q_index);
        if let Some(ot) = self.ot.as_mut() {
            ot.scores[team] += points;
        }
        // The lead is checked the moment it is taken, not at the end of the
        // quarter: five touchdowns in the first is a game that is over there and
        // then, and the rule the league plays by is written that way.
        self.apply_mercy_rule();
    }

    fn start_series(&mut self, offense: usize, spot: f32) {
        self.end_drive("Series ended");
        self.possession = offense;
        self.refresh_direction();
        self.spot = clamp(spot, 0.0, FIELD_LENGTH);
        self.ball_y = 0.0;
        self.down = 1;
        self.to_go = self.yards_to_goal().min(YARDS_TO_GO).max(1.0);
        self.phase = GamePhase::Scrimmage;
        self.clock_stopped = true;
        self.start_drive();
    }

    // ------------------------------------------------------------------
    // Throw-offs
    // ------------------------------------------------------------------

    /// Set up the throw-off that is about to be played, without running it, so a
    /// human can throw it off himself or return it.
    pub fn throw_off_setup(&mut self, receiving: usize, from_own: f32, free_throw: bool) -> ThrowOffSetup {
        let throwing = 1 - receiving;
        let seed = self.next_seed();
        ThrowOffSetup {
            throwing_team: throwing,
            receiving_team: receiving,
            from_own_yards: from_own,
            receive_direction: self.direction_for(receiving),
            seed,
            free_throw,
        }
    }

    /// Book a throw-off that has been played out, and move the game on from it.
    pub fn finish_throw_off(&mut self, setup: ThrowOffSetup, result: ThrowOffResult) -> PlayRecord {
        let free_throw = setup.free_throw;
        let score_before = self.score;
        let quarter = self.quarter;
        let clock = self.clock;
        self.apply_throw_off(&result, free_throw);
        let record = ThrowOffRecord {
            quarter,
            clock,
            setup,
            result,
            score_before,
            score_after: self.score,
            is_fourth_down_throw: free_throw,
        };
        let rec = PlayRecord::ThrowOff(record);
        self.log.push(rec.clone());
        self.last_record = Some(rec.clone());
        self.play_clock = PLAY_CLOCK_SECONDS;
        rec
    }

    pub fn run_throw_off(&mut self, receiving: usize, from_own: f32, free_throw: bool) -> PlayRecord {
        let setup = self.throw_off_setup(receiving, from_own, free_throw);
        let throwing = setup.throwing_team;
        let receiver = setup.receiving_team;
        let result = simulate_throw_off(&setup, &self.teams[throwing], &self.teams[receiver]);
        self.finish_throw_off(setup, result)
    }

    /// The fourth-down throw-off (the 7 on 7 replacement for a punt), set up but
    /// not run.
    pub fn fourth_down_throw_off_setup(&mut self) -> Option<ThrowOffSetup> {
        if self.phase != GamePhase::Scrimmage || self.down != 4 {
            return None;
        }
        let from_own = self.own_yards_at_spot();
        Some(self.throw_off_setup(self.defense(), from_own, true))
    }

    fn apply_throw_off(&mut self, r: &ThrowOffResult, _free_throw: bool) {
        self.commentary.push(r.description.clone());
        if r.touchdown {
            let scoring = r.scoring_team.unwrap_or(r.next_offense);
            self.apply_score(scoring, 6);
            self.possession = scoring;
            self.refresh_direction();
            self.end_drive("Return touchdown");
            self.after_touchdown(scoring);
            return;
        }
        if r.safety {
            let scoring = r.scoring_team.unwrap_or(1 - r.next_offense);
            self.apply_score(scoring, 2);
            self.possession = scoring;
            self.refresh_direction();
            self.end_drive("Safety");
            self.phase = GamePhase::ThrowOff {
                receiving: r.next_offense,
                from_own: SAFETY_THROW_OFF_YARD_LINE,
            };
            self.clock_stopped = true;
            return;
        }
        self.end_drive("Throw-off");
        self.start_series(r.next_offense, r.start_spot);
        self.advance_clock(r.elapsed * 0.4, false);
    }

    // ------------------------------------------------------------------
    // Clock, quarters, overtime
    // ------------------------------------------------------------------

    fn advance_clock(&mut self, elapsed: f32, stopped: bool) {
        if self.is_overtime() {
            self.clock = 0.0;
            return;
        }
        if !stopped {
            self.clock -= elapsed + TIME_BETWEEN_SNAPS;
        }
        if !self.two_minute_warned
            && (self.quarter == 2 || self.quarter == 4)
            && self.clock <= TWO_MINUTE_WARNING
        {
            self.clock = TWO_MINUTE_WARNING;
            self.two_minute_warned = true;
            self.clock_stopped = true;
            self.commentary.push("Two-minute warning.".to_string());
            return;
        }
        if self.clock <= 0.0 {
            self.clock = 0.0;
            self.end_quarter();
        }
    }

    pub fn burn_clock(&mut self, seconds: f32) {
        self.clock = (self.clock - seconds).max(0.0);
        if self.clock <= 0.0 && !self.is_overtime() {
            self.end_quarter();
        }
    }

    fn end_quarter(&mut self) {
        self.commentary
            .push(format!("End of the {} quarter.", self.quarter));
        if self.quarter < 4 {
            self.quarter += 1;
            self.clock = self.quarter_seconds;
            self.two_minute_warned = false;
            self.clock_stopped = true;
            if self.quarter == 3 {
                self.timeouts = [TIMEOUTS_PER_HALF, TIMEOUTS_PER_HALF];
                let receiving = 1 - self.first_half_receiver;
                self.possession = receiving;
                self.refresh_direction();
                self.phase = GamePhase::ThrowOff {
                    receiving,
                    from_own: THROW_OFF_SPOT_YARD_LINE,
                };
                self.end_drive("Halftime");
                self.commentary
                    .push("Second half. The throw-offs are reversed.".to_string());
            } else {
                // Teams switch ends; the team with the ball keeps it.
                self.refresh_direction();
            }
            self.apply_mercy_rule();
            return;
        }

        // End of regulation.
        if self.score[0] == self.score[1] {
            self.begin_overtime();
        } else {
            let winner = if self.score[0] > self.score[1] { 0 } else { 1 };
            self.finish_game(Some(winner));
        }
    }

    fn apply_mercy_rule(&mut self) {
        if self.over || self.is_overtime() {
            return;
        }
        let diff = (self.score[0] as i32 - self.score[1] as i32).abs();
        // Five touchdowns ends it in the first quarter; seven is the mark from the
        // second quarter on, where a team has had a half to find something.
        let early = self.quarter <= 1;
        let tds = if early {
            MERCY_LEAD_TOUCHDOWNS_EARLY
        } else {
            MERCY_LEAD_TOUCHDOWNS
        };
        if diff >= (tds as i32) * 6 {
            let winner = if self.score[0] > self.score[1] { 0 } else { 1 };
            let when = if early {
                "five touchdowns before the end of the first quarter"
            } else {
                "seven touchdowns"
            };
            self.commentary
                .push(format!("Mercy rule: the lead is {when}, ball game."));
            self.finish_game(Some(winner));
        } else if self.quarter == 4 && diff >= 4 * 6 {
            self.no_timeouts_for_leader = true;
            self.commentary
                .push("Four-score lead in the fourth: the leader can't use timeouts.".to_string());
        }
    }

    fn begin_overtime(&mut self) {
        self.quarter = 5;
        self.clock = 0.0;
        self.two_minute_warned = false;
        self.timeouts = [1, 1];
        let toss = Rng::new(self.seed ^ 0x0A7F_1E).index(2);
        self.ot = Some(OtState {
            round: 1,
            possession_index: 0,
            scores: [0, 0],
            first_downs: 0,
            forced_two: false,
        });
        self.commentary.push(
            "We're tied. Overtime: each team gets the ball at the 25 with two first downs to work with."
                .to_string(),
        );
        self.end_drive("End of regulation");
        self.start_ot_series(toss);
    }

    fn start_ot_series(&mut self, offense: usize) {
        self.possession = offense;
        self.refresh_direction();
        let spot = field::spot_from_yards_to_goal(FIELD_LENGTH - OVERTIME_SPOT_YARD_LINE, self.direction);
        self.spot = spot;
        self.ball_y = 0.0;
        self.down = 1;
        self.to_go = 10.0;
        self.phase = GamePhase::Scrimmage;
        self.clock_stopped = true;
        self.start_drive();
        if let Some(ot) = self.ot.as_mut() {
            ot.first_downs = 0;
        }
    }

    fn end_ot_possession(&mut self) {
        let Some(mut ot) = self.ot.clone() else { return };
        if self.over {
            return;
        }
        ot.possession_index += 1;
        if ot.possession_index < 2 {
            self.ot = Some(ot);
            let next = 1 - self.possession;
            self.start_ot_series(next);
            return;
        }
        if ot.scores[0] != ot.scores[1] {
            let winner = if ot.scores[0] > ot.scores[1] { 0 } else { 1 };
            self.ot = Some(ot);
            self.finish_game(Some(winner));
            return;
        }
        // Still tied: another overtime round. From the second round on, everybody
        // has to go for two.
        ot.round += 1;
        ot.possession_index = 0;
        ot.scores = [0, 0];
        ot.forced_two = ot.round >= 2;
        let round = ot.round;
        self.commentary.push(format!(
            "Overtime round {round}. Conversions are now two-point only."
        ));
        self.ot = Some(ot);
        let next = 1 - self.possession;
        self.start_ot_series(next);
    }

    fn finish_game(&mut self, winner: Option<usize>) {
        self.over = true;
        self.winner = winner;
        self.phase = GamePhase::GameOver;
        self.end_drive("End of game");
        match winner {
            Some(_) => self.commentary.push(format!(
                "Final: {} {}, {} {}.",
                self.teams[0].abbr,
                self.score[0],
                self.teams[1].abbr,
                self.score[1]
            )),
            None => self.commentary.push(format!(
                "Final: {} {}, {} {} — tie.",
                self.teams[0].abbr, self.score[0], self.teams[1].abbr, self.score[1]
            )),
        }
        let _ = winner;
    }

    /// True when overtime rules force a two-point attempt.
    pub fn conversion_forced_two(&self) -> bool {
        self.ot.as_ref().map(|o| o.forced_two).unwrap_or(false)
    }

    /// Combined play list for the broadcast ticker.
    pub fn recent_plays(&self, count: usize) -> Vec<String> {
        let start = self.log.len().saturating_sub(count);
        self.log[start..]
            .iter()
            .map(|r| r.description().to_string())
            .collect()
    }

    pub fn total_plays(&self) -> usize {
        self.log.len()
    }

    /// Quick text summary, used by the CLI and tests.
    pub fn summary(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "{} {} — {} {}\n",
            self.teams[0].full_name(),
            self.score[0],
            self.teams[1].full_name(),
            self.score[1]
        ));
        for q in 0..5 {
            let a = self.box_score.quarter_scores[0][q];
            let b = self.box_score.quarter_scores[1][q];
            if a > 0 || b > 0 {
                out.push_str(&format!("  Q{}: {}-{}\n", q + 1, a, b));
            }
        }
        out
    }

    pub fn outcome_label(record: &PlayRecord) -> String {
        match record {
            PlayRecord::Scrimmage(r) => match r.result.outcome {
                PlayOutcome::Touchdown => "TOUCHDOWN".to_string(),
                _ => format!("{:?}", r.result.outcome).to_uppercase(),
            },
            PlayRecord::ThrowOff(r) => format!("{:?}", r.result.outcome).to_uppercase(),
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn the_opening_receiver_can_be_set_from_the_toss() {
        let teams = crate::team::league_teams();
        let mut g = GameState::new(teams[0].clone(), teams[1].clone(), 9);
        g.set_opening_receiver(1);
        assert_eq!(g.first_half_receiver, 1);
        assert_eq!(g.possession, 1);
        assert!(matches!(g.phase, GamePhase::ThrowOff { receiving: 1, .. }));
        assert!(g.commentary[0].contains("throw-off"), "{:?}", g.commentary);
        // And the other way, because the toss can go either way.
        g.set_opening_receiver(0);
        assert_eq!(g.first_half_receiver, 0);
        assert!(matches!(g.phase, GamePhase::ThrowOff { receiving: 0, .. }));
        assert_eq!(g.commentary.len(), 1, "the old toss line was left behind");
    }
    use super::*;
    use crate::team::league_teams;

    fn new_game(seed: u64) -> GameState {
        let t = league_teams();
        GameState::new(t[0].clone(), t[1].clone(), seed)
    }

    #[test]
    fn a_game_reaches_a_final_score() {
        let mut g = new_game(11);
        g.simulate_game();
        assert!(g.over, "game should end");
        assert!(g.total_plays() > 40, "too few plays: {}", g.total_plays());
        let [a, b] = g.score;
        assert!(a > 0 || b > 0, "nobody scored");
        if a != b {
            let w = if a > b { 0 } else { 1 };
            assert_eq!(g.winner, Some(w));
        }
    }

    #[test]
    fn every_quarter_gets_played_and_clock_is_sane() {
        let mut g = new_game(3);
        let mut last_q = 1;
        let mut guard = 0;
        while !g.over && guard < 2000 {
            if g.simulate_next().is_none() {
                break;
            }
            assert!(g.clock >= 0.0 && g.clock <= QUARTER_SECONDS + 1.0, "clock {}", g.clock);
            assert!(g.quarter >= last_q, "quarter went backwards");
            last_q = g.quarter;
            guard += 1;
        }
        // Five touchdowns before the end of the first quarter ends a game by the
        // mercy rule, so a short game is not a failure. A game that stops early
        // without that lead is.
        let diff = (g.score[0] as i32 - g.score[1] as i32).abs();
        assert!(
            g.quarter >= 4 || (g.over && diff >= 30),
            "game ended in quarter {} at {} - {} (over {})",
            g.quarter,
            g.score[0],
            g.score[1],
            g.over
        );
    }

    #[test]
    fn points_are_only_awarded_in_rulebook_units() {
        let mut g = new_game(7);
        g.simulate_game();
        // 6, 3? No kicking: every score is 6, 2 (safety) or 1/2 (conversion).
        let total = g.score[0] + g.score[1];
        assert!(total > 0);
        assert!(total < 400, "absurd score {total}");
    }

    #[test]
    fn throw_off_starts_the_game() {
        let g = new_game(1);
        assert!(matches!(g.phase, GamePhase::ThrowOff { .. }));
        assert_eq!(g.quarter, 1);
        assert_eq!(g.clock, QUARTER_SECONDS);
    }

    #[test]
    fn a_quarter_is_as_long_as_it_is_set_to_be() {
        let mut g = new_game(21);
        g.set_quarter_seconds(4.0 * 60.0);
        assert!((g.clock - 240.0).abs() < 0.01, "opening clock {}", g.clock);
        g.clock = 0.1;
        g.end_quarter();
        assert_eq!(g.quarter, 2);
        assert!(
            (g.clock - 240.0).abs() < 0.01,
            "the second quarter is {} seconds",
            g.clock
        );
        // And a whole game at that length still reaches a result.
        g.simulate_game();
        assert!(g.over, "a short game never ended");
    }

    #[test]
    fn overtime_starts_when_regulation_ends_tied() {
        let t = league_teams();
        let mut g = GameState::new(t[0].clone(), t[1].clone(), 5);
        g.score = [20, 20];
        g.quarter = 4;
        g.clock = 1.0;
        g.phase = GamePhase::Scrimmage;
        g.end_quarter();
        assert!(g.is_overtime(), "should be in overtime");
        assert_eq!(g.quarter, 5);
        assert!(g.ot.is_some());
    }

    #[test]
    fn mercy_rule_ends_blowouts_at_the_start_of_the_fourth() {
        let t = league_teams();
        let mut g = GameState::new(t[0].clone(), t[1].clone(), 9);
        g.score = [56, 0];
        g.quarter = 3;
        g.clock = 1.0;
        // Ending the third quarter walks into the fourth, where the mercy rule runs.
        g.end_quarter();
        assert_eq!(g.quarter, 4);
        assert!(g.over, "a 56-point lead should end it at the start of the fourth");
        assert_eq!(g.winner, Some(0));
    }

    #[test]
    fn five_touchdowns_in_the_first_quarter_ends_it() {
        let t = league_teams();
        let mut g = GameState::new(t[0].clone(), t[1].clone(), 11);
        g.quarter = 1;
        g.apply_score(0, 24);
        assert!(!g.over, "four scores in the first quarter is still a game");
        g.apply_score(0, 6);
        assert!(g.over, "thirty points in the first should call the game");
        assert_eq!(g.winner, Some(0));
    }

    #[test]
    fn seven_touchdowns_is_the_mark_after_the_first_quarter() {
        let t = league_teams();
        let mut g = GameState::new(t[0].clone(), t[1].clone(), 12);
        g.quarter = 2;
        g.apply_score(0, 30);
        assert!(!g.over, "a five-score lead after the first is not the rule");
        g.apply_score(0, 12);
        assert!(g.over, "a forty-two point lead should call the game");
        assert_eq!(g.winner, Some(0));
    }

    #[test]
    fn conversion_choice_locks_in_the_distance() {
        let t = league_teams();
        let mut g = GameState::new(t[0].clone(), t[1].clone(), 13);
        g.phase = GamePhase::ConversionChoice { offense: 0 };
        g.possession = 0;
        g.refresh_direction();
        let rec = g.choose_conversion(true).expect("try should run");
        if let PlayRecord::Scrimmage(r) = rec {
            assert!(r.is_try);
            assert!((field::yards_to_goal(r.setup.los, r.setup.direction) - 10.0).abs() < 0.01);
        } else {
            panic!("conversion should be a scrimmage snap");
        }
    }
}

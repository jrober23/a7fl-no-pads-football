//! `a7fl arcade` — play the game live, in a window or in the terminal.
//!
//! The field is drawn by the same software renderer the broadcast package uses:
//! a real window when the machine has a display, ANSI colour blocks when it does
//! not. The quarterbacks are yours: pick a play from the call sheet, then move the
//! quarterback with the stick and throw to whichever receiver you have locked on.
//! Everything else — defense, clock, downs, the rulebook — runs as it does in
//! `sim`.

use std::io::{self, Write};
use std::time::{Duration, Instant};

use a7fl_core::ai;
use a7fl_core::game::{Action, GameState};
use a7fl_core::math::{Rng, Vec2};
use a7fl_core::playbook::{defensive_playbook, playbook, DefCall, Play};
use a7fl_core::sim::{PlaySim, QbControl};
use a7fl_render::broadcast::{Broadcast, CameraMode};
use a7fl_render::terminal;

use crate::pad::{self, Input, Pad};
use crate::view::View;

/// The live field is drawn at half the canvas and scaled up to it. A 640x360
/// frame costs about 40 ms, which caps the client at 16 fps and makes the
/// controls feel laggy; the half-size field costs a fraction of that.
pub const LIVE_W: usize = 320;
pub const LIVE_H: usize = 180;
/// Picture refresh for live play: 50 fps on the half-size field, which leaves
/// headroom on a slower CPU because the render is 9 ms rather than 25 ms.
const FRAME: Duration = Duration::from_millis(20);
const STEP: f32 = 1.0 / 60.0;
/// How long a result stays on screen before it moves on by itself.
const RESULT_HOLD: Duration = Duration::from_millis(2600);

pub struct Arcade {
    game: GameState,
    pad: Option<Pad>,
    view: View,
    broadcast: Broadcast,
    book: Vec<Play>,
    defbook: Vec<DefCall>,
    pick: usize,
    target: usize,
    quit: bool,
    /// Penalty insurance: some pads report the stick's Y the other way round.
    flip_y: bool,
}

impl Arcade {
    pub fn new(game: GameState, pad: Option<Pad>, view: View) -> Self {
        let book = playbook();
        let defbook = defensive_playbook();
        let teams = [game.teams[0].clone(), game.teams[1].clone()];
        let broadcast = Broadcast::new(LIVE_W, LIVE_H, teams);
        Arcade {
            game,
            pad,
            view,
            broadcast,
            book,
            defbook,
            pick: 0,
            target: 0,
            quit: false,
            flip_y: false,
        }
    }

    /// Reverse the stick's vertical axis, for pads that report it inverted.
    pub fn with_flip_y(mut self, flip: bool) -> Self {
        self.flip_y = flip;
        self
    }

    pub fn run(&mut self) {
        let terminal_mode = !self.view.is_window();
        if terminal_mode {
            print!("{}", terminal::enter());
            io::stdout().flush().ok();
        }
        self.play();
        if terminal_mode {
            print!("{}", terminal::leave());
            io::stdout().flush().ok();
        }
    }

    fn play(&mut self) {
        while !self.game.is_over() && !self.quit {
            match self.game.action() {
                Action::GameOver => break,
                Action::ThrowOff => {
                    let Some(record) = self.game.simulate_next() else { break };
                    let text = record.description().to_string();
                    self.show_result(&text, None);
                }
                Action::ChooseConversion => {
                    let two = self.choose_points();
                    if let Some(record) = self.game.choose_conversion(two) {
                        let text = record.description().to_string();
                        let points = if two { 2 } else { 1 };
                        self.show_result(&text, Some(points));
                    }
                }
                Action::CallPlay { fourth_down } => {
                    if fourth_down && self.ask_throw_off() {
                        if let Some(record) = self.game.fourth_down_throw_off() {
                            let text = record.description().to_string();
                            self.show_result(&text, None);
                        }
                        continue;
                    }
                    if self.call_sheet() {
                        self.run_snap();
                    }
                }
            }
        }
        if !self.quit {
            self.show_final();
        }
    }

    // ------------------------------------------------------------------
    // Play calling
    // ------------------------------------------------------------------

    /// The call sheet. Returns true when a play has been chosen.
    fn call_sheet(&mut self) -> bool {
        loop {
            let lines = self.call_sheet_lines();
            if !self.view.page(&lines) {
                self.quit = true;
                return false;
            }
            let input = self.wait(Duration::from_millis(16));
            if input.pressed.start {
                self.quit = true;
                return false;
            }
            if input.pressed.b {
                self.pick = (self.pick + 1) % self.book.len();
            }
            if input.pressed.x {
                self.pick = (self.pick + self.book.len() - 1) % self.book.len();
            }
            if input.pressed.y {
                // "Ask the coach": let the AI call it.
                let mut rng = Rng::new(self.game.seed ^ 0x51CE);
                self.pick = rng.index(self.book.len());
            }
            if input.pressed.a {
                return true;
            }
        }
    }

    fn call_sheet_lines(&self) -> Vec<String> {
        let mut lines = vec![
            self.header(),
            String::new(),
            format!(
                "  CALL SHEET - {} / {}",
                self.book[self.pick].family.label(),
                self.book[self.pick].formation.label()
            ),
            String::new(),
        ];
        let window = 11usize;
        let len = self.book.len();
        let start = self.pick.saturating_sub(window / 2).min(len.saturating_sub(window));
        for (i, play) in self.book.iter().enumerate().skip(start).take(window) {
            let marker = if i == self.pick { ">>" } else { "  " };
            lines.push(format!(
                "{marker} {:>2}. {:<16} {:<9} {}",
                i + 1,
                play.name,
                play.family.label(),
                play.formation.label()
            ));
        }
        lines.push(String::new());
        if matches!(self.game.action(), Action::CallPlay { fourth_down: true }) {
            lines.push("  4TH DOWN - go for it, or B on the next prompt to throw it off".to_string());
        }
        lines.push("  [B] NEXT   [X] PREVIOUS   [Y] ASK COACH   [A] SNAP   [START] QUIT".to_string());
        if let Some(last) = self.game.recent_plays(1).pop() {
            lines.push(String::new());
            lines.push(format!("  Last: {last}"));
        }
        lines
    }

    fn choose_points(&mut self) -> bool {
        // The second overtime round forces two-point tries.
        let mut two = self.game.conversion_forced_two();
        loop {
            let lines = vec![
                self.header(),
                String::new(),
                "  CONVERSION".to_string(),
                String::new(),
                format!("    {} 1 point from the 5", if two { "  " } else { ">>" }),
                format!("    {} 2 points from the 10", if two { ">>" } else { "  " }),
                String::new(),
                "  [B] TOGGLE   [A] CONFIRM".to_string(),
            ];
            if !self.view.page(&lines) {
                self.quit = true;
                return two;
            }
            let input = self.wait(Duration::from_millis(16));
            if input.pressed.b {
                two = !two;
            }
            if input.pressed.a || input.pressed.start {
                return two;
            }
        }
    }

    /// Fourth down: go for it, or use the A7FL throw-off in place of a punt.
    fn ask_throw_off(&mut self) -> bool {
        let mut throw_off = false;
        loop {
            let lines = vec![
                self.header(),
                String::new(),
                "  FOURTH DOWN - no punting in this league".to_string(),
                String::new(),
                format!("    {} GO FOR IT", if throw_off { "  " } else { ">>" }),
                format!("    {} THROW IT OFF", if throw_off { ">>" } else { "  " }),
                String::new(),
                "  [B] TOGGLE   [A] CONFIRM".to_string(),
            ];
            if !self.view.page(&lines) {
                self.quit = true;
                return false;
            }
            let input = self.wait(Duration::from_millis(16));
            if input.pressed.b {
                throw_off = !throw_off;
            }
            if input.pressed.a || input.pressed.start {
                return throw_off;
            }
        }
    }

    // ------------------------------------------------------------------
    // The snap itself
    // ------------------------------------------------------------------

    fn run_snap(&mut self) {
        let play = self.book[self.pick].clone();
        let def = self.cpu_defense();
        let header = self.header();
        let play_line = format!(
            "{} - {} - {} shows {}",
            play.name,
            play.formation.label(),
            self.game.teams[self.game.defense()].abbr,
            def.name
        );
        let off_abbr = self.game.teams[self.game.offense()].abbr.clone();

        // Borrow the fields separately: the game owns the snap, the rest of the
        // struct drives the picture and reads the controls.
        let game = &mut self.game;
        let broadcast = &mut self.broadcast;
        let view = &mut self.view;
        let pad = &mut self.pad;
        let target = &mut self.target;
        let quit = &mut self.quit;
        let flip_y = self.flip_y;

        let record = game.snap_with(play, def, |sim| {
            let finished = drive(
                sim,
                broadcast,
                view,
                pad.as_mut(),
                target,
                flip_y,
                &header,
                &play_line,
                &off_abbr,
            );
            if !finished {
                *quit = true;
            }
        });

        if let Some(record) = record {
            let text = record.description().to_string();
            self.show_result(&text, None);
        }
    }

    fn cpu_defense(&self) -> DefCall {
        let book = &self.defbook;
        let situation = ai::Situation::from_game(&self.game);
        if situation.yards_to_goal <= 5.0 {
            if let Some(call) = book.iter().find(|d| d.name == "GOAL LINE BEAR") {
                return call.clone();
            }
        }
        if situation.down >= 3 && situation.to_go <= 3.0 {
            if let Some(call) = book.iter().find(|d| d.name == "MIKE BLITZ") {
                return call.clone();
            }
        }
        if situation.down >= 3 && situation.to_go >= 8.0 {
            if let Some(call) = book.iter().find(|d| d.name == "COVER 3 ZONE") {
                return call.clone();
            }
        }
        let mut rng = Rng::new(self.game.seed ^ 0xD37E_C0DE);
        book[rng.index(book.len())].clone()
    }

    // ------------------------------------------------------------------
    // Screens
    // ------------------------------------------------------------------

    fn header(&self) -> String {
        format!(
            "{} {} - {} {}   {} {}   {}   BALL ON {}",
            self.game.teams[0].abbr,
            self.game.score[0],
            self.game.teams[1].abbr,
            self.game.score[1],
            self.game.quarter_text(),
            self.game.clock_text(),
            self.game.down_and_distance(),
            a7fl_core::field::ball_on_label(self.game.yards_to_goal())
        )
    }

    fn show_result(&mut self, text: &str, points: Option<u32>) {
        let mut lines = vec![self.header(), String::new()];
        if let Some(points) = points {
            let label = if points == 2 { "POINTS" } else { "POINT" };
            lines.push(format!("  CONVERSION - {points} {label}"));
            lines.push(String::new());
        }
        lines.push(format!("  {}", text.to_uppercase()));
        lines.push(String::new());
        for line in self.game.recent_plays(3) {
            lines.push(format!("  · {line}"));
        }
        lines.push(String::new());
        lines.push("  [A] CONTINUE".to_string());
        if !self.view.page(&lines) {
            self.quit = true;
            return;
        }

        let start = Instant::now();
        loop {
            let input = self.wait(Duration::from_millis(24));
            if input.pressed.a || input.pressed.start {
                break;
            }
            if start.elapsed() >= RESULT_HOLD {
                break;
            }
        }
    }

    fn show_final(&mut self) {
        let winner = self.game.winner.unwrap_or(0);
        let mut lines = vec![
            String::new(),
            format!("  FINAL   {}", self.game.teams[winner].display_upper()),
            String::new(),
            format!(
                "  {} {}  -  {} {}",
                self.game.teams[0].abbr,
                self.game.score[0],
                self.game.teams[1].abbr,
                self.game.score[1]
            ),
            String::new(),
        ];
        let book = &self.game.box_score;
        for side in 0..2 {
            let t = &book.teams[side];
            lines.push(format!(
                "  {:<4} {:.0} yds   {}/{} pass {:.0}   {} rush {:.0}   {} TO   {} 1st",
                self.game.teams[side].abbr,
                t.total_yards,
                t.pass_comp,
                t.pass_att,
                t.pass_yards,
                t.rush_att,
                t.rush_yards,
                t.turnovers,
                t.first_downs
            ));
        }
        lines.push(String::new());
        for line in self.game.recent_plays(5) {
            lines.push(format!("  · {line}"));
        }
        lines.push(String::new());
        lines.push("  [A] EXIT".to_string());
        if !self.view.page(&lines) {
            return;
        }
        loop {
            let input = self.wait(Duration::from_millis(30));
            if input.pressed.a || input.pressed.start {
                break;
            }
        }
    }

    fn input(&mut self) -> Input {
        pad::mix(self.pad.as_mut(), self.view.poll())
    }

    fn wait(&mut self, gap: Duration) -> Input {
        std::thread::sleep(gap);
        self.input()
    }
}

/// Run one snap live: simulate in fixed steps, render at a steady rate and let the
/// player steer the quarterback until the play resolves. Returns false when the
/// player has quit.
#[allow(clippy::too_many_arguments)]
fn drive(
    sim: &mut PlaySim,
    broadcast: &mut Broadcast,
    view: &mut View,
    mut pad: Option<&mut Pad>,
    target: &mut usize,
    flip_y: bool,
    header: &str,
    play_line: &str,
    off_abbr: &str,
) -> bool {
    let mut acc = 0.0f64;
    let mut last = Instant::now();
    let mut drew = Instant::now() - FRAME;

    loop {
        let now = Instant::now();
        acc += now.duration_since(last).as_secs_f64();
        last = now;

        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.start {
            return false;
        }

        let targets = sim.eligible_targets();
        if !targets.is_empty() {
            if input.pressed.b {
                *target = (*target + 1) % targets.len();
            } else if input.pressed.x {
                *target = (*target + targets.len() - 1) % targets.len();
            }
        }
        let locked = if targets.is_empty() { None } else { Some(targets[*target % targets.len()]) };
        // The arcade camera looks across the field, so screen-right is world +x
        // and screen-up is world -y. Feed the stick through that mapping: handed
        // in raw, up and down ran backwards.
        let vertical = if flip_y { input.stick.y } else { -input.stick.y };
        let stick = Vec2::new(input.stick.x, vertical);
        sim.set_control(Some(QbControl {
            move_dir: stick,
            target: locked.map(|(_, index)| index),
            throw: input.pressed.a || input.pressed.throw,
        }));

        while acc >= STEP as f64 && sim.result.is_none() {
            sim.step(STEP);
            acc -= STEP as f64;
        }

        if now.duration_since(drew) >= FRAME || sim.result.is_some() {
            drew = now;
            let mut frame = broadcast.live_frame(sim, CameraMode::Broadcast);
            let mut lines = vec![header.to_string(), play_line.to_string()];

            if let Some((role, index)) = locked {
                let name = broadcast.teams[sim.setup.offense_team].roster[index].full_name();
                let separation = sim.separation(index);
                let read = if separation > 3.0 {
                    "OPEN"
                } else if separation > 1.6 {
                    "TIGHT"
                } else {
                    "COVERED"
                };
                lines.push(format!(
                    "TARGET {:<3} {:<20} {:>4.1} yds  {read}",
                    role.label(),
                    name,
                    separation
                ));
            } else {
                lines.push("TARGET --".to_string());
            }
            lines.push(format!(
                "{off_abbr} ball   MOVE  TARGET  THROW  [START] QUIT"
            ));

            if !view.show_scaled(&mut frame, &lines) {
                return false;
            }
        }

        if sim.result.is_some() {
            // Let the last picture sit for a moment so the catch or the hit reads.
            std::thread::sleep(Duration::from_millis(260));
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

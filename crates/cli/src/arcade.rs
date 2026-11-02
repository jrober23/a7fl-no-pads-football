//! `a7fl arcade` — play the game live, in a window or in the terminal.
//!
//! The field is drawn by the same software renderer the broadcast package uses:
//! a real window when the machine has a display, ANSI colour blocks when it does
//! not. The side is yours: call a play from the layered call sheet, snap it yourself
//! at the line, audible out of it if the look is wrong, and then take the field on
//! defense when the other team has the ball. The clock, the downs and the rulebook
//! run as they do in `sim`.

use std::io::{self, Write};
use std::time::{Duration, Instant};

use a7fl_core::ai;
use a7fl_core::game::{Action, GamePhase, GameState, PlayRecord, ScrimmageRecord};
use a7fl_core::math::{Rng, Vec2, Vec3};
use a7fl_core::moves::Kind;
use a7fl_core::playbook::{
    def_calls_in, defensive_playbook, formations_in, fronts_in, playbook, plays_in, DefCall,
    DefFront, DefRole, Formation, Play,
};
use a7fl_core::rules::{FoulTeam, THROW_OFF_MIN_AIR_YARDS};
use a7fl_core::sim::{DefControl, Phase, PlaySim, QbControl};
use a7fl_core::throwoff::{power_for_air, ThrowOffSetup, ThrowOffSim, ThrowPhase, ThrowRole};
use a7fl_render::broadcast::{Broadcast, CameraMode};
use a7fl_render::passing::{Badge, Guide, PassUi, Tone};
use a7fl_render::callsheet;
use a7fl_render::defsheet;
use a7fl_render::playsheet::Dir;
use a7fl_render::terminal;

use crate::pad::{self, Input, Pad};
use crate::view::{View, CANVAS_H, CANVAS_W};

/// Picture refresh for live play. The field is drawn at the full canvas size, so
/// what you see is the renderer's real detail rather than a scaled-up quarter
/// image; when a frame costs more than this the loop renders as fast as it can
/// instead of sleeping on the clock.
const FRAME: Duration = Duration::from_millis(16);
const STEP: f32 = 1.0 / 60.0;
/// How long a result stays on screen before it moves on by itself.
const RESULT_HOLD: Duration = Duration::from_millis(2600);
/// How long the picture holds on the dead ball before the card comes up: long
/// enough for the tackle to finish, the men to land, and the word over the picture
/// to be read. The play used to cut to the card a quarter of a second after the
/// hit, which put the result screen over a tackle nobody had seen.
const DEAD_BALL_HOLD: Duration = Duration::from_millis(1500);
/// How long before [A] can move it on: past the hit, not past the replay.
const DEAD_BALL_SKIP: Duration = Duration::from_millis(520);
/// How long the pass meter takes to fill from nothing to a full-weight throw.
const CHARGE_TIME: f32 = 1.05;
/// Even a tap throws the ball; it just does not throw it far.
const MIN_POWER: f32 = 0.18;
/// How far the aim stick can bend a pass off the man it is aimed at.
const MAX_LEAD_YARDS: f32 = 7.5;
/// The four passing buttons, in the order the badges are drawn.
const PASS_LABELS: [&str; 4] = ["A", "B", "X", "Y"];
/// The same four buttons once the ball is in a runner's hands.
const MOVES: [Kind; 4] = [Kind::Spin, Kind::Hurdle, Kind::Dive, Kind::Slide];
/// How far across the field a throw-off can be aimed: the width of the landing
/// zone the returner has to work in.
const THROW_OFF_AIM_YARDS: f32 = 13.0;
/// How fast the aim marker sweeps across the field while the stick is held.
const AIM_SWEEP: f32 = 26.0;
/// How long the thrower waits before the ball goes when the computer is throwing
/// it at a human returner. Long enough to see the field and find the ball.
const WATCH_SNAP_DELAY: f32 = 2.6;

/// The views the player can switch between while the ball is live.
const CAMS: [CameraMode; 3] = [
    CameraMode::Broadcast,
    CameraMode::Classic,
    CameraMode::SkyCam,
];

fn cam_name(mode: CameraMode) -> &'static str {
    match mode {
        CameraMode::Classic => "CLASSIC",
        CameraMode::SkyCam => "SKY",
        _ => "BROADCAST",
    }
}

/// The stick, in screen space, turned into the world axes the sim wants.
///
/// The broadcast camera sits on the sideline and looks across the field, so up the
/// screen is across the field and right is down it, whichever way the team is
/// going -- that is the mapping this file used for every camera, and it is only
/// right for one of the three. Classic and Sky stand behind the ball and look down
/// the field instead, where the long axis runs up the screen: the same stick sent
/// the quarterback sideways and drove him backwards whenever he changed ends.
///
/// `dir` is the way the shot points in world x, which is the offense's direction of
/// travel, and the two axes swap roles around it.
fn stick_to_world(stick: Vec2, flip_y: bool, mode: CameraMode, dir: i8) -> Vec2 {
    // `flip_y` is for pads that report the stick's Y the other way round.
    let up = if flip_y { -stick.y } else { stick.y };
    match mode {
        // Behind the ball: up the screen is up the field, right is his right.
        CameraMode::Classic | CameraMode::SkyCam => {
            let d = if dir >= 0 { 1.0 } else { -1.0 };
            Vec2::new(d * up, d * stick.x)
        }
        // Sideline: the long axis runs across the screen either way.
        _ => Vec2::new(stick.x, -up),
    }
}

/// Sweep the throw-off aim marker across the field.
///
/// The lens sits behind the ball looking downfield in all three cameras, so the
/// field's width runs left and right across the screen: the stick's horizontal
/// axis is the one that moves the marker, and it stops at the sideline.
fn aim_step(aim_y: f32, stick_x: f32, dt: f32) -> f32 {
    if stick_x.abs() <= 0.20 {
        return aim_y;
    }
    (aim_y + stick_x * AIM_SWEEP * dt).clamp(-THROW_OFF_AIM_YARDS, THROW_OFF_AIM_YARDS)
}

/// The stick's horizontal axis turned into the world `y` that the throw-off lens
/// shows as right.
///
/// The lens looks down the throwing team's direction, so screen right is world `+y`
/// for one team and world `-y` for the other. The stick used to be applied raw, so
/// for one of the two teams the marker walked away from the push: push left, aim
/// right. The readout under the picture already knew which way the shot pointed,
/// which is why it and the marker could disagree.
fn aim_push(stick_x: f32, throw_dir: i8) -> f32 {
    stick_x * throw_dir as f32
}

/// The aim in the words the player sees it in: the marker is on the field, this
/// is the readout under the picture.
fn aim_readout(throw_dir: i8, aim_y: f32) -> String {
    // Screen right for a lens looking along `dir` is `dir` in world y.
    let across = throw_dir as f32 * aim_y;
    if across.abs() < 0.5 {
        "DOWN THE MIDDLE".to_string()
    } else {
        format!(
            "{:.0} YARDS {}",
            across.abs().round(),
            if across > 0.0 { "RIGHT" } else { "LEFT" }
        )
    }
}

/// How long the direction has to be held before the call sheet starts scrolling.
const REPEAT_DELAY: f32 = 0.34;
/// How fast it then scrolls.
const REPEAT_EVERY: f32 = 0.09;

/// Menu navigation off the stick: one step for the push, then repeats while it
/// is held, the way a call sheet is meant to feel.
#[derive(Default)]
struct Nav {
    x: i8,
    y: i8,
    held: f32,
}

impl Nav {
    fn step(&mut self, stick: Vec2, dt: f32) -> Option<Dir> {
        let x = axis(stick.x);
        let y = axis(stick.y);
        // The stick is round, so a diagonal has to pick one axis: vertical wins,
        // because moving between sets is the bigger jump.
        let (dx, dy) = if y != 0 { (0, y) } else { (x, 0) };
        if (dx, dy) != (self.x, self.y) {
            self.x = dx;
            self.y = dy;
            self.held = 0.0;
            // A fresh push is a fresh step, including when the stick was let go.
            return dir_of(dx, dy);
        }
        if (dx, dy) == (0, 0) {
            return None;
        }
        self.held += dt;
        if self.held >= REPEAT_DELAY {
            self.held -= REPEAT_EVERY;
            return dir_of(dx, dy);
        }
        None
    }
}

fn dir_of(dx: i8, dy: i8) -> Option<Dir> {
    match (dx, dy) {
        (0, d) if d > 0 => Some(Dir::Up),
        (0, d) if d < 0 => Some(Dir::Down),
        (d, _) if d > 0 => Some(Dir::Right),
        (d, _) if d < 0 => Some(Dir::Left),
        _ => None,
    }
}

fn axis(v: f32) -> i8 {
    if v > 0.5 {
        1
    } else if v < -0.5 {
        -1
    } else {
        0
    }
}

pub struct Arcade {
    game: GameState,
    pad: Option<Pad>,
    view: View,
    broadcast: Broadcast,
    book: Vec<Play>,
    defbook: Vec<DefCall>,
    /// Which formation the call sheet is sitting on, and which play inside it.
    form: usize,
    sheet_pick: usize,
    /// True while the stick is driving the play cards rather than the formations.
    in_plays: bool,
    /// Run the called play to the other side of the formation.
    flip: bool,
    /// The defensive call the player used last, kept as the default.
    def_pick: usize,
    /// The team the player is running. He is on that side of the ball both ways,
    /// so this is whichever side of the opening throw-off he is on.
    my_team: Option<usize>,
    /// Which of `CAMS` the picture is coming from.
    cam: usize,
    quit: bool,
    /// Penalty insurance: some pads report the stick's Y the other way round.
    flip_y: bool,
}

impl Arcade {
    pub fn new(game: GameState, pad: Option<Pad>, view: View) -> Self {
        let book = playbook();
        let defbook = defensive_playbook();
        let teams = [game.teams[0].clone(), game.teams[1].clone()];
        let broadcast = Broadcast::new(CANVAS_W, CANVAS_H, teams);
        Arcade {
            game,
            pad,
            view,
            broadcast,
            book,
            defbook,
            form: 0,
            sheet_pick: 0,
            in_plays: false,
            flip: false,
            def_pick: 0,
            my_team: None,
            cam: 0,
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
        if !self.kickoff_card() {
            return;
        }
        while !self.game.is_over() && !self.quit {
            match self.game.action() {
                Action::GameOver => break,
                Action::ThrowOff => {
                    if !self.play_throw_off(false) {
                        break;
                    }
                }
                Action::ChooseConversion => {
                    // Pick the try, then go back round: a conversion is a snap
                    // like any other, so it gets called and played, not rolled.
                    // Whose try it is decides who picks it: once the player can
                    // play defence, the other lot scoring is his business to stop,
                    // not his to call.
                    let mine = self.my_team.unwrap_or(self.game.possession);
                    let two = if self.game.possession == mine {
                        self.choose_points()
                    } else {
                        self.game.conversion_forced_two()
                    };
                    self.game.set_conversion(two);
                }
                Action::CallPlay { fourth_down } => {
                    let mine = self.my_team.unwrap_or(self.game.possession);
                    if self.game.possession != mine {
                        // Their ball. Call a defence, then go and play one of the
                        // seven instead of watching the coordinator do it.
                        if let Some(def) = self.def_sheet() {
                            self.run_defense_snap(def);
                        }
                        continue;
                    }
                    if fourth_down && self.ask_throw_off() {
                        if !self.play_throw_off(true) {
                            break;
                        }
                        continue;
                    }
                    if let Some(play) = self.call_sheet() {
                        self.run_snap(play);
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

    /// The call sheet, layered the way a coach's file is: the formations down the
    /// left, the plays of the highlighted formation on the cards.
    ///
    /// It used to be one list of twenty-four plays paged eight at a time, which is
    /// not a playbook anybody can read at the line. The first choice is now the
    /// formation and the second is a play inside it, and the flip toggle runs that
    /// play to the other side without a second card for it.
    fn call_sheet(&mut self) -> Option<Play> {
        let forms = formations_in(&self.book);
        if forms.is_empty() {
            return None;
        }
        self.form = self.form.min(forms.len() - 1);
        // Every visit starts in the formation column. It used to keep whichever
        // column the last call ended in, so after the first snap the stick was
        // already inside the plays and the formations could never be reached again
        // without finding B.
        self.in_plays = false;
        self.flip = false;
        let mut nav = Nav::default();
        let mut last = Instant::now();
        self.view.reset();
        loop {
            let now = Instant::now();
            let dt = now.duration_since(last).as_secs_f32().min(0.1);
            last = now;

            let forms = formations_in(&self.book);
            let list = plays_in(&self.book, forms[self.form]);
            self.sheet_pick = self.sheet_pick.min(list.len().saturating_sub(1));

            let mut frame = a7fl_render::Frame::new(CANVAS_W, CANVAS_H);
            callsheet::draw_layered(
                &mut frame,
                &self.book,
                &forms,
                self.form,
                self.sheet_pick,
                self.in_plays,
                self.flip,
            );
            let lines = self.call_sheet_lines(&forms, &list);
            if !self.view.show(&mut frame, &lines) {
                self.quit = true;
                return None;
            }
            let input = self.input();
            if input.pressed.start {
                // The call sheet is where a player spends most of the game, so this
                // is the door to the camera setting and to quitting, not an instant
                // exit the way it used to be.
                if !pause_menu(&mut self.view, self.pad.as_mut(), &mut self.cam) {
                    self.quit = true;
                    return None;
                }
                last = Instant::now();
                continue;
            }
            if input.pressed.y {
                self.flip = !self.flip;
            }
            if input.pressed.b {
                self.in_plays = false;
            }
            if let Some(dir) = nav.step(input.stick, dt) {
                if self.in_plays && dir == Dir::Left && self.sheet_pick % callsheet::COLS == 0 {
                    // Left off the first card is the way back to the formations, so
                    // a player who wandered into the plays is not stuck there.
                    self.in_plays = false;
                } else if self.in_plays {
                    self.sheet_pick = callsheet::step_in(list.len(), self.sheet_pick, dir);
                } else {
                    match dir {
                        Dir::Up => {
                            self.form = (self.form + forms.len() - 1) % forms.len();
                            self.sheet_pick = 0;
                        }
                        Dir::Down => {
                            self.form = (self.form + 1) % forms.len();
                            self.sheet_pick = 0;
                        }
                        Dir::Right => self.in_plays = true,
                        Dir::Left => {}
                    }
                }
            }
            if input.pressed.a {
                if self.in_plays {
                    let mut play = self.book[list[self.sheet_pick]].clone();
                    if self.flip {
                        play = play.flipped();
                    }
                    self.flip = false;
                    return Some(play);
                }
                self.in_plays = true;
            }
            std::thread::sleep(Duration::from_millis(8));
        }
    }

    fn call_sheet_lines(&self, forms: &[Formation], list: &[usize]) -> Vec<String> {
        let fallback = &self.book[0];
        let play = list.get(self.sheet_pick).map(|&i| &self.book[i]).unwrap_or(fallback);
        let side = if self.flip { "FLIPPED" } else { "AS DRAWN" };
        let mut lines = vec![
            self.header(),
            format!(
                "  {:<16} {}   {}   PLAY {} OF {} IN {}   {} FORMATIONS",
                play.name,
                play.family.label(),
                side,
                self.sheet_pick + 1,
                list.len(),
                forms.get(self.form).map(|f| f.label()).unwrap_or(""),
                forms.len()
            ),
        ];
        if self.in_plays {
            lines.push(
                "  [STICK] PLAY   [A] CALL IT   [B] BACK TO FORMATIONS   [Y] FLIP   [START] PAUSE"
                    .to_string(),
            );
        } else {
            lines.push(
                "  [STICK] FORMATION   [A]/[RIGHT] INTO THE PLAYS   [Y] FLIP   [START] PAUSE"
                    .to_string(),
            );
        }
        if let Some(last) = self.game.recent_plays(1).pop() {
            lines.push(format!("  Last: {last}"));
        }
        lines
    }

    /// A question with a few answers: left, right, up and down on the stick move
    /// the highlight, A takes it. Returns `None` when the player quit.
    ///
    /// These screens used to flash between two options on B, so a player reaching
    /// for the d-pad — the thing that moves a highlight everywhere else in the
    /// game — got nothing back.
    fn ask(&mut self, title: &str, note: &[&str], options: &[String], start: usize) -> Option<usize> {
        let mut nav = Nav::default();
        let mut pick = start.min(options.len().saturating_sub(1));
        let mut last = Instant::now();
        // A button held from the last screen must not answer this one.
        self.view.reset();
        loop {
            let now = Instant::now();
            let dt = now.duration_since(last).as_secs_f32().min(0.1);
            last = now;
            let mut lines = vec![self.header(), String::new(), format!("  {title}"), String::new()];
            for (n, option) in options.iter().enumerate() {
                lines.push(format!("    {} {}", if n == pick { ">>" } else { "  " }, option));
            }
            lines.push(String::new());
            for line in note {
                lines.push(format!("  {line}"));
            }
            lines.push(String::new());
            lines.push("  [STICK] CHOOSE   [A] CONFIRM   [START] PAUSE".to_string());
            if !self.view.page(&lines) {
                self.quit = true;
                return None;
            }
            let input = self.input();
            if input.pressed.start {
                if !pause_menu(&mut self.view, self.pad.as_mut(), &mut self.cam) {
                    self.quit = true;
                    return None;
                }
                last = Instant::now();
                continue;
            }
            // Up and down always work, and left and right do too: with only two
            // answers standing side by side in some of these, either reads right.
            let count = options.len();
            match nav.step(input.stick, dt) {
                Some(Dir::Up | Dir::Left) if count > 1 => pick = (pick + count - 1) % count,
                Some(Dir::Down | Dir::Right) if count > 1 => pick = (pick + 1) % count,
                _ => {}
            }
            if input.pressed.a {
                return Some(pick);
            }
            std::thread::sleep(Duration::from_millis(16));
        }
    }

    fn choose_points(&mut self) -> bool {
        // The second overtime round forces two-point tries.
        let forced = self.game.conversion_forced_two();
        if forced {
            let options = vec!["2 POINTS - SNAP FROM THE 10".to_string()];
            return self
                .ask(
                    "CONVERSION",
                    &["Overtime: the two-point try is the only one on offer."],
                    &options,
                    0,
                )
                .is_some();
        }
        let options = vec![
            "1 POINT  - SNAP FROM THE 5".to_string(),
            "2 POINTS - SNAP FROM THE 10".to_string(),
        ];
        match self.ask(
            "CONVERSION",
            &["The A7FL lets the offense pick the try, and it is locked in."],
            &options,
            0,
        ) {
            Some(pick) => pick == 1,
            None => false,
        }
    }

    /// Fourth down: go for it, or use the A7FL throw-off in place of a punt.
    fn ask_throw_off(&mut self) -> bool {
        let options = vec![
            "GO FOR IT  - SNAP THE FOURTH DOWN".to_string(),
            "THROW IT OFF - THE THROW-OFF REPLACES THE PUNT".to_string(),
        ];
        match self.ask(
            "FOURTH DOWN",
            &["No punting in this league. The throw-off hands the ball back from the 35."],
            &options,
            0,
        ) {
            Some(pick) => pick == 1,
            None => false,
        }
    }

    /// The card in front of the first snap of the game. The client used to drop a
    /// player straight into a throw-off that was already in the air, so this holds
    /// the kickoff until he has seen the matchup and knows what the pad does.
    fn kickoff_card(&mut self) -> bool {
        let (receiving, from_own) = match self.game.phase {
            GamePhase::ThrowOff { receiving, from_own } => (receiving, from_own),
            _ => return true,
        };
        let throwing = 1 - receiving;
        // Which side is the player's, and which is the computer's. The client used
        // to hand him the receiving team without asking, which is how a player ends
        // up on defense not knowing whose shirt he is wearing.
        let mut pick = receiving;
        let mut nav = Nav::default();
        let mut last = Instant::now();
        self.view.reset();
        loop {
            let now = Instant::now();
            let dt = now.duration_since(last).as_secs_f32().min(0.1);
            last = now;
            let names = [
                self.game.teams[0].display_upper(),
                self.game.teams[1].display_upper(),
            ];
            let lines = vec![
                String::new(),
                "  A7FL NO PADS FOOTBALL".to_string(),
                String::new(),
                format!("  {} AT {}", names[throwing], names[receiving]),
                String::new(),
                format!(
                    "  {} {:<12} {}",
                    if pick == 0 { ">" } else { " " },
                    names[0],
                    if pick == 0 { "YOU" } else { "THE COMPUTER" }
                ),
                format!(
                    "  {} {:<12} {}",
                    if pick == 1 { ">" } else { " " },
                    names[1],
                    if pick == 1 { "YOU" } else { "THE COMPUTER" }
                ),
                String::new(),
                format!(
                    "  TO OPEN: {} THROWS IT OFF FROM ITS OWN {}",
                    self.game.teams[throwing].abbr, from_own as i32
                ),
                if pick == throwing {
                    "  YOU THROW THE FIRST ONE OFF, THEN DEFEND THE RETURN".to_string()
                } else {
                    "  YOU RECEIVE THE FIRST ONE AND TAKE THE FIELD ON OFFENSE".to_string()
                },
                String::new(),
                "  THROW-OFF: STICK AIMS, HOLD A TO LOAD POWER, LET GO TO THROW".to_string(),
                "  RETURN: STICK RUNS, [X] CHANGES CAMERA, [START] PAUSES".to_string(),
                String::new(),
                "  [STICK] CHOOSE YOUR TEAM   [A] KICK OFF".to_string(),
            ];
            if !self.view.page(&lines) {
                self.quit = true;
                return false;
            }
            let input = self.input();
            if input.pressed.start {
                if !pause_menu(&mut self.view, self.pad.as_mut(), &mut self.cam) {
                    self.quit = true;
                    return false;
                }
                last = Instant::now();
                continue;
            }
            // Two sides, so any push of the stick swaps them.
            if nav.step(input.stick, dt).is_some() {
                pick = 1 - pick;
            }
            if input.pressed.a {
                self.my_team = Some(pick);
                return true;
            }
            std::thread::sleep(Duration::from_millis(24));
        }
    }

    // ------------------------------------------------------------------
    // Throw-offs
    // ------------------------------------------------------------------

    /// Play a throw-off: throw it off if it is yours to throw, or get under it
    /// and bring it back if it is not. Returns false when the player quit.
    fn play_throw_off(&mut self, free_throw: bool) -> bool {
        let Some(setup) = self.throw_off_to_play(free_throw) else {
            return true;
        };
        // The player runs the offense, so a throw-off is his to throw exactly
        // when his team is the one throwing it: after he scores, he throws it
        // off; after the other lot score, he receives it.
        let my_team = self.my_team.unwrap_or(setup.receiving_team);
        self.my_team = Some(my_team);
        let header = self.header();

        // The sim borrows the two teams, so hand it its own copies and leave the
        // rest of the struct free to draw the picture.
        let throwing = self.game.teams[setup.throwing_team].clone();
        let receiving = self.game.teams[setup.receiving_team].clone();
        let mut sim = ThrowOffSim::new(&setup, &throwing, &receiving);
        sim.human_thrower = setup.throwing_team == my_team;
        if !sim.human_thrower {
            // The ball is coming at you: give the man on the pad a moment to see
            // the field before it goes.
            sim.snap_delay = WATCH_SNAP_DELAY;
        }

        let finished = self.drive_throw_off(&mut sim, my_team, &header);
        if !finished {
            return false;
        }
        if let Some(result) = sim.result.clone() {
            let rec = self.game.finish_throw_off(setup, result);
            let text = rec.description().to_string();
            self.show_result(&text, None);
        }
        true
    }

    /// The throw-off the game is waiting on, set up but not run.
    fn throw_off_to_play(&mut self, free_throw: bool) -> Option<ThrowOffSetup> {
        if free_throw {
            return self.game.fourth_down_throw_off_setup();
        }
        match &self.game.phase {
            GamePhase::ThrowOff { receiving, from_own } => {
                Some(self.game.throw_off_setup(*receiving, *from_own, false))
            }
            _ => None,
        }
    }

    /// The live throw-off. Returns false when the player has quit.
    fn drive_throw_off(&mut self, sim: &mut ThrowOffSim, my_team: usize, header: &str) -> bool {
        let mut acc = 0.0f64;
        let mut last = Instant::now();
        let mut drew = Instant::now() - FRAME;
        // The meter, filled while the throw button is held.
        let mut hold = 0.0f32;
        let mut charging = false;
        // Where across the field the throw is aimed, in yards.
        let mut aim_y = 0.0f32;
        // The throw button has been up since this throw-off started.
        let mut armed = false;
        // Which of your men the switch button has walked to.
        let mut cover_pick = 0usize;
        // When the ball died, so the picture can hold on the tackle before the card.
        let mut dead_at: Option<Instant> = None;

        loop {
            let now = Instant::now();
            let frame_dt = now.duration_since(last).as_secs_f64();
            acc += frame_dt;
            last = now;

            let input = pad::mix(self.pad.as_mut(), self.view.poll());
            if input.pressed.start {
                // START pauses: the camera setting and quitting both live behind it.
                if !pause_menu(&mut self.view, self.pad.as_mut(), &mut self.cam) {
                    self.quit = true;
                    return false;
                }
                last = Instant::now();
                acc = 0.0;
                continue;
            }
            // The trigger cycles the lens while you are covering or returning.
            // While you are throwing it is the throw itself, so the camera lives
            // in the pause menu instead -- X is the dive now.
            let throwing = sim.phase == ThrowPhase::Presnap && sim.human_thrower;
            if input.pressed.throw && !throwing {
                self.cam = (self.cam + 1) % CAMS.len();
            }
            // The same screen mapping as a scrimmage snap, pointed the way this
            // shot points: downfield with the throw, then behind the returner once
            // the ball is his.
            let cam_dir = if matches!(
                sim.phase,
                ThrowPhase::Loose | ThrowPhase::Return | ThrowPhase::Dead
            ) {
                sim.setup.receive_direction
            } else {
                sim.setup.throwing_direction()
            };
            let stick = stick_to_world(input.stick, self.flip_y, CameraMode::Classic, cam_dir);

            if sim.phase == ThrowPhase::Presnap && sim.human_thrower {
                // Three beats, the way a kickoff is played: pick the line the ball
                // will take, hold A to load the arm, let go to throw it. The throw
                // waits for the man on the pad, so there is no clock to beat.
                let holding = input.held.a || input.held.throw;
                // A still held from the kickoff card must not load the arm: the
                // button has to come up once before it means anything here.
                if !holding {
                    armed = true;
                }
                if holding && armed {
                    charging = true;
                    hold = (hold + frame_dt as f32).min(CHARGE_TIME);
                } else if charging && !holding {
                    let power = (hold / CHARGE_TIME).clamp(MIN_POWER, 1.0);
                    sim.launch_with(power, aim_y);
                    charging = false;
                    hold = 0.0;
                }
                // The line is set before the arm comes back and locked there.
                if !charging {
                    let push = aim_push(input.stick.x, sim.setup.throwing_direction());
                    aim_y = aim_step(aim_y, push, frame_dt as f32);
                }
            }

            // Whoever is yours: the returner while you are receiving, otherwise
            // one of the three men in coverage, and B walks the pad round them.
            if input.pressed.b && sim.setup.throwing_team == my_team {
                cover_pick = cover_pick.wrapping_add(1);
            }
            let mine = human_man(sim, my_team, cover_pick);
            sim.set_human(mine, stick);
            // On the cover: A wraps him up, X dives at him from further out.
            if sim.phase == ThrowPhase::Return && sim.setup.throwing_team == my_team {
                if input.pressed.a {
                    sim.call_tackle(false);
                }
                if input.pressed.x {
                    sim.call_tackle(true);
                }
            }

            // Fair catch: the receiving player calls it while the ball is in the
            // air and takes it where it comes down, instead of running it out.
            if sim.phase == ThrowPhase::Flight
                && sim.setup.receiving_team == my_team
                && input.pressed.y
            {
                sim.call_fair_catch();
            }

            while acc >= STEP as f64 && sim.result.is_none() {
                sim.step(STEP);
                acc -= STEP as f64;
            }

            if now.duration_since(drew) >= FRAME || sim.result.is_some() {
                drew = now;
                let mut guide = None;
                let mut short = false;
                let mut needed = None;
                let mut power = None;
                if sim.phase == ThrowPhase::Presnap && sim.human_thrower {
                    let p = (hold / CHARGE_TIME).clamp(0.0, 1.0);
                    let plan = sim.throw_off_plan(p, aim_y);
                    short = plan.too_short;
                    // The notch is the 40-yard rule: under it is a Run Back.
                    needed = Some(power_for_air(THROW_OFF_MIN_AIR_YARDS));
                    power = Some(p);
                    let hand = sim.players[0].pos;
                    guide = Some(Guide {
                        from: Vec3::new(hand.x, hand.y, 1.8),
                        receiver: Vec3::new(plan.landing.x, plan.landing.y, 0.12),
                        landing: Vec3::new(plan.landing.x, plan.landing.y, 0.12),
                    });
                }
                let ui = PassUi {
                    badges: Vec::new(),
                    guide,
                    power,
                    short,
                    needed,
                };
                let mode = CAMS[self.cam % CAMS.len()];
                let mut frame = self.broadcast.throw_off_frame_live(sim, mode, &ui);
                let lines = vec![
                    header.to_string(),
                    throw_off_line(self, sim, my_team, aim_y, charging),
                    throw_off_controls(self, sim, my_team, cover_pick),
                ];
                if !self.view.show(&mut frame, &lines) {
                    self.quit = true;
                    return false;
                }
            }

            if sim.result.is_some() {
                // The ball is dead; the picture is not. A return that ends in a
                // tackle gets the hit and the landing before the card.
                let since = *dead_at.get_or_insert(now);
                let waited = since.elapsed();
                if waited >= DEAD_BALL_HOLD || (waited >= DEAD_BALL_SKIP && input.pressed.a) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    // ------------------------------------------------------------------
    // The snap itself
    // ------------------------------------------------------------------

    fn run_snap(&mut self, play: Play) {
        // Playing a snap is what tells us whose team this is, the first time.
        if self.my_team.is_none() {
            self.my_team = Some(self.game.possession);
        }
        let def = self.cpu_defense();
        let header = self.header();
        let off_abbr = self.game.teams[self.game.offense()].abbr.clone();
        let book: &[Play] = &self.book;

        // Borrow the fields separately: the game owns the snap, the rest of the
        // struct drives the picture and reads the controls.
        let game = &mut self.game;
        let broadcast = &mut self.broadcast;
        let view = &mut self.view;
        let pad = &mut self.pad;
        let quit = &mut self.quit;
        let cam = &mut self.cam;
        let flip_y = self.flip_y;

        let record = game.snap_with(play, def, |sim| {
            // No automatic snap on offense: the ball goes when the player says so.
            sim.await_snap(true);
            let finished = drive(
                sim,
                broadcast,
                view,
                pad.as_mut(),
                flip_y,
                cam,
                &header,
                &off_abbr,
                book,
            );
            if !finished {
                *quit = true;
            }
        });

        if let Some(record) = record {
            self.report(record);
        }
    }

    /// Their ball: call a defence, then put the pad on one of the seven.
    fn run_defense_snap(&mut self, def: DefCall) {
        let play = self.cpu_offense();
        let header = self.header();
        let game = &mut self.game;
        let broadcast = &mut self.broadcast;
        let view = &mut self.view;
        let pad = &mut self.pad;
        let quit = &mut self.quit;
        let cam = &mut self.cam;
        let flip_y = self.flip_y;

        let record = game.snap_with(play, def, |sim| {
            sim.set_human_defender(sim.nearest_defender());
            let finished = drive_defense(sim, broadcast, view, pad.as_mut(), flip_y, cam, &header);
            if !finished {
                *quit = true;
            }
        });

        if let Some(record) = record {
            self.report(record);
        }
    }

    /// The card after a snap: a flag gets its own read-out, everything else the
    /// score.
    fn report(&mut self, record: PlayRecord) {
        match &record {
            PlayRecord::Scrimmage(r) if r.result.penalty.is_some() => {
                self.penalty_card(r);
            }
            _ => {
                let text = record.description().to_string();
                self.show_result(&text, None);
            }
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

    /// What the computer runs at a player who is playing defence: down and distance
    /// first, then a coin toss, so a third and long is not a draw play.
    fn cpu_offense(&self) -> Play {
        let situation = ai::Situation::from_game(&self.game);
        let mut rng = Rng::new(self.game.seed ^ 0x0FFE_115E);
        let want_pass = if situation.down >= 3 && situation.to_go > 4.0 {
            true
        } else if situation.yards_to_goal <= 3.0 || situation.to_go <= 2.0 {
            false
        } else {
            rng.chance(0.55)
        };
        let pool: Vec<&Play> = self
            .book
            .iter()
            .filter(|p| p.family.is_pass() == want_pass)
            .collect();
        if pool.is_empty() {
            return self.book[rng.index(self.book.len())].clone();
        }
        pool[rng.index(pool.len())].clone()
    }

    /// The defensive call sheet. One list is enough here: the player is picking a
    /// front and a coverage, not a file of plays.
    fn def_sheet(&mut self) -> Option<DefCall> {
        let fronts = fronts_in(&self.defbook);
        if fronts.is_empty() {
            return None;
        }
        let start = self.def_pick.min(self.defbook.len() - 1);
        let mut front = fronts
            .iter()
            .position(|f| *f == self.defbook[start].front)
            .unwrap_or(0);
        let mut pick = def_calls_in(&self.defbook, fronts[front])
            .iter()
            .position(|&i| i == start)
            .unwrap_or(0);
        // The sheet opens in the front column, the way the offensive one does.
        let mut in_calls = false;
        let mut nav = Nav::default();
        let mut last = Instant::now();
        self.view.reset();
        loop {
            let now = Instant::now();
            let dt = now.duration_since(last).as_secs_f32().min(0.1);
            last = now;
            let list = def_calls_in(&self.defbook, fronts[front]);
            pick = pick.min(list.len().saturating_sub(1));

            let mut frame = a7fl_render::Frame::new(CANVAS_W, CANVAS_H);
            defsheet::draw_layered(&mut frame, &self.defbook, &fronts, front, pick, in_calls);
            let lines = self.def_sheet_lines(fronts[front], &list, pick, in_calls);
            if !self.view.show(&mut frame, &lines) {
                self.quit = true;
                return None;
            }
            let input = self.input();
            if input.pressed.start {
                if !pause_menu(&mut self.view, self.pad.as_mut(), &mut self.cam) {
                    self.quit = true;
                    return None;
                }
                last = Instant::now();
                continue;
            }
            if input.pressed.b {
                in_calls = false;
            }
            if let Some(dir) = nav.step(input.stick, dt) {
                if in_calls && dir == Dir::Left && pick % defsheet::COLS == 0 {
                    in_calls = false;
                } else if in_calls {
                    pick = callsheet::step_in(list.len(), pick, dir);
                } else {
                    match dir {
                        Dir::Up => {
                            front = (front + fronts.len() - 1) % fronts.len();
                            pick = 0;
                        }
                        Dir::Down => {
                            front = (front + 1) % fronts.len();
                            pick = 0;
                        }
                        Dir::Right => in_calls = true,
                        Dir::Left => {}
                    }
                }
            }
            if input.pressed.a {
                if in_calls {
                    let index = list[pick];
                    self.def_pick = index;
                    return Some(self.defbook[index].clone());
                }
                in_calls = true;
            }
            std::thread::sleep(Duration::from_millis(8));
        }
    }

    /// The lines under the defensive sheet: the call, its front and techniques,
    /// and what each of the seven has been told to do.
    fn def_sheet_lines(
        &self,
        front: DefFront,
        list: &[usize],
        pick: usize,
        in_calls: bool,
    ) -> Vec<String> {
        let fallback = &self.defbook[0];
        let call = list.get(pick).map(|&i| &self.defbook[i]).unwrap_or(fallback);
        let jobs: Vec<String> = DefRole::ALL
            .iter()
            .map(|r| format!("{} {}", role_tag(*r), call.assignment(*r)))
            .collect();
        let mut lines = vec![
            self.header(),
            format!(
                "  {:<14} {:<9} {:<14} {} RUSH",
                call.name,
                call.coverage.label(),
                format!("{} {}", front.techs(), front.line_call()),
                call.rushers()
            ),
            format!("  {}", jobs.join("   ")),
        ];
        lines.push(if in_calls {
            "  [STICK] CALL   [A] TAKE IT   [B] BACK TO FRONTS   [START] PAUSE".to_string()
        } else {
            "  [STICK] FRONT   [A]/[RIGHT] INTO THE CALLS   [START] PAUSE".to_string()
        });
        lines
    }

    // ------------------------------------------------------------------
    // Screens
    // ------------------------------------------------------------------

    fn header(&self) -> String {
        // Whose team this is, on every screen that carries the score, so a player
        // never has to guess which shirt he is in.
        let mine = self
            .my_team
            .map(|t| format!("   YOU: {}", self.game.teams[t].abbr))
            .unwrap_or_default();
        format!(
            "{} {} - {} {}{}   {} {}   {}   BALL ON {}",
            self.game.teams[0].abbr,
            self.game.score[0],
            self.game.teams[1].abbr,
            self.game.score[1],
            mine,
            self.game.quarter_text(),
            self.game.clock_text(),
            self.game.down_and_distance(),
            a7fl_core::field::ball_on_label(self.game.yards_to_goal())
        )
    }

    /// The flag, read out after the play: what was called, on whom, how far it
    /// walked and the down it leaves behind. The picture carried the flag itself
    /// while the snap ran.
    fn penalty_card(&mut self, record: &ScrimmageRecord) {
        let Some(call) = record.result.penalty.clone() else {
            return;
        };
        let team = match call.against {
            FoulTeam::Offense => record.setup.offense_team,
            FoulTeam::Defense => record.setup.defense_team,
        };
        let side = match call.against {
            FoulTeam::Offense => "OFFENSE",
            FoulTeam::Defense => "DEFENSE",
        };
        let who = call
            .player
            .and_then(|i| self.game.teams[team].roster.get(i))
            .map(|p| p.last_name().to_string());
        let from = a7fl_core::field::yards_to_goal(record.setup.los, record.setup.direction);
        let mut lines = vec![
            self.header(),
            String::new(),
            "  FLAG".to_string(),
            String::new(),
            format!(
                "  {} - {} - {}",
                call.kind.label().to_uppercase(),
                self.game.teams[team].abbr,
                side
            ),
        ];
        if let Some(who) = who {
            lines.push(format!("  ON {who}"));
        }
        lines.push(format!("  {} YARDS WALKED OFF", record.penalty_yards));
        lines.push(format!(
            "  BALL MOVED FROM {} TO {}",
            a7fl_core::field::ball_on_label(from),
            a7fl_core::field::ball_on_label(self.game.yards_to_goal())
        ));
        lines.push(String::new());
        lines.push(format!("  {}", record.result.description.to_uppercase()));
        if let Some(sentence) = self.game.commentary.last().cloned() {
            lines.push(format!("  {sentence}"));
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
/// The pause screen. Returns false when the player chose to quit the game.
///
/// START used to close the game on the spot, which put the camera setting — the
/// one thing a player wants to change in the middle of a drive — out of reach
/// once the ball was snapped.
fn pause_menu(
    view: &mut View,
    mut pad: Option<&mut Pad>,
    cam: &mut usize,
) -> bool {
    const ITEMS: usize = 3;
    let mut nav = Nav::default();
    let mut pick = 0usize;
    let mut last = Instant::now();
    // A button still held from the game must not select the first item.
    view.reset();
    let keep = loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.start || input.pressed.b {
            break true;
        }
        match nav.step(input.stick, dt) {
            Some(Dir::Up) => pick = (pick + ITEMS - 1) % ITEMS,
            Some(Dir::Down) => pick = (pick + 1) % ITEMS,
            _ => {}
        }
        let mark = |on: bool| if on { ">>" } else { "  " };
        let lines = vec![
            String::new(),
            String::new(),
            "  PAUSED".to_string(),
            String::new(),
            format!("    {} RESUME", mark(pick == 0)),
            format!(
                "    {} CAMERA    {:<9}   [A] CHANGE",
                mark(pick == 1),
                cam_name(CAMS[*cam % CAMS.len()])
            ),
            format!("    {} QUIT GAME", mark(pick == 2)),
            String::new(),
            "  [STICK] CHOOSE   [A] SELECT   [START] RESUME".to_string(),
        ];
        if !view.page(&lines) {
            break false;
        }
        if input.pressed.a {
            match pick {
                0 => break true,
                1 => *cam = (*cam + 1) % CAMS.len(),
                _ => break false,
            }
        }
        std::thread::sleep(Duration::from_millis(16));
    };
    // Whatever was pressed in here must not carry into the game.
    view.reset();
    keep
}

fn drive(
    sim: &mut PlaySim,
    broadcast: &mut Broadcast,
    view: &mut View,
    mut pad: Option<&mut Pad>,
    flip_y: bool,
    cam: &mut usize,
    header: &str,
    off_abbr: &str,
    book: &[Play],
) -> bool {
    let mut acc = 0.0f64;
    let mut last = Instant::now();
    let mut drew = Instant::now() - FRAME;
    // The button being held for a pass: which receiver, and for how long. The
    // meter fills while it is down and the ball leaves when it comes up.
    let mut charge: Option<(usize, f32)> = None;
    // Where the aim stick has bent the throw, in yards off the receiver.
    let mut aim = Vec2::ZERO;
    // When the ball died, so the picture can hold on the tackle before the card.
    let mut dead_at: Option<Instant> = None;

    loop {
        let now = Instant::now();
        let frame_dt = now.duration_since(last).as_secs_f64();
        acc += frame_dt;
        last = now;

        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.start {
            if !pause_menu(view, pad.as_deref_mut(), cam) {
                return false;
            }
            // The whole pause was spent inside the wall clock: do not run the play
            // forward by it.
            last = Instant::now();
            acc = 0.0;
            continue;
        }
        // The trigger is free during a snap now that the face buttons pass, so it
        // cycles the camera.
        if input.pressed.throw {
            *cam = (*cam + 1) % CAMS.len();
        }
        let view_mode = CAMS[*cam % CAMS.len()];

        // Before the snap the pad belongs to the line, not to the quarterback: A
        // goes, X opens the audible, Y flips the call.
        if sim.phase == Phase::PreSnap {
            if input.pressed.y {
                sim.flip_play();
            }
            if input.pressed.x {
                match audible_sheet(view, pad.as_deref_mut(), book, &sim.play) {
                    Audible::Picked(play) => sim.call_play(play),
                    Audible::Back => {}
                    Audible::Quit => return false,
                }
                last = Instant::now();
                acc = 0.0;
                continue;
            }
            if input.pressed.a {
                sim.request_snap();
            }
        }
        let presnap = sim.phase == Phase::PreSnap;

        // Once the ball is in a runner's hands and there is no pass to throw, the
        // face buttons are his move set rather than the receivers'.
        let running = !presnap && sim.face_buttons_are_moves();
        let targets = if running || presnap {
            Vec::new()
        } else {
            sim.eligible_targets()
        };
        // Which way the stick points in the world depends on the shot on screen:
        // the sideline camera and the two behind-the-ball ones disagree about
        // which axis runs up it.
        let stick = stick_to_world(input.stick, flip_y, view_mode, sim.setup.direction);
        let held = [input.held.a, input.held.b, input.held.x, input.held.y];
        let down = [input.pressed.a, input.pressed.b, input.pressed.x, input.pressed.y];
        if running {
            // A pass wound up before he crossed the line is not a throw any more.
            charge = None;
            for (slot, kind) in MOVES.iter().enumerate() {
                if down[slot] {
                    sim.request_move(*kind);
                }
            }
        }

        // Holding a receiver's button winds the pass up. The stick bends it off
        // him while the meter fills, which is how you lead a man into grass or
        // put it on his back shoulder.
        if let Some((slot, held_for)) = charge {
            if slot >= targets.len() {
                charge = None;
            } else {
                if stick.length() > 0.30 {
                    aim = Vec2::new(stick.x, stick.y).normalize_or_zero()
                        * (MAX_LEAD_YARDS * stick.length().min(1.0));
                }
                charge = Some((slot, held_for + frame_dt as f32));
            }
        }
        if charge.is_none() && !running {
            if let Some(slot) = (0..targets.len().min(PASS_LABELS.len())).find(|&s| down[s]) {
                charge = Some((slot, 0.0));
                aim = Vec2::ZERO;
            }
        }

        // Letting go throws it at whatever weight the meter got to.
        let mut release: Option<(usize, f32)> = None;
        if let Some((slot, held_for)) = charge {
            if !held[slot] {
                let power = (held_for / CHARGE_TIME).clamp(MIN_POWER, 1.0);
                release = Some((targets[slot].1, power));
                charge = None;
            }
        }

        let aimed = charge
            .map(|(slot, _)| targets[slot].1)
            .or_else(|| release.map(|(index, _)| index));
        let power = release.map(|(_, p)| p).unwrap_or_else(|| {
            charge
                .map(|(_, t)| (t / CHARGE_TIME).clamp(0.0, 1.0))
                .unwrap_or(0.0)
        });
        sim.set_control(Some(QbControl {
            move_dir: stick,
            target: aimed,
            throw: release.is_some(),
            power,
            aim,
        }));
        if release.is_some() {
            aim = Vec2::ZERO;
        }

        while acc >= STEP as f64 && sim.result.is_none() {
            sim.step(STEP);
            acc -= STEP as f64;
        }

        if now.duration_since(drew) >= FRAME || sim.result.is_some() {
            drew = now;
            // A button over every eligible man, and while a pass is wound up,
            // the guide showing where it will come down and the meter showing
            // whether the arm can get it there.
            let mut badges = Vec::new();
            for (slot, (_, index)) in targets.iter().enumerate().take(PASS_LABELS.len()) {
                let separation = sim.separation(*index);
                let tone = if separation > 3.0 {
                    Tone::Open
                } else if separation > 1.6 {
                    Tone::Tight
                } else {
                    Tone::Covered
                };
                let man = sim.players[*index].pos;
                badges.push(Badge {
                    at: Vec3::new(man.x, man.y, 2.15),
                    label: PASS_LABELS[slot],
                    tone,
                    aimed: charge.map(|(s, _)| s) == Some(slot),
                });
            }

            let mut guide = None;
            let mut short = false;
            let mut needed = None;
            if let Some((slot, _)) = charge {
                if let Some((_, index)) = targets.get(slot) {
                    let plan = sim.throw_plan(sim.qb, *index, power, aim);
                    short = plan.distance > plan.reach + 0.4;
                    needed = Some(plan.power_needed);
                    let hand = sim.qb_pos();
                    let man = sim.players[*index].pos;
                    guide = Some(Guide {
                        from: Vec3::new(hand.x, hand.y, 1.8),
                        receiver: Vec3::new(man.x, man.y, 1.0),
                        landing: Vec3::new(plan.landing.x, plan.landing.y, 0.12),
                    });
                }
            }
            let ui = PassUi {
                badges,
                guide,
                power: charge.map(|(_, t)| (t / CHARGE_TIME).clamp(0.0, 1.0)),
                short,
                needed,
            };

            let mut frame = broadcast.live_frame_ui(sim, view_mode, &ui);
            // The call is read off the sim, not off the sheet that was chosen a
            // minute ago: an audible changes the play, and the overlay has to say so.
            let call = format!(
                "{} - {}{} - {} shows {}",
                sim.play.name,
                sim.play.formation.label(),
                if sim.play.flip { " (FLIPPED)" } else { "" },
                broadcast.teams[sim.setup.defense_team].abbr,
                sim.def.name
            );
            let mut lines = vec![header.to_string(), call];

            if running {
                // The moves, and which one is in flight right now.
                let mut row = String::from("  [A] SPIN   [B] HURDLE   [X] DIVE   [Y] SLIDE");
                if let Some(kind) = sim.carrier_move().kind() {
                    row.push_str(&format!("      {}", kind.label()));
                }
                lines.push(row);
            } else {
                // Who is open, on the buttons the badges are showing.
                let mut row = String::new();
                for (slot, (_, index)) in targets.iter().enumerate().take(PASS_LABELS.len()) {
                    let name = broadcast.teams[sim.setup.offense_team].roster[*index].last_name();
                    if !row.is_empty() {
                        row.push_str("   ");
                    }
                    row.push_str(&format!(
                        "{} {:<9} {:>4.1}",
                        PASS_LABELS[slot],
                        name,
                        sim.separation(*index)
                    ));
                }
                lines.push(row);
            }
            if presnap {
                lines.push(format!(
                    "  AT THE LINE   [A] SNAP   [X] AUDIBLE   [Y] FLIP   [RT] {}   [START] PAUSE",
                    cam_name(view_mode)
                ));
            } else {
                lines.push(format!(
                    "{off_abbr} ball   MOVE   HOLD A/B/X/Y TO PASS   AIM WITH STICK   [RT] {}   [START] PAUSE",
                    cam_name(view_mode)
                ));
            }
            // A live-ball foul does not stop anything: the flag is on the ground and
            // the ball moves when the play is over.
            if let Some(flag) = &sim.flag {
                lines.push(format!(
                    "  YELLOW FLAG - {} ON THE {}   PLAY ON, BALL MOVES AFTER IT",
                    flag.kind.label().to_uppercase(),
                    match flag.against {
                        FoulTeam::Offense => "OFFENSE",
                        FoulTeam::Defense => "DEFENSE",
                    }
                ));
            }

            if !view.show(&mut frame, &lines) {
                return false;
            }
        }

        if sim.result.is_some() {
            // The ball is dead; the picture is not. Hold the shot while the hit
            // finishes and the players get up, then let the card come up.
            let since = *dead_at.get_or_insert(now);
            let waited = since.elapsed();
            if waited >= DEAD_BALL_HOLD || (waited >= DEAD_BALL_SKIP && input.pressed.a) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Where the audible's cursor goes for one push of the stick.
///
/// The plays are a grid and the formations are the column beside it, exactly as on
/// the sideline sheet: left off the first card walks back into the formations,
/// right walks into the plays, and up and down stay in whichever column the cursor
/// is in. It used to be that left and right changed formation, which left the
/// second and third card of every row unreachable -- the only plays an audible
/// could ever call were the first of each row.
fn audible_nav(
    in_plays: bool,
    pick: usize,
    len: usize,
    form: usize,
    forms: usize,
    dir: Dir,
) -> (bool, usize, usize) {
    let forms = forms.max(1);
    if in_plays && len > 0 {
        if dir == Dir::Left && pick.min(len - 1) % callsheet::COLS == 0 {
            return (false, pick.min(len - 1), form);
        }
        return (true, callsheet::step_in(len, pick, dir), form);
    }
    match dir {
        Dir::Up => (false, 0, (form + forms - 1) % forms),
        Dir::Down => (false, 0, (form + 1) % forms),
        // Into the plays on the first card: the formations column is where the
        // set is chosen, and the highlight is not drawn there.
        Dir::Right if len > 0 => (true, 0, form),
        _ => (false, pick.min(len.max(1) - 1), form),
    }
}

/// The audible is the same layered sheet as the sideline call sheet, opened at the
/// line with the snap already waiting on the button.
enum Audible {
    Picked(Play),
    Back,
    Quit,
}

fn audible_sheet(
    view: &mut View,
    mut pad: Option<&mut Pad>,
    book: &[Play],
    current: &Play,
) -> Audible {
    let forms = formations_in(book);
    if forms.is_empty() {
        return Audible::Back;
    }
    let mut form = forms
        .iter()
        .position(|f| *f == current.formation)
        .unwrap_or(0);
    let mut pick = plays_in(book, forms[form])
        .iter()
        .position(|&i| book[i].name == current.name)
        .unwrap_or(0);
    let mut in_plays = true;
    let mut flip = current.flip;
    let mut nav = Nav::default();
    let mut last = Instant::now();
    // A button still held from the line must not pick the first card.
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;

        let list = plays_in(book, forms[form]);
        if list.is_empty() {
            return Audible::Back;
        }
        pick = pick.min(list.len() - 1);
        let mut frame = a7fl_render::Frame::new(CANVAS_W, CANVAS_H);
        callsheet::draw_layered(&mut frame, book, &forms, form, pick, in_plays, flip);
        let play = &book[list[pick]];
        let help = if in_plays {
            "  [STICK] PLAY   [LEFT] FORMATIONS   [A] CALL IT   [Y] FLIP   [B] BACK"
        } else {
            "  [STICK] FORMATION   [RIGHT] INTO THE PLAYS   [A] CALL IT   [Y] FLIP   [B] BACK"
        };
        let lines = vec![
            format!(
                "  AUDIBLE - {} - {}   {}   {} PLAYS IN THIS SET",
                play.name,
                play.formation.label(),
                if flip { "FLIPPED" } else { "AS DRAWN" },
                list.len()
            ),
            help.to_string(),
        ];
        if !view.show(&mut frame, &lines) {
            return Audible::Quit;
        }
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.start || input.pressed.b {
            return Audible::Back;
        }
        if input.pressed.y {
            flip = !flip;
        }
        if let Some(dir) = nav.step(input.stick, dt) {
            let (inside, next_pick, next_form) =
                audible_nav(in_plays, pick, list.len(), form, forms.len(), dir);
            in_plays = inside;
            pick = next_pick;
            form = next_form;
        }
        if input.pressed.a {
            let mut called = book[list[pick]].clone();
            if flip != called.flip {
                called = called.flipped();
            }
            return Audible::Picked(called);
        }
        std::thread::sleep(Duration::from_millis(8));
    }
}

/// Run one snap with the pad on a defender. The offense is the computer's, and
/// when the snap starts nobody is steering it.
fn drive_defense(
    sim: &mut PlaySim,
    broadcast: &mut Broadcast,
    view: &mut View,
    mut pad: Option<&mut Pad>,
    flip_y: bool,
    cam: &mut usize,
    header: &str,
) -> bool {
    let mut acc = 0.0f64;
    let mut last = Instant::now();
    let mut drew = Instant::now() - FRAME;
    let mut dead_at: Option<Instant> = None;
    let mut switched = Instant::now() - Duration::from_millis(400);

    loop {
        let now = Instant::now();
        let frame_dt = now.duration_since(last).as_secs_f64();
        acc += frame_dt;
        last = now;

        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.start {
            if !pause_menu(view, pad.as_deref_mut(), cam) {
                return false;
            }
            last = Instant::now();
            acc = 0.0;
            continue;
        }
        if input.pressed.throw {
            *cam = (*cam + 1) % CAMS.len();
        }
        let view_mode = CAMS[*cam % CAMS.len()];
        let stick = stick_to_world(input.stick, flip_y, view_mode, sim.setup.direction);

        // B walks the pad round the seven. A wraps up, X dives at him, Y picks.
        if input.pressed.b && now.duration_since(switched) >= Duration::from_millis(120) {
            sim.cycle_defender();
            switched = now;
        }
        sim.set_def_control(Some(DefControl {
            move_dir: stick,
            tackle: input.pressed.a,
            dive: input.pressed.x,
            swat: false,
            intercept: input.pressed.y,
        }));

        while acc >= STEP as f64 && sim.result.is_none() {
            sim.step(STEP);
            acc -= STEP as f64;
        }

        if now.duration_since(drew) >= FRAME || sim.result.is_some() {
            drew = now;
            let ui = PassUi {
                badges: Vec::new(),
                guide: None,
                power: None,
                short: false,
                needed: None,
            };
            let mut frame = broadcast.live_frame_ui(sim, view_mode, &ui);
            let role = sim.human_defender_role().map(|r| r.label()).unwrap_or("DEF");
            let who = sim
                .human_defender
                .map(|i| {
                    broadcast.teams[sim.setup.defense_team].roster[sim.players[i].player]
                        .last_name()
                        .to_string()
                })
                .unwrap_or_default();
            let mut lines = vec![
                header.to_string(),
                format!(
                    "  {} - {}{} - {} BALL",
                    sim.play.name,
                    sim.play.formation.label(),
                    if sim.play.flip { " (FLIPPED)" } else { "" },
                    broadcast.teams[sim.setup.offense_team].abbr
                ),
                format!("  YOU: {role} {who}   [B] SWITCH DEFENDER"),
                "  [A] TACKLE   [X] DIVE TACKLE   [Y] INTERCEPT   [RT] CAMERA   [START] PAUSE"
                    .to_string(),
            ];
            if let Some(flag) = &sim.flag {
                lines.push(format!(
                    "  YELLOW FLAG - {} ON THE {}   PLAY ON, BALL MOVES AFTER IT",
                    flag.kind.label().to_uppercase(),
                    match flag.against {
                        FoulTeam::Offense => "OFFENSE",
                        FoulTeam::Defense => "DEFENSE",
                    }
                ));
            }
            if !view.show(&mut frame, &lines) {
                return false;
            }
        }

        if sim.result.is_some() {
            // The ball is dead; the picture is not. Hold the shot while the hit
            // finishes, then let the card come up.
            let since = *dead_at.get_or_insert(now);
            let waited = since.elapsed();
            if waited >= DEAD_BALL_HOLD || (waited >= DEAD_BALL_SKIP && input.pressed.a) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The three letters a defender's job line starts with on the call sheet.
fn role_tag(role: DefRole) -> &'static str {
    match role {
        DefRole::EdgeL | DefRole::EdgeR => "EDG",
        DefRole::Mike => "MIK",
        DefRole::Will => "WIL",
        DefRole::CbL | DefRole::CbR => "CB",
        DefRole::Safety => "SAF",
    }
}

/// Which man on your team you are driving during a throw-off: the returner while
/// you are receiving, and otherwise the man nearest the ball once it is in the
/// air — which is how you get downfield to make the tackle.
fn human_men(sim: &ThrowOffSim, my_team: usize) -> Vec<usize> {
    let returner = sim.players.iter().position(|p| p.role == ThrowRole::Returner);
    if let Some(r) = returner {
        if sim.players[r].team == my_team {
            return vec![r];
        }
    }
    if sim.phase == ThrowPhase::Presnap {
        return Vec::new();
    }
    // Only the three gunners are yours: the thrower has thrown it and is not a
    // coverage man, so putting the pad on him handed the player a man who cannot
    // make a tackle.
    (0..sim.players.len())
        .filter(|&i| {
            let p = &sim.players[i];
            p.team == my_team && !p.down && p.role == ThrowRole::Gunner
        })
        .collect()
}

/// The man the pad is on, walking round [`human_men`] with the switch button.
fn human_man(sim: &ThrowOffSim, my_team: usize, pick: usize) -> Option<usize> {
    let men = human_men(sim, my_team);
    if men.is_empty() {
        return None;
    }
    Some(men[pick % men.len()])
}

fn throw_off_line(
    arcade: &Arcade,
    sim: &ThrowOffSim,
    my_team: usize,
    aim_y: f32,
    charging: bool,
) -> String {
    let abbr = |t: usize| arcade.game.teams[t].abbr.clone();
    match sim.phase {
        ThrowPhase::Presnap => {
            if sim.human_thrower {
                let aim = aim_readout(sim.setup.throwing_direction(), aim_y);
                let job = if charging { "RELEASE [A] TO THROW" } else { "AIM, THEN HOLD [A]" };
                format!("  {} THROW-OFF FROM THE {} - {job} - AIMING {aim}", abbr(sim.setup.throwing_team), sim.setup.from_own_yards as i32)
            } else {
                format!(
                    "  {} THROW-OFF FROM THE {} - GET UNDER IT IN {}",
                    abbr(sim.setup.throwing_team),
                    sim.setup.from_own_yards as i32,
                    sim.presnap_left().ceil() as i32
                )
            }
        }
        ThrowPhase::Flight if sim.fair_catch => {
            "  FAIR CATCH CALLED - STAY UNDER IT".to_string()
        }
        ThrowPhase::Flight => "  THE BALL IS IN THE AIR".to_string(),
        ThrowPhase::Loose => "  LOOSE BALL - SOMEBODY GET ON IT".to_string(),
        ThrowPhase::Return if sim.tackle_in_progress() => {
            "  THE RETURNER IS DOWN".to_string()
        }
        ThrowPhase::Return => format!(
            "  RETURN - {} BALL IN HAND",
            abbr(sim.setup.receiving_team)
        ),
        ThrowPhase::Dead => format!("  THROW-OFF OVER - {} BALL", abbr(my_team)),
    }
}

fn throw_off_controls(arcade: &Arcade, sim: &ThrowOffSim, my_team: usize, pick: usize) -> String {
    let cam = cam_name(CAMS[arcade.cam % CAMS.len()]);
    if sim.phase == ThrowPhase::Presnap && sim.human_thrower {
        return format!(
            "  THROW   AIM WITH STICK   HOLD [A] TO LOAD   [START] PAUSE   CAMERA LIVES IN THERE"
        );
    }
    if sim.setup.throwing_team == my_team {
        let men = human_men(sim, my_team);
        let who = men
            .get(pick % men.len().max(1))
            .map(|&i| {
                arcade.game.teams[sim.players[i].team].roster[sim.players[i].roster]
                    .last_name()
                    .to_string()
            })
            .unwrap_or_else(|| "NOBODY".to_string());
        return format!(
            "  COVER   YOU: {who:<11} MOVE   [A] TACKLE   [X] DIVE   [B] SWITCH   [RT] {cam}   [START] PAUSE"
        );
    }
    let fair = if sim.phase == ThrowPhase::Flight {
        if sim.fair_catch {
            "   FAIR CATCH CALLED"
        } else {
            "   [Y] FAIR CATCH"
        }
    } else {
        ""
    };
    format!(
        "  RETURN IT   {} BALL   MOVE WITH STICK{fair}   [RT] {cam}   [START] PAUSE",
        arcade.game.teams[sim.setup.receiving_team].abbr,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn the_audible_can_reach_every_card_in_a_row() {
        // The bug: left and right changed formation, so only the first card of
        // each row could ever be called.
        let len = 6;
        let (mut in_plays, mut pick, mut form) = (true, 0usize, 0usize);
        for want in [1usize, 2] {
            let (inside, p, f) = audible_nav(in_plays, pick, len, form, 3, Dir::Right);
            in_plays = inside;
            pick = p;
            form = f;
            assert!(inside && form == 0, "right off a card went to card {p} of set {f}");
            assert_eq!(pick, want, "right did not walk along the row");
        }
        // Down is the next row of the same column, not the next formation.
        let (inside, p, f) = audible_nav(in_plays, pick, len, form, 3, Dir::Down);
        assert!(inside && p == 5 && f == 0, "down went to card {p} of set {f}");
        // Left off the first column is the way out to the formations.
        let (inside, ..) = audible_nav(true, 3, len, 1, 3, Dir::Left);
        assert!(!inside, "left off the row did not reach the formations");
        // The formations still walk, and right comes back in on the first card.
        let (inside, _, f) = audible_nav(false, 0, len, 1, 3, Dir::Down);
        assert!(!inside && f == 2, "down in the formations went to {f}");
        let (inside, p, _) = audible_nav(false, 4, len, 1, 3, Dir::Right);
        assert!(inside && p == 0, "right into the plays landed on {p}");
    }

    #[test]
    fn the_stick_follows_the_shot_on_screen() {
        // The sideline camera: up the screen is across the field, right is
        // downfield, and changing ends changes nothing.
        for dir in [1i8, -1] {
            let up = stick_to_world(Vec2::new(0.0, 1.0), false, CameraMode::Broadcast, dir);
            assert!(close(up, Vec2::new(0.0, -1.0)), "broadcast up -> {:?}", up);
            let right = stick_to_world(Vec2::new(1.0, 0.0), false, CameraMode::Broadcast, dir);
            assert!(close(right, Vec2::new(1.0, 0.0)), "broadcast right -> {:?}", right);
        }
        // Behind the ball, driving +x: up the stick is +x, right is +y.
        let up = stick_to_world(Vec2::new(0.0, 1.0), false, CameraMode::Classic, 1);
        assert!(close(up, Vec2::new(1.0, 0.0)), "classic up -> {:?}", up);
        let right = stick_to_world(Vec2::new(1.0, 0.0), false, CameraMode::Classic, 1);
        assert!(close(right, Vec2::new(0.0, 1.0)), "classic right -> {:?}", right);
        // Driving -x: up the stick is still up the field, which is now -x, and
        // right is the runner's right hand, which is now -y.
        let up = stick_to_world(Vec2::new(0.0, 1.0), false, CameraMode::SkyCam, -1);
        assert!(close(up, Vec2::new(-1.0, 0.0)), "sky up -> {:?}", up);
        let right = stick_to_world(Vec2::new(1.0, 0.0), false, CameraMode::SkyCam, -1);
        assert!(close(right, Vec2::new(0.0, -1.0)), "sky right -> {:?}", right);
    }

    /// The bug the pad player found: one of the two teams aimed the other way,
    /// because the marker was pushed in world y while the lens looks down the
    /// throwing team's direction.
    #[test]
    fn pushing_right_aims_right_whichever_team_is_throwing() {
        for throw_dir in [1i8, -1] {
            // A right push, and the direction the shot is looking.
            let push = aim_push(1.0, throw_dir);
            let aim = aim_step(0.0, push, 0.1);
            // On screen, `across` is what the readout prints and what the marker
            // travels: it has to grow for a right push on both teams.
            let across = throw_dir as f32 * aim;
            assert!(across > 0.0, "throw_dir {throw_dir}: right push aimed {across}");
            assert_eq!(
                aim_readout(throw_dir, aim),
                "3 YARDS RIGHT",
                "throw_dir {throw_dir}"
            );
            // And left is left.
            let back = aim_step(aim, aim_push(-1.0, throw_dir), 0.1);
            assert!(
                (throw_dir as f32 * back).abs() < 1.0,
                "throw_dir {throw_dir}: left push did not come back"
            );
        }
    }

    #[test]
    fn the_way_marker_walks_with_the_stick_and_stops_at_the_sideline() {
        // Right on the stick moves it one way, left the other, and the centre is
        // where it starts.
        let right = aim_step(0.0, 1.0, 0.1);
        let left = aim_step(0.0, -1.0, 0.1);
        assert!(right > 0.0 && left < 0.0, "right {right} left {left}");
        assert!(close(Vec2::new(right, left), Vec2::new(-left, -right)));
        // A resting stick leaves it where it was put, so the aim is a choice and
        // not a balancing act.
        assert_eq!(aim_step(6.0, 0.0, 0.1), 6.0);
        // And it cannot be walked off the field.
        let mut aim = 0.0;
        for _ in 0..600 {
            aim = aim_step(aim, 1.0, 0.1);
        }
        assert!((aim - THROW_OFF_AIM_YARDS).abs() < 1e-4, "aim ran to {aim}");
    }

    #[test]
    fn the_aim_readout_calls_the_side_the_player_sees() {
        // Screen right for a lens looking along +x is +y, and along -x it is -y.
        assert_eq!(aim_readout(1, 8.0), "8 YARDS RIGHT");
        assert_eq!(aim_readout(1, -8.0), "8 YARDS LEFT");
        assert_eq!(aim_readout(-1, 8.0), "8 YARDS LEFT");
        assert_eq!(aim_readout(-1, 0.0), "DOWN THE MIDDLE");
    }

    #[test]
    fn the_inverted_pad_option_flips_up_and_down_only() {
        for mode in [CameraMode::Broadcast, CameraMode::Classic, CameraMode::SkyCam] {
            for dir in [1i8, -1] {
                let flipped = stick_to_world(Vec2::new(0.4, 0.8), true, mode, dir);
                let raw = stick_to_world(Vec2::new(0.4, -0.8), false, mode, dir);
                assert!(
                    close(flipped, raw),
                    "{} {}: {:?} != {:?}",
                    cam_name(mode),
                    dir,
                    flipped,
                    raw
                );
            }
        }
    }
}

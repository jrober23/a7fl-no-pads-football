//! `7on7 arcade` — play the game live, in a window or in the terminal.
//!
//! The field is drawn by the same software renderer the broadcast package uses:
//! a real window when the machine has a display, ANSI colour blocks when it does
//! not. The side is yours: call a play from the layered call sheet, snap it yourself
//! at the line, audible out of it if the look is wrong, and then take the field on
//! defense when the other team has the ball. The clock, the downs and the rulebook
//! run as they do in `sim`.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use nopads_core::ai;
use nopads_core::game::{Action, GamePhase, GameState, PlayRecord, ScrimmageRecord};
use nopads_core::math::{Rgb, Rng, Vec2, Vec3};
use nopads_core::moves::Kind;
use nopads_core::playbook::{
    def_calls_in, defensive_playbook, formations_in, fronts_in, playbook, plays_in, DefCall,
    DefFront, DefRole, Formation, Play,
};
use nopads_core::rules::{FoulTeam, THROW_OFF_MIN_AIR_YARDS};
use nopads_core::sim::{DefControl, Phase, PlaySim, QbControl};
use nopads_core::team::league_teams;
use nopads_core::throwoff::{power_for_air, ThrowOffSetup, ThrowOffSim, ThrowPhase, ThrowRole};
use nopads_render::broadcast::{Broadcast, CameraMode};
use nopads_render::hud;
use nopads_render::passing::{Badge, Focus, Guide, PassUi, Tone};
use nopads_render::callsheet;
use nopads_render::defsheet;
use nopads_render::playsheet::Dir;
use nopads_render::terminal;

use crate::menu;
use nopads_platform::pad::{self, Input, Pad};
use nopads_platform::settings::Settings;
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
/// How long the word over a dead ball takes to arrive on the picture. The play
/// used to be announced the frame it ended; this is the beat it rises over.
const ANNOUNCE_EASE: f32 = 0.55;
/// How long before [A] can move it on: past the hit, not past the replay.
const DEAD_BALL_SKIP: Duration = Duration::from_millis(520);
/// How long the pass meter takes to fill from nothing to a full-weight throw.
const CHARGE_TIME: f32 = 1.05;
/// Even a short hold throws the ball; it just does not throw it far.
const MIN_POWER: f32 = 0.18;
/// A press shorter than this is not a throw at all: it is a pump fake. Tap the
/// same receiver twice and the quarterback pumps twice.
const PUMP_TAP: f32 = 0.16;
/// How long the picture holds while an interception turns it round.
const TURNAROUND: Duration = Duration::from_millis(700);
/// How many cards an audible shows at once over the field.
const POPUP_CARDS: usize = 3;
/// The four passing buttons, in the order the badges are drawn.
const PASS_LABELS: [&str; 4] = ["A", "B", "X", "Y"];
/// The same four buttons once the ball is in a runner's hands. Y is the stiff
/// arm, which is what a carrier does with the hand not holding the ball.
const MOVES: [Kind; 4] = [Kind::Spin, Kind::Hurdle, Kind::Dive, Kind::StiffArm];

/// The moves for the man holding the ball. A quarterback still has a slide in
/// him -- he gives himself up rather than take the hit -- and a runner does not.
fn moves_for(sim: &PlaySim) -> [Kind; 4] {
    if sim.ball.holder == Some(sim.qb) {
        [Kind::Spin, Kind::Hurdle, Kind::Dive, Kind::Slide]
    } else {
        MOVES
    }
}
/// How far across the field a throw-off can be aimed: the width of the landing
/// zone the returner has to work in.
const THROW_OFF_AIM_YARDS: f32 = 13.0;
/// How fast the aim marker sweeps across the field while the stick is held.
const AIM_SWEEP: f32 = 26.0;
/// How long the thrower waits before the ball goes when the computer is throwing
/// it at a human returner. Long enough to see the field and find the ball.
const WATCH_SNAP_DELAY: f32 = 2.6;

/// The views the player can switch between while the ball is live, in the order
/// the trigger walks them. Classic comes first: the shot from behind the ball is
/// the one you can play from, and it is where the game opens.
const CAMS: [CameraMode; 3] = [
    CameraMode::Classic,
    CameraMode::Broadcast,
    CameraMode::SkyCam,
];

/// The cameras in the order `CAMS` holds them, for the settings screen.
pub const CAMERA_LABELS: [&str; 3] = ["CLASSIC", "BROADCAST", "SKY"];

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
    /// The settings this game was started with, kept so a lens moved in the pause
    /// menu is still there the next time the game opens.
    settings: Option<Settings>,
    /// The ground the game is played at, so a rematch is played on it too.
    venue: usize,
}

/// What the player asked for at the final whistle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AfterFinal {
    /// The same two clubs, on the same ground, from zero.
    Again,
    /// Back to the front end.
    Menu,
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
            settings: None,
            venue: 0,
        }
    }

    /// Reverse the stick's vertical axis, for pads that report it inverted.
    pub fn with_flip_y(mut self, flip: bool) -> Self {
        self.flip_y = flip;
        self
    }

    /// The camera the picture starts on, from the settings screen.
    pub fn with_camera(mut self, cam: usize) -> Self {
        self.cam = cam % CAMS.len();
        self
    }

    /// The lens the player tuned on the settings screen.
    pub fn with_lens(mut self, zoom: f32, tilt: f32) -> Self {
        self.broadcast.set_lens(zoom, tilt);
        self
    }

    /// The settings the game was opened with. The lens lives on the picture while
    /// the game is running, and moves are written back here when it closes.
    pub fn with_settings(mut self, settings: Settings) -> Self {
        self.settings = Some(settings);
        self
    }

    /// The ground the game is played at, so a rematch opens on the same field.
    ///
    /// The picture was built on the practice field before the player had picked
    /// anything, so the choice has to be built into the world here rather than
    /// only remembered for the scoreboard.
    pub fn with_venue(mut self, venue: usize) -> Self {
        self.venue = venue;
        self.broadcast
            .set_venue(nopads_render::venues::Venue::from_index(venue));
        self
    }

    /// The side of the ball the player runs, chosen in exhibition.
    pub fn with_my_team(mut self, side: usize) -> Self {
        self.my_team = Some(side.min(1));
        self
    }

    pub fn run(&mut self) {
        let terminal_mode = !self.view.is_window();
        if terminal_mode {
            print!("{}", terminal::enter());
            io::stdout().flush().ok();
        }
        loop {
            BACK_TO_MENU.store(false, Ordering::Relaxed);
            self.quit = false;
            let after = self.play();
            // A lens moved in the pause menu is the player's lens too: it goes
            // back into the settings file rather than lasting only as long as
            // this game.
            if let Some(settings) = &mut self.settings {
                let (zoom, tilt) = self.broadcast.lens();
                if (settings.lens_zoom - zoom).abs() > 0.001
                    || (settings.lens_tilt - tilt).abs() > 0.001
                {
                    settings.lens_zoom = zoom;
                    settings.lens_tilt = tilt;
                    settings.save();
                }
            }
            // The final card asks what is next. Playing again is the same two
            // clubs on the same ground; the other way out is the front end. Only
            // the menu's own quit, or a closed window, ends the program.
            match after {
                Some(AfterFinal::Again) => self.restart(),
                Some(AfterFinal::Menu) => {
                    if !self.reopen_menu() {
                        break;
                    }
                }
                None => {
                    if BACK_TO_MENU.load(Ordering::Relaxed) && self.reopen_menu() {
                        continue;
                    }
                    break;
                }
            }
        }
        if terminal_mode {
            print!("{}", terminal::leave());
            io::stdout().flush().ok();
        }
    }

    /// Back to the front end after a game the player left on purpose: pick the
    /// clubs again and start over. False when they quit from the menu, which is
    /// the one way out of the program now.
    fn reopen_menu(&mut self) -> bool {
        let Some(settings) = self.settings.as_mut() else {
            return false;
        };
        let kick = match menu::run(&mut self.view, self.pad.as_mut(), settings) {
            Some(kick) => kick,
            None => return false,
        };
        let teams = league_teams();
        let seed = self.game.seed ^ 0x00A7_F1E5;
        let mut game = GameState::new(teams[kick.yours].clone(), teams[kick.theirs].clone(), seed);
        game.set_opening_receiver(if kick.receive { 0 } else { 1 });
        game.set_difficulty(nopads_core::difficulty::Difficulty::from_index(settings.difficulty));
        // The quarter length and the ground are chosen on the front end, so the
        // game is built to them rather than patched afterwards.
        game.set_quarter_seconds(settings.quarter_minutes as f32 * 60.0);
        self.broadcast = Broadcast::with_venue(
            CANVAS_W,
            CANVAS_H,
            [game.teams[0].clone(), game.teams[1].clone()],
            nopads_render::venues::Venue::from_index(kick.venue),
        );
        self.broadcast.set_lens(settings.lens_zoom, settings.lens_tilt);
        self.venue = kick.venue;
        self.game = game;
        self.my_team = Some(0);
        true
    }

    /// A rematch: the same two clubs, on the same ground, under the same clock
    /// and the same difficulty, as a new game rather than the one that ended.
    fn restart(&mut self) {
        let teams = [self.game.teams[0].clone(), self.game.teams[1].clone()];
        let seed = self.game.seed ^ 0x00A7_1206;
        let mut game = GameState::new(teams[0].clone(), teams[1].clone(), seed);
        game.set_opening_receiver(0);
        if let Some(settings) = &self.settings {
            game.set_difficulty(nopads_core::difficulty::Difficulty::from_index(
                settings.difficulty,
            ));
            game.set_quarter_seconds(settings.quarter_minutes as f32 * 60.0);
        }
        self.broadcast = Broadcast::with_venue(
            CANVAS_W,
            CANVAS_H,
            [game.teams[0].clone(), game.teams[1].clone()],
            nopads_render::venues::Venue::from_index(self.venue),
        );
        if let Some(settings) = &self.settings {
            self.broadcast.set_lens(settings.lens_zoom, settings.lens_tilt);
        }
        self.game = game;
        self.my_team = Some(0);
        self.form = 0;
        self.sheet_pick = 0;
        self.def_pick = 0;
    }

    fn play(&mut self) -> Option<AfterFinal> {
        if !self.kickoff_card() {
            return None;
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
        if self.quit {
            return None;
        }
        self.show_final()
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

            let mut frame = nopads_render::Frame::new(CANVAS_W, CANVAS_H);
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
                if !pause_menu(
                    &mut self.view,
                    self.pad.as_mut(),
                    &mut self.cam,
                    &mut self.broadcast,
                    PausedShot::None,
                ) {
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
                if !pause_menu(
                    &mut self.view,
                    self.pad.as_mut(),
                    &mut self.cam,
                    &mut self.broadcast,
                    PausedShot::None,
                ) {
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
            &["7 on 7 lets the offense pick the try, and it is locked in."],
            &options,
            0,
        ) {
            Some(pick) => pick == 1,
            None => false,
        }
    }

    /// Fourth down: go for it, or use the throw-off in place of a punt.
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
        // The menu picked the club and tossed for the ball, so there is nothing
        // left to ask. A scripted run (`--home`/`--away`) still gets the card.
        if self.my_team.is_some() {
            return true;
        }
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
                "  7-ON-7 NO PADS FOOTBALL".to_string(),
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
                "  RETURN: STICK RUNS, [SELECT] CHANGES CAMERA, [START] PAUSES".to_string(),
                String::new(),
                "  [STICK] CHOOSE YOUR TEAM   [A] KICK OFF".to_string(),
            ];
            if !self.view.page(&lines) {
                self.quit = true;
                return false;
            }
            let input = self.input();
            if input.pressed.start {
                if !pause_menu(
                    &mut self.view,
                    self.pad.as_mut(),
                    &mut self.cam,
                    &mut self.broadcast,
                    PausedShot::None,
                ) {
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

        let board = hud::Scoreboard::from_game(&self.game);
        let finished = self.drive_throw_off(&mut sim, my_team, &header, &board);
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
    fn drive_throw_off(
        &mut self,
        sim: &mut ThrowOffSim,
        my_team: usize,
        header: &str,
        board: &hud::Scoreboard,
    ) -> bool {
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
        // The men you can cover with, nearest the ball first, taken once the ball is
        // caught so the pad starts on a man who can actually make the tackle.
        let mut cover_order: Option<Vec<usize>> = None;
        // When the ball died, so the picture can hold on the tackle before the card.
        let mut dead_at: Option<Instant> = None;
        // The throw-off frame by frame, for the replay a score gets.
        let mut tape: Vec<ThrowOffSim<'_>> = Vec::new();

        loop {
            let now = Instant::now();
            let frame_dt = now.duration_since(last).as_secs_f64();
            acc += frame_dt;
            last = now;

            let input = pad::mix(self.pad.as_mut(), self.view.poll());
            if input.pressed.start {
                // START pauses: the camera setting and quitting both live behind it.
                if !pause_menu(
                    &mut self.view,
                    self.pad.as_mut(),
                    &mut self.cam,
                    &mut self.broadcast,
                    PausedShot::ThrowOff(&*sim),
                ) {
                    self.quit = true;
                    return false;
                }
                last = Instant::now();
                acc = 0.0;
                continue;
            }
            // SELECT cycles the lens while you are covering or returning; the
            // trigger is the throw itself, so it cannot be both.
            let throwing = sim.phase == ThrowPhase::Presnap && sim.human_thrower;
            if input.pressed.select && !throwing {
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
            // Covering an AI return used to start on the first gunner in the list,
            // however far downfield he was. The pad now starts on the man closest to
            // the ball, and the order is taken when it is caught rather than every
            // frame, so the man under the pad does not change mid-stride.
            if cover_order.is_none() && sim.catch_time().is_some() {
                cover_order = Some(men_by_distance(sim, my_team));
            }
            let mine = cover_order
                .as_ref()
                .and_then(|order| live_man(sim, order, cover_pick))
                .or_else(|| human_man(sim, my_team, cover_pick));
            sim.set_human(mine, stick);
            // The returner has the ball, so the four buttons are the four moves: the
            // same set the back gets, on the same buttons.
            let returning_it = sim.phase == ThrowPhase::Return
                && mine
                    .map(|i| sim.players[i].role == ThrowRole::Returner)
                    .unwrap_or(false);
            if returning_it {
                let buttons = [input.pressed.a, input.pressed.b, input.pressed.x, input.pressed.y];
                for (slot, kind) in MOVES.iter().enumerate() {
                    if buttons[slot] {
                        sim.request_move(*kind);
                    }
                }
            }
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
                // The tape starts when he catches it. The throw and the flight are
                // not the return, and a replay that began with them was still showing
                // the kick when the play it was showing was already over.
                if sim.catch_time().is_some() && tape.len() < 3600 {
                    tape.push(sim.clone());
                }
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
                // The pad is on a man here too: the returner, a gunner, or the
                // thrower while the ball is still his.
                let focus_index = mine.or_else(|| {
                    sim.players
                        .iter()
                        .position(|p| p.team == my_team && p.role == ThrowRole::Returner)
                });
                let focus = focus_index.map(|i| Focus {
                    at: Vec3::new(sim.players[i].pos.x, sim.players[i].pos.y, 0.0),
                    label: "YOU".to_string(),
                });
                let ui = PassUi {
                    focus,
                    badges: Vec::new(),
                    guide,
                    power,
                    short,
                    needed,
                };
                let mode = CAMS[self.cam % CAMS.len()];
                let mut frame = self.broadcast.throw_off_frame_live(sim, mode, &ui);

                if sim.phase == ThrowPhase::Presnap {
                    hud::draw_scoreboard(&mut frame, board);
                }
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
                    // Only a score. A twenty-five yard return is a good return, not a
                    // big play, and a replay on every one of them came round far too
                    // often to be worth watching.
                    let big = sim.result.as_ref().map(|r| r.touchdown).unwrap_or(false);
                    if big
                        && self.view.is_window()
                        && !play_back_throw_off(
                            &tape,
                            &mut self.broadcast,
                            &mut self.view,
                            self.pad.as_mut(),
                        )
                    {
                        self.quit = true;
                        return false;
                    }
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
        let board = hud::Scoreboard::from_game(&self.game);

        let my_team = self.my_team.unwrap_or(self.game.possession);

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
                my_team,
                &board,
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
        let defbook: &[DefCall] = &self.defbook;
        let board = hud::Scoreboard::from_game(&self.game);
        let game = &mut self.game;
        let broadcast = &mut self.broadcast;
        let view = &mut self.view;
        let pad = &mut self.pad;
        let quit = &mut self.quit;
        let cam = &mut self.cam;
        let flip_y = self.flip_y;

        let record = game.snap_with(play, def, |sim| {
            sim.set_human_defender(sim.nearest_defender());
            let finished = drive_defense(
                sim,
                broadcast,
                view,
                pad.as_mut(),
                flip_y,
                cam,
                &header,
                defbook,
                &board,
            );
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

            let mut frame = nopads_render::Frame::new(CANVAS_W, CANVAS_H);
            defsheet::draw_layered(&mut frame, &self.defbook, &fronts, front, pick, in_calls);
            let lines = self.def_sheet_lines(fronts[front], &list, pick, in_calls);
            if !self.view.show(&mut frame, &lines) {
                self.quit = true;
                return None;
            }
            let input = self.input();
            if input.pressed.start {
                if !pause_menu(
                    &mut self.view,
                    self.pad.as_mut(),
                    &mut self.cam,
                    &mut self.broadcast,
                    PausedShot::None,
                ) {
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
            nopads_core::field::ball_on_label(self.game.yards_to_goal())
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
        let from = nopads_core::field::yards_to_goal(record.setup.los, record.setup.direction);
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
            nopads_core::field::ball_on_label(from),
            nopads_core::field::ball_on_label(self.game.yards_to_goal())
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

    fn show_final(&mut self) -> Option<AfterFinal> {
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
        lines.push("  [A] PLAY AGAIN    [B] MAIN MENU".to_string());
        if !self.view.page(&lines) {
            return None;
        }
        loop {
            let input = self.wait(Duration::from_millis(30));
            if input.pressed.a || input.pressed.start {
                return Some(AfterFinal::Again);
            }
            if input.pressed.b {
                return Some(AfterFinal::Menu);
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

/// What the pause screen can put behind its menu: the call sheet and the
/// between-plays cards have no live picture, but a snap and a throw-off do.
enum PausedShot<'s, 'a> {
    None,
    Play(&'s PlaySim<'a>),
    ThrowOff(&'s ThrowOffSim<'a>),
}

/// A still of the game as the lens currently sees it.
fn paused_still(
    broadcast: &mut Broadcast,
    shot: &PausedShot<'_, '_>,
    mode: CameraMode,
) -> Option<nopads_render::Frame> {
    match shot {
        PausedShot::None => None,
        PausedShot::Play(sim) => Some(broadcast.live_frame(sim, mode)),
        PausedShot::ThrowOff(sim) => {
            let ui = PassUi {
                focus: None,
                badges: Vec::new(),
                guide: None,
                power: None,
                short: false,
                needed: None,
            };
            Some(broadcast.throw_off_frame_live(sim, mode, &ui))
        }
    }
}

/// The menu, set over the frozen picture rather than on a page of their own.
fn draw_pause_lines(frame: &mut nopads_render::Frame, lines: &[String]) {
    let line_h = 18;
    let h = lines.len() as i32 * line_h + 20;
    let y0 = ((frame.h as i32 - h) / 2).max(0);
    frame.fill_rect_alpha(34, y0, frame.w as i32 - 68, h, Rgb::new(8, 12, 18), 0.80);
    for (i, line) in lines.iter().enumerate() {
        frame.text_shadowed(
            50,
            y0 + 10 + i as i32 * line_h,
            line,
            2,
            Rgb::new(232, 238, 246),
            1,
        );
    }
}

/// Set when the player asked for the front end instead of the desktop.
///
/// The pause screen is a free function called from the bottom of a snap, a
/// throw-off and the call sheets, and everything up that stack only knows "carry
/// on" or "the game is over". The one answer that has to survive the walk out is
/// this one: a player who wants another game should not have to start the program
/// again.
static BACK_TO_MENU: AtomicBool = AtomicBool::new(false);

/// The pause screen. Returns false when the player chose to quit the game.
///
/// START used to close the game on the spot, which put the camera setting — the
/// one thing a player wants to change in the middle of a drive — out of reach
/// once the ball was snapped. The lens is here too now: a player can zoom and
/// tilt the game he is already watching, on the frozen picture, and the shot he
/// leaves behind is the shot the rest of the game is played in.
fn pause_menu(
    view: &mut View,
    mut pad: Option<&mut Pad>,
    cam: &mut usize,
    broadcast: &mut Broadcast,
    shot: PausedShot<'_, '_>,
) -> bool {
    const ITEMS: usize = 4;
    let mut nav = Nav::default();
    let mut pick = 0usize;
    // The lens sub-screen: the stick on the camera, the frozen picture in front
    // of it, exactly like the lens screen in the settings.
    let mut tuning = false;
    // The quit prompt: 0 is the menu, 1 is the desktop.
    let mut leaving: Option<usize> = None;
    let mut last = Instant::now();
    // The picture as the lens last saw it, redrawn only when the lens moves.
    let mut still: Option<nopads_render::Frame> = None;
    // A button still held from the game must not select the first item.
    view.reset();
    let keep = loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.start || input.pressed.b {
            // B backs out of the lens without leaving the game paused behind it.
            if tuning {
                tuning = false;
            } else if leaving.is_some() {
                leaving = None;
            } else {
                break true;
            }
        }
        if tuning {
            let (mut zoom, mut tilt) = broadcast.lens();
            let mut changed = false;
            if input.stick.x.abs() > 0.25 {
                zoom = (zoom + input.stick.x * dt * 0.8).clamp(0.6, 1.8);
                changed = true;
            }
            if input.stick.y.abs() > 0.25 {
                tilt = (tilt - input.stick.y * dt * 0.6).clamp(-0.35, 0.45);
                changed = true;
            }
            if input.pressed.x {
                zoom = 1.0;
                tilt = 0.0;
                changed = true;
            }
            if changed {
                broadcast.set_lens(zoom, tilt);
                still = None;
            }
            if input.pressed.a {
                tuning = false;
            }
        } else if let Some(choice) = leaving.as_mut() {
            // Quitting asks a question first: this game, or the whole game?
            // Leaving used to close the window on the spot, so a player who only
            // wanted another club had to start the program again.
            match nav.step(input.stick, dt) {
                Some(Dir::Up) | Some(Dir::Down) => *choice = (*choice + 1) % 2,
                _ => {}
            }
            if input.pressed.a {
                if *choice == 0 {
                    BACK_TO_MENU.store(true, Ordering::Relaxed);
                }
                break false;
            }
        } else {
            match nav.step(input.stick, dt) {
                Some(Dir::Up) => pick = (pick + ITEMS - 1) % ITEMS,
                Some(Dir::Down) => pick = (pick + 1) % ITEMS,
                _ => {}
            }
            if input.pressed.a {
                match pick {
                    0 => break true,
                    1 => {
                        *cam = (*cam + 1) % CAMS.len();
                        still = None;
                    }
                    2 => tuning = true,
                    3 => leaving = Some(0),
                    _ => {}
                }
            }
        }
        let (zoom, tilt) = broadcast.lens();
        let mark = |on: bool| if on { ">>" } else { "  " };
        let lines = if tuning {
            vec![
                String::new(),
                "  LENS".to_string(),
                String::new(),
                format!("  ZOOM {zoom:.2}    TILT {tilt:+.2}"),
                String::new(),
                "  [STICK] ZOOM / TILT   [X] RESET   [A] DONE".to_string(),
            ]
        } else if let Some(choice) = leaving {
            vec![
                String::new(),
                "  LEAVE THIS GAME?".to_string(),
                String::new(),
                format!("    {} BACK TO THE MENU", mark(choice == 0)),
                format!("    {} QUIT THE GAME", mark(choice == 1)),
                String::new(),
                "  [STICK] CHOOSE   [A] SELECT   [B] STAY".to_string(),
            ]
        } else {
            vec![
                String::new(),
                "  PAUSED".to_string(),
                String::new(),
                format!("    {} RESUME", mark(pick == 0)),
                format!(
                    "    {} CAMERA    {:<9}   [A] CHANGE",
                    mark(pick == 1),
                    cam_name(CAMS[*cam % CAMS.len()])
                ),
                format!("    {} LENS      {zoom:.2} / {tilt:+.2}   [A] ADJUST", mark(pick == 2)),
                format!("    {} LEAVE GAME", mark(pick == 3)),
                String::new(),
                "  [STICK] CHOOSE   [A] SELECT   [START] RESUME".to_string(),
            ]
        };
        // The picture behind the menu, unless there is none or this is the
        // terminal, where a page of text is the only thing that reads.
        let mut shown = false;
        if view.is_window() && !matches!(shot, PausedShot::None) {
            if still.is_none() {
                still = paused_still(broadcast, &shot, CAMS[*cam % CAMS.len()]);
            }
            if let Some(base) = &still {
                let mut frame = base.clone();
                draw_pause_lines(&mut frame, &lines);
                shown = true;
                if !view.show(&mut frame, &[]) {
                    break false;
                }
            }
        }
        if !shown && !view.page(&lines) {
            break false;
        }
        std::thread::sleep(Duration::from_millis(16));
    };
    // Whatever was pressed in here must not carry into the game.
    view.reset();
    keep
}

/// Whether a dead snap is worth running back: fifteen yards of offence, six
/// points, a return that came out of a turnover, or any flag at all.
///
/// A penalty is always worth a look: the flag is the one thing on the field a
/// player cannot see for himself from the far sideline, and the snap is where
/// the foul happened.
fn worth_a_replay(sim: &PlaySim) -> bool {
    sim.result
        .as_ref()
        .map(|r| {
            r.touchdown
                || r.yards >= 15.0
                || r.return_yards >= 15.0
                || r.penalty.is_some()
                || sim.flag.is_some()
        })
        .unwrap_or(false)
}

/// Run the taped snap back over the broadcast camera. A (or START) skips it.
///
/// The tape holds the played frames themselves rather than re-running the snap:
/// the player's throws, moves and stick work are all in it, and a re-simulation
/// would have none of them and would disagree with the result it is replaying.
///
/// A snap with a flag on it is reviewed where the flag is. The call is what the
/// tape is being run for, so the shot is cut to the spot of the foul rather than
/// following the big play it happened on.
fn play_back(
    tape: &[PlaySim],
    broadcast: &mut Broadcast,
    view: &mut View,
    pad: Option<&mut Pad>,
) -> bool {
    let flag = tape
        .iter()
        .rev()
        .find_map(|snap| snap.flag.as_ref())
        .map(|call| nopads_core::math::Vec2::new(call.spot_of_foul, 0.0));
    let (mode, label) = if flag.is_some() {
        (CameraMode::Replay, "THE FLAG")
    } else {
        (CameraMode::Broadcast, "REPLAY")
    };
    broadcast.pin_focus(flag);
    let done = play_back_tape(tape, broadcast, view, pad, mode, label);
    broadcast.pin_focus(None);
    done
}

fn play_back_tape(
    tape: &[PlaySim],
    broadcast: &mut Broadcast,
    view: &mut View,
    pad: Option<&mut Pad>,
    mode: CameraMode,
    label: &str,
) -> bool {
    let ui = PassUi {
        focus: None,
        badges: Vec::new(),
        guide: None,
        power: None,
        short: false,
        needed: None,
    };
    let screen = broadcast;
    play_back_frames(tape, view, pad, label, false, |snap| {
        screen.live_frame_ui(snap, mode, &ui)
    })
}

/// A return that goes the distance gets the same second look a snap gets. It was
/// the one score in the game with no replay on it: a throw-off had no tape at all,
/// so a hundred-yard return went straight to the card.
///
/// It is the only throw-off that gets one, and the tape starts at the catch, so the
/// replay is the return itself, run all the way through rather than sampled.
fn play_back_throw_off(
    tape: &[ThrowOffSim<'_>],
    broadcast: &mut Broadcast,
    view: &mut View,
    pad: Option<&mut Pad>,
) -> bool {
    let ui = PassUi {
        focus: None,
        badges: Vec::new(),
        guide: None,
        power: None,
        short: false,
        needed: None,
    };
    let screen = broadcast;
    play_back_frames(tape, view, pad, "REPLAY", true, |snap| {
        screen.throw_off_frame_live(snap, CameraMode::Broadcast, &ui)
    })
}

/// The tape itself: the frames that were played, run back at the rate they were
/// played at, with the label plate in the corner.
///
/// A `whole` replay plays every frame in order. A return that scores is shown that
/// way, from the catch to the whistle: sampling it down to keep the second look
/// short was what made the picture appear to start and then jump to the end of the
/// play. A long snap is still sampled, so a replay cannot outlast the game.
fn play_back_frames<T>(
    tape: &[T],
    view: &mut View,
    mut pad: Option<&mut Pad>,
    label: &str,
    whole: bool,
    mut draw: impl FnMut(&T) -> nopads_render::raster::Frame,
) -> bool {
    let started = Instant::now();
    let mut next = Instant::now();
    // Six seconds of tape is a replay, forty is the whole game again: a snap that
    // ran long is sampled rather than run back for as long as it took.
    let step = if whole { 1 } else { (tape.len() / 360).max(1) };
    let take = if whole { tape.len() } else { 360 };
    for snapshot in tape.iter().step_by(step).take(take) {
        if started.elapsed() > Duration::from_millis(400) {
            let input = pad::mix(pad.as_deref_mut(), view.poll());
            if input.pressed.a || input.pressed.start {
                return true;
            }
        }
        // The tape was recorded one frame per fixed step, so play it back at
        // the same rate.
        loop {
            if Instant::now() >= next {
                next += Duration::from_millis(16);
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
            if started.elapsed() > Duration::from_millis(400) {
                let input = pad::mix(pad.as_deref_mut(), view.poll());
                if input.pressed.a || input.pressed.start {
                    return true;
                }
            }
        }
        let mut frame = draw(snapshot);
        let tw = nopads_render::font::text_width(label, 3) as i32;
        frame.fill_rect_alpha(12, 12, tw + 22, 30, hud::INK, 0.75);
        frame.fill_rect(12, 12, 4, 30, hud::RED);
        frame.text_shadowed(23, 20, label, 3, hud::WHITE, 1);
        let lines = vec!["REPLAY   [A] SKIP".to_string()];
        if !view.show(&mut frame, &lines) {
            return false;
        }
    }

    // The whistle is not the end of the replay. Hold on the last frame a beat so
    // the play reads as finished rather than cut off mid-stride, then sink to
    // black and hand the picture back to the call sheet.
    let Some(last) = tape.last() else {
        return true;
    };
    let mut frame = draw(last);
    let tw = nopads_render::font::text_width("END OF PLAY", 3) as i32;
    frame.fill_rect_alpha(12, 12, tw + 22, 30, hud::INK, 0.75);
    frame.fill_rect(12, 12, 4, 30, hud::RED);
    frame.text_shadowed(23, 20, "END OF PLAY", 3, hud::WHITE, 1);
    if !view.show(&mut frame, &["REPLAY DONE   [A] CONTINUE".to_string()]) {
        return false;
    }
    let hold = Instant::now();
    while hold.elapsed() < Duration::from_millis(1200) {
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.a || input.pressed.start {
            break;
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    // A cut on the whistle reads as the game having fallen over, so the picture
    // goes down to the ink over a quarter of a second first.
    let fade = Instant::now();
    while fade.elapsed() < Duration::from_millis(280) {
        let t = (fade.elapsed().as_secs_f32() / 0.28).clamp(0.0, 1.0);
        let mut frame = draw(last);
        frame.fill_rect_alpha(0, 0, frame.w as i32, frame.h as i32, hud::INK, t);
        if !view.show(&mut frame, &[]) {
            return false;
        }
    }
    true
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
    my_team: usize,
    board: &hud::Scoreboard,
) -> bool {
    let mut acc = 0.0f64;
    let mut last = Instant::now();
    let mut drew = Instant::now() - FRAME;
    // The button being held for a pass: which receiver, and for how long. The
    // meter fills while it is down and the ball leaves when it comes up.
    let mut charge: Option<(usize, f32)> = None;
    // When the ball died, so the picture can hold on the tackle before the card.
    let mut dead_at: Option<Instant> = None;
    // When the play turned round on an interception, so the screen can swing.
    let mut turn: Option<Instant> = None;
    // The snap frame by frame, for the replay.
    let mut tape: Vec<PlaySim> = Vec::new();

    loop {
        let now = Instant::now();
        let frame_dt = now.duration_since(last).as_secs_f64();
        acc += frame_dt;
        last = now;

        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.start {
            if !pause_menu(view, pad.as_deref_mut(), cam, broadcast, PausedShot::Play(&*sim)) {
                return false;
            }
            // The whole pause was spent inside the wall clock: do not run the play
            // forward by it.
            last = Instant::now();
            acc = 0.0;
            continue;
        }
        // SELECT cycles the camera: the trigger is not a camera button any more.
        if input.pressed.select {
            *cam = (*cam + 1) % CAMS.len();
        }
        let view_mode = CAMS[*cam % CAMS.len()];

        // Before the snap the pad belongs to the line, not to the quarterback: A
        // goes, X opens the audible, Y flips the call, LB barks a hard count and
        // holding LT shows the whole route tree until it is let go.
        if sim.phase == Phase::PreSnap {
            sim.set_preview(input.held.preview);
            if input.pressed.dummy {
                sim.dummy_audible();
            }
            if input.pressed.y {
                sim.flip_play();
            }
            if input.pressed.x {
                let current = sim.play.clone();
                // An audible changes the play, not the formation: the men are
                // already lined up, and re-shaping the huddle at the line is a
                // different thing from checking to another call out of it.
                let list = plays_in(book, current.formation);
                match audible_popup(
                    sim,
                    broadcast,
                    view,
                    pad.as_deref_mut(),
                    view_mode,
                    header,
                    book,
                    &list,
                    &current,
                ) {
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
        } else {
            sim.set_preview(false);
        }
        let presnap = sim.phase == Phase::PreSnap;
        // A running play has no throw to weigh, so it has no meter, no badges and
        // no guide. The face buttons are the ball carrier's moves from the snap.
        let run_call = sim.play.family.is_run();

        // Once the ball is in a runner's hands and there is no pass to throw, the
        // face buttons are his move set rather than the receivers'.
        let returning = sim.phase == Phase::Return;
        let running = !presnap && !returning && sim.face_buttons_are_moves();
        let targets = if running || presnap || returning || run_call {
            Vec::new()
        } else {
            sim.eligible_targets()
        };
        // Which way the stick points in the world depends on the shot on screen:
        // the sideline camera and the two behind-the-ball ones disagree about
        // which axis runs up it.
        let stick = stick_to_world(input.stick, flip_y, view_mode, sim.facing());
        let held = [input.held.a, input.held.b, input.held.x, input.held.y];
        let down = [input.pressed.a, input.pressed.b, input.pressed.x, input.pressed.y];
        if running {
            // A pass wound up before he crossed the line is not a throw any more.
            charge = None;
            let set = moves_for(sim);
            for (slot, kind) in set.iter().enumerate() {
                if down[slot] {
                    sim.request_move(*kind);
                }
            }
        }

        // Holding a receiver's button winds the pass up. The ball goes to the man
        // the button names, led by the sim rather than by the stick, so the only
        // thing being decided here is how hard it gets there.
        if let Some((slot, held_for)) = charge {
            if slot >= targets.len() {
                charge = None;
            } else {
                charge = Some((slot, held_for + frame_dt as f32));
            }
        }
        if charge.is_none() && !running && !run_call {
            if let Some(slot) = (0..targets.len().min(PASS_LABELS.len())).find(|&s| down[s]) {
                charge = Some((slot, 0.0));
            }
        }

        // Letting go throws it at whatever weight the meter got to.
        let mut release: Option<(usize, f32)> = None;
        if let Some((slot, held_for)) = charge {
            if !held[slot] {
                if held_for < PUMP_TAP {
                    // A tap is not a throw: the arm comes forward and the ball
                    // stays in his hand. Tap twice and he pumps twice.
                    sim.pump_fake();
                } else {
                    let power = (held_for / CHARGE_TIME).clamp(MIN_POWER, 1.0);
                    release = Some((targets[slot].1, power));
                }
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
        }));

        // An interception turns the play round: the picture swings, the pad goes
        // to the man who took it if he is ours, and then he runs.
        if returning {
            if turn.is_none() {
                turn = Some(now);
                if my_team == sim.setup.defense_team {
                    sim.set_human_defender(sim.returner_index());
                }
            }
            sim.set_def_control(Some(DefControl {
                move_dir: stick,
                tackle: false,
                dive: false,
                swat: false,
                intercept: false,
            }));
            // The man running it back is a runner, so the four face buttons are the
            // four moves: a pick return used to be the one carry in the game where
            // the buttons did nothing.
            if sim.returner_index().is_some() && sim.returner_index() == sim.human_defender {
                let set = moves_for(sim);
                for (slot, kind) in set.iter().enumerate() {
                    if down[slot] {
                        sim.request_move(*kind);
                    }
                }
            }
        }
        let turning = turn
            .map(|since| now.duration_since(since) < TURNAROUND)
            .unwrap_or(false);
        if !turning {
            while acc >= STEP as f64 && sim.result.is_none() {
                sim.step(STEP);
                acc -= STEP as f64;
                // The tape starts with the snap: the shuffle at the line is not
                // the part of the play anybody rewinds for.
                if sim.phase != Phase::PreSnap && tape.len() < 2400 {
                    tape.push(sim.clone());
                }
            }
        } else {
            acc = 0.0;
        }

        // The word over a dead ball arrives over half a second rather than cutting
        // onto the frame the play ended.
        broadcast.set_announce(
            dead_at
                .map(|since| (since.elapsed().as_secs_f32() / ANNOUNCE_EASE).clamp(0.0, 1.0))
                .unwrap_or(1.0),
        );

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
                    let plan = sim.throw_plan(sim.qb, *index, power);
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
            // Who the pad is on: the man with the ball, or the quarterback before
            // there is one.
            let focus = if returning && my_team != sim.setup.defense_team {
                None
            } else {
                sim.carrier.or(Some(sim.qb)).map(|i| Focus {
                    at: Vec3::new(sim.players[i].pos.x, sim.players[i].pos.y, 0.0),
                    label: "YOU".to_string(),
                })
            };
            let ui = PassUi {
                focus,
                badges,
                guide,
                power: if run_call {
                    None
                } else {
                    charge.map(|(_, t)| (t / CHARGE_TIME).clamp(0.0, 1.0))
                },
                short,
                needed,
            };

            let mut frame = broadcast.live_frame_ui(sim, view_mode, &ui);
            // The scoreboard is a broadcast bug: it gets out of the way when the
            // ball is hiked, and while the route tree is up for the preview, and
            // comes back when the play is dead and reset.
            if sim.phase == Phase::PreSnap && !sim.previewing() {
                hud::draw_scoreboard(&mut frame, board);
            }
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

            if returning {
                if sim.returner_index().is_some() && sim.returner_index() == sim.human_defender {
                    // He is a runner now, so he gets the runner's row.
                    let set = moves_for(sim);
                    let mut row = format!(
                        "  RETURN IT   [A] {}   [B] {}   [X] {}   [Y] {}",
                        set[0].label(),
                        set[1].label(),
                        set[2].label(),
                        set[3].label()
                    );
                    if let Some(kind) = sim.carrier_move().kind() {
                        row.push_str(&format!("      {}", kind.label()));
                    }
                    lines.push(row);
                } else if my_team == sim.setup.defense_team {
                    lines.push(
                        "  INTERCEPTION - THE BALL IS YOURS   MOVE TO RUN IT BACK   [START] PAUSE"
                            .to_string(),
                    );
                } else {
                    lines.push(
                        "  INTERCEPTION - THEY HAVE IT AND THE PICTURE HAS TURNED ROUND".to_string(),
                    );
                }
            } else if running {
                // The moves, and which one is in flight right now.
                let set = moves_for(sim);
                let mut row = format!(
                    "  [A] {}   [B] {}   [X] {}   [Y] {}",
                    set[0].label(),
                    set[1].label(),
                    set[2].label(),
                    set[3].label()
                );
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
                    "  [A] SNAP   [X] AUDIBLE   [Y] FLIP   HOLD [LT] PREVIEW   [LB] DUMMY COUNT   [SELECT] {}",
                    cam_name(view_mode)
                ));
            } else {
                lines.push(format!(
                    "{off_abbr} ball   MOVE   HOLD A/B/X/Y TO PASS   TAP TO PUMP   [SELECT] {}   [START] PAUSE",
                    cam_name(view_mode)
                ));
            }
            // A live-ball foul does not stop anything: the flag is on the ground and
            // the ball moves when the play is over.
            // The clock at the line, where the player can see it while the ball is
            // still his to snap: the scoreboard bar is the game's clock, not this.
            if matches!(sim.phase, Phase::PreSnap) {
                lines.push(format!("  PLAY CLOCK  {:.0}", sim.play_clock.ceil()));
            }
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
            // Let the play come to rest: the bodies coast, the ball settles. The
            // card comes up after that, not on top of a frozen picture.
            sim.settle(frame_dt as f32);
            let waited = since.elapsed();
            if waited >= DEAD_BALL_HOLD || (waited >= DEAD_BALL_SKIP && input.pressed.a) {
                // A big play gets a second look before the card comes up. The
                // tape is the play as it was played, not a re-run of it.
                if worth_a_replay(sim) && view.is_window()
                    && !play_back(&tape, broadcast, view, pad.as_deref_mut())
                {
                    return false;
                }
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
#[cfg(test)]
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

/// Where the three-card window of an audible starts.
fn popup_window(pick: usize) -> usize {
    pick / POPUP_CARDS * POPUP_CARDS
}

/// A small card drawn over the live picture. The full sheet used to take the
/// field away, which meant a player checking out of a call could not see the look
/// he was checking out of.
fn draw_popup(frame: &mut nopads_render::Frame, title: &str, cards: &[String], pick: usize) {
    use nopads_core::math::Rgb;
    let ink = Rgb::hex(0x0E1116);
    let accent = Rgb::hex(0xFFC440);
    let paper = Rgb::new(232, 238, 246);
    let faded = Rgb::new(150, 164, 184);
    let w = 392i32;
    let h = 66i32;
    let x = (frame.w as i32 - w) / 2;
    let y = frame.h as i32 - 104;
    frame.fill_rect_alpha(x, y, w, h, ink, 0.88);
    frame.fill_rect(x, y, w, 2, Rgb::hex(0x345C9C));
    frame.text(x + 8, y + 7, title, 1, accent);
    let first = popup_window(pick);
    let last = (first + POPUP_CARDS).min(cards.len());
    let card_w = (w - 16) / POPUP_CARDS as i32;
    for (n, i) in (first..last).enumerate() {
        let cx = x + 8 + n as i32 * card_w;
        if i == pick {
            frame.fill_rect_alpha(cx, y + 20, card_w - 4, 26, accent, 0.22);
            frame.fill_rect(cx, y + 20, 2, 26, accent);
        }
        frame.text(
            cx + 6,
            y + 28,
            &cards[i],
            1,
            if i == pick { accent } else { paper },
        );
    }
    frame.text(x + 8, y + 52, "[STICK] CYCLE   [A] TAKE IT   [B] BACK", 1, faded);
}

/// The offensive audible, over the picture: three plays at a time, the field
/// still on screen behind it.
fn audible_popup(
    sim: &mut PlaySim,
    broadcast: &mut Broadcast,
    view: &mut View,
    mut pad: Option<&mut Pad>,
    view_mode: CameraMode,
    header: &str,
    book: &[Play],
    list: &[usize],
    current: &Play,
) -> Audible {
    if list.is_empty() {
        return Audible::Back;
    }
    let mut pick = list
        .iter()
        .position(|&i| book[i].name == current.name)
        .unwrap_or(0);
    let mut flip = current.flip;
    let mut nav = Nav::default();
    let mut last = Instant::now();
    // A button still held from the line must not take the first card.
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let cards: Vec<String> = list
            .iter()
            .map(|&i| format!("{}  {}", book[i].name, book[i].family.label()))
            .collect();
        let mut frame = broadcast.live_frame(sim, view_mode);
        draw_popup(
            &mut frame,
            if flip { "AUDIBLE - FLIPPED" } else { "AUDIBLE" },
            &cards,
            pick,
        );
        let called = &book[list[pick]];
        let lines = vec![
            header.to_string(),
            format!(
                "  {} - {} - TAKE IT OR BACK OUT, THE LOOK STAYS ON THE FIELD",
                called.name,
                called.formation.label()
            ),
        ];
        if !view.show(&mut frame, &lines) {
            return Audible::Quit;
        }
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.start || input.pressed.b || input.pressed.x {
            return Audible::Back;
        }
        if input.pressed.y {
            flip = !flip;
        }
        if let Some(dir) = nav.step(input.stick, dt) {
            match dir {
                Dir::Right => pick = (pick + 1) % list.len(),
                Dir::Left => pick = (pick + list.len() - 1) % list.len(),
                _ => {}
            }
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

enum DefenseAudible {
    Picked(DefCall),
    Back,
    Quit,
}

/// The defensive check, the same card over the same picture: three calls at a
/// time, cycled with the stick.
fn defense_popup(
    sim: &mut PlaySim,
    broadcast: &mut Broadcast,
    view: &mut View,
    mut pad: Option<&mut Pad>,
    view_mode: CameraMode,
    header: &str,
    book: &[DefCall],
) -> DefenseAudible {
    if book.is_empty() {
        return DefenseAudible::Back;
    }
    let mut pick = book
        .iter()
        .position(|d| d.name == sim.def.name)
        .unwrap_or(0);
    let mut nav = Nav::default();
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let cards: Vec<String> = book
            .iter()
            .map(|d| format!("{}  {}", d.name, d.coverage.label()))
            .collect();
        let mut frame = broadcast.live_frame(sim, view_mode);
        draw_popup(&mut frame, "DEFENSIVE CALL", &cards, pick);
        let lines = vec![
            header.to_string(),
            format!(
                "  {} - {} - CHECK TO IT OR STAY IN WHAT YOU HAVE",
                book[pick].name,
                book[pick].coverage.label()
            ),
        ];
        if !view.show(&mut frame, &lines) {
            return DefenseAudible::Quit;
        }
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.start || input.pressed.b {
            return DefenseAudible::Back;
        }
        if let Some(dir) = nav.step(input.stick, dt) {
            match dir {
                Dir::Right => pick = (pick + 1) % book.len(),
                Dir::Left => pick = (pick + book.len() - 1) % book.len(),
                _ => {}
            }
        }
        if input.pressed.a {
            return DefenseAudible::Picked(book[pick].clone());
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
    defbook: &[DefCall],
    board: &hud::Scoreboard,
) -> bool {
    let mut acc = 0.0f64;
    let mut last = Instant::now();
    let mut drew = Instant::now() - FRAME;
    let mut dead_at: Option<Instant> = None;
    let mut turn: Option<Instant> = None;
    let mut switched = Instant::now() - Duration::from_millis(400);
    // The player has taken the pad somewhere himself; the follow leaves it there.
    let mut took_control = false;
    // The snap frame by frame, for the replay.
    let mut tape: Vec<PlaySim> = Vec::new();

    loop {
        let now = Instant::now();
        let frame_dt = now.duration_since(last).as_secs_f64();
        acc += frame_dt;
        last = now;

        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.start {
            if !pause_menu(view, pad.as_deref_mut(), cam, broadcast, PausedShot::Play(&*sim)) {
                return false;
            }
            last = Instant::now();
            acc = 0.0;
            continue;
        }
        if input.pressed.select {
            *cam = (*cam + 1) % CAMS.len();
        }
        let view_mode = CAMS[*cam % CAMS.len()];
        let stick = stick_to_world(input.stick, flip_y, view_mode, sim.facing());

        // The preview button is the same one the offense holds: it lifts the shot
        // over the whole field, and while it is down the defence's assignments are
        // on the grass instead of the offense's routes.
        sim.set_preview(sim.phase == Phase::PreSnap && input.held.preview);

        // RB checks the defence at the line: three calls at a time over the
        // picture, so the look being checked out of stays on the field.
        if sim.phase == Phase::PreSnap && input.pressed.shoulder {
            // A check stays in the front that is on the field: it is a call, not
            // a new alignment with seven men walking around before the snap.
            let calls: Vec<DefCall> = def_calls_in(defbook, sim.def.front)
                .iter()
                .map(|&i| defbook[i].clone())
                .collect();
            match defense_popup(
                sim,
                broadcast,
                view,
                pad.as_deref_mut(),
                view_mode,
                header,
                &calls,
            ) {
                DefenseAudible::Picked(call) => sim.call_defense(call),
                DefenseAudible::Back => {}
                DefenseAudible::Quit => return false,
            }
            last = Instant::now();
            acc = 0.0;
            continue;
        }

        // The ball in your hands makes you the runner, and the four buttons are the
        // four moves. B is a move then, so it does not walk the pad around.
        let my_ball = sim.returner_index().is_some() && sim.returner_index() == sim.human_defender;
        if my_ball {
            let set = moves_for(sim);
            let buttons = [input.pressed.a, input.pressed.b, input.pressed.x, input.pressed.y];
            for (slot, kind) in set.iter().enumerate() {
                if buttons[slot] {
                    sim.request_move(*kind);
                }
            }
        }
        // B walks the pad round the seven. A wraps up, X dives at him, Y picks.
        if input.pressed.b && !my_ball && now.duration_since(switched) >= Duration::from_millis(120) {
            sim.cycle_defender();
            switched = now;
            took_control = true;
        }
        // Their ball: the pad goes to the man closest to the carrier, so the player
        // is always steering somebody who can get to the play. B still overrides it,
        // and after a turnover the ball is the defence's, so the follow stands down.
        if !took_control {
            if let Some(nearest) = sim.nearest_defender_to_carrier() {
                sim.set_human_defender(Some(nearest));
            }
        }
        sim.set_def_control(Some(DefControl {
            move_dir: stick,
            tackle: input.pressed.a,
            dive: input.pressed.x,
            swat: false,
            intercept: input.pressed.y,
        }));

        // A pick turns the play round: hold the shot while the camera swings.
        if sim.phase == Phase::Return && turn.is_none() {
            turn = Some(now);
        }
        let turning = turn
            .map(|since| now.duration_since(since) < TURNAROUND)
            .unwrap_or(false);
        if !turning {
            while acc >= STEP as f64 && sim.result.is_none() {
                sim.step(STEP);
                acc -= STEP as f64;
                // The tape starts with the snap: the shuffle at the line is not
                // the part of the play anybody rewinds for.
                if sim.phase != Phase::PreSnap && tape.len() < 2400 {
                    tape.push(sim.clone());
                }
            }
        } else {
            acc = 0.0;
        }

        // The word over a dead ball arrives over half a second rather than cutting
        // onto the frame the play ended.
        broadcast.set_announce(
            dead_at
                .map(|since| (since.elapsed().as_secs_f32() / ANNOUNCE_EASE).clamp(0.0, 1.0))
                .unwrap_or(1.0),
        );

        if now.duration_since(drew) >= FRAME || sim.result.is_some() {
            drew = now;
            let focus = sim.human_defender.map(|i| Focus {
                at: Vec3::new(sim.players[i].pos.x, sim.players[i].pos.y, 0.0),
                label: "YOU".to_string(),
            });
            let ui = PassUi {
                focus,
                badges: Vec::new(),
                guide: None,
                power: None,
                short: false,
                needed: None,
            };
            let mut frame = broadcast.live_frame_ui(sim, view_mode, &ui);
            // The scoreboard is a broadcast bug: it gets out of the way when the
            // ball is hiked, and while the route tree is up for the preview, and
            // comes back when the play is dead and reset.
            if sim.phase == Phase::PreSnap && !sim.previewing() {
                hud::draw_scoreboard(&mut frame, board);
            }
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
                format!("  YOU: {role} {who}"),
            ];
            if sim.phase == Phase::Return && my_ball {
                // A pick is not a dead ball: the man who took it is a runner now, and
                // a runner gets the runner's row of moves.
                let set = moves_for(sim);
                let mut row = format!(
                    "  RETURN IT   [A] {}   [B] {}   [X] {}   [Y] {}",
                    set[0].label(),
                    set[1].label(),
                    set[2].label(),
                    set[3].label()
                );
                if let Some(kind) = sim.carrier_move().kind() {
                    row.push_str(&format!("      {}", kind.label()));
                }
                lines.push(row);
            } else if sim.phase == Phase::Return {
                lines.push(
                    "  INTERCEPTION - THEY HAVE IT AND THE PICTURE HAS TURNED ROUND".to_string(),
                );
            } else {
                if sim.previewing() {
                    lines.push(format!(
                        "  YOUR CALL: {} - {} - {} RUSH - LET GO OF [LT] TO GET BACK TO THE LINE",
                        sim.def.name,
                        sim.def.coverage.label(),
                        sim.def.rushers(),
                    ));
                } else if sim.phase == Phase::PreSnap {
                    lines.push(
                        "  WALK UP TO THE LINE   HOLD [LT] PREVIEW YOUR CALL   [RB] CHECK   [B] SWITCH   [SELECT] CAMERA"
                            .to_string(),
                    );
                } else {
                    lines.push(
                        "  [B] SWITCH DEFENDER   [A] TACKLE   [X] DIVE TACKLE   [Y] INTERCEPT   [SELECT] CAMERA   [START] PAUSE"
                            .to_string(),
                    );
                }
            }
            // The clock at the line, where the player can see it while the ball is
            // still his to snap: the scoreboard bar is the game's clock, not this.
            if matches!(sim.phase, Phase::PreSnap) {
                lines.push(format!("  PLAY CLOCK  {:.0}", sim.play_clock.ceil()));
            }
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
            // Let the play come to rest: the bodies coast, the ball settles. The
            // card comes up after that, not on top of a frozen picture.
            sim.settle(frame_dt as f32);
            let waited = since.elapsed();
            if waited >= DEAD_BALL_HOLD || (waited >= DEAD_BALL_SKIP && input.pressed.a) {
                // A big play gets a second look before the card comes up. The
                // tape is the play as it was played, not a re-run of it.
                if worth_a_replay(sim) && view.is_window()
                    && !play_back(&tape, broadcast, view, pad.as_deref_mut())
                {
                    return false;
                }
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

/// The men you can cover with, nearest the ball first.
fn men_by_distance(sim: &ThrowOffSim, my_team: usize) -> Vec<usize> {
    let mut men = human_men(sim, my_team);
    men.sort_by(|&a, &b| {
        let da = sim.players[a].pos.dist(sim.ball);
        let db = sim.players[b].pos.dist(sim.ball);
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    });
    men
}

/// The pick'th of a remembered order who is still on his feet.
fn live_man(sim: &ThrowOffSim, order: &[usize], pick: usize) -> Option<usize> {
    let live: Vec<usize> = order
        .iter()
        .copied()
        .filter(|&i| !sim.players[i].down)
        .collect();
    if live.is_empty() {
        return None;
    }
    Some(live[pick % live.len()])
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
            "  THROW   AIM WITH STICK   HOLD [A] TO LOAD   [SELECT] CAMERA   [START] PAUSE"
        );
    }
    if sim.phase == ThrowPhase::Return && sim.setup.receiving_team == my_team {
        let mut row = format!(
            "  RETURN IT   [A] {}   [B] {}   [X] {}   [Y] {}      MOVE WITH STICK   [SELECT] {cam}   [START] PAUSE",
            MOVES[0].label(),
            MOVES[1].label(),
            MOVES[2].label(),
            MOVES[3].label()
        );
        if let Some(kind) = sim.carrier_move().kind() {
            row.push_str(&format!("      {}", kind.label()));
        }
        return row;
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
            "  COVER   YOU: {who:<11} MOVE   [A] TACKLE   [X] DIVE   [B] SWITCH   [SELECT] {cam}   [START] PAUSE"
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
        "  RETURN IT   {} BALL   MOVE WITH STICK{fair}   [SELECT] {cam}   [START] PAUSE",
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
    fn the_audible_pages_three_cards_at_a_time() {
        assert_eq!(popup_window(0), 0);
        assert_eq!(popup_window(2), 0);
        assert_eq!(popup_window(3), 3);
        assert_eq!(popup_window(7), 6);
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

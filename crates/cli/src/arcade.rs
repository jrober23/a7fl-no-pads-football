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
use nopads_core::math::{Rng, Vec2, Vec3};
use nopads_core::moves::Kind;
use nopads_core::playbook::{
    def_calls_in, defensive_playbook, formations_in, fronts_in, playbook, plays_in, DefCall,
    DefFront, DefRole, Formation, OffRole, Play,
};
use nopads_core::rules::{FoulTeam, THROW_OFF_MIN_AIR_YARDS};
use nopads_core::sim::{Hit, DefControl, Phase, PlaySim, QbControl};
use nopads_core::team::{league_teams, Team};
use nopads_core::throwoff::{power_for_air, ThrowOffSetup, ThrowOffSim, ThrowPhase, ThrowRole};
use nopads_render::broadcast::{HandCamera, Broadcast, CameraMode};
use nopads_render::hud;
use nopads_render::passing::{Badge, Focus, Guide, PassUi, Tone};
use nopads_render::callsheet;
use nopads_render::defsheet;
use nopads_render::playsheet::Dir;
use nopads_render::terminal;

use crate::menu;
use nopads_platform::pad::{self, Input, Pad};
use nopads_platform::settings::Settings;
use crate::view::{View, CANVAS_H, CANVAS_W, UI_H, UI_SCALE, UI_W};

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
/// How long the picture holds while an interception turns it round.
const TURNAROUND: Duration = Duration::from_millis(700);
/// How many cards an audible shows at once over the field.
const POPUP_CARDS: usize = 3;
/// The runner's row of the text bar: the same eight actions on every carry, so
/// the bar cannot teach one set of buttons on a kick return and another from
/// scrimmage.
const RUNNER_ROW: &str =
    "   [X] DIVE   [B] SPIN   [Y] HURDLE   [LB/RB] BREAK   [RS] TRUCK / JUKE / STOP";

/// The four passing buttons, in the order the badges are drawn.
const PASS_LABELS: [&str; 4] = ["A", "B", "X", "Y"];
/// The runner's hands, and the same hands on every carry in the game: a run from
/// scrimmage, a pick return and a kick return all answer to these eight actions
/// on these buttons.
///
/// Y hurdles, B spins, X dives -- and a quarterback slides on it instead, because
/// he gives himself up rather than go head first -- the bumpers break left and
/// right, and the right stick is his body: up is a shoulder into the man in front
/// of him, a side is the jump cut, down is a dead stop.
fn runner_moves(input: &Input, mut act: impl FnMut(Kind, f32)) {
    if input.pressed.y {
        act(Kind::Hurdle, 0.0);
    }
    if input.pressed.b {
        act(Kind::Spin, 0.0);
    }
    if input.pressed.x {
        act(Kind::Dive, 0.0);
    }
    if input.pressed.dummy {
        act(Kind::Cut, -1.0);
    }
    if input.pressed.shoulder {
        act(Kind::Cut, 1.0);
    }
    if input.pressed.truck {
        act(Kind::StiffArm, 0.0);
    }
    if input.pressed.juke_l {
        act(Kind::Juke, -1.0);
    }
    if input.pressed.juke_r {
        act(Kind::Juke, 1.0);
    }
    if input.pressed.stop {
        act(Kind::Stop, 0.0);
    }
}

/// The right stick on defense is the hit stick: up goes over the nameplate, down
/// goes under it. It is the same stick that is the carrier's body on offense, so
/// this is only ever read for a man in coverage, and a tackle with the stick left
/// alone is the ordinary wrap-up.
fn hit_stick(input: &Input) -> Option<Hit> {
    if input.held.truck {
        Some(Hit::High)
    } else if input.held.stop {
        Some(Hit::Low)
    } else {
        None
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
        // The replay's lens stands in the same place, so it takes the same
        // mapping: sending its pan through the sideline rule walked the camera
        // across the field when the stick was pushed up it.
        CameraMode::Classic | CameraMode::SkyCam | CameraMode::Replay => {
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
    /// Halftime is a presentation, not a question: it runs once.
    halftime_shown: bool,
    /// This game is one fixture of a season. A season fixture is one game: there
    /// is no rematch after it and no front end inside it, because the season
    /// screen is what comes next and it needs the result.
    series: bool,
    /// A game, or one of the drills under MORE MODES.
    mode: Mode,
    /// The tutorial's step, and the gauntlet's wave and points. They are the
    /// drill's own scoreboard: a drill has no clock and no score on the bug.
    drill_step: usize,
    drill_wave: u32,
    /// The gauntlet's two cards, which side of the ball the tutorial teaches, and
    /// which side it is teaching right now.
    gauntlet: [u32; 2],
    track: Track,
    teaching: Track,
    /// Whether the gunners are taught to contain the returner.
    contain: bool,
    /// Who gets the first return in the gauntlet, from the opening toss.
    first_return: usize,
}

/// What the player asked for at the final whistle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AfterFinal {
    /// The same two clubs, on the same ground, from zero.
    Again,
    /// Back to the front end.
    Menu,
}

/// What the arcade does when a game is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AfterGame {
    /// Leave the arcade and give the view back to whoever started the game.
    Finish,
    /// The same two clubs, from zero.
    Again,
    /// Back to the front end, inside the arcade.
    Menu,
}

/// One game of a season is over when it is over: no rematch, no menu, just the
/// result and the view back.
fn after_game(after: Option<AfterFinal>, series: bool, menu_requested: bool) -> AfterGame {
    if series {
        return AfterGame::Finish;
    }
    match after {
        Some(AfterFinal::Again) => AfterGame::Again,
        Some(AfterFinal::Menu) => AfterGame::Menu,
        None if menu_requested => AfterGame::Menu,
        None => AfterGame::Finish,
    }
}

impl Arcade {
    pub fn new(game: GameState, pad: Option<Pad>, view: View) -> Self {
        let book = playbook();
        let defbook = defensive_playbook();
        let teams = [game.teams[0].clone(), game.teams[1].clone()];
        let broadcast = Broadcast::new(CANVAS_W, CANVAS_H, teams);
        Arcade {
            halftime_shown: false,
            series: false,
            mode: Mode::Game,
            drill_step: 0,
            drill_wave: 0,
            gauntlet: [0, 0],
            track: Track::Offense,
            teaching: Track::Offense,
            contain: true,
            first_return: 0,
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

    /// One of the modes under MORE MODES rather than a game. A drill takes the
    /// same pad, the same picture and the same snap loop, and plays no clock.
    pub fn with_mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    /// The book the player runs, both ways: his club's unless he picked another.
    /// A book is the whole offense and the whole defense.
    pub fn with_book(mut self, book: nopads_core::playbook::Book) -> Self {
        self.book = nopads_core::playbook::playbook_for(book);
        self.defbook = nopads_core::playbook::defensive_playbook_for(book);
        self
    }

    /// Which side of the ball the tutorial teaches.
    pub fn with_track(mut self, track: Track) -> Self {
        self.track = track;
        self
    }

    /// Whether the gunners are taught to contain the returner. They are on every
    /// difficulty but easy, which is where the return is the reward.
    pub fn with_contain(mut self, contain: bool) -> Self {
        self.contain = contain;
        self
    }

    /// Which club gets the first return of the gauntlet, from the opening toss.
    pub fn with_first_return(mut self, side: usize) -> Self {
        self.first_return = side.min(1);
        self
    }

    /// The score on the board, for whoever asked for the game.
    pub fn final_score(&self) -> (u32, u32) {
        (self.game.score[0], self.game.score[1])
    }

    /// True when the game ran to the final whistle rather than being quit.
    pub fn finished(&self) -> bool {
        self.game.phase == GamePhase::GameOver
    }

    /// Hand the view and the pad back, so the front end can run again after a
    /// game: a season is a string of them.
    pub fn into_parts(self) -> (View, Option<Pad>) {
        (self.view, self.pad)
    }

    /// This game is one fixture of a season rather than a standalone game.
    pub fn for_series(mut self, on: bool) -> Self {
        self.series = on;
        self
    }

    pub fn run(&mut self) {
        let terminal_mode = !self.view.is_window();
        if terminal_mode {
            print!("{}", terminal::enter());
            io::stdout().flush().ok();
        }
        loop {
            // A drill is a session, not a game: it runs once and hands the view
            // straight back to the front end, with no final card and no rematch.
            if self.mode.is_drill() {
                self.drill();
                break;
            }
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
            //
            // A season fixture is the exception: it is one game, and the season
            // screen is what comes next. This is where a season game used to start
            // itself over -- the arcade took every final whistle for an invitation
            // to play again, so the result never reached the table.
            match after_game(after, self.series, BACK_TO_MENU.load(Ordering::Relaxed)) {
                AfterGame::Finish => break,
                AfterGame::Again => self.restart(),
                AfterGame::Menu => {
                    if !self.reopen_menu() {
                        break;
                    }
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
            Some(menu::Front::Friendly(kick)) => kick,
            // A season is run from the top of the program, not from inside a
            // game: leaving one to the front end ends it here.
            _ => return false,
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

    // ------------------------------------------------------------------
    // The modes under MORE MODES
    // ------------------------------------------------------------------

    fn drill(&mut self) {
        match self.mode {
            Mode::Gauntlet => self.drill_returns(),
            Mode::Practice => match self.track {
                Track::Offense => self.drill_offense(false),
                Track::Defense => self.drill_defense(false),
                Track::Both => {
                    self.drill_offense(false);
                    if !self.quit {
                        self.drill_defense(false);
                    }
                }
            },
            Mode::Tutorial => match self.track {
                Track::Offense => self.drill_offense(true),
                Track::Defense => self.drill_defense(true),
                // Both: the offense first, then the defense, as two sessions in
                // one. Leaving the first leaves the tutorial, not half of it.
                Track::Both => {
                    self.drill_offense(true);
                    if !self.quit {
                        self.drill_defense(true);
                    }
                }
            },
            Mode::Game => {}
        }
    }

    /// Practice and the tutorial: the offense, snap after snap, from the same
    /// spot, with no clock, no downs and no turnover to lose. The tutorial is the
    /// same session with a list of things to do, and does not go on to the next
    /// thing until the last one has been done.
    fn drill_offense(&mut self, tutorial: bool) {
        self.teaching = Track::Offense;
        // The tutorial lines up on the ten, so the last step of it -- score --
        // is a thing a player can actually do in a couple of snaps.
        let yards_to_goal = if tutorial { 10.0 } else { 30.0 };
        let mut seed = self.game.seed ^ 0xD21_11;
        self.drill_step = 0;
        self.view.reset();
        while !self.quit {
            if tutorial {
                if self.drill_step >= TUTORIAL_OFFENSE.len() {
                    let done = vec![
                        "TUTORIAL COMPLETE".to_string(),
                        String::new(),
                        format!("  ALL {} STEPS DONE.", TUTORIAL_OFFENSE.len()),
                        String::new(),
                        "  THAT IS THE OFFENSE. THE REST OF THE CONTROLS ARE ON THE".to_string(),
                        "  CONTROLS PAGE, WHICH IS ON EVERY PAUSE MENU AND IN THE SETTINGS.".to_string(),
                        String::new(),
                        "  [A] BACK TO THE MODES".to_string(),
                    ];
                    self.drill_card(&done);
                    return;
                }
                let (title, ask) = TUTORIAL_OFFENSE[self.drill_step];
                let card = vec![
                    format!("  STEP {} OF {}   {}", self.drill_step + 1, TUTORIAL_OFFENSE.len(), title),
                    String::new(),
                    format!("  {ask}"),
                    String::new(),
                    "  [A] TO THE CALL SHEET".to_string(),
                ];
                if !self.drill_card(&card) {
                    return;
                }
            }
            let Some(play) = self.call_sheet() else {
                return;
            };
            let dir = 1i8;
            let los = nopads_core::field::spot_from_yards_to_goal(yards_to_goal, dir);
            let def = self.cpu_defense();
            let teams = [self.game.teams[0].clone(), self.game.teams[1].clone()];
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let setup = nopads_core::sim::SnapSetup {
                los,
                ball_y: 0.0,
                direction: dir,
                down: 1,
                to_go: 10.0,
                seed,
                offense_team: 0,
                defense_team: 1,
                is_try: false,
            };
            let mut sim = PlaySim::new(&teams[0], &teams[1], &play, &def, &setup);
            sim.difficulty = self.game.difficulty;
            sim.await_snap(true);
            let header = self.drill_header();
            let board = drill_board(&teams, yards_to_goal);
            let book: &[Play] = &self.book;
            let off_abbr = teams[0].abbr.clone();
            let mut caller = OffenseSnap {
                header: &header,
                off_abbr: &off_abbr,
                book,
                my_team: 0,
                charge: None,
                claimed: false,
                running: false,
                run_call: false,
                targets: Vec::new(),
            };
            let finished = snap_loop(
                &mut SnapScreen {
                    sim: &mut sim,
                    broadcast: &mut self.broadcast,
                    view: &mut self.view,
                    pad: self.pad.as_mut(),
                    cam: &mut self.cam,
                    flip_y: self.flip_y,
                },
                &mut caller,
                &header,
                &board,
            );
            if !finished {
                self.quit = true;
                return;
            }
            let Some(result) = sim.result.clone() else {
                continue;
            };
            let mut card = vec![
                self.drill_header(),
                String::new(),
                format!("  {}", result.description.to_uppercase()),
                String::new(),
                format!("  {:.0} YARDS ON THE PLAY", result.yards),
            ];
            if tutorial && tutorial_met(self.drill_step, result.outcome, result.touchdown) {
                self.drill_step += 1;
                card.push(String::new());
                if self.drill_step >= TUTORIAL_OFFENSE.len() {
                    card.push("  STEP DONE - THAT WAS THE LAST ONE".to_string());
                } else {
                    card.push(format!(
                        "  STEP DONE - NEXT: {}",
                        TUTORIAL_OFFENSE[self.drill_step].0
                    ));
                }
            } else if tutorial {
                card.push(String::new());
                card.push(format!("  NOT YET: {}", TUTORIAL_OFFENSE[self.drill_step].0));
            }
            card.push(String::new());
            card.push("  [A] NEXT SNAP".to_string());
            if !self.drill_card(&card) {
                return;
            }
        }
    }

    /// The gauntlet: the throw-off, back and forth. One club brings it back,
    /// then the other, five returns each, and the score is the drill's own. A tie
    /// after the ten -- or a lead of one -- is sudden death: more returns in
    /// pairs, and it takes a lead of two to end it.
    fn drill_returns(&mut self) {
        const RETURNS: u32 = 5;
        /// Nobody separated after twenty returns each: the drill is a draw.
        const CAP: u32 = 40;
        let mine = self.my_team.unwrap_or(0);
        let mut seed = self.game.seed ^ 0x7A11_0F;
        self.gauntlet = [0, 0];
        self.drill_wave = 0;
        let mut turn = 0u32;
        self.view.reset();
        while !self.quit {
            let returning = if turn % 2 == 0 {
                self.first_return
            } else {
                1 - self.first_return
            };
            let setup = ThrowOffSetup {
                throwing_team: 1 - returning,
                receiving_team: returning,
                seed,
                ..ThrowOffSetup::normal()
            };
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let throwing = self.game.teams[1 - returning].clone();
            let receiving = self.game.teams[returning].clone();
            let mut sim =
                ThrowOffSim::new(&setup, &throwing, &receiving).with_contain(self.contain);
            let header = self.drill_header();
            let board = drill_board(&[receiving.clone(), throwing.clone()], 80.0);
            // Both ends of the throw-off are the player's, the way they are in a
            // game: when his club is receiving he brings it back, and when the
            // other club is receiving he throws it off and they bring it back.
            sim.human_thrower = setup.throwing_team == mine;
            if !sim.human_thrower {
                // The ball is coming at you: a beat to see the field before it goes.
                sim.snap_delay = WATCH_SNAP_DELAY;
            }
            let mut caller = ThrowOffSnap {
                game: &mut self.game,
                my_team: mine,
                hold: 0.0,
                charging: false,
                aim_y: 0.0,
                armed: false,
                cover_pick: 0,
                cover_order: None,
            };
            let finished = snap_loop(
                &mut SnapScreen {
                    sim: &mut sim,
                    broadcast: &mut self.broadcast,
                    view: &mut self.view,
                    pad: self.pad.as_mut(),
                    cam: &mut self.cam,
                    flip_y: self.flip_y,
                },
                &mut caller,
                &header,
                &board,
            );
            if !finished {
                self.quit = true;
                return;
            }
            let Some(result) = sim.result.clone() else {
                turn += 1;
                continue;
            };
            let points = gauntlet_points(result.touchdown, result.return_yards);
            self.gauntlet[returning] += points;
            self.drill_wave += 1;
            turn += 1;
            let lead = (self.gauntlet[0] as i32 - self.gauntlet[1] as i32).abs();
            let both_had_five = turn >= RETURNS * 2;
            let over = both_had_five && turn % 2 == 0 && (lead >= 2 || turn >= CAP);
            let mut card = vec![
                format!(
                    "  {} POINT{} FOR {}",
                    points,
                    if points == 1 { "" } else { "S" },
                    self.game.teams[returning].abbr.to_uppercase()
                ),
                String::new(),
                format!("  {}", result.description.to_uppercase()),
                String::new(),
                format!("  {:.0} YARDS BACK", result.return_yards),
                String::new(),
                format!(
                    "  {} {} - {} {}",
                    self.game.teams[mine].abbr.to_uppercase(),
                    self.gauntlet[mine],
                    self.game.teams[1 - mine].abbr.to_uppercase(),
                    self.gauntlet[1 - mine]
                ),
            ];
            if over {
                card.push(String::new());
                card.push(format!(
                    "  GAUNTLET OVER {} - {}",
                    self.gauntlet[mine], self.gauntlet[1 - mine]
                ));
                card.push("  [A] BACK TO THE MODES".to_string());
            } else {
                if both_had_five {
                    card.push(String::new());
                    card.push("  SUDDEN DEATH - WIN BY TWO".to_string());
                }
                card.push(String::new());
                card.push("  [A] NEXT RETURN".to_string());
            }
            if !self.drill_card(&card) {
                return;
            }
            if over {
                return;
            }
        }
    }

    /// The defense tutorial: call it, stop it, cover it. The same picture and the
    /// same snap loop from the other side of the ball, with the drill's own card.
    fn drill_defense(&mut self, tutorial: bool) {
        self.teaching = Track::Defense;
        self.drill_step = 0;
        let yards_to_goal = 30.0;
        let mut seed = self.game.seed ^ 0xDEF_E115;
        self.view.reset();
        while !self.quit {
            if tutorial {
                if self.drill_step >= TUTORIAL_DEFENSE.len() {
                    let done = vec![
                        "TUTORIAL COMPLETE".to_string(),
                        String::new(),
                        format!("  ALL {} DEFENSIVE STEPS DONE.", TUTORIAL_DEFENSE.len()),
                        String::new(),
                        "  THAT IS THE DEFENSE. THE REST OF THE CONTROLS ARE ON THE".to_string(),
                        "  CONTROLS PAGE, WHICH IS ON EVERY PAUSE MENU AND IN THE SETTINGS."
                            .to_string(),
                        String::new(),
                        "  [A] BACK TO THE MODES".to_string(),
                    ];
                    self.drill_card(&done);
                    return;
                }
                let (title, ask) = TUTORIAL_DEFENSE[self.drill_step];
                let card = vec![
                    format!(
                        "  STEP {} OF {}   {}",
                        self.drill_step + 1,
                        TUTORIAL_DEFENSE.len(),
                        title
                    ),
                    String::new(),
                    format!("  {ask}"),
                    String::new(),
                    "  [A] TO THE CALL SHEET".to_string(),
                ];
                if !self.drill_card(&card) {
                    return;
                }
            }
            let Some(def) = self.def_sheet() else {
                return;
            };
            let play = self.cpu_offense();
            let dir = 1i8;
            let los = nopads_core::field::spot_from_yards_to_goal(yards_to_goal, dir);
            let teams = [self.game.teams[0].clone(), self.game.teams[1].clone()];
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let setup = nopads_core::sim::SnapSetup {
                los,
                ball_y: 0.0,
                direction: dir,
                down: 1,
                to_go: 10.0,
                seed,
                offense_team: 1,
                defense_team: 0,
                is_try: false,
            };
            let mut sim = PlaySim::new(&teams[1], &teams[0], &play, &def, &setup);
            sim.difficulty = self.game.difficulty;
            // The offense is the machine's, so the snap is its own: waiting on a
            // button here waited on a button nobody had, and the ball never went.
            sim.await_snap(false);
            sim.set_human_defender(sim.nearest_defender());
            let header = self.drill_header();
            let board = drill_board(&teams, yards_to_goal);
            let defbook: &[DefCall] = &self.defbook;
            let mut caller = DefenseSnap {
                header: &header,
                defbook,
                switched: Instant::now() - Duration::from_millis(400),
            };
            let finished = snap_loop(
                &mut SnapScreen {
                    sim: &mut sim,
                    broadcast: &mut self.broadcast,
                    view: &mut self.view,
                    pad: self.pad.as_mut(),
                    cam: &mut self.cam,
                    flip_y: self.flip_y,
                },
                &mut caller,
                &header,
                &board,
            );
            if !finished {
                self.quit = true;
                return;
            }
            let Some(result) = sim.result.clone() else {
                continue;
            };
            let mut card = vec![
                self.drill_header(),
                String::new(),
                format!("  {}", result.description.to_uppercase()),
                String::new(),
                format!("  {:.0} YARDS ALLOWED", result.yards),
            ];
            if tutorial && defense_met(self.drill_step, &result) {
                self.drill_step += 1;
                card.push(String::new());
                if self.drill_step >= TUTORIAL_DEFENSE.len() {
                    card.push("  STEP DONE - THAT WAS THE LAST ONE".to_string());
                } else {
                    card.push(format!(
                        "  STEP DONE - NEXT: {}",
                        TUTORIAL_DEFENSE[self.drill_step].0
                    ));
                }
            } else if tutorial {
                card.push(String::new());
                card.push(format!("  NOT YET: {}", TUTORIAL_DEFENSE[self.drill_step].0));
            }
            card.push(String::new());
            card.push("  [A] NEXT SNAP".to_string());
            if !self.drill_card(&card) {
                return;
            }
        }
    }

    /// A card in a drill, on the same page a result card uses. A or START goes
    /// on; B leaves the drill.
    fn drill_card(&mut self, lines: &[String]) -> bool {
        if !self.view.page(lines) {
            self.quit = true;
            return false;
        }
        let start = Instant::now();
        loop {
            let input = self.wait(Duration::from_millis(24));
            if input.pressed.b {
                self.quit = true;
                return false;
            }
            if input.pressed.a || input.pressed.start {
                return true;
            }
            if start.elapsed() >= RESULT_HOLD {
                return true;
            }
        }
    }

    /// The line over a drill's screens: what it is, and how far through it is.
    fn drill_header(&self) -> String {
        match self.mode {
            Mode::Tutorial => {
                let steps: &[(&str, &str)] = if self.teaching == Track::Defense {
                    &TUTORIAL_DEFENSE
                } else {
                    &TUTORIAL_OFFENSE
                };
                let step = self.drill_step.min(steps.len().saturating_sub(1));
                format!(
                    "TUTORIAL   {}   STEP {} OF {}   {}",
                    if self.teaching == Track::Defense {
                        "DEFENSE"
                    } else {
                        "OFFENSE"
                    },
                    step + 1,
                    steps.len(),
                    steps.get(step).map(|s| s.0).unwrap_or("DONE")
                )
            }
            Mode::Practice => format!(
                "PRACTICE   {}   NO CLOCK, NO DOWNS   THE SAME SPOT EVERY SNAP",
                if self.teaching == Track::Defense {
                    "DEFENSE"
                } else {
                    "OFFENSE"
                }
            ),
            Mode::Gauntlet => {
                let mine = self.my_team.unwrap_or(0);
                format!(
                    "THREE-ON-ONE GAUNTLET   {} {} - {} {}",
                    self.game.teams[mine].abbr.to_uppercase(),
                    self.gauntlet[mine],
                    self.game.teams[1 - mine].abbr.to_uppercase(),
                    self.gauntlet[1 - mine]
                )
            }
            Mode::Game => self.header(),
        }
    }

    fn play(&mut self) -> Option<AfterFinal> {
        if !self.kickoff_card() {
            return None;
        }
        while !self.game.is_over() && !self.quit {
            // The second half opens with the numbers and the reel, the way the
            // broadcast does, before the reversed throw-off.
            if self.game.quarter >= 3 && !self.halftime_shown {
                self.halftime_shown = true;
                if !self.halftime() {
                    return None;
                }
            }
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
                    // play defense, the other lot scoring is his business to stop,
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
                        // Their ball. Call a defense, then go and play one of the
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

    /// Stop the clock, if the rules and the allowance let it be stopped.
    ///
    /// SELECT used to cycle the camera, which was the wrong job for a button in
    /// the middle of the pad: a lens is a setting and a timeout is a decision, and
    /// the decision is the one that wants to be a single press away.
    fn call_timeout(&mut self) {
        let side = self.my_team.unwrap_or(self.game.possession);
        // The call writes its own line into the commentary; one the rules refuse
        // simply does not happen.
        let _ = self.game.use_timeout(side);
    }

    /// The matchup before the kick: who is playing, what they are good at, and who
    /// to watch. It replaces a line of text at the top of the first card with the
    /// introduction a broadcast would open on.
    fn matchup_screen(&mut self) -> bool {
        let team_lines = |teams: &Arcade, t: usize| -> Vec<String> {
            let team = &teams.game.teams[t];
            let r = team.ratings();
            vec![
                team.display_upper(),
                String::new(),
                format!("OVERALL   {}", r.overall),
                format!("OFFENSE   {}", r.offense),
                format!("DEFENSE   {}", r.defense),
                String::new(),
                if r.offense >= r.defense + 6 {
                    "THEY WILL THROW IT"
                } else if r.defense >= r.offense + 6 {
                    "THEY WILL COME AFTER YOU"
                } else {
                    "NO WEAKNESS TO PICK ON"
                }
                .to_string(),
            ]
        };
        let star_lines = |teams: &Arcade, t: usize| -> Vec<String> {
            let team = &teams.game.teams[t];
            let mut stars: Vec<(u8, String, String)> = team
                .roster
                .iter()
                .map(|p| {
                    let pos = format!("{:?}", p.pos).to_uppercase();
                    (p.overall(), format!("#{} {}", p.number, p.full_name()), pos)
                })
                .collect();
            stars.sort_by(|a, b| b.0.cmp(&a.0));
            let mut lines = vec![team.display_upper(), String::new(), "PLAYERS TO WATCH".to_string()];
            for (overall, name, pos) in stars.into_iter().take(4) {
                lines.push(format!("{name}"));
                lines.push(format!("   {}   OVERALL {}", pos, overall));
            }
            lines
        };
        // Two sets of cards, read in order: the clubs and how the units line up,
        // then the men to watch.
        let pages = [
            (
                "MATCHUP",
                team_lines(self, 0),
                team_lines(self, 1),
                "  [A] PLAYERS TO WATCH",
            ),
            (
                "PLAYERS TO WATCH",
                star_lines(self, 0),
                star_lines(self, 1),
                "  [A] KICK OFF",
            ),
        ];
        let subtitle = format!(
            "{} AT {}",
            self.game.teams[0].abbr.to_uppercase(),
            self.game.teams[1].abbr.to_uppercase()
        );
        let accents = [
            self.game.teams[0].primary.lerp(nopads_core::math::Rgb::hex(0xFFFFFF), 0.3),
            self.game.teams[1].primary.lerp(nopads_core::math::Rgb::hex(0xFFFFFF), 0.3),
        ];

        // The cards sit over the field, not over a black page. A throw-off is run
        // behind them as a warm-up — the two clubs out on the field, the ball
        // going and the coverage running down — and the lens cuts between the
        // angles while the introduction is up.
        let setup = self.throw_off_to_play(false);
        let (tside, rside) = match &setup {
            Some(s) => (s.throwing_team, s.receiving_team),
            None => (0, 1),
        };
        let throwing = self.game.teams[tside].clone();
        let receiving = self.game.teams[rside].clone();
        let mut warmup = setup
            .as_ref()
            .map(|s| ThrowOffSim::new(s, &throwing, &receiving));
        let mut mode = 0usize;
        let mut page = 0usize;
        let opened = Instant::now();
        let mut last = Instant::now();
        let mut cut = Instant::now();
        let mut acc = 0.0f64;
        self.view.reset();
        loop {
            let now = Instant::now();
            let dt = now.duration_since(last).as_secs_f32().min(0.1);
            last = now;
            if now.duration_since(cut) >= Duration::from_millis(4200) {
                cut = now;
                mode = (mode + 1) % CAMS.len();
            }
            acc += dt as f64;
            while acc >= 1.0 / 60.0 {
                let ended = match warmup.as_mut() {
                    Some(sim) => {
                        sim.step(1.0 / 60.0);
                        sim.result.is_some()
                    }
                    None => true,
                };
                acc -= 1.0 / 60.0;
                if ended {
                    // The warm-up play finished: run another from the top, the
                    // way a ground crew keeps the drill going behind a set.
                    warmup = setup
                        .as_ref()
                        .map(|s| ThrowOffSim::new(s, &throwing, &receiving));
                    acc = 0.0;
                    break;
                }
            }
            let mut frame = match &warmup {
                Some(sim) => self.broadcast.throw_off_frame_live(sim, CAMS[mode], &ui_none()),
                None => nopads_render::Frame::new(CANVAS_W, CANVAS_H),
            };
            let (title, left, right, prompt) = &pages[page];
            // The banner goes up on its own for a beat, the cards arrive on it,
            // and then it leaves: a broadcast open, not a form.
            let reveal = opened.elapsed().as_secs_f32();
            nopads_render::hud::draw_half_cards(
                &mut frame,
                title,
                &subtitle,
                left,
                right,
                accents,
                reveal,
            );
            let lines = vec![self.header(), String::new(), prompt.to_string()];
            if !self.view.show(&mut frame, &lines) {
                return false;
            }
            let input = self.input();
            if input.pressed.a || input.pressed.start {
                if page + 1 < pages.len() {
                    page += 1;
                    self.view.reset();
                } else {
                    return true;
                }
            }
            std::thread::sleep(Duration::from_millis(16));
        }
    }

    /// Halftime: the numbers first, then the reel of the half's big plays.
    fn halftime(&mut self) -> bool {
        let mut frame = nopads_render::Frame::new(CANVAS_W, CANVAS_H);
        nopads_render::hud::draw_stats_panel(&mut frame, &self.game, "HALFTIME");
        let lines = vec![self.header(), String::new(), "  [A] HIGHLIGHTS".to_string()];
        if !self.view.show(&mut frame, &lines) {
            return false;
        }
        let start = Instant::now();
        loop {
            let input = self.wait(Duration::from_millis(24));
            if input.pressed.a || input.pressed.start {
                break;
            }
            // A presentation must not be able to freeze the game: if no press
            // ever arrives, the half starts anyway.
            if start.elapsed() >= Duration::from_secs(20) {
                break;
            }
        }
        self.highlight_reel()
    }

    /// The reel: the plays that made the half, one card each over the picture.
    ///
    /// A card per play rather than a replay: the tape a snap is replayed from
    /// borrows the roster and the call sheet it was run with, so keeping one from
    /// the first quarter to the half would mean keeping the whole game alive at
    /// once. What the reel shows is the play, held long enough to read it.
    fn highlight_reel(&mut self) -> bool {
        let accent = self.game.teams[self.my_team.unwrap_or(0)]
            .primary
            .lerp(nopads_core::math::Rgb::hex(0xFFFFFF), 0.3);
        let plays = self.game.recent_plays(3);
        if plays.is_empty() {
            return true;
        }
        let count = plays.len();
        for (i, play) in plays.iter().enumerate() {
            let mut frame = nopads_render::Frame::new(CANVAS_W, CANVAS_H);
            let left = vec![play.to_uppercase()];
            nopads_render::hud::draw_card(
                &mut frame,
                "HIGHLIGHT",
                &format!("{} OF {}", i + 1, count),
                &left,
                &[],
                accent,
            );
            let lines = vec![String::new(), "  [A] NEXT".to_string()];
            if !self.view.show(&mut frame, &lines) {
                return false;
            }
            let start = Instant::now();
            loop {
                let input = self.wait(Duration::from_millis(30));
                if input.pressed.a
                    || input.pressed.start
                    || start.elapsed() >= Duration::from_millis(2600)
                {
                    break;
                }
            }
        }
        true
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

            let mut frame = nopads_render::Frame::new(UI_W, UI_H);
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
            if !self.view.show_ui(&mut frame, &lines) {
                self.quit = true;
                return None;
            }
            let input = self.input();
            if input.pressed.select {
                self.call_timeout();
                last = Instant::now();
                continue;
            }
            if input.pressed.start {
                // The call sheet is where a player spends most of the game, so this
                // is the door to the camera setting and to quitting, not an instant
                // exit the way it used to be.
                if pause_menu(
                    &mut self.view,
                    self.pad.as_mut(),
                    &mut self.cam,
                    &mut self.broadcast,
                    PausedShot::sheet(frame),
                false) == PauseAction::Quit {
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
            // A drill has no score to show, and the game it was opened with never
            // moved: the line over the sheet is the drill's own.
            if self.mode.is_drill() {
                self.drill_header()
            } else {
                self.header()
            },
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
                "  [STICK] PLAY   [A] CALL IT   [B] BACK   [Y] FLIP   [SELECT] TIMEOUT   [START] PAUSE"
                    .to_string(),
            );
        } else {
            lines.push(
                "  [STICK] FORMATION   [A] INTO THE PLAYS   [Y] FLIP   [SELECT] TIMEOUT   [START] PAUSE"
                    .to_string(),
            );
        }
        if !self.mode.is_drill() {
            if let Some(last) = self.game.recent_plays(1).pop() {
                lines.push(format!("  Last: {last}"));
            }
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
                if pause_menu(
                    &mut self.view,
                    self.pad.as_mut(),
                    &mut self.cam,
                    &mut self.broadcast,
                    PausedShot::None,
                false) == PauseAction::Quit {
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
        // The introduction comes before anything is asked: whoever is playing,
        // the first thing on the screen is who is playing.
        if !self.matchup_screen() {
            return false;
        }
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
                if pause_menu(
                    &mut self.view,
                    self.pad.as_mut(),
                    &mut self.cam,
                    &mut self.broadcast,
                    PausedShot::None,
                false) == PauseAction::Quit {
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
        // A throw-off is a live play like any other, so it is the same loop: the
        // clock, the pause, the tape, the dead-ball hold and the second look all
        // belong to the loop, and all this side owns is the pad and the bar.
        let mut caller = ThrowOffSnap {
            game: &mut self.game,
            my_team,
            hold: 0.0,
            charging: false,
            aim_y: 0.0,
            armed: false,
            cover_pick: 0,
            cover_order: None,
        };
        let finished = snap_loop(
            &mut SnapScreen {
                sim: &mut sim,
                broadcast: &mut self.broadcast,
                view: &mut self.view,
                pad: self.pad.as_mut(),
                cam: &mut self.cam,
                flip_y: self.flip_y,
            },
            &mut caller,
            &header,
            &board,
        );
        if !finished {
            self.quit = true;
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

        let mut ask_timeout = false;
        let record = game.snap_with(play, def, |sim| {
            // No automatic snap on offense: the ball goes when the player says so.
            sim.await_snap(true);
            let mut caller = OffenseSnap {
                header: &header,
                off_abbr: &off_abbr,
                book,
                my_team,
                charge: None,
                claimed: false,
                running: false,
                run_call: false,
                targets: Vec::new(),
            };
            let finished = snap_loop(
                &mut SnapScreen {
                    sim,
                    broadcast,
                    view,
                    pad: pad.as_mut(),
                    cam,
                    flip_y,
                },
                &mut caller,
                &header,
                &board,
            );
            ask_timeout = sim.timeout_asked;
            if !finished {
                *quit = true;
            }
        });
        if ask_timeout {
            self.call_timeout();
        }

        if let Some(record) = record {
            self.report(record);
        }
    }

    /// Their ball: call a defense, then put the pad on one of the seven.
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

        let mut ask_timeout = false;
        let record = game.snap_with(play, def, |sim| {
            sim.set_human_defender(sim.nearest_defender());
            let mut caller = DefenseSnap {
                header: &header,
                defbook,
                switched: Instant::now() - Duration::from_millis(400),
            };
            let finished = snap_loop(
                &mut SnapScreen {
                    sim,
                    broadcast,
                    view,
                    pad: pad.as_mut(),
                    cam,
                    flip_y,
                },
                &mut caller,
                &header,
                &board,
            );
            ask_timeout = sim.timeout_asked;
            if !finished {
                *quit = true;
            }
        });
        if ask_timeout {
            self.call_timeout();
        }

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
        // Their book, not the player's: the defense across the ball is the club
        // that is playing it, and every club plays its own.
        let book = nopads_core::playbook::defensive_playbook_for(
            self.game.teams[self.game.possession].book,
        );
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
        let pick = rng.index(book.len());
        book[pick].clone()
    }

    /// What the computer runs at a player who is playing defense: down and distance
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
        // Their book, not the player's: the other club calls its own game.
        let book =
            nopads_core::playbook::playbook_for(self.game.teams[self.game.possession].book);
        let pool: Vec<&Play> = book.iter().filter(|p| p.family.is_pass() == want_pass).collect();
        if pool.is_empty() {
            return book[rng.index(book.len())].clone();
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

            let mut frame = nopads_render::Frame::new(UI_W, UI_H);
            defsheet::draw_layered(&mut frame, &self.defbook, &fronts, front, pick, in_calls);
            let lines = self.def_sheet_lines(fronts[front], &list, pick, in_calls);
            if !self.view.show_ui(&mut frame, &lines) {
                self.quit = true;
                return None;
            }
            let input = self.input();
            if input.pressed.select {
                self.call_timeout();
                continue;
            }
            if input.pressed.start {
                if pause_menu(
                    &mut self.view,
                    self.pad.as_mut(),
                    &mut self.cam,
                    &mut self.broadcast,
                    PausedShot::sheet(frame),
                false) == PauseAction::Quit {
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
        if self.series {
            lines.push("  [A] BACK TO THE SEASON".to_string());
        } else {
            lines.push("  [A] PLAY AGAIN    [B] MAIN MENU".to_string());
        }
        if !self.view.page(&lines) {
            return None;
        }
        loop {
            let input = self.wait(Duration::from_millis(30));
            if self.series {
                // There is nothing to choose after a season fixture: any key is
                // the way back to the table.
                if input.pressed.a || input.pressed.b || input.pressed.start {
                    return Some(AfterFinal::Menu);
                }
                continue;
            }
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

/// No markers on the picture: used for the warm-up behind the matchup cards.
fn ui_none() -> PassUi {
    PassUi {
        flag: None,
        focus: None,
        badges: Vec::new(),
        guide: None,
        power: None,
        short: false,
        needed: None,
    }
}

/// What the pause screen can put behind its menu: a snap, a throw-off and a call
/// sheet all have a picture to freeze; the between-plays cards do not.
enum PausedShot<'s> {
    None,
    /// The frozen picture, drawn by whichever sim is behind the menu: a snap and
    /// a throw-off are both live plays, and the menu sits over both.
    Frozen(Box<dyn Fn(&mut Broadcast, CameraMode) -> nopads_render::Frame + 's>),
}

impl<'s> PausedShot<'s> {
    /// The picture of a live play, as its own sim draws it.
    fn live<S: SnapSim + 's>(sim: &'s S) -> Self {
        PausedShot::Frozen(Box::new(move |broadcast, mode| {
            S::frame(broadcast, sim, mode, None)
        }))
    }

    /// A picture the caller has already drawn. A call sheet's pause keeps the
    /// sheet it was opened on: the same glass over a page, rather than a page of
    /// text where the sheet used to be.
    fn sheet(frame: nopads_render::Frame) -> Self {
        PausedShot::Frozen(Box::new(move |_, _| frame.clone()))
    }
}

/// A still of the game as the lens currently sees it.
fn paused_still(
    broadcast: &mut Broadcast,
    shot: &PausedShot<'_>,
    mode: CameraMode,
) -> Option<nopads_render::Frame> {
    match shot {
        PausedShot::None => None,
        PausedShot::Frozen(draw) => Some(draw(broadcast, mode)),
    }
}

/// What the pause screen was asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PauseAction {
    /// Back to the game.
    Resume,
    /// Out of the game.
    Quit,
    /// The tape of the last snap, in the player's hands.
    Replay,
}

/// Where the pause menu's rows sit. A caller with no tape to run has no last
/// play to watch, so every row under Resume moves up one.
#[derive(Clone, Copy)]
struct PauseSlots {
    /// Whether the caller has a tape, which is what puts Replay in the list.
    can_replay: bool,
    items: usize,
    replay: usize,
    cam: usize,
    lens: usize,
    help: usize,
    leave: usize,
}

fn pause_slots(can_replay: bool) -> PauseSlots {
    let cam = if can_replay { 2 } else { 1 };
    let help = cam + 2;
    PauseSlots {
        can_replay,
        items: help + 2,
        replay: 1,
        cam,
        lens: cam + 1,
        help,
        leave: help + 1,
    }
}

/// The pause menu as data. Everything the panel says is decided by these, and
/// nothing else, which is what lets a phase of it be drawn -- and looked at --
/// without a pad in hand.
struct PausePanel<'a> {
    slots: PauseSlots,
    pick: usize,
    cam: &'a str,
    zoom: f32,
    tilt: f32,
    /// The lens sub-screen, over the frozen picture.
    tuning: bool,
    /// The quit prompt: 0 is the menu, 1 is the desktop.
    leaving: Option<usize>,
}

impl PausePanel<'_> {
    /// The panel as words. It is built once and used twice -- the picture and
    /// the terminal page -- so the two can never disagree about what it says.
    fn sheet(&self) -> PauseSheet<'_> {
        if self.tuning {
            PauseSheet {
                title: "LENS",
                rows: vec![
                    ("ZOOM", format!("{:.2}", self.zoom)),
                    ("TILT", format!("{:+.2}", self.tilt)),
                ],
                selected: None,
                hint_line: "[STICK] ZOOM / TILT   [X] RESET   [A] DONE",
            }
        } else if let Some(choice) = self.leaving {
            PauseSheet {
                title: "LEAVE THIS GAME?",
                rows: vec![
                    ("BACK TO THE MENU", String::new()),
                    ("QUIT THE GAME", String::new()),
                ],
                selected: Some(choice.min(1)),
                hint_line: "[STICK] CHOOSE   [A] SELECT   [B] STAY",
            }
        } else {
            let slots = self.slots;
            let mut rows = vec![("RESUME", String::new())];
            if slots.can_replay {
                rows.push(("REPLAY", "LAST PLAY".to_string()));
            }
            rows.push(("CAMERA", self.cam.to_string()));
            rows.push(("LENS", format!("{:.2} / {:+.2}", self.zoom, self.tilt)));
            rows.push(("CONTROLS", String::new()));
            rows.push(("LEAVE GAME", String::new()));
            PauseSheet {
                title: "PAUSED",
                rows,
                selected: Some(self.pick),
                hint_line: "[STICK] CHOOSE   [A] SELECT   [START] RESUME",
            }
        }
    }

    /// The same panel as a page of text, for the terminal, where a picture is
    /// not the thing that reads.
    fn lines(&self) -> Vec<String> {
        let sheet = self.sheet();
        let mut lines = vec![
            String::new(),
            format!("  {}", sheet.title),
            String::new(),
        ];
        for (i, (label, value)) in sheet.rows.iter().enumerate() {
            let mark = if sheet.selected == Some(i) { ">>" } else { "  " };
            if value.is_empty() {
                lines.push(format!("    {mark} {label}"));
            } else {
                lines.push(format!("    {mark} {label}    {value}"));
            }
        }
        lines.push(String::new());
        lines.push(format!("  {}", sheet.hint_line));
        lines
    }
}

/// The pause menu's words: what the picture draws and what the terminal prints.
struct PauseSheet<'a> {
    title: &'a str,
    /// Each row's label and what it currently reads. A plain action has no
    /// reading, only its name.
    rows: Vec<(&'a str, String)>,
    /// Which row the cursor is on, if the panel is a list at all.
    selected: Option<usize>,
    /// The button strip, in the bracketed form the kit parses.
    hint_line: &'a str,
}

/// The menu, set over the frozen picture rather than on a page of their own.
///
/// Drawn on the interface's own 640-wide grid and then lifted onto the picture.
/// The kit's rows, type and hint strip are designed at that size: drawn at the
/// picture's size instead, the rows sat four pixels apart and the footnotes came
/// out half the height of the row labels.
fn draw_pause(frame: &mut nopads_render::Frame, sheet: &PauseSheet<'_>) {
    use nopads_render::ui;
    // A live picture is drawn at the canvas's size; a call sheet is already on
    // the interface's own grid. Either way the panel is built at 640 wide and
    // lifted by whichever whole step the frame is drawn at.
    let step = (frame.w / UI_W).max(1);
    let mut page = frame.downsample(step);
    let s = step as i32;
    let row_h = 26;
    // Wide enough for the longest thing the panel has to say -- the quit
    // question is wider than the menu's rows -- and never wider than the screen.
    let mut w = 300;
    w = w.max(
        nopads_render::font::text_width(sheet.title, ui::T_BODY) as i32 + ui::S5 * 2 + ui::S3,
    );
    for (label, value) in &sheet.rows {
        let need = ui::S4
            + nopads_render::font::text_width(label, ui::T_LABEL) as i32
            + ui::S4
            + nopads_render::font::text_width(value, ui::T_LABEL) as i32
            + ui::S4;
        w = w.max(need);
    }
    let w = w.min(UI_W as i32 - ui::S6 * 2);
    let head = 46;
    let foot = 34;
    let h = head + sheet.rows.len() as i32 * row_h + foot;
    let x = ((UI_W as i32 - w) / 2).max(0);
    let y = ((UI_H as i32 - h) / 2).max(0);
    // The same glass as the introduction cards: a pause is another thing the
    // game puts over a live picture, so it is the same piece of glass.
    ui::glass(&mut page, x, y, w, h, ui::ACCENT, 0.86);
    page.text(x + ui::S5, y + ui::S4, sheet.title, ui::T_BODY, ui::ACCENT);
    ui::divider(&mut page, x + ui::S5, y + head - ui::S3, w - ui::S5 * 2);
    for (i, (label, value)) in sheet.rows.iter().enumerate() {
        let ry = y + head + i as i32 * row_h;
        let heat = if sheet.selected == Some(i) { 1.0 } else { 0.0 };
        ui::row(
            &mut page,
            x + ui::S3,
            ry,
            w - ui::S3 * 2,
            row_h - ui::S1,
            label,
            Some(value.as_str()),
            heat,
            ui::ACCENT,
        );
    }
    let hints = ui::parse_hints(sheet.hint_line);
    ui::hints(&mut page, x + w / 2, y + h - ui::S5, &hints);
    // Only the panel is lifted up: the rest of the picture is already drawn at
    // its own size, and resampling all of it would soften the field.
    for py in y..(y + h).min(UI_H as i32) {
        for px in x..(x + w).min(UI_W as i32) {
            frame.fill_rect(px * s, py * s, s, s, page.get(px, py));
        }
    }
}

/// The controls page over a frozen picture.
///
/// Drawn on the interface's own grid and lifted by a whole step, for the same
/// reason the pause panel is: the page's type and rows are designed at 640 wide,
/// and drawing them at the picture's size would halve its footnotes again. Only
/// the card -- and the line of paging hints under it -- is lifted, so the field
/// around it is not resampled.
fn draw_controls_over(frame: &mut nopads_render::Frame, page: usize) {
    use nopads_render::ui;
    let step = (frame.w / UI_W).max(1);
    let mut grid = frame.downsample(step);
    let (x, y, w, h) = crate::controls::draw(&mut grid, page);
    let hints = ui::parse_hints("[LEFT/RIGHT] PAGE   [A] DONE");
    ui::hints(&mut grid, UI_W as i32 / 2, y + h + 4, &hints);
    let s = step as i32;
    for py in y..(y + h + ui::S5).min(UI_H as i32) {
        for px in x..(x + w).min(UI_W as i32) {
            frame.fill_rect(px * s, py * s, s, s, grid.get(px, py));
        }
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
    shot: PausedShot<'_>,
    can_replay: bool,
) -> PauseAction {
    // The replay sits under Resume when the caller has a tape to run: the pause
    // screen of a call sheet has no play behind it.
    let slots = pause_slots(can_replay);
    let mut nav = Nav::default();
    let mut pick = 0usize;
    // The lens sub-screen: the stick on the camera, the frozen picture in front
    // of it, exactly like the lens screen in the settings.
    let mut tuning = false;
    // The controls page, over the frozen picture: which of its pages is open.
    let mut help: Option<usize> = None;
    let mut pager = crate::controls::Pager::default();
    // The quit prompt: 0 is the menu, 1 is the desktop.
    let mut leaving: Option<usize> = None;
    let mut last = Instant::now();
    // The picture as the lens last saw it, redrawn only when the lens moves.
    let mut still: Option<nopads_render::Frame> = None;
    // A button still held from the game must not select the first item.
    view.reset();
    let action = loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if let Some(page) = help {
            // The controls page has the pad while it is up: the stick and the
            // bumpers walk its four pages, and A, B or START puts it away and
            // leaves the game paused behind it.
            let mut page = page;
            if input.pressed.a || input.pressed.b || input.pressed.start {
                help = None;
            } else {
                let left = input.stick.x < -0.55 || input.pressed.dummy;
                let right = input.stick.x > 0.55 || input.pressed.shoulder;
                if let Some(turn) = pager.turn(left, right) {
                    page = crate::controls::walk(page, turn);
                    help = Some(page);
                }
            }
            let mut shown = false;
            if view.is_window() && !matches!(shot, PausedShot::None) {
                if still.is_none() {
                    still = paused_still(broadcast, &shot, CAMS[*cam % CAMS.len()]);
                }
                if let Some(base) = &still {
                    let mut frame = base.clone();
                    draw_controls_over(&mut frame, page);
                    shown = true;
                    if !view.show(&mut frame, &[]) {
                        break PauseAction::Quit;
                    }
                }
            }
            if !shown && !view.page(&crate::controls::lines(page)) {
                break PauseAction::Quit;
            }
            std::thread::sleep(Duration::from_millis(16));
            continue;
        }
        if input.pressed.start || input.pressed.b {
            // B backs out of the lens without leaving the game paused behind it.
            if tuning {
                tuning = false;
            } else if leaving.is_some() {
                leaving = None;
            } else {
                break PauseAction::Resume;
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
                break PauseAction::Quit;
            }
        } else {
            match nav.step(input.stick, dt) {
                Some(Dir::Up) => pick = (pick + slots.items - 1) % slots.items,
                Some(Dir::Down) => pick = (pick + 1) % slots.items,
                _ => {}
            }
            if input.pressed.a {
                match pick {
                    0 => break PauseAction::Resume,
                    p if can_replay && p == slots.replay => break PauseAction::Replay,
                    p if p == slots.cam => {
                        *cam = (*cam + 1) % CAMS.len();
                        still = None;
                    }
                    p if p == slots.lens => tuning = true,
                    p if p == slots.help => {
                        help = Some(0);
                        pager = crate::controls::Pager::default();
                    }
                    p if p == slots.leave => leaving = Some(0),
                    _ => {}
                }
            }
        }
        let (zoom, tilt) = broadcast.lens();
        let panel = PausePanel {
            slots,
            pick,
            cam: cam_name(CAMS[*cam % CAMS.len()]),
            zoom,
            tilt,
            tuning,
            leaving,
        };
        let lines = panel.lines();
        // The picture behind the menu, unless there is none or this is the
        // terminal, where a page of text is the only thing that reads.
        let mut shown = false;
        if view.is_window() && !matches!(shot, PausedShot::None) {
            if still.is_none() {
                still = paused_still(broadcast, &shot, CAMS[*cam % CAMS.len()]);
            }
            if let Some(base) = &still {
                let mut frame = base.clone();
                draw_pause(&mut frame, &panel.sheet());
                shown = true;
                if !view.show(&mut frame, &[]) {
                    break PauseAction::Quit;
                }
            }
        }
        if !shown && !view.page(&lines) {
            break PauseAction::Quit;
        }
        std::thread::sleep(Duration::from_millis(16));
    };
    // Whatever was pressed in here must not carry into the game.
    view.reset();
    action
}

/// How fast the tape runs. It was recorded one frame per fixed step, so it runs
/// back at the rate it was played at.
const TAPE_FPS: f32 = 60.0;
/// How much faster the bumpers scrub it.
const SCRUB_FAST: f32 = 4.0;
/// How far the stick pans the replay's camera, in yards a second.
const PAN_SPEED: f32 = 14.0;
/// How fast the right stick turns the replay's camera, in radians a second.
const ORBIT_RATE: f32 = 1.6;
/// How fast the right stick pulls the shot in and out, a second.
const ZOOM_RATE: f32 = 0.9;
const MIN_ZOOM: f32 = 0.35;
const MAX_ZOOM: f32 = 3.0;

/// Where the tape lands after one tick: `A` plays it at the rate it was recorded,
/// the bumpers run it fast either way, and the game clamps at both ends.
fn scrub(pos: f32, last: f32, playing: bool, fwd: bool, back: bool, dt: f32) -> f32 {
    let mut p = pos;
    if playing {
        p += dt * TAPE_FPS;
    }
    if fwd {
        p += dt * TAPE_FPS * SCRUB_FAST;
    }
    if back {
        p -= dt * TAPE_FPS * SCRUB_FAST;
    }
    p.clamp(0.0, last)
}

/// How a tape is being watched.
#[derive(Clone, Copy)]
enum Tape {
    /// The second look a big play gets: the play runs back once at the rate it was
    /// recorded, A skips it, and the picture sinks to black on the whistle.
    Auto { label: &'static str, whole: bool },
    /// The pause menu's theatre, where the pad has the tape and the camera.
    Hand { dir: i8, start: Vec2 },
}

/// The plate in the corner of a replay: the label, and the tab that says this is
/// tape rather than live.
fn tape_plate(frame: &mut nopads_render::raster::Frame, label: &str) {
    let tw = nopads_render::font::text_width(label, 3) as i32;
    frame.fill_rect_alpha(12, 12, tw + 22, 30, hud::INK, 0.75);
    frame.fill_rect(12, 12, 4, 30, hud::RED);
    frame.text_shadowed(23, 20, label, 3, hud::WHITE, 1);
}

/// Run a tape back, whichever way it is being watched.
///
/// The two ways are the same five things -- the frames, the plate, the bar under
/// the picture, the quit, and the rate the tape is played at -- and they were four
/// functions with a copy of each between them: the theatre, the automatic second
/// look, the flag's pinned version of it, and the throw-off's whole-tape version.
/// What actually differs is the pad. The automatic look answers A and nothing
/// else; the theatre runs, rewinds, steps and flies the camera. That is the
/// `Tape` this is handed, and it is the only branch in the loop.
///
/// Returns false only when the picture is gone, which is the one thing the game
/// cannot carry on from.
fn run_tape<T>(
    tape: &[T],
    broadcast: &mut Broadcast,
    view: &mut View,
    mut pad: Option<&mut Pad>,
    flip_y: bool,
    mode: Tape,
    mut draw: impl FnMut(&mut Broadcast, &T) -> nopads_render::raster::Frame,
) -> bool {
    if tape.is_empty() {
        return true;
    }
    let last = tape.len().saturating_sub(1);
    let started = Instant::now();
    let mut next = Instant::now();
    // Six seconds of tape is a replay, forty is the whole game again: a snap that
    // ran too long is sampled rather than run back for as long as it took.
    let (step, take) = match mode {
        Tape::Auto { whole, .. } => (
            if whole { 1 } else { (tape.len() / 360).max(1) },
            if whole { tape.len() } else { 360 },
        ),
        Tape::Hand { .. } => (1, usize::MAX),
    };
    // It opens playing, so the replay is a replay: the player can stop it with A
    // the moment he sees what he wants to look at.
    let mut playing = true;
    // Where the tape is, in frames.
    let mut pos = 0.0f32;
    let mut hand = match mode {
        Tape::Hand { start, .. } => Some(HandCamera {
            at: start,
            zoom: 1.0,
            yaw: 0.0,
        }),
        Tape::Auto { .. } => None,
    };
    let mut drawn = 0usize;
    let mut last_tick = Instant::now();
    if hand.is_some() {
        view.reset();
    }

    let keep: bool = 'tape: {
        loop {
            let now = Instant::now();
            let dt = now.duration_since(last_tick).as_secs_f32().min(0.1);
            last_tick = now;
            let mut input = pad::mix(pad.as_deref_mut(), view.poll());
            // A press in the frame the replay opens is the press that ended the
            // play, not a request to skip the look at it.
            let ready = !matches!(mode, Tape::Auto { .. })
                || started.elapsed() >= Duration::from_millis(400);

            match mode {
                Tape::Auto { .. } => {
                    if ready && (input.pressed.a || input.pressed.start) {
                        break 'tape true;
                    }
                    if drawn >= take {
                        break;
                    }
                    // Hold the rate the tape was recorded at, and keep watching the
                    // pad while the clock runs out.
                    loop {
                        if Instant::now() >= next {
                            next += Duration::from_millis(16);
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(2));
                        input = pad::mix(pad.as_deref_mut(), view.poll());
                        if started.elapsed() >= Duration::from_millis(400)
                            && (input.pressed.a || input.pressed.start)
                        {
                            break 'tape true;
                        }
                    }
                }
                Tape::Hand { dir, .. } => {
                    if input.pressed.b || input.pressed.start {
                        break 'tape true;
                    }
                    // A plays and pauses. Y and X are one frame of tape each way,
                    // and leave it stopped so the frame can be looked at.
                    if input.pressed.a {
                        playing = !playing;
                    }
                    if input.pressed.y {
                        pos = (pos + 1.0).min(last as f32);
                        playing = false;
                    }
                    if input.pressed.x {
                        pos = (pos - 1.0).max(0.0);
                        playing = false;
                    }
                    let before = pos;
                    pos = scrub(
                        pos,
                        last as f32,
                        playing,
                        input.held.shoulder,
                        input.held.dummy,
                        dt,
                    );
                    // The end of the tape stops it rather than sitting there
                    // spinning.
                    if playing && before < last as f32 && pos >= last as f32 {
                        playing = false;
                    }
                    if let Some(lens) = hand.as_mut() {
                        // The right stick is the lens: up and down are zoom, the
                        // sides are the orbit round the play.
                        if input.held.truck {
                            lens.zoom = (lens.zoom - ZOOM_RATE * dt).max(MIN_ZOOM);
                        }
                        if input.held.stop {
                            lens.zoom = (lens.zoom + ZOOM_RATE * dt).min(MAX_ZOOM);
                        }
                        if input.held.juke_l {
                            lens.yaw += ORBIT_RATE * dt;
                        }
                        if input.held.juke_r {
                            lens.yaw -= ORBIT_RATE * dt;
                        }
                        // The left stick walks the camera over the field.
                        let pan = stick_to_world(input.stick, flip_y, CameraMode::Replay, dir);
                        lens.at += pan * PAN_SPEED * dt;
                        lens.at.x = lens.at.x.clamp(-12.0, 112.0);
                        lens.at.y = lens.at.y.clamp(-32.0, 32.0);
                    }
                }
            }

            let index = match mode {
                Tape::Auto { .. } => (drawn * step).min(last),
                Tape::Hand { .. } => (pos.round() as usize).min(last),
            };
            drawn += 1;
            if let Some(lens) = hand {
                broadcast.set_hand_camera(Some(lens));
            }
            let mut frame = draw(broadcast, &tape[index]);
            let label = match mode {
                Tape::Auto { label, .. } => label,
                Tape::Hand { .. } if playing => "REPLAY",
                Tape::Hand { .. } => "REPLAY - PAUSED",
            };
            tape_plate(&mut frame, label);
            let lines = match mode {
                Tape::Auto { .. } => vec!["REPLAY   [A] SKIP".to_string()],
                Tape::Hand { .. } => vec![
                    format!("  REPLAY   FRAME {}/{last}", index + 1),
                    "  [A] PLAY / PAUSE   [RB] FAST FORWARD   [LB] REWIND   [Y] STEP FORWARD   [X] STEP BACK"
                        .to_string(),
                    "  [RS] ZOOM / ORBIT   [LS] PAN   [B] BACK TO THE GAME".to_string(),
                ],
            };
            if !view.show(&mut frame, &lines) {
                break 'tape false;
            }
            std::thread::sleep(Duration::from_millis(16));
        }

        // The whistle is not the end of the replay. Hold on the last frame a beat
        // so the play reads as finished rather than cut off mid-stride, then sink
        // to black and hand the picture back to the call sheet.
        if matches!(mode, Tape::Auto { .. }) {
            let Some(end) = tape.last() else {
                break 'tape true;
            };
            let mut frame = draw(broadcast, end);
            tape_plate(&mut frame, "END OF PLAY");
            if !view.show(&mut frame, &["REPLAY DONE   [A] CONTINUE".to_string()]) {
                break 'tape false;
            }
            let hold = Instant::now();
            while hold.elapsed() < Duration::from_millis(1200) {
                let input = pad::mix(pad.as_deref_mut(), view.poll());
                if input.pressed.a || input.pressed.start {
                    break;
                }
                std::thread::sleep(Duration::from_millis(4));
            }
            // A cut on the whistle reads as the game having fallen over, so the
            // picture goes down to the ink over a quarter of a second first.
            let fade = Instant::now();
            while fade.elapsed() < Duration::from_millis(280) {
                let t = (fade.elapsed().as_secs_f32() / 0.28).clamp(0.0, 1.0);
                let mut frame = draw(broadcast, end);
                frame.fill_rect_alpha(0, 0, frame.w as i32, frame.h as i32, hud::INK, t);
                if !view.show(&mut frame, &[]) {
                    break 'tape false;
                }
            }
        }
        true
    };

    if hand.is_some() {
        broadcast.set_hand_camera(None);
        view.reset();
    }
    keep
}

/// The replay theatre: the tape of the last snap, run by hand.
///
/// The picture is the same broadcast the game is played in, with the camera off
/// its rail: the stick pans, the right stick zooms and orbits, and the tape runs,
/// rewinds and steps a frame at a time. B or START puts the game back.
#[allow(clippy::too_many_arguments)]
fn replay_theatre<T>(
    tape: &[T],
    broadcast: &mut Broadcast,
    view: &mut View,
    pad: Option<&mut Pad>,
    flip_y: bool,
    dir: i8,
    start: Vec2,
    draw: impl FnMut(&mut Broadcast, &T) -> nopads_render::raster::Frame,
) -> bool {
    run_tape(
        tape,
        broadcast,
        view,
        pad,
        flip_y,
        Tape::Hand { dir, start },
        draw,
    )
}

/// What a dead play is worth running back, if anything.
///
/// The label says what the tape is being run for. Whether it is run at all is
/// the sim's answer, because only the sim knows what happened: a forty-yard
/// catch and a hundred-yard return are the same question asked of two plays.
#[derive(Clone, Copy)]
struct Replay {
    label: &'static str,
    /// Run the tape through rather than sampling it: a return is the run itself,
    /// not a snap that lasted forty seconds.
    whole: bool,
}

/// Everything the live loop asks of the play it is driving.
///
/// A scrimmage snap and a throw-off are the same loop: one clock, one pause, one
/// tape, one dead-ball hold, one scoreboard, one quit. What differs is the sim,
/// and this is the whole of it. Two loops used to hold two copies of the shared
/// half, and the throw-off's had already drifted: it announced nothing, its tape
/// started at the catch rather than the snap, and it never settled the picture
/// after the tackle.
trait SnapSim: Clone {
    /// Run the sim forward by a step.
    fn advance(&mut self, dt: f32);

    /// The ball is dead and the play has a result.
    fn is_done(&self) -> bool;

    /// The ball is still the player's to put in play.
    fn at_the_line(&self) -> bool;

    /// Which way the play runs on screen, for the stick mapping.
    fn facing(&self) -> i8;

    /// Where the ball is, for the camera a theatre opens on.
    fn ball_spot(&self) -> Vec2;

    /// Let the bodies come to rest while the shot holds on the tackle.
    fn settle(&mut self, _dt: f32) {}

    /// Hold the shot while something turns the play round.
    fn holds_shot(&self) -> bool {
        false
    }

    /// Whether this frame belongs on the tape. A snap starts at the hike; a
    /// return starts at the catch.
    fn taping(&self) -> bool;

    /// How many frames a tape of this play may hold.
    fn tape_cap(&self) -> usize {
        2400
    }

    /// Whether the scoreboard belongs on this frame.
    fn scoreboard_visible(&self) -> bool;

    /// The play clock, at the line where it is still the player's to spend.
    fn clock_row(&self) -> Option<String> {
        None
    }

    /// The flag row, when there is a flag on the play.
    fn flag_row(&self) -> Option<String> {
        None
    }

    /// The second look this play has earned, if it has earned one.
    fn replay_look(&self) -> Option<Replay> {
        None
    }

    /// The spot a replay pins its camera to, on the frames that have one.
    fn replay_pin(&self) -> Option<Vec2> {
        None
    }

    /// The overlay a tape frame carries: the arrow on the man a flag is about.
    fn replay_ui(&self) -> Option<PassUi> {
        None
    }

    /// The picture, with or without an overlay. `None` is the still behind the
    /// pause menu, which carries no badges at all.
    fn frame(
        broadcast: &mut Broadcast,
        sim: &Self,
        mode: CameraMode,
        ui: Option<&PassUi>,
    ) -> nopads_render::Frame;
}

impl SnapSim for PlaySim<'_> {
    fn advance(&mut self, dt: f32) {
        self.step(dt);
    }

    fn is_done(&self) -> bool {
        self.result.is_some()
    }

    fn at_the_line(&self) -> bool {
        self.phase == Phase::PreSnap
    }

    fn facing(&self) -> i8 {
        PlaySim::facing(self)
    }

    fn ball_spot(&self) -> Vec2 {
        self.ball_position().0
    }

    fn settle(&mut self, dt: f32) {
        PlaySim::settle(self, dt);
    }

    fn holds_shot(&self) -> bool {
        self.phase == Phase::Return
    }

    fn taping(&self) -> bool {
        self.phase != Phase::PreSnap
    }

    fn scoreboard_visible(&self) -> bool {
        self.phase == Phase::PreSnap && !self.previewing()
    }

    fn clock_row(&self) -> Option<String> {
        (self.phase == Phase::PreSnap)
            .then(|| format!("  PLAY CLOCK  {:.0}", self.play_clock.ceil()))
    }

    fn flag_row(&self) -> Option<String> {
        self.flag.as_ref().map(|flag| {
            format!(
                "  YELLOW FLAG - {} ON THE {}   PLAY ON, BALL MOVES AFTER IT",
                flag.kind.label().to_uppercase(),
                match flag.against {
                    FoulTeam::Offense => "OFFENSE",
                    FoulTeam::Defense => "DEFENSE",
                }
            )
        })
    }

    fn replay_look(&self) -> Option<Replay> {
        let result = self.result.as_ref()?;
        let worth = result.touchdown
            || result.yards >= 15.0
            || result.return_yards >= 15.0
            || result.penalty.is_some()
            || self.flag.is_some();
        worth.then_some(Replay {
            label: if self.flag.is_some() {
                "THE FLAG"
            } else {
                "REPLAY"
            },
            whole: false,
        })
    }

    fn replay_pin(&self) -> Option<Vec2> {
        self.flag
            .as_ref()
            .map(|call| Vec2::new(call.spot_of_foul, 0.0))
    }

    fn replay_ui(&self) -> Option<PassUi> {
        // The arrow follows the man the flag is on, frame by frame: he is running
        // while the tape plays, so a marker pinned to the spot of the foul would
        // be pointing at empty grass by the third frame.
        Some(PassUi {
            flag: self
                .flagged_player()
                .map(|i| Vec3::new(self.players[i].pos.x, self.players[i].pos.y, 0.0)),
            ..ui_none()
        })
    }

    fn frame(
        broadcast: &mut Broadcast,
        sim: &Self,
        mode: CameraMode,
        ui: Option<&PassUi>,
    ) -> nopads_render::Frame {
        match ui {
            Some(ui) => broadcast.live_frame_ui(sim, mode, ui),
            None => broadcast.live_frame(sim, mode),
        }
    }
}

impl SnapSim for ThrowOffSim<'_> {
    fn advance(&mut self, dt: f32) {
        let _ = self.step(dt);
    }

    fn is_done(&self) -> bool {
        self.result.is_some()
    }

    fn at_the_line(&self) -> bool {
        self.phase == ThrowPhase::Presnap
    }

    fn facing(&self) -> i8 {
        // Downfield with the throw, then behind the returner once the ball is his.
        if matches!(
            self.phase,
            ThrowPhase::Loose | ThrowPhase::Return | ThrowPhase::Dead
        ) {
            self.setup.receive_direction
        } else {
            self.setup.throwing_direction()
        }
    }

    fn ball_spot(&self) -> Vec2 {
        self.ball_position().0
    }

    fn taping(&self) -> bool {
        // The throw and the flight are not the return, and a replay that began
        // with them was still showing the kick when the play it was showing was
        // already over.
        self.catch_time().is_some()
    }

    fn tape_cap(&self) -> usize {
        3600
    }

    fn scoreboard_visible(&self) -> bool {
        self.phase == ThrowPhase::Presnap
    }

    fn replay_look(&self) -> Option<Replay> {
        // Only a score. A twenty-five yard return is a good return, not a big
        // play, and a replay on every one of them came round far too often to be
        // worth watching.
        self.result
            .as_ref()
            .filter(|result| result.touchdown)
            .map(|_| Replay {
                label: "REPLAY",
                whole: true,
            })
    }

    fn frame(
        broadcast: &mut Broadcast,
        sim: &Self,
        mode: CameraMode,
        ui: Option<&PassUi>,
    ) -> nopads_render::Frame {
        match ui {
            Some(ui) => broadcast.throw_off_frame_live(sim, mode, ui),
            None => {
                let none = ui_none();
                broadcast.throw_off_frame_live(sim, mode, &none)
            }
        }
    }
}

/// What the arcade is doing. `Game` is the game; the rest are the sessions under
/// MORE MODES, which take the same pad and picture without a clock on them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Game,
    Practice,
    Tutorial,
    Gauntlet,
}

impl Mode {
    /// True for the modes that are not a game.
    pub fn is_drill(self) -> bool {
        self != Mode::Game
    }
}

/// Which side of the ball a tutorial teaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    Offense,
    Defense,
    Both,
}

/// The tutorial, in order: the step, and what it asks the player to do. The check
/// that a step is done lives in `tutorial_met`.
const TUTORIAL_OFFENSE: [(&str, &str); 4] = [
    ("SNAP IT", "PRESS [A] AT THE LINE AND THE BALL GOES"),
    ("THROW IT", "CALL A PASS, HOLD A/B/X/Y, LET GO"),
    ("RUN IT", "CALL A RUN AND TAKE THE HAND-OFF"),
    ("SCORE IT", "GET THE BALL INTO THE END ZONE"),
];

/// The defense tutorial, in order: call a defense, get a stop, cover a throw.
const TUTORIAL_DEFENSE: [(&str, &str); 3] = [
    ("CALL IT", "PICK A DEFENSE ON THE CALL SHEET"),
    ("STOP IT", "MAKE THE TACKLE BEFORE THEY GET TEN"),
    ("COVER IT", "BREAK UP A PASS OR PICK IT OFF"),
];

/// Whether a defensive snap did what its tutorial step asked for.
fn defense_met(step: usize, result: &nopads_core::sim::PlayResult) -> bool {
    use nopads_core::sim::PlayOutcome;
    match step {
        // The call has been made by the time a snap is played.
        0 => true,
        1 => !result.touchdown && result.yards < 10.0,
        2 => result.incomplete || result.outcome == PlayOutcome::Interception,
        _ => false,
    }
}

/// Whether a snap did what the tutorial step asked for. Step one is the snap
/// itself, so any play that was run at all counts as having done it.
fn tutorial_met(step: usize, outcome: nopads_core::sim::PlayOutcome, touchdown: bool) -> bool {
    use nopads_core::sim::PlayOutcome;
    match step {
        0 => true,
        1 => outcome.is_pass_attempt(),
        2 => matches!(outcome, PlayOutcome::Run | PlayOutcome::QbScramble),
        3 => touchdown,
        _ => false,
    }
}

/// What a return in the gauntlet is worth: the house is six, sixty yards is two,
/// thirty is one. A gauntlet is scored in the thing it is a drill for -- how far
/// one man got with three of them coming.
fn gauntlet_points(touchdown: bool, return_yards: f32) -> u32 {
    if touchdown {
        6
    } else if return_yards >= 60.0 {
        2
    } else if return_yards >= 30.0 {
        1
    } else {
        0
    }
}

/// The bug over a drill: the two clubs, and no score or clock, because a drill
/// has neither. The down and distance are the drill's own spot, so the snap is
/// still read the way a snap is read.
fn drill_board(teams: &[nopads_core::team::Team; 2], yards_to_goal: f32) -> hud::Scoreboard {
    hud::Scoreboard {
        abbr: [teams[0].abbr.to_uppercase(), teams[1].abbr.to_uppercase()],
        primary: [teams[0].primary, teams[1].primary],
        accent: [teams[0].accent, teams[1].accent],
        score: [0, 0],
        timeouts: [0, 0],
        possession: 0,
        quarter: 1,
        clock: 0.0,
        down: 1,
        to_go: 10.0,
        yards_to_goal,
    }
}

/// One frame of a snap as the side playing it sees it: the sim, the picture, the
/// pad and the camera, bundled because a card over the field mid-play writes to
/// all four.
struct SnapScreen<'a, S> {
    sim: &'a mut S,
    broadcast: &'a mut Broadcast,
    view: &'a mut View,
    pad: Option<&'a mut Pad>,
    cam: &'a mut usize,
    flip_y: bool,
}

impl<S> SnapScreen<'_, S> {
    fn mode(&self) -> CameraMode {
        CAMS[*self.cam % CAMS.len()]
    }
}

/// What a side wants done with the frame it was just handed.
enum Flow {
    /// Carry on into this frame's controls and picture.
    Run,
    /// The frame was spent in a card over the field: restart the clock so the
    /// pause does not run the play forward by it.
    Restart,
    /// The player wants out of the game.
    Quit,
}

/// The half of a live play that depends on which side the player is on.
///
/// The clock, the pause, the replay tape, the dead-ball hold, the announce, the
/// scoreboard and the second look are the same on both sides of the ball and live
/// in `snap_loop`. This is the rest of it: what the pad means, and what the bar
/// under the picture says. Two loops used to hold two copies of the shared half,
/// and they had already drifted -- the replay was wired into both by hand, twice.
trait SnapCaller<S> {
    /// True when the side spends the left bumper at the line on itself, so the
    /// camera stays where it is until the ball is hiked.
    fn takes_dummy_at_line(&self) -> bool {
        false
    }

    /// SELECT: the timeout. On a snap it is a word to the sim, which is where the
    /// clock lives; on a throw-off it is the call itself, and it goes to the game.
    fn ask_timeout(&mut self, screen: &mut SnapScreen<'_, S>);

    /// Anything the side does before the frame's controls: previews, cards,
    /// the snap itself.
    fn before_frame(&mut self, screen: &mut SnapScreen<'_, S>, input: &Input) -> Flow;

    /// This frame's pad, as the sim wants it.
    fn controls(&mut self, screen: &mut SnapScreen<'_, S>, input: &Input, stick: Vec2, dt: f32);

    /// The badges and guide over the field, and the side's own rows of the bar.
    /// The header, the play clock and the flag are the loop's to add.
    fn picture(&mut self, screen: &mut SnapScreen<'_, S>, input: &Input) -> (PassUi, Vec<String>);
}

/// Play one live down from the ball going in to the card: a scrimmage snap or a
/// throw-off, whichever side the player is on.
///
/// Returns false when the player wants out of the game rather than out of the
/// play, which is the one thing the caller has to act on.
fn snap_loop<S: SnapSim>(
    screen: &mut SnapScreen<'_, S>,
    caller: &mut impl SnapCaller<S>,
    header: &str,
    board: &hud::Scoreboard,
) -> bool {
    let mut acc = 0.0f64;
    let mut last = Instant::now();
    let mut drew = Instant::now() - FRAME;
    // When the ball died, so the picture can hold on the tackle before the card.
    let mut dead_at: Option<Instant> = None;
    // When the play turned round on an interception, so the screen can swing.
    let mut turn: Option<Instant> = None;
    // The live play frame by frame, for the replay.
    let mut tape: Vec<S> = Vec::new();

    loop {
        let now = Instant::now();
        let frame_dt = now.duration_since(last).as_secs_f64();
        acc += frame_dt;
        last = now;

        let input = pad::mix(screen.pad.as_deref_mut(), screen.view.poll());
        if input.pressed.select {
            caller.ask_timeout(screen);
        }
        if input.pressed.start {
            match pause_menu(
                screen.view,
                screen.pad.as_deref_mut(),
                screen.cam,
                screen.broadcast,
                PausedShot::live(&*screen.sim),
                !tape.is_empty(),
            ) {
                PauseAction::Quit => return false,
                PauseAction::Replay => {
                    let keep = replay_theatre(
                        &tape,
                        screen.broadcast,
                        screen.view,
                        screen.pad.as_deref_mut(),
                        screen.flip_y,
                        screen.sim.facing(),
                        screen.sim.ball_spot(),
                        |b, snap| {
                            let ui = ui_none();
                            S::frame(b, snap, CameraMode::Replay, Some(&ui))
                        },
                    );
                    if !keep {
                        return false;
                    }
                }
                PauseAction::Resume => {}
            }
            // The whole pause was spent inside the wall clock: do not run the play
            // forward by it.
            last = Instant::now();
            acc = 0.0;
            continue;
        }
        // The camera is on the left bumper, except at the line on offense, where
        // the bumper is the hard count. SELECT is the timeout, and it is called at
        // the sheet.
        if input.pressed.dummy
            && !(screen.sim.at_the_line() && caller.takes_dummy_at_line())
        {
            *screen.cam = (*screen.cam + 1) % CAMS.len();
        }
        let view_mode = screen.mode();

        match caller.before_frame(screen, &input) {
            Flow::Quit => return false,
            Flow::Restart => {
                last = Instant::now();
                acc = 0.0;
                continue;
            }
            Flow::Run => {}
        }

        // Which way the stick points in the world depends on the shot on screen:
        // the sideline camera and the two behind-the-ball ones disagree about
        // which axis runs up it.
        let stick = stick_to_world(input.stick, screen.flip_y, view_mode, screen.sim.facing());
        caller.controls(screen, &input, stick, frame_dt as f32);

        // A pick turns the play round: hold the shot while the camera swings.
        if screen.sim.holds_shot() && turn.is_none() {
            turn = Some(now);
        }
        let turning = turn
            .map(|since| now.duration_since(since) < TURNAROUND)
            .unwrap_or(false);
        if !turning {
            while acc >= STEP as f64 && !screen.sim.is_done() {
                screen.sim.advance(STEP);
                acc -= STEP as f64;
                if screen.sim.taping() && tape.len() < screen.sim.tape_cap() {
                    tape.push(screen.sim.clone());
                }
            }
        } else {
            acc = 0.0;
        }

        // The word over a dead ball arrives over half a second rather than cutting
        // onto the frame the play ended.
        screen.broadcast.set_announce(
            dead_at
                .map(|since| (since.elapsed().as_secs_f32() / ANNOUNCE_EASE).clamp(0.0, 1.0))
                .unwrap_or(1.0),
        );

        if now.duration_since(drew) >= FRAME || screen.sim.is_done() {
            drew = now;
            let (ui, rows) = caller.picture(screen, &input);
            let mut frame = S::frame(screen.broadcast, &*screen.sim, view_mode, Some(&ui));
            // The scoreboard is a broadcast bug: it gets out of the way when the
            // ball is hiked, and while the route tree is up for the preview, and
            // comes back when the play is dead and reset.
            if screen.sim.scoreboard_visible() {
                hud::draw_scoreboard(&mut frame, board);
            }
            // The picture carries no bar of button hints any more: the controls
            // are a page, opened from the settings and from every pause menu, so
            // the ball is not read through a strip of text. A terminal has no
            // picture to read a formation off at all, so it still gets the lines
            // under it -- they are the only thing a terminal can say.
            let mut lines = Vec::new();
            if !screen.view.is_window() {
                lines = Vec::with_capacity(rows.len() + 3);
                lines.push(header.to_string());
                lines.extend(rows);
                if let Some(clock) = screen.sim.clock_row() {
                    lines.push(clock);
                }
                if let Some(flag) = screen.sim.flag_row() {
                    lines.push(flag);
                }
            }
            if !screen.view.show(&mut frame, &lines) {
                return false;
            }
        }

        if screen.sim.is_done() {
            // The ball is dead; the picture is not. Hold the shot while the hit
            // finishes and the players get up, then let the card come up.
            let since = *dead_at.get_or_insert(now);
            // Let the play come to rest: the bodies coast, the ball settles. The
            // card comes up after that, not on top of a frozen picture.
            screen.sim.settle(frame_dt as f32);
            let waited = since.elapsed();
            if waited >= DEAD_BALL_HOLD || (waited >= DEAD_BALL_SKIP && input.pressed.a) {
                // A big play gets a second look before the card comes up. The tape
                // is the play as it was played, not a re-run of it. A flag is
                // reviewed where the flag is: the call is what the tape is being
                // run for, so the shot is cut to the spot of the foul rather than
                // following the big play it happened on.
                if let Some(look) = screen.sim.replay_look().filter(|_| screen.view.is_window()) {
                    let pin = tape.iter().rev().find_map(|snap| snap.replay_pin());
                    let mode = if pin.is_some() {
                        CameraMode::Replay
                    } else {
                        CameraMode::Broadcast
                    };
                    screen.broadcast.pin_focus(pin);
                    let kept = run_tape(
                        &tape,
                        screen.broadcast,
                        screen.view,
                        screen.pad.as_deref_mut(),
                        false,
                        Tape::Auto {
                            label: look.label,
                            whole: look.whole,
                        },
                        |b, snap| {
                            let ui = snap.replay_ui();
                            S::frame(b, snap, mode, ui.as_ref())
                        },
                    );
                    screen.broadcast.pin_focus(None);
                    if !kept {
                        return false;
                    }
                }
                return true;
            }
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Your ball: the quarterback's pad, the pass meter and the audible.
struct OffenseSnap<'a> {
    header: &'a str,
    off_abbr: &'a str,
    book: &'a [Play],
    my_team: usize,
    // The button being held for a pass: which receiver, and for how long. The
    // meter fills while it is down and the ball leaves when it comes up.
    charge: Option<(usize, f32)>,
    // The interception has already been given to the player driving the play.
    claimed: bool,
    running: bool,
    run_call: bool,
    targets: Vec<(OffRole, usize)>,
}

impl<'p> SnapCaller<PlaySim<'p>> for OffenseSnap<'_> {
    /// SELECT stops the clock: the word goes to the sim, which is where a snap's
    /// clock lives.
    fn ask_timeout(&mut self, screen: &mut SnapScreen<'_, PlaySim<'p>>) {
        screen.sim.timeout_asked = true;
    }

    /// At the line the bumper is the hard count, not the camera.
    fn takes_dummy_at_line(&self) -> bool {
        true
    }

    fn before_frame(&mut self, screen: &mut SnapScreen<'_, PlaySim<'p>>, input: &Input) -> Flow {
        let mode = screen.mode();
        let sim = &mut *screen.sim;
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
                let list = plays_in(self.book, current.formation);
                match audible_popup(
                    sim,
                    screen.broadcast,
                    screen.view,
                    screen.pad.as_deref_mut(),
                    mode,
                    self.header,
                    self.book,
                    &list,
                    &current,
                ) {
                    Audible::Picked(play) => sim.call_play(play),
                    Audible::Back => {}
                    Audible::Quit => return Flow::Quit,
                }
                return Flow::Restart;
            }
            if input.pressed.a {
                sim.request_snap();
            }
        } else {
            sim.set_preview(false);
        }
        Flow::Run
    }

    fn controls(&mut self, screen: &mut SnapScreen<'_, PlaySim<'p>>, input: &Input, stick: Vec2, dt: f32) {
        let sim = &mut *screen.sim;
        let presnap = sim.phase == Phase::PreSnap;
        // A running play has no throw to weigh, so it has no meter, no badges and
        // no guide. The face buttons are the ball carrier's moves from the snap.
        self.run_call = sim.play.family.is_run();

        // Once the ball is in a runner's hands and there is no pass to throw, the
        // face buttons are his move set rather than the receivers'.
        let returning = sim.phase == Phase::Return;
        self.running = !presnap && !returning && sim.face_buttons_are_moves();
        self.targets = if self.running || presnap || returning || self.run_call {
            Vec::new()
        } else {
            sim.eligible_targets()
        };
        let held = [input.held.a, input.held.b, input.held.x, input.held.y];
        let down = [input.pressed.a, input.pressed.b, input.pressed.x, input.pressed.y];
        if self.running {
            // A pass wound up before he crossed the line is not a throw any more.
            self.charge = None;
            runner_moves(input, |kind, side| {
                if side == 0.0 {
                    sim.request_move(kind);
                } else {
                    sim.request_move_dir(kind, side);
                }
            });
        }

        // Holding a receiver's button winds the pass up. The ball goes to the man
        // the button names, led by the sim rather than by the stick, so the only
        // thing being decided here is how high it comes down: a tap is a line
        // drive, a long hold is the lob over the middle. The arm sets the speed.
        if let Some((slot, held_for)) = self.charge {
            if slot >= self.targets.len() {
                self.charge = None;
            } else {
                self.charge = Some((slot, held_for + dt));
            }
        }
        if self.charge.is_none() && !self.running && !self.run_call {
            if let Some(slot) = (0..self.targets.len().min(PASS_LABELS.len())).find(|&s| down[s]) {
                self.charge = Some((slot, 0.0));
            }
        }

        // Letting go throws it at whatever weight the meter got to.
        let mut release: Option<(usize, f32)> = None;
        if let Some((slot, held_for)) = self.charge {
            if !held[slot] {
                // A tap is now the lowest pass in the game -- hips and knees --
                // rather than a pump fake, which is what the user asked for. The
                // hold sets the arc from there.
                let power = (held_for / CHARGE_TIME).clamp(MIN_POWER, 1.0);
                release = Some((self.targets[slot].1, power));
                self.charge = None;
            }
        }

        let aimed = self
            .charge
            .map(|(slot, _)| self.targets[slot].1)
            .or_else(|| release.map(|(index, _)| index));
        let power = release.map(|(_, p)| p).unwrap_or_else(|| {
            self.charge
                .map(|(_, t)| (t / CHARGE_TIME).clamp(0.0, 1.0))
                .unwrap_or(0.0)
        });
        sim.set_control(Some(QbControl {
            move_dir: stick,
            target: aimed,
            throw: release.is_some(),
            power,
        }));

        // An interception turns the play round: the pad goes to the man who took
        // it if he is ours, and then he runs.
        if returning {
            if !self.claimed {
                self.claimed = true;
                if self.my_team == sim.setup.defense_team {
                    sim.set_human_defender(sim.returner_index());
                }
            }
            sim.set_def_control(Some(DefControl {
                move_dir: stick,
                tackle: false,
                dive: false,
                swat: false,
                intercept: false,
                hit: None,
            }));
            // The man running it back is a runner, so the four face buttons are the
            // four moves: a pick return used to be the one carry in the game where
            // the buttons did nothing.
            if sim.returner_index().is_some() && sim.returner_index() == sim.human_defender {
                runner_moves(input, |kind, side| {
                    if side == 0.0 {
                        sim.request_move(kind);
                    } else {
                        sim.request_move_dir(kind, side);
                    }
                });
            }
        }
    }

    fn picture(&mut self, screen: &mut SnapScreen<'_, PlaySim<'p>>, _input: &Input) -> (PassUi, Vec<String>) {
        let mode = screen.mode();
        let sim = &*screen.sim;
        let presnap = sim.phase == Phase::PreSnap;
        let returning = sim.phase == Phase::Return;
        let power = self
            .charge
            .map(|(_, t)| (t / CHARGE_TIME).clamp(0.0, 1.0))
            .unwrap_or(0.0);

        // A button over every eligible man, and while a pass is wound up, the
        // guide showing where it will come down and the meter showing whether the
        // arm can get it there.
        let mut badges = Vec::new();
        for (slot, (_, index)) in self.targets.iter().enumerate().take(PASS_LABELS.len()) {
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
                aimed: self.charge.map(|(s, _)| s) == Some(slot),
            });
        }

        let mut guide = None;
        let mut short = false;
        let mut needed = None;
        if let Some((slot, _)) = self.charge {
            if let Some((_, index)) = self.targets.get(slot) {
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
        let focus = if returning && self.my_team != sim.setup.defense_team {
            None
        } else {
            sim.carrier.or(Some(sim.qb)).map(|i| Focus {
                at: Vec3::new(sim.players[i].pos.x, sim.players[i].pos.y, 0.0),
                label: "YOU".to_string(),
            })
        };
        let ui = PassUi {
            flag: None,
            focus,
            badges,
            guide,
            // No meter in a scrimmage down. How far the ball goes is the
            // quarterback's arm and how accurately it arrives is his accuracy;
            // the button is the arc, so a fill bar would be telling the player
            // to hold for power he does not get from holding. The throw-off
            // keeps its own meter, because there the power *is* the throw.
            power: None,
            short,
            needed,
        };

        // The call is read off the sim, not off the sheet that was chosen a minute
        // ago: an audible changes the play, and the overlay has to say so.
        let call = format!(
            "{} - {}{} - {} shows {}",
            sim.play.name,
            sim.play.formation.label(),
            if sim.play.flip { " (FLIPPED)" } else { "" },
            screen.broadcast.teams[sim.setup.defense_team].abbr,
            sim.def.name
        );
        let mut lines = vec![call];

        if returning {
            if sim.returner_index().is_some() && sim.returner_index() == sim.human_defender {
                // He is a runner now, so he gets the runner's row.
                let mut row = format!("  RETURN IT{RUNNER_ROW}");
                if let Some(kind) = sim.carrier_move().kind() {
                    row.push_str(&format!("      {}", kind.label()));
                }
                lines.push(row);
            } else if self.my_team == sim.setup.defense_team {
                lines.push(
                    "  INTERCEPTION - THE BALL IS YOURS   MOVE TO RUN IT BACK   [START] PAUSE"
                        .to_string(),
                );
            } else {
                lines.push(
                    "  INTERCEPTION - THEY HAVE IT AND THE PICTURE HAS TURNED ROUND".to_string(),
                );
            }
        } else if self.running {
            // The moves, and which one is in flight right now.
            let mut row = RUNNER_ROW.trim_start().to_string();
            if let Some(kind) = sim.carrier_move().kind() {
                row.push_str(&format!("      {}", kind.label()));
            }
            lines.push(row);
        } else {
            // Who is open, on the buttons the badges are showing.
            let mut row = String::new();
            for (slot, (_, index)) in self.targets.iter().enumerate().take(PASS_LABELS.len()) {
                let name = screen.broadcast.teams[sim.setup.offense_team].roster[*index].last_name();
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
                cam_name(mode)
            ));
        } else {
            lines.push(format!(
                "{} ball   MOVE   HOLD A/B/X/Y TO PASS   TAP TO PUMP   [SELECT] {}   [START] PAUSE",
                self.off_abbr,
                cam_name(mode)
            ));
        }
        (ui, lines)
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
/// Their ball: call a defense, then put the pad on one of the seven.
struct DefenseSnap<'a> {
    header: &'a str,
    defbook: &'a [DefCall],
    // When the pad was last walked round the seven, so one press is one man.
    switched: Instant,
}

impl<'p> SnapCaller<PlaySim<'p>> for DefenseSnap<'_> {
    /// SELECT stops the clock: the word goes to the sim, which is where a snap's
    /// clock lives.
    fn ask_timeout(&mut self, screen: &mut SnapScreen<'_, PlaySim<'p>>) {
        screen.sim.timeout_asked = true;
    }

    fn before_frame(&mut self, screen: &mut SnapScreen<'_, PlaySim<'p>>, input: &Input) -> Flow {
        let mode = screen.mode();
        {
            let sim = &mut *screen.sim;
            // The preview button is the same one the offense holds: it lifts the
            // shot over the whole field, and while it is down the defense's
            // assignments are on the grass instead of the offense's routes.
            sim.set_preview(sim.phase == Phase::PreSnap && input.held.preview);
            // The same trigger is the technique at the line: a tap puts the
            // corners up on the receivers, another tap backs them off.
            if sim.phase == Phase::PreSnap && input.pressed.preview {
                let press = !sim.press();
                sim.set_press(press);
            }
        }

        // RB checks the defense at the line: three calls at a time over the
        // picture, so the look being checked out of stays on the field.
        if screen.sim.phase == Phase::PreSnap && input.pressed.shoulder {
            // A check stays in the front that is on the field: it is a call, not
            // a new alignment with seven men walking around before the snap.
            let front = screen.sim.def.front;
            let calls: Vec<DefCall> = def_calls_in(self.defbook, front)
                .iter()
                .map(|&i| self.defbook[i].clone())
                .collect();
            match defense_popup(
                screen.sim,
                screen.broadcast,
                screen.view,
                screen.pad.as_deref_mut(),
                mode,
                self.header,
                &calls,
            ) {
                DefenseAudible::Picked(call) => screen.sim.call_defense(call),
                DefenseAudible::Back => {}
                DefenseAudible::Quit => return Flow::Quit,
            }
            return Flow::Restart;
        }
        Flow::Run
    }

    fn controls(&mut self, screen: &mut SnapScreen<'_, PlaySim<'p>>, input: &Input, stick: Vec2, _dt: f32) {
        let sim = &mut *screen.sim;
        // The ball in your hands makes you the runner, and the four buttons are the
        // four moves. B is a move then, so it does not walk the pad around.
        let my_ball = sim.returner_index().is_some() && sim.returner_index() == sim.human_defender;
        if my_ball {
            runner_moves(input, |kind, side| {
                if side == 0.0 {
                    sim.request_move(kind);
                } else {
                    sim.request_move_dir(kind, side);
                }
            });
        }
        // B walks the pad round the seven. A wraps up, X dives at him, Y picks.
        let now = Instant::now();
        if input.pressed.b
            && !my_ball
            && now.duration_since(self.switched) >= Duration::from_millis(120)
        {
            sim.cycle_defender();
            self.switched = now;
        }
        // The pad stays on the man it was put on. It used to follow the nearest
        // defender to the carrier every frame, which took the pad off the man the
        // player had just moved: choosing who you cover with is the point of the
        // switch button, and a pad that walks itself makes the choice meaningless.
        sim.set_def_control(Some(DefControl {
            move_dir: stick,
            tackle: input.pressed.a,
            dive: input.pressed.x,
            swat: false,
            intercept: input.pressed.y,
            hit: hit_stick(input),
        }));
    }

    fn picture(&mut self, screen: &mut SnapScreen<'_, PlaySim<'p>>, input: &Input) -> (PassUi, Vec<String>) {
        let sim = &*screen.sim;
        let my_ball =
            sim.returner_index().is_some() && sim.returner_index() == sim.human_defender;
        let focus = sim.human_defender.map(|i| Focus {
            at: Vec3::new(sim.players[i].pos.x, sim.players[i].pos.y, 0.0),
            label: "YOU".to_string(),
        });
        let ui = PassUi {
            flag: None,
            focus,
            badges: Vec::new(),
            guide: None,
            power: None,
            short: false,
            needed: None,
        };
        let role = sim.human_defender_role().map(|r| r.label()).unwrap_or("DEF");
        let who = sim
            .human_defender
            .map(|i| {
                screen.broadcast.teams[sim.setup.defense_team].roster[sim.players[i].player]
                    .last_name()
                    .to_string()
            })
            .unwrap_or_default();
        let mut lines = vec![
            format!(
                "  {} - {}{} - {} BALL",
                sim.play.name,
                sim.play.formation.label(),
                if sim.play.flip { " (FLIPPED)" } else { "" },
                screen.broadcast.teams[sim.setup.offense_team].abbr
            ),
            format!(
                "  YOU: {role} {who}{}",
                match hit_stick(input) {
                    Some(h) => format!(" - {}", h.label()),
                    None => String::new(),
                }
            ),
        ];
        if sim.phase == Phase::Return && my_ball {
            // A pick is not a dead ball: the man who took it is a runner now, and
            // a runner gets the runner's row of moves.
            let mut row = format!("  RETURN IT{RUNNER_ROW}");
            if let Some(kind) = sim.carrier_move().kind() {
                row.push_str(&format!("      {}", kind.label()));
            }
            lines.push(row);
        } else if sim.phase == Phase::Return {
            lines.push("  INTERCEPTION - THEY HAVE IT AND THE PICTURE HAS TURNED ROUND".to_string());
        } else if sim.previewing() {
            lines.push(format!(
                "  YOUR CALL: {} - {} - {} RUSH - LET GO OF [LT] TO GET BACK TO THE LINE",
                sim.def.name,
                sim.def.coverage.label(),
                sim.def.rushers(),
            ));
        } else if sim.phase == Phase::PreSnap {
            lines.push(
                "  WALK UP TO THE LINE   HOLD [LT] PREVIEW   TAP [LT] PRESS/OFF   [RB] CHECK   [B] SWITCH"
                    .to_string(),
            );
        } else {
            lines.push(
                "  [B] SWITCH DEFENDER   [A] TACKLE   [X] DIVE TACKLE   [Y] INTERCEPT   [RS] HIGH/LOW HIT   [SELECT] CAMERA   [START] PAUSE"
                    .to_string(),
            );
        }
        (ui, lines)
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

/// The ball in the air: the charge and the aim for the man throwing it, and the
/// walk round the coverage for the man receiving it.
struct ThrowOffSnap<'a> {
    game: &'a mut GameState,
    my_team: usize,
    // The meter, filled while the throw button is held.
    hold: f32,
    charging: bool,
    // Where across the field the throw is aimed, in yards.
    aim_y: f32,
    // The throw button has been up since this throw-off started.
    armed: bool,
    // Which of your men the switch button has walked to.
    cover_pick: usize,
    // The men you can cover with, nearest the ball first, taken once the ball is
    // caught so the pad starts on a man who can actually make the tackle.
    cover_order: Option<Vec<usize>>,
}

impl<'g, 'p> SnapCaller<ThrowOffSim<'p>> for ThrowOffSnap<'g> {
    /// SELECT on a throw-off is the timeout itself rather than a word to the sim:
    /// a throw-off has no clock for the sim to stop, so the call goes to the game.
    fn ask_timeout(&mut self, _screen: &mut SnapScreen<'_, ThrowOffSim<'p>>) {
        let _ = self.game.use_timeout(self.my_team);
    }

    fn before_frame(
        &mut self,
        _screen: &mut SnapScreen<'_, ThrowOffSim<'p>>,
        _input: &Input,
    ) -> Flow {
        Flow::Run
    }

    fn controls(
        &mut self,
        screen: &mut SnapScreen<'_, ThrowOffSim<'p>>,
        input: &Input,
        stick: Vec2,
        dt: f32,
    ) {
        let sim = &mut *screen.sim;
        if sim.phase == ThrowPhase::Presnap && sim.human_thrower {
            // Three beats, the way a kickoff is played: pick the line the ball
            // will take, hold A to load the arm, let go to throw it. The throw
            // waits for the man on the pad, so there is no clock to beat.
            let holding = input.held.a || input.held.throw;
            // A still held from the kickoff card must not load the arm: the
            // button has to come up once before it means anything here.
            if !holding {
                self.armed = true;
            }
            if holding && self.armed {
                self.charging = true;
                self.hold = (self.hold + dt).min(CHARGE_TIME);
            } else if self.charging && !holding {
                let power = (self.hold / CHARGE_TIME).clamp(MIN_POWER, 1.0);
                sim.launch_with(power, self.aim_y);
                self.charging = false;
                self.hold = 0.0;
            }
            // The line is set before the arm comes back and locked there.
            if !self.charging {
                let push = aim_push(input.stick.x, sim.setup.throwing_direction());
                self.aim_y = aim_step(self.aim_y, push, dt);
            }
        }

        // Whoever is yours: the returner while you are receiving, otherwise one of
        // the three men in coverage, and B walks the pad round them.
        if input.pressed.b && sim.setup.throwing_team == self.my_team {
            self.cover_pick = self.cover_pick.wrapping_add(1);
        }
        // Covering an AI return used to start on the first gunner in the list,
        // however far downfield he was. The pad now starts on the man closest to
        // the ball, and the order is taken when it is caught rather than every
        // frame, so the man under the pad does not change mid-stride.
        if self.cover_order.is_none() && sim.catch_time().is_some() {
            self.cover_order = Some(men_by_distance(sim, self.my_team));
        }
        let mine = self
            .cover_order
            .as_ref()
            .and_then(|order| live_man(sim, order, self.cover_pick))
            .or_else(|| human_man(sim, self.my_team, self.cover_pick));
        sim.set_human(mine, stick);
        // The returner has the ball, so the four buttons are the four moves: the
        // same set the back gets, on the same buttons.
        let returning_it = sim.phase == ThrowPhase::Return
            && mine
                .map(|i| sim.players[i].role == ThrowRole::Returner)
                .unwrap_or(false);
        if returning_it {
            runner_moves(input, |kind, side| {
                if side == 0.0 {
                    sim.request_move(kind);
                } else {
                    sim.request_move_dir(kind, side);
                }
            });
        }
        // On the cover: A wraps him up, X dives at him from further out.
        if sim.phase == ThrowPhase::Return && sim.setup.throwing_team == self.my_team {
            if input.pressed.a {
                sim.call_tackle(false);
            }
            if input.pressed.x {
                sim.call_tackle(true);
            }
        }

        // Fair catch: the receiving player calls it while the ball is in the air
        // and takes it where it comes down, instead of running it out.
        if sim.phase == ThrowPhase::Flight
            && sim.setup.receiving_team == self.my_team
            && input.pressed.y
        {
            sim.call_fair_catch();
        }
    }

    fn picture(
        &mut self,
        screen: &mut SnapScreen<'_, ThrowOffSim<'p>>,
        _input: &Input,
    ) -> (PassUi, Vec<String>) {
        let cam = screen.mode();
        let sim = &*screen.sim;
        let mut guide = None;
        let mut short = false;
        let mut needed = None;
        let mut power = None;
        if sim.phase == ThrowPhase::Presnap && sim.human_thrower {
            let p = (self.hold / CHARGE_TIME).clamp(0.0, 1.0);
            let plan = sim.throw_off_plan(p, self.aim_y);
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
        // The pad is on a man here too: the returner, a gunner, or the thrower
        // while the ball is still his.
        let mine = self
            .cover_order
            .as_ref()
            .and_then(|order| live_man(sim, order, self.cover_pick))
            .or_else(|| human_man(sim, self.my_team, self.cover_pick));
        let focus_index = mine.or_else(|| {
            sim.players
                .iter()
                .position(|p| p.team == self.my_team && p.role == ThrowRole::Returner)
        });
        let focus = focus_index.map(|i| Focus {
            at: Vec3::new(sim.players[i].pos.x, sim.players[i].pos.y, 0.0),
            label: "YOU".to_string(),
        });
        let ui = PassUi {
            flag: None,
            focus,
            badges: Vec::new(),
            guide,
            power,
            short,
            needed,
        };
        let lines = vec![
            throw_off_line(&self.game.teams, sim, self.my_team, self.aim_y, self.charging),
            throw_off_controls(&self.game.teams, cam, sim, self.my_team, self.cover_pick),
        ];
        (ui, lines)
    }
}

fn throw_off_line(
    teams: &[Team],
    sim: &ThrowOffSim,
    my_team: usize,
    aim_y: f32,
    charging: bool,
) -> String {
    let abbr = |t: usize| teams[t].abbr.clone();
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

fn throw_off_controls(
    teams: &[Team],
    cam_mode: CameraMode,
    sim: &ThrowOffSim,
    my_team: usize,
    pick: usize,
) -> String {
    let cam = cam_name(cam_mode);
    if sim.phase == ThrowPhase::Presnap && sim.human_thrower {
        return format!(
            "  THROW   AIM WITH STICK   HOLD [A] TO LOAD   [SELECT] CAMERA   [START] PAUSE"
        );
    }
    if sim.phase == ThrowPhase::Return && sim.setup.receiving_team == my_team {
        let mut row = format!(
            "  RETURN IT   [X] DIVE   [B] SPIN   [Y] HURDLE   [LB/RB] BREAK   [RS] TRUCK / JUKE / STOP      MOVE WITH STICK   [SELECT] {cam}   [START] PAUSE"
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
                teams[sim.players[i].team].roster[sim.players[i].roster]
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
        teams[sim.setup.receiving_team].abbr,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use nopads_core::sim::PlayOutcome;

    /// A step is done by the thing it asked for and nothing else: a run does not
    /// finish the throw, and a play that stopped on the one does not finish the
    /// score.
    #[test]
    fn the_tutorial_only_counts_the_thing_it_asked_for() {
        assert!(tutorial_met(0, PlayOutcome::Run, false), "any snap is step one");
        assert!(tutorial_met(1, PlayOutcome::PassComplete, false));
        assert!(!tutorial_met(1, PlayOutcome::Run, false));
        assert!(tutorial_met(2, PlayOutcome::Run, false));
        assert!(!tutorial_met(2, PlayOutcome::PassComplete, false));
        assert!(tutorial_met(3, PlayOutcome::Run, true));
        assert!(!tutorial_met(3, PlayOutcome::Run, false));
    }

    /// The gauntlet pays for the ground one man made, not for the drive: the
    /// house is six, sixty is two, thirty is one.
    #[test]
    fn the_gauntlet_pays_for_the_ground_one_man_made() {
        assert_eq!(gauntlet_points(false, 12.0), 0);
        assert_eq!(gauntlet_points(false, 29.9), 0);
        assert_eq!(gauntlet_points(false, 30.0), 1);
        assert_eq!(gauntlet_points(false, 60.0), 2);
        assert_eq!(gauntlet_points(true, 4.0), 6);
    }

    /// A drill has no clock on the bug and no score, because it has neither: the
    /// board is the two clubs and the spot the ball is on.
    #[test]
    fn a_drill_has_no_clock_on_the_bug() {
        let teams = nopads_core::team::league_teams();
        let board = drill_board(&[teams[0].clone(), teams[1].clone()], 30.0);
        assert_eq!(board.clock, 0.0);
        assert_eq!(board.score, [0, 0]);
        assert_eq!(board.yards_to_goal, 30.0);
        assert_eq!(board.abbr[0], teams[0].abbr.to_uppercase());
    }

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

    /// The tape plays at the rate it was recorded, the bumpers run it fast, and
    /// neither end runs off the end of it.
    /// The pause menu is not one screen. A live snap has a tape under Resume, a
    /// first snap does not, and the lens and the quit prompt replace the menu
    /// rather than sitting inside it. One picture each, over a frozen frame of
    /// the same play: the panel is glass over a picture, so the picture belongs
    /// in the shot.
    /// The hard count as the player watches it: a defender over the line is his
    /// foul, a member of your own line leaning at the count is yours. Both are
    /// five yards and a replay, and the flag names the man, not the side the
    /// count was aimed at.
    /// The replay's lens stands behind the ball like the classic shot does, so
    /// its pan has to run down the field: the sideline mapping it used to share
    /// walked the camera across the field when the stick was pushed up it.
    #[test]
    fn the_replay_pan_goes_where_the_stick_points() {
        let up = stick_to_world(Vec2::new(0.0, 1.0), false, CameraMode::Replay, 1);
        assert!(up.x > 0.9, "up the stick did not walk downfield: {up:?}");
        let right = stick_to_world(Vec2::new(1.0, 0.0), false, CameraMode::Replay, 1);
        assert!(right.y > 0.9, "right did not go to the offense's right: {right:?}");
        // Turned round, the same push still goes up the screen.
        let up_turned = stick_to_world(Vec2::new(0.0, 1.0), false, CameraMode::Replay, -1);
        assert!(
            up_turned.x < -0.9,
            "up went behind the play when it turned round: {up_turned:?}"
        );
    }

    #[test]
    fn the_hard_count_can_be_rendered_to_pictures() {
        if std::env::var_os("NOPADS_DUMP_UI").is_none() {
            return;
        }
        use nopads_core::playbook::{defensive_playbook, playbook};
        use nopads_core::rules::PenaltyKind;
        use nopads_core::sim::{PlaySim, SnapSetup};
        use nopads_core::team::league_teams;

        let teams = league_teams();
        let (home, away) = (teams[0].clone(), teams[1].clone());
        let (book, dbook) = (playbook(), defensive_playbook());
        let dir = format!("{}/../../out/ui", env!("CARGO_MANIFEST_DIR"));
        let mut broadcast = Broadcast::new(CANVAS_W, CANVAS_H, [home.clone(), away.clone()]);
        let mut wrote = [false; 2];
        for seed in 0..200u64 {
            let setup = SnapSetup {
                los: 35.0,
                ball_y: 0.0,
                direction: 1,
                down: 2,
                to_go: 7.0,
                seed,
                offense_team: 0,
                defense_team: 1,
                is_try: false,
            };
            let mut sim = PlaySim::new(&home, &away, &book[0], &dbook[0], &setup);
            // Held at the line, so the picture is the jump itself and not a play
            // that carried on behind it.
            sim.await_snap(true);
            for _ in 0..20 {
                sim.step(1.0 / 60.0);
            }
            sim.dummy_audible();
            let Some(call) = sim.result.as_ref().and_then(|r| r.penalty.clone()) else {
                continue;
            };
            let slot = match call.kind {
                PenaltyKind::Encroachment => 0,
                PenaltyKind::FalseStart => 1,
                _ => continue,
            };
            if wrote[slot] {
                continue;
            }
            wrote[slot] = true;
            let frame = broadcast.live_frame(&sim, CameraMode::Classic);
            let name = if slot == 0 { "hard_defence" } else { "hard_false_start" };
            nopads_render::write_png(format!("{dir}/{name}.png"), &frame).expect("write png");
            if wrote[0] && wrote[1] {
                break;
            }
        }
        assert!(
            wrote[0] && wrote[1],
            "the count did not produce both kinds: {wrote:?}"
        );
    }

    #[test]
    fn the_pause_menu_can_be_rendered_to_pictures() {
        if std::env::var_os("NOPADS_DUMP_UI").is_none() {
            return;
        }
        use nopads_core::playbook::{defensive_playbook, playbook};
        use nopads_core::sim::{PlaySim, SnapSetup};
        use nopads_core::team::league_teams;

        let teams = league_teams();
        let (home, away) = (teams[0].clone(), teams[1].clone());
        let setup = SnapSetup {
            los: 35.0,
            ball_y: 0.0,
            direction: 1,
            down: 2,
            to_go: 7.0,
            seed: 11,
            offense_team: 0,
            defense_team: 1,
            is_try: false,
        };
        let (book, dbook) = (playbook(), defensive_playbook());
        let mut sim = PlaySim::new(&home, &away, &book[0], &dbook[0], &setup);
        // Held at the line, so the frozen picture is a formation and not a play
        // that carried itself out behind the menu.
        sim.await_snap(true);
        for _ in 0..20 {
            sim.step(1.0 / 60.0);
        }
        // The sim borrows the teams, so the picture gets its own copies.
        let mut broadcast = Broadcast::new(CANVAS_W, CANVAS_H, [home.clone(), away.clone()]);
        let still = broadcast.live_frame(&sim, CameraMode::Classic);

        let panel =
            |pick: usize, tuning: bool, leaving: Option<usize>, can_replay: bool| PausePanel {
                slots: pause_slots(can_replay),
                pick,
                cam: cam_name(CameraMode::Broadcast),
                zoom: 1.0,
                tilt: 0.0,
                tuning,
                leaving,
            };
        let dir = format!("{}/../../out/ui", env!("CARGO_MANIFEST_DIR"));
        let save = |name: &str, panel: &PausePanel<'_>| {
            let mut frame = still.clone();
            draw_pause(&mut frame, &panel.sheet());
            nopads_render::write_png(format!("{dir}/{name}.png"), &frame).expect("write png");
        };
        // Paused on a snap with a tape: five rows, Replay under Resume.
        save("pause_snap", &panel(0, false, None, true));
        // Paused on the first snap of a series: no tape, so four rows.
        save("pause_first_snap", &panel(0, false, None, false));
        // The lens, and the question that quitting asks.
        save("pause_lens", &panel(0, true, None, true));
        save("pause_leave", &panel(0, false, Some(1), true));
        // Over a call sheet: the sheet stays behind the glass, at the size the
        // sheet is drawn at, rather than being replaced by a page of text.
        let forms = nopads_core::playbook::formations_in(&book);
        let mut sheet = nopads_render::Frame::new(UI_W, UI_H);
        nopads_render::callsheet::draw_layered(&mut sheet, &book, &forms, 0, 0, false, false);
        draw_pause(&mut sheet, &panel(0, false, None, false).sheet());
        let mut canvas = nopads_render::Frame::new(CANVAS_W, CANVAS_H);
        canvas.blit_up(&sheet, UI_SCALE);
        nopads_render::write_png(format!("{dir}/pause_sheet.png"), &canvas).expect("write png");
        println!("wrote the pause menu to {dir}");
    }

    #[test]
    fn the_tape_scrubs_without_running_off_either_end() {
        // Half a second of tape at sixty frames a second.
        assert!((scrub(0.0, 100.0, true, false, false, 0.5) - 30.0).abs() < 0.01);
        // Fast forward is four times that, and forward stops on the last frame.
        assert!((scrub(0.0, 1000.0, false, true, false, 0.5) - 120.0).abs() < 0.01);
        assert!((scrub(990.0, 1000.0, true, true, false, 0.5) - 1000.0).abs() < 0.01);
        // Rewinding stops on the first.
        assert!(scrub(10.0, 1000.0, false, false, true, 0.5).abs() < 0.01);
        // A paused tape with nothing held does not move at all.
        assert!((scrub(42.0, 1000.0, false, false, false, 0.5) - 42.0).abs() < 0.01);
    }
}

#[cfg(test)]
mod after_game_tests {
    use super::{after_game, AfterFinal, AfterGame};

    /// The bug this guards: a season fixture that reached the final card came
    /// straight back as a new game, so the season stopped and the result was
    /// never written into the table.
    #[test]
    fn a_season_game_never_starts_itself_over() {
        assert_eq!(
            after_game(Some(AfterFinal::Again), true, false),
            AfterGame::Finish
        );
        assert_eq!(
            after_game(Some(AfterFinal::Menu), true, false),
            AfterGame::Finish
        );
        assert_eq!(after_game(None, true, true), AfterGame::Finish);
        // An exhibition still rematches off its own card.
        assert_eq!(
            after_game(Some(AfterFinal::Again), false, false),
            AfterGame::Again
        );
        assert_eq!(
            after_game(Some(AfterFinal::Menu), false, false),
            AfterGame::Menu
        );
    }
}

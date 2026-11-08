//! The front of the game: the start menu, the settings screen and the
//! exhibition picker.
//!
//! Everything here is drawn on the same 640x360 canvas the broadcast uses, so
//! the menu looks like the game does. On a machine with no display the frames
//! are averaged down into terminal cells by `View`, exactly as live play is.

use std::time::{Duration, Instant};

use nopads_core::playbook::Book;
use nopads_core::game::{GameState, PlayRecord};
use nopads_core::math::Rgb;
use nopads_core::season::{self, Phase, Season};
use nopads_core::team::{
    clubs, depth_order, league_teams, set_depth_order, Club, Division, Player, Position, Team,
    TeamRatings,
};
use nopads_render::raster::Frame;
use nopads_render::ui;
use nopads_render::venues::{self, Venue};
use nopads_render::{Broadcast, CameraMode};

use crate::arcade::CAMERA_LABELS;
use crate::view::{View, UI_H, UI_W};
use nopads_platform::pad::{self, Pad};
use nopads_platform::settings::{Settings, QUARTER_CHOICES, RESOLUTIONS};

// The screens take their colours from the design kit, so a change to a token
// changes every screen at once instead of the one that was edited.
fn paper() -> Rgb {
    ui::PAPER
}

fn dim() -> Rgb {
    ui::MUTED
}

fn accent() -> Rgb {
    ui::ACCENT
}

fn backdrop() -> Rgb {
    ui::INK
}

fn panel() -> Rgb {
    ui::SURFACE_TOP
}

/// What the player picked in exhibition: the two clubs, the side he runs, the
/// shirt each of them wears, and the ground.
pub struct Kickoff {
    /// The club the player picked.
    pub yours: usize,
    /// The computer's club, filled in by the menu.
    pub theirs: usize,
    /// True when the player's club is the home side. The side decides who is
    /// listed first and who takes the ball first, not which shirt is worn: the
    /// shirts were chosen by hand.
    pub player_home: bool,
    /// The shirt each club wears, `true` for its home kit, in game order: the
    /// home club's first.
    pub kits: [bool; 2],
    /// True when the player takes the opening throw-off.
    pub receive: bool,
    /// The ground the game is played at: an index into the venue list.
    pub venue: usize,
    /// The book the player is running: his club's own unless he picked another.
    pub book: Book,
}

/// One of the clubs, agreed before anything is picked: who hosts, who travels,
/// which side of the ball the player takes, and which shirt each of them wears.
pub struct Sides {
    pub yours: usize,
    pub theirs: usize,
    pub player_home: bool,
    pub kits: [bool; 2],
}

/// Which side of the ball a session teaches: the offense, the defense, or both of
/// them one after the other. The tutorial and the practice session both ask it,
/// because both are played from one side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    Offense,
    Defense,
    Both,
}

/// Which of the sessions under MORE MODES the player picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrillKind {
    Tutorial(Track),
    Practice(Track),
    Gauntlet,
}

/// What the player asked the front end for.
pub enum Front {
    /// One exhibition game.
    Friendly(Kickoff),
    /// A whole season, already scheduled.
    Season(Box<Season>),
    /// One of the sessions under MORE MODES: no clock, no season, a drill, on
    /// the same clubs and the same book the game would have been played with, and
    /// the difficulty it was told to play against.
    Drill(DrillKind, Kickoff, usize),
}

enum Start {
    Exhibition,
    Season,
    Modes,
    Roster,
    Customize,
    Settings,
    Quit,
}

/// Run the front end. `None` means the player quit (or closed the window).
pub fn run(view: &mut View, pad: Option<&mut Pad>, settings: &mut Settings) -> Option<Front> {
    let mut pad = pad;
    loop {
        match start_screen(view, &mut pad, settings)? {
            Start::Exhibition => {
                if let Some(kick) = exhibition(view, &mut pad, settings) {
                    return Some(Front::Friendly(kick));
                }
            }
            Start::Season => {
                if let Some(season) = season_setup(view, &mut pad, settings) {
                    return Some(Front::Season(Box::new(season)));
                }
            }
            Start::Modes => {
                if let Some(kind) = modes_screen(view, &mut pad) {
                    if let Some((kick, difficulty)) =
                        drill_kickoff(view, &mut pad, kind, settings.difficulty)
                    {
                        return Some(Front::Drill(kind, kick, difficulty));
                    }
                }
            }
            Start::Roster => {
                roster_grid(view, &mut pad)?;
            }
            Start::Customize => {
                customize_screen(view, &mut pad)?;
            }
            Start::Settings => {
                settings_screen(view, &mut pad, settings)?;
            }
            Start::Quit => return None,
        }
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

fn text_width(text: &str, scale: usize) -> i32 {
    let n = text.chars().count() as i32;
    if n == 0 {
        0
    } else {
        n * 6 * scale as i32 - scale as i32
    }
}

fn centered(frame: &mut Frame, y: i32, text: &str, scale: usize, color: Rgb) {
    let x = (UI_W as i32 - text_width(text, scale)) / 2;
    frame.text_shadowed(x, y, text, scale, color, 1);
}

fn centered_in(frame: &mut Frame, cx: i32, y: i32, text: &str, scale: usize, color: Rgb) {
    frame.text_shadowed(cx - text_width(text, scale) / 2, y, text, scale, color, 1);
}

fn night(frame: &mut Frame) {
    // The page, the pool of light and the painted lines all come from the kit:
    // every screen sits on the same surface rather than on its own fill.
    ui::backdrop(frame);
}

/// A club's badge: a shield in its colours with the three-letter mark on it.
/// There is no art pipeline here, so the mark is drawn from the club's own kit.
fn badge(frame: &mut Frame, cx: i32, cy: i32, size: i32, club: &Club, lit: bool) {
    let tone = |c: Rgb| if lit { c } else { c.lerp(backdrop(), 0.62) };
    let w = size;
    let h = size * 6 / 5;
    let x = cx - w / 2;
    let y = cy - h / 2;
    frame.rounded_rect(x - 3, y - 3, w + 6, h + 6, w / 4, tone(club.accent));
    frame.rounded_rect(x, y, w, h, w / 5, tone(club.primary));
    // A shoulder band and a stripe down the left: the kit, at a glance.
    frame.fill_rect(
        x + 3,
        y + h / 6,
        w - 6,
        (h / 10).max(3),
        tone(club.secondary),
    );
    frame.fill_rect(
        x + 3,
        y + h / 6,
        (w / 16).max(2),
        h - h / 3,
        tone(club.accent),
    );
    let scale = ((w as usize) / (club.abbr.chars().count() * 6 + 3)).max(1);
    let tw = text_width(club.abbr, scale);
    frame.text_shadowed(
        cx - tw / 2,
        cy - (7 * scale as i32) / 2 + h / 10,
        club.abbr,
        scale,
        if lit { paper() } else { tone(paper()) },
        1,
    );
}

fn footer(frame: &mut Frame, text: &str) {
    let h = UI_H as i32;
    frame.fill_rect_alpha(0, h - 26, UI_W as i32, 26, ui::INK, 0.72);
    ui::divider(frame, 0, h - 26, UI_W as i32);
    let hints = ui::parse_hints(text);
    ui::hints(frame, UI_W as i32 / 2, h - 18, &hints);
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// A stick axis with a first tap and then a repeat, so a menu is walked one row
/// at a time instead of scrolling through itself.
struct Repeat {
    held: f32,
    fired: bool,
    delay: f32,
    gap: f32,
}

impl Default for Repeat {
    fn default() -> Self {
        Repeat {
            held: 0.0,
            fired: false,
            delay: 0.30,
            gap: 0.16,
        }
    }
}

impl Repeat {
    /// A lazier repeat, for a screen where one press should move one item and
    /// holding the key should not run away past it.
    fn slow() -> Self {
        Repeat {
            delay: 0.45,
            gap: 0.24,
            ..Repeat::default()
        }
    }

    fn step(&mut self, v: f32, dt: f32) -> i8 {
        let dir = if v > 0.55 {
            1
        } else if v < -0.55 {
            -1
        } else {
            0
        };
        if dir == 0 {
            self.held = 0.0;
            self.fired = false;
            return 0;
        }
        self.held += dt;
        if !self.fired {
            self.fired = true;
            return dir;
        }
        if self.held >= self.delay {
            self.held = self.delay - self.gap;
            return dir;
        }
        0
    }
}

fn wrap(i: usize, len: usize, d: i8) -> usize {
    ((i as i32 + d as i32).rem_euclid(len as i32)) as usize
}

fn beat() {
    std::thread::sleep(Duration::from_millis(16));
}

// ---------------------------------------------------------------------------
// Start menu
// ---------------------------------------------------------------------------

/// The start menu, drawn on the interface grid.
///
/// This is a function of the cursor rather than the body of the loop that reads
/// the stick, so the layout can be looked at: it is the screen a player meets
/// first, and it is laid out corner to corner on the grid rather than in a list
/// of numbers that only means anything on one canvas.
fn draw_start(frame: &mut Frame, pick: usize, slide: &ui::Slide, t: f32) {
    const LABELS: [&str; 7] = [
        "EXHIBITION",
        "SEASON",
        "MORE MODES",
        "ROSTER",
        "CUSTOMIZATION",
        "SETTINGS",
        "QUIT",
    ];
    const NOTES: [&str; 7] = [
        "ONE GAME, PICKED IN A MINUTE",
        "A CLUB, A LEAGUE, A SEASON",
        "TUTORIAL, PRACTICE, THE GAUNTLET",
        "DEPTH CHART AND ROSTER FOR EVERY CLUB",
        "MAKE A CLUB, REPAINT A KIT",
        "CAMERA, LENS AND DISPLAY",
        "CLOSE THE GAME",
    ];
    ui::backdrop(frame);
    let w = frame.w as i32;
    let h = frame.h as i32;
    let cx = w / 2;

    // The masthead arrives first and settles: the title is the one thing on the
    // screen that is not a choice.
    let hero = ui::enter(t, 0, LABELS.len() + 3);
    let title_y = (h as f32 * 0.075) as i32 - ((1.0 - hero) * 6.0) as i32;
    ui::heading(frame, cx, title_y, "7-ON-7", ui::T_HERO, ui::ACCENT);
    let sub = "NO PADS FOOTBALL";
    ui::caption(
        frame,
        cx - ui::tracked_width(sub, ui::T_CAPTION, 2) / 2,
        title_y + 58,
        sub,
        ui::MUTED,
    );

    // One card holding every choice: the rows are a list, so the card is the
    // surface and the accent bar is the selection.
    let card_w = (w - 160).clamp(220, 360);
    let row_h = 24;
    let gap = 5;
    let pad = ui::S3;
    let n = LABELS.len() as i32;
    let card_h = pad * 2 + n * row_h + (n - 1) * gap;
    let card_x = cx - card_w / 2;
    let card_y = (h as f32 * 0.30) as i32;
    let lit = ui::enter(t, 1, LABELS.len() + 3);
    ui::card(frame, card_x, card_y, card_w, card_h, ui::ACCENT, lit);

    for (i, label) in LABELS.iter().enumerate() {
        let y = card_y + pad + i as i32 * (row_h + gap);
        let heat = slide.heat(i) * ui::enter(t, i + 2, LABELS.len() + 3);
        ui::row(
            frame,
            card_x + pad,
            y,
            card_w - pad * 2,
            row_h,
            label,
            None,
            heat,
            ui::ACCENT,
        );
    }

    // The line under the card says what the highlighted choice is, so the menu
    // explains itself without a second screen.
    frame.text_shadowed(
        cx - text_width(NOTES[pick], ui::T_CAPTION) / 2,
        card_y + card_h + ui::S4,
        NOTES[pick],
        ui::T_CAPTION,
        dim(),
        1,
    );
    footer(frame, "[STICK] CHOOSE   [A] SELECT   [START] QUIT");
}

fn start_screen(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    _settings: &mut Settings,
) -> Option<Start> {
    const ITEMS: usize = 7;
    let mut pick = 0usize;
    let mut nav = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut t = 0.0f32;
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        t += dt;
        slide.step(pick, dt);
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.start {
            return Some(Start::Quit);
        }
        if let d = nav.step(input.stick.y, dt) {
            if d != 0 {
                pick = wrap(pick, ITEMS, -d);
            }
        }

        let mut frame = Frame::new(UI_W, UI_H);
        draw_start(&mut frame, pick, &slide, t);

        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        if input.pressed.a {
            return Some(match pick {
                0 => Start::Exhibition,
                1 => Start::Season,
                2 => Start::Modes,
                3 => Start::Roster,
                4 => Start::Customize,
                5 => Start::Settings,
                _ => Start::Quit,
            });
        }
        beat();
    }
}

// ---------------------------------------------------------------------------
// More modes
// ---------------------------------------------------------------------------

/// The three sessions under MORE MODES, with the line the screen says about each.
/// They are drills, not games: no clock, no season, one thing to do.
const MODES: [(&str, &str); 3] = [
    ("TUTORIAL", "THE OFFENSE ONE CONTROL AT A TIME, OFF THE TEN"),
    ("PRACTICE", "YOUR CALL, YOUR PACE: NO CLOCK, NO DOWNS, SAME SPOT"),
    (
        "THREE-ON-ONE GAUNTLET",
        "FIVE THROW-OFF RETURNS, THREE GUNNERS, NOBODY ELSE",
    ),
];

/// The list of modes, drawn on the grid so it can be looked at.
fn draw_modes(frame: &mut Frame, pick: usize, slide: &ui::Slide) {
    let mut rows: Vec<(&str, String)> = MODES.iter().map(|(name, _)| (*name, String::new())).collect();
    rows.push(("BACK", String::new()));
    let note = MODES
        .get(pick)
        .map(|(_, line)| *line)
        .unwrap_or("BACK TO THE MAIN MENU");
    draw_option_screen(
        frame,
        "MORE MODES",
        "DRILLS, NOT GAMES: NO CLOCK, NO SEASON",
        &rows,
        slide,
        note,
        "[UP/DOWN] MODE   [A] START   [B] BACK",
    );
}

/// The three sides of the ball, and the line under them on the screen that asks.
const TRACKS: [(&str, &str); 3] = [
    ("OFFENSE", "SNAP IT, THROW IT, RUN IT, SCORE IT"),
    ("DEFENSE", "CALL IT, STOP IT, COVER IT"),
    ("BOTH", "THE OFFENSE AND THEN THE DEFENSE"),
];

/// The same three for a practice session: the same question, with a session
/// behind it instead of a list of steps.
const PRACTICE_NOTES: [(&str, &str); 3] = [
    ("OFFENSE", "YOUR CALL FROM YOUR BOOK, SNAP AFTER SNAP"),
    ("DEFENSE", "CALL IT AND PLAY IT, SNAP AFTER SNAP"),
    ("BOTH", "THE OFFENSE AND THEN THE DEFENSE"),
];

/// A screen that asks which side of the ball. The tutorial and the practice
/// session are the same question, so they are the same screen with a different
/// heading and a different line under it.
fn draw_track_screen(
    frame: &mut Frame,
    title: &str,
    caption: &str,
    notes: [(&str, &str); 3],
    pick: usize,
    slide: &ui::Slide,
) {
    let mut rows: Vec<(&str, String)> =
        notes.iter().map(|(name, _)| (*name, String::new())).collect();
    rows.push(("BACK", String::new()));
    let note = notes
        .get(pick)
        .map(|(_, line)| *line)
        .unwrap_or("BACK TO THE MODES");
    draw_option_screen(
        frame,
        title,
        caption,
        &rows,
        slide,
        note,
        "[UP/DOWN] SESSION   [A] START   [B] BACK",
    );
}

fn draw_tutorial(frame: &mut Frame, pick: usize, slide: &ui::Slide) {
    draw_track_screen(
        frame,
        "TUTORIAL",
        "WHICH SIDE OF THE BALL DO YOU WANT TAUGHT?",
        TRACKS,
        pick,
        slide,
    );
}

fn draw_practice(frame: &mut Frame, pick: usize, slide: &ui::Slide) {
    draw_track_screen(
        frame,
        "PRACTICE",
        "WHICH SIDE OF THE BALL DO YOU WANT TO PRACTISE?",
        PRACTICE_NOTES,
        pick,
        slide,
    );
}

fn track_screen(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    title: &str,
    caption: &str,
    notes: [(&str, &str); 3],
) -> Option<Track> {
    let mut pick = 0usize;
    let mut nav = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return None;
        }
        if let d = nav.step(input.stick.y, dt) {
            if d != 0 {
                pick = wrap(pick, TRACKS.len() + 1, -d);
            }
        }
        slide.step(pick, dt);
        let mut frame = Frame::new(UI_W, UI_H);
        draw_track_screen(&mut frame, title, caption, notes, pick, &slide);
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        if input.pressed.a {
            return match pick {
                0 => Some(Track::Offense),
                1 => Some(Track::Defense),
                2 => Some(Track::Both),
                _ => None,
            };
        }
        beat();
    }
}

/// The three difficulties, and what each one means on the field.
const DIFFICULTIES: [(&str, &str); 3] = [
    ("EASY", "THE GUNNERS COME AT THE RETURNER IN A LINE, AND THE AI SITS BACK"),
    ("NORMAL", "THE GUNNERS CONTAIN: A LANE EITHER SIDE AND THE CHASE UP THE MIDDLE"),
    ("HARD", "THE SAME CONTAINMENT, AND THE AI TAKES ITS SHOTS"),
];

fn draw_difficulty(frame: &mut Frame, pick: usize, slide: &ui::Slide) {
    let rows: Vec<(&str, String)> = DIFFICULTIES
        .iter()
        .map(|(name, _)| (*name, String::new()))
        .collect();
    let note = DIFFICULTIES
        .get(pick)
        .map(|(_, line)| *line)
        .unwrap_or("");
    draw_option_screen(
        frame,
        "DIFFICULTY",
        "HOW HARD SHOULD THE OTHER LOT PLAY?",
        &rows,
        slide,
        note,
        "[UP/DOWN] DIFFICULTY   [A] TAKE IT   [B] BACK",
    );
}

fn difficulty_screen(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    start: usize,
) -> Option<usize> {
    let mut pick = start.min(DIFFICULTIES.len() - 1);
    let mut nav = Repeat::default();
    let mut slide = ui::Slide::new(pick);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return None;
        }
        if let d = nav.step(input.stick.y, dt) {
            if d != 0 {
                pick = wrap(pick, DIFFICULTIES.len(), -d);
            }
        }
        slide.step(pick, dt);
        let mut frame = Frame::new(UI_W, UI_H);
        draw_difficulty(&mut frame, pick, &slide);
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        if input.pressed.a {
            return Some(pick);
        }
        beat();
    }
}

fn tutorial_screen(view: &mut View, pad: &mut Option<&mut Pad>) -> Option<Track> {
    track_screen(
        view,
        pad,
        "TUTORIAL",
        "WHICH SIDE OF THE BALL DO YOU WANT TAUGHT?",
        TRACKS,
    )
}

/// Practice asks the same question the tutorial does, because a practice session
/// is played from a side: offense or defense, or both one after the other. There
/// is no toss in front of it -- the side is the choice, so a coin has nothing to
/// decide.
fn practice_screen(view: &mut View, pad: &mut Option<&mut Pad>) -> Option<Track> {
    track_screen(
        view,
        pad,
        "PRACTICE",
        "WHICH SIDE OF THE BALL DO YOU WANT TO PRACTISE?",
        PRACTICE_NOTES,
    )
}

fn modes_screen(view: &mut View, pad: &mut Option<&mut Pad>) -> Option<DrillKind> {
    let mut pick = 0usize;
    let mut nav = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return None;
        }
        if let d = nav.step(input.stick.y, dt) {
            if d != 0 {
                pick = wrap(pick, MODES.len() + 1, -d);
            }
        }
        slide.step(pick, dt);
        let mut frame = Frame::new(UI_W, UI_H);
        draw_modes(&mut frame, pick, &slide);
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        if input.pressed.a {
            return match pick {
                0 => tutorial_screen(view, pad).map(DrillKind::Tutorial),
                1 => practice_screen(view, pad).map(DrillKind::Practice),
                2 => Some(DrillKind::Gauntlet),
                _ => None,
            };
        }
        beat();
    }
}

// ---------------------------------------------------------------------------
// One list of choices
// ---------------------------------------------------------------------------

/// A screen that is a heading, a caption and one card of rows. The settings and
/// the game setup were the same forty lines twice; this is that screen once, so
/// a change to the row height or the card does not have to be made twice and
/// cannot drift between them.
fn draw_option_screen(
    frame: &mut Frame,
    title: &str,
    caption: &str,
    rows: &[(&str, String)],
    slide: &ui::Slide,
    note: &str,
    keys: &str,
) {
    draw_option_rows(frame, title, caption, rows, slide, note, keys, 30, 6);
}

/// The same screen with a chosen row height: the shelf's start screen has eight
/// rows in it, and eight of the standard rows run off the bottom of the canvas.
#[allow(clippy::too_many_arguments)]
fn draw_option_rows(
    frame: &mut Frame,
    title: &str,
    caption: &str,
    rows: &[(&str, String)],
    slide: &ui::Slide,
    note: &str,
    keys: &str,
    row_h: i32,
    gap: i32,
) {
    night(frame);
    let w = frame.w as i32;
    ui::heading(frame, w / 2, 22, title, ui::T_BODY, ui::ACCENT);
    ui::caption(
        frame,
        w / 2 - ui::tracked_width(caption, ui::T_CAPTION, 1) / 2,
        50,
        caption,
        dim(),
    );
    let x = 110;
    let cw = w - 220;
    let n = rows.len() as i32;
    let card_h = n * (row_h + gap) + ui::S3;
    ui::card(
        frame,
        x - ui::S3,
        74,
        cw + ui::S3 * 2,
        card_h,
        ui::ACCENT,
        1.0,
    );
    for (i, (name, value)) in rows.iter().enumerate() {
        let y = 84 + i as i32 * (row_h + gap);
        let value = (!value.is_empty()).then_some(value.as_str());
        ui::row(
            frame,
            x,
            y,
            cw,
            row_h,
            name,
            value,
            slide.heat(i),
            ui::ACCENT,
        );
    }
    centered(frame, 74 + card_h + 20, note, 1, dim());
    footer(frame, keys);
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

fn settings_screen(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    settings: &mut Settings,
) -> Option<()> {
    const ROWS: usize = 6;
    let mut pick = 0usize;
    let mut nav = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b || input.pressed.start {
            return Some(());
        }
        if let d = nav.step(input.stick.y, dt) {
            if d != 0 {
                pick = wrap(pick, ROWS, -d);
            }
        }

        // Left and right walk a value; A cycles it forward, so the row can be
        // changed without a stick.
        let left = input.stick.x < -0.55 || input.pressed.x;
        let right = input.stick.x > 0.55;
        let forward = input.pressed.a;
        let mut changed = false;
        if left || right || forward {
            let dir = if left { -1isize } else { 1 };
            match pick {
                0 => {
                    let n = CAMERA_LABELS.len() as isize;
                    settings.camera = ((settings.camera as isize + dir).rem_euclid(n)) as usize;
                    changed = true;
                }
                1 => {
                    // The lens opens its own live preview: two numbers are easier
                    // to judge against the picture than in a menu row.
                    if forward {
                        lens_screen(view, pad, settings);
                        last = Instant::now();
                        continue;
                    }
                }
                2 => {
                    // The controls are a page, not a value: the row opens it.
                    if forward {
                        controls_screen(view, pad);
                        last = Instant::now();
                        continue;
                    }
                }
                3 => {
                    let n = RESOLUTIONS.len() as isize;
                    let i = ((settings.resolution_index() as isize + dir).rem_euclid(n)) as usize;
                    settings.set_resolution(i);
                    changed = true;
                }
                4 => {
                    settings.fullscreen = !settings.fullscreen;
                    changed = true;
                }
                _ => {
                    if forward {
                        return Some(());
                    }
                }
            }
        }
        if changed {
            settings.camera = settings.camera % CAMERA_LABELS.len();
            settings.save();
            view.apply(settings);
            view.reset();
            last = Instant::now();
            continue;
        }
        slide.step(pick, dt);
        let rows = [
            (
                "CAMERA",
                CAMERA_LABELS[settings.camera % CAMERA_LABELS.len()].to_string(),
            ),
            ("LENS", settings.lens_label()),
            ("CONTROLS", String::new()),
            ("RESOLUTION", settings.resolution_label()),
            ("DISPLAY", settings.display_label().to_string()),
            ("BACK", String::new()),
        ];
        let mut frame = Frame::new(UI_W, UI_H);
        draw_option_screen(
            &mut frame,
            "SETTINGS",
            "THE GAME'S OWN DEFAULTS",
            &rows,
            &slide,
            "CHANGES ARE SAVED AS YOU MAKE THEM",
            "[UP/DOWN] ROW   [LEFT/RIGHT] CHANGE   [B] BACK",
        );
        footer(&mut frame, "[UP/DOWN] ROW   [LEFT/RIGHT] CHANGE   [B] BACK");
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

/// The controls page, opened from the settings. It is the same page the pause
/// menu lifts over a frozen picture; here it gets the whole screen, because the
/// settings screen has no game behind it to keep.
fn controls_screen(view: &mut View, pad: &mut Option<&mut Pad>) -> Option<()> {
    let mut page = 0usize;
    let mut pager = crate::controls::Pager::default();
    view.reset();
    loop {
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.a || input.pressed.b || input.pressed.start {
            return Some(());
        }
        let left = input.stick.x < -0.55 || input.pressed.dummy;
        let right = input.stick.x > 0.55 || input.pressed.shoulder;
        if let Some(turn) = pager.turn(left, right) {
            page = crate::controls::walk(page, turn);
            // The push that turned the page must not also be seen as held.
            view.reset();
            continue;
        }
        let mut frame = Frame::new(UI_W, UI_H);
        night(&mut frame);
        crate::controls::draw(&mut frame, page);
        footer(&mut frame, "[LEFT/RIGHT] PAGE   [A] DONE");
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

// ---------------------------------------------------------------------------
// Game setup
// ---------------------------------------------------------------------------

/// The card between the ground and the coin: how long the game runs and how hard
/// the other sideline plays. Both lived on the settings screen, which is where a
/// player looks for the machine's defaults, not for the rules of the game he is
/// about to start.
fn game_setup(view: &mut View, pad: &mut Option<&mut Pad>, settings: &mut Settings) -> Option<()> {
    const ROWS: usize = 3;
    const DIFFICULTY: [&str; 3] = ["EASY", "NORMAL", "HARD"];
    let mut pick = 0usize;
    let mut nav = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return None;
        }
        if let d = nav.step(input.stick.y, dt) {
            if d != 0 {
                pick = wrap(pick, ROWS, -d);
            }
        }
        let left = input.stick.x < -0.55 || input.pressed.x;
        let right = input.stick.x > 0.55;
        if left || right || input.pressed.a {
            let dir = if left { -1isize } else { 1 };
            match pick {
                0 => {
                    let n = QUARTER_CHOICES.len() as isize;
                    let i = ((settings.quarter_index() as isize + dir).rem_euclid(n)) as usize;
                    settings.set_quarter(i);
                    settings.save();
                }
                1 => {
                    settings.difficulty =
                        ((settings.difficulty as isize + dir).rem_euclid(3)) as usize;
                    settings.save();
                }
                _ => {
                    if input.pressed.a {
                        return Some(());
                    }
                }
            }
        }
        slide.step(pick, dt);
        let rows = [
            ("QUARTERS", settings.quarter_label()),
            (
                "DIFFICULTY",
                DIFFICULTY[settings.difficulty % 3].to_string(),
            ),
            ("KICK OFF", String::new()),
        ];
        let mut frame = Frame::new(UI_W, UI_H);
        draw_option_screen(
            &mut frame,
            "GAME SETUP",
            "HOW LONG, AND HOW HARD",
            &rows,
            &slide,
            "SAVED, AND USED BY THIS GAME",
            "[UP/DOWN] ROW   [LEFT/RIGHT] CHANGE   [A] KICK OFF   [B] BACK",
        );
        footer(
            &mut frame,
            "[UP/DOWN] ROW   [LEFT/RIGHT] CHANGE   [A] KICK OFF   [B] BACK",
        );
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

// ---------------------------------------------------------------------------
// Roster: every club's depth chart and roster
// ---------------------------------------------------------------------------

/// The clubs, four across, with their ratings. A opens a club's roster.
fn roster_grid(view: &mut View, pad: &mut Option<&mut Pad>) -> Option<()> {
    let all = clubs();
    let ratings: Vec<TeamRatings> = all.iter().map(|c| c.profile(1).0).collect();
    let mut sel = 0usize;
    let mut nav_x = Repeat::default();
    let mut nav_y = Repeat::default();
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        if let d = nav_x.step(input.stick.x, dt) {
            if d != 0 {
                sel = wrap(sel, all.len(), d);
            }
        }
        if let d = nav_y.step(input.stick.y, dt) {
            if d != 0 {
                let next = if d > 0 {
                    sel.checked_sub(4)
                } else {
                    let n = sel + 4;
                    (n < all.len()).then_some(n)
                };
                if let Some(n) = next {
                    sel = n;
                }
            }
        }
        if input.pressed.a {
            roster_screen(view, pad, &all[sel])?;
            view.reset();
            last = Instant::now();
            continue;
        }
        let mut frame = Frame::new(UI_W, UI_H);
        night(&mut frame);
        centered(&mut frame, 12, "ROSTER", 3, accent());
        centered(
            &mut frame,
            44,
            "DEPTH CHART AND ROSTER FOR EVERY CLUB",
            1,
            dim(),
        );
        for i in 0..all.len() {
            let col = (i % 4) as i32;
            let row = (i / 4) as i32;
            let cx = 94 + col * 152;
            let cy = 96 + row * 60;
            let on = i == sel;
            if on {
                frame.rounded_rect(cx - 62, cy - 28, 124, 56, 8, panel());
                frame.fill_rect(cx - 62, cy - 28, 124, 2, accent());
            }
            badge(&mut frame, cx - 34, cy, 26, &all[i], on);
            frame.text_shadowed(
                cx - 12,
                cy - 14,
                all[i].abbr,
                2,
                if on { accent() } else { dim() },
                1,
            );
            let ovr = format!("{} OVR", ratings[i].overall);
            frame.text_shadowed(
                cx - 12,
                cy + 2,
                &ovr,
                1,
                if on { paper() } else { dim() },
                1,
            );
            frame.text_shadowed(cx - 12, cy + 14, all[i].city, 1, dim(), 1);
        }
        footer(&mut frame, "[STICK] CLUB   [A] ROSTER   [B] BACK");
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

/// How many men a roster page holds, and how many the chart shows at a
/// position before the rest of the depth has to be reached by editing it.
const CHART_ROWS: usize = 16;
const CHART_SHOWN: usize = 3;

/// The chart as the game will use it: one row per position, best man first,
/// except where the player has moved somebody and the club remembers it.
fn depth_chart(club: &Club) -> (Vec<(Position, Vec<usize>)>, Vec<Player>) {
    const ORDER: [Position; 8] = [
        Position::Qb,
        Position::Rb,
        Position::Wr,
        Position::Te,
        Position::Ol,
        Position::Dl,
        Position::Lb,
        Position::Db,
    ];
    let roster = club.roster();
    let mut depth: Vec<(Position, Vec<usize>)> = Vec::new();
    for (row, pos) in ORDER.into_iter().enumerate() {
        let mut men: Vec<usize> = (0..roster.len())
            .filter(|&i| roster[i].pos == pos)
            .collect();
        men.sort_by_key(|&i| std::cmp::Reverse(roster[i].overall()));
        // A row that has been edited keeps the order the player left it in.
        if let Some(ids) = depth_order(club.seed, row) {
            men.sort_by_key(|&i| {
                ids.iter()
                    .position(|&id| id == roster[i].id)
                    .unwrap_or(usize::MAX)
            });
        }
        depth.push((pos, men));
    }
    (depth, roster)
}

/// One club's chart and roster, drawn from the cursor: a card of positions with
/// the men under each other, and the whole roster beside it as a table.
fn draw_roster(
    frame: &mut Frame,
    club: &Club,
    roster: &[Player],
    depth: &[(Position, Vec<usize>)],
    sel: (usize, usize),
    page: usize,
    pages: usize,
    slide: &ui::Slide,
    t: f32,
) {
    night(frame);
    let w = frame.w as i32;
    let (row, nth) = sel;

    // The club, then what the screen is, and where in the roster we are.
    badge(frame, 40, 40, 34, club, true);
    frame.text_shadowed(
        66,
        26,
        &club.full_name().to_uppercase(),
        ui::T_LABEL,
        paper(),
        1,
    );
    ui::caption(frame, 66, 45, "DEPTH CHART", dim());
    let counter = format!("ROSTER  {} / {}", page + 1, pages);
    let cw = ui::tracked_width(&counter, ui::T_CAPTION, 1);
    ui::caption(frame, w - 22 - cw, 34, &counter, dim());

    let x0 = 18;
    let chart_w = 300;
    let top = 64;
    let row_h = 29;
    let gap = 1;
    let n = depth.len() as i32;
    let card_h = ui::S2 * 2 + n * (row_h + gap) - gap;
    ui::card(
        frame,
        x0,
        top,
        chart_w,
        card_h,
        ui::ACCENT,
        ui::enter(t, 0, 6),
    );
    for (r, (pos, men)) in depth.iter().enumerate() {
        let y = top + ui::S2 + r as i32 * (row_h + gap);
        let heat = slide.heat(r);
        if heat > 0.01 {
            frame.rounded_rect_alpha(
                x0 + 4,
                y,
                chart_w - 8,
                row_h,
                6,
                Rgb::hex(0x2A3A4D),
                0.42 + 0.34 * heat,
            );
        }
        // The position sits on the first man's line, not in the middle of the
        // row: a two-man row would otherwise look like it started lower.
        ui::caption(
            frame,
            x0 + 12,
            y + 3,
            pos.abbr(),
            if r == row { accent() } else { dim() },
        );
        for (k, &i) in men.iter().take(CHART_SHOWN).enumerate() {
            let p = &roster[i];
            let on = r == row && k == nth;
            let ly = y + 3 + k as i32 * 9;
            if on {
                frame.fill_rect(x0 + 44, ly, 2, 7, ui::ACCENT);
            }
            let ink = if on {
                accent()
            } else if k == 0 {
                paper()
            } else {
                dim()
            };
            let num = format!("{:>2}", p.number);
            frame.text_shadowed(x0 + 52, ly, &num, ui::T_CAPTION, ink, 1);
            frame.text_shadowed(
                x0 + 70,
                ly,
                &p.last_name().to_uppercase(),
                ui::T_CAPTION,
                ink,
                1,
            );
            let ovr = format!("{}", p.overall());
            frame.text_shadowed(
                x0 + chart_w - 14 - text_width(&ovr, ui::T_CAPTION),
                ly,
                &ovr,
                ui::T_CAPTION,
                ink,
                1,
            );
        }
    }

    // The roster beside it: a table with the counts lined up, so a page can be
    // read down a column instead of across a sentence.
    let rx = x0 + chart_w + 12;
    let rw = w - 18 - rx;
    ui::card(frame, rx, top, rw, card_h, club.accent, ui::enter(t, 1, 6));
    ui::caption(frame, rx + 12, top + 10, "FULL ROSTER", dim());
    frame.fill_rect_alpha(rx + 12, top + 24, rw - 24, 1, ui::EDGE, 0.9);
    let first = page * CHART_ROWS;
    for (n, i) in (first..(first + CHART_ROWS).min(roster.len())).enumerate() {
        let p = &roster[i];
        let y = top + 32 + n as i32 * 13;
        let num = format!("#{}", p.number);
        frame.text_shadowed(
            rx + 34 - text_width(&num, ui::T_CAPTION),
            y,
            &num,
            ui::T_CAPTION,
            dim(),
            1,
        );
        frame.text_shadowed(
            rx + 44,
            y,
            &p.last_name().to_uppercase(),
            ui::T_CAPTION,
            paper(),
            1,
        );
        frame.text_shadowed(rx + 146, y, p.pos.abbr(), ui::T_CAPTION, dim(), 1);
        let ovr = format!("{}", p.overall());
        frame.text_shadowed(
            rx + rw - 14 - text_width(&ovr, ui::T_CAPTION),
            y,
            &ovr,
            ui::T_CAPTION,
            paper(),
            1,
        );
    }

    footer(
        frame,
        "[UP/DOWN] MAN   [X] UP   [Y] DOWN   [A] CARD   [LEFT/RIGHT] ROSTER   [B] BACK",
    );
}

/// One club's depth chart, best man first at every position, with the full roster
/// beside it. The depths are the same ordering the game uses to pick its seven,
/// and they can be edited: X walks a man up his row and Y walks him down, and the
/// chart is saved against the club, so the next game starts the men the player
/// picked.
fn roster_screen(view: &mut View, pad: &mut Option<&mut Pad>, club: &Club) -> Option<()> {
    let (mut depth, roster) = depth_chart(club);
    let pages = roster.len().div_ceil(CHART_ROWS).max(1);
    let mut page = 0usize;
    let mut sel = (0usize, 0usize);
    let mut nav = Repeat::default();
    let mut nav_y = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut t = 0.0f32;
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        t += dt;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        if let d = nav.step(input.stick.x, dt) {
            if d != 0 {
                page = wrap(page, pages, d);
            }
        }
        // Up and down walk the cursor through the chart, row by row.
        if let d = nav_y.step(input.stick.y, dt) {
            if d != 0 {
                let rows = depth.len() as i32;
                let mut r = sel.0 as i32;
                let mut nth = sel.1 as i32 + d as i32;
                while nth < 0 {
                    r = (r + rows - 1) % rows;
                    nth += depth[r as usize].1.len().min(CHART_SHOWN).max(1) as i32;
                }
                while nth >= depth[r as usize].1.len().min(CHART_SHOWN).max(1) as i32 {
                    nth -= depth[r as usize].1.len().min(CHART_SHOWN).max(1) as i32;
                    r = (r + 1) % rows;
                }
                sel = (r as usize, nth as usize);
            }
        }
        let row = sel.0;
        let shown = depth[row].1.len().min(CHART_SHOWN).max(1);
        if sel.1 >= shown {
            sel.1 = shown - 1;
        }
        // X promotes the man the cursor is on, Y demotes him. The chart is saved
        // the moment it changes: it belongs to the club, not to this screen.
        if input.pressed.x && sel.1 > 0 {
            depth[row].1.swap(sel.1, sel.1 - 1);
            sel.1 -= 1;
            save_depth(club.seed, row, &roster, &depth[row].1);
        }
        if input.pressed.y && sel.1 + 1 < depth[row].1.len() {
            depth[row].1.swap(sel.1, sel.1 + 1);
            sel.1 += 1;
            save_depth(club.seed, row, &roster, &depth[row].1);
        }
        if input.pressed.a {
            let i = depth[row].1[sel.1];
            player_card(view, pad, &roster[i])?;
            view.reset();
            last = Instant::now();
            continue;
        }
        slide.step(sel.0, dt);
        let mut frame = Frame::new(UI_W, UI_H);
        draw_roster(
            &mut frame, club, &roster, &depth, sel, page, pages, &slide, t,
        );
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

/// Save one row of the chart against the club.
fn save_depth(seed: u64, row: usize, roster: &[Player], men: &[usize]) {
    let ids: Vec<u16> = men.iter().map(|&i| roster[i].id).collect();
    set_depth_order(seed, row, ids);
}

/// One man's card: the overall set as the card's hero, the ten numbers a roster
/// is judged by beside it, each bar growing into place so the card reads as a
/// card being filled in rather than as a chart that appeared.
fn draw_player_card(frame: &mut Frame, player: &Player, t: f32) {
    night(frame);
    let w = frame.w as i32;
    let cx = w / 2;
    let ratings = player.attrs.ratings();
    let card_w = 400;
    let card_h = 258;
    let card_x = cx - card_w / 2;
    let card_y = 40;
    ui::card(
        frame,
        card_x,
        card_y,
        card_w,
        card_h,
        ui::ACCENT,
        ui::enter(t, 0, 4),
    );

    let ovr = format!("{}", player.overall());
    // Set plainly: at hero size the drop shadow the small type uses reads as a
    // second digit rather than as depth.
    frame.text(
        card_x + 76 - text_width(&ovr, ui::T_HERO) / 2,
        card_y + 26,
        &ovr,
        ui::T_HERO,
        ui::ACCENT,
    );
    let label = "OVERALL";
    ui::caption(
        frame,
        card_x + 76 - ui::tracked_width(label, ui::T_CAPTION, 2) / 2,
        card_y + 82,
        label,
        dim(),
    );
    frame.fill_rect_alpha(card_x + 140, card_y + 26, 1, 78, ui::EDGE, 0.9);

    let nx = card_x + 160;
    let num = format!("#{}", player.number);
    ui::caption(frame, nx, card_y + 24, &num, dim());
    frame.text_shadowed(
        nx,
        card_y + 38,
        &player.full_name().to_uppercase(),
        ui::T_LABEL,
        paper(),
        1,
    );
    ui::tag(frame, nx, card_y + 62, player.pos.abbr(), ui::ACCENT);

    frame.fill_rect_alpha(card_x + 16, card_y + 112, card_w - 32, 1, ui::EDGE, 0.9);

    let per = ratings.len().div_ceil(2);
    for (n, (name, value)) in ratings.iter().enumerate() {
        let col = (n / per) as i32;
        let row = (n % per) as i32;
        let x = card_x + 20 + col * 186;
        let y = card_y + 126 + row * 24;
        let grow = ui::enter(t, n, ratings.len() + 2);
        ui::caption(frame, x, y + 2, name, dim());
        // The track starts past the longest label ("BLOCK SHEDDING") and stops
        // short of the number, so neither ever sits on the bar.
        ui::meter(
            frame,
            x + 94,
            y,
            58,
            7,
            (*value as f32 / 99.0) * grow,
            ui::ACCENT,
        );
        let v = format!("{value}");
        frame.text_shadowed(
            x + 168 - text_width(&v, ui::T_CAPTION),
            y,
            &v,
            ui::T_CAPTION,
            paper(),
            1,
        );
    }
    footer(frame, "[A/B] BACK");
}

/// One man's card, held until he lets it go.
fn player_card(view: &mut View, pad: &mut Option<&mut Pad>, player: &Player) -> Option<()> {
    view.reset();
    let mut t = 0.0f32;
    let mut last = Instant::now();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        t += dt;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.a || input.pressed.b || input.pressed.start {
            return Some(());
        }
        let mut frame = Frame::new(UI_W, UI_H);
        draw_player_card(&mut frame, player, t);
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

// ---------------------------------------------------------------------------
// Exhibition
// ---------------------------------------------------------------------------

fn ids_in(all: &[Club], division: Division) -> Vec<usize> {
    (0..all.len())
        .filter(|&i| all[i].division == division)
        .collect()
}

fn exhibition(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    settings: &mut Settings,
) -> Option<Kickoff> {
    let all = clubs();
    // One roster build per club: the card's ratings and the men it names come
    // off the same roster, so sixteen cards cost sixteen rosters, not thirty-two.
    let cards: Vec<Card> = all
        .iter()
        .map(|c| {
            let (ratings, men) = c.profile(3);
            Card {
                ratings,
                stars: men
                    .into_iter()
                    .map(|(pos, name, rating)| {
                        (format!("{} {}", pos.abbr(), name.to_uppercase()), rating)
                    })
                    .collect(),
            }
        })
        .collect();
    let sides = choose_sides(view, pad, &all, &cards)?;
    let (yours, theirs, player_home, kits) = (
        sides.yours,
        sides.theirs,
        sides.player_home,
        sides.kits,
    );
    // Which book he runs: his club's own unless he wants another one. Every club
    // has one, the way its NFL counterpart does.
    let book = pick_book(view, pad, yours)?;
    let home = if player_home { yours } else { theirs };
    // The ground comes before the toss: the player picks where the game is
    // played, and then stands on it to call the coin.
    let venue = pick_venue(view, pad, home)?;
    // How long and how hard, then the coin: both are about this game, so they are
    // asked here rather than buried in the settings screen.
    game_setup(view, pad, settings)?;
    let receive = coin_flip(view, pad, &all, yours, theirs, venue)?;
    Some(Kickoff {
        yours,
        theirs,
        player_home,
        kits,
        receive,
        venue,
        book,
    })
}

// ---------------------------------------------------------------------------
// Customization: clubs of the player's own, and repainted kits
// ---------------------------------------------------------------------------

/// Where the created clubs and repainted kits are kept: beside the settings, so
/// one directory holds everything the player has saved.
fn custom_path() -> std::path::PathBuf {
    Settings::path().with_file_name("custom.txt")
}

fn save_custom() {
    let path = custom_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, nopads_core::custom::text());
}

/// Read what the player made, before the first menu is drawn.
pub fn load_custom() {
    if let Ok(text) = std::fs::read_to_string(custom_path()) {
        nopads_core::custom::parse(&text);
    }
}

/// The city names a created club can be given. A pad has no keyboard, so a name
/// is picked off a list; it is a list, not a limit, and it is long enough that
/// fifty clubs do not read as copies of one another.
const CITIES: [&str; 16] = [
    "Springfield",
    "Fresno",
    "Bakersfield",
    "Tulsa",
    "Wichita",
    "Omaha",
    "Boise",
    "Reno",
    "Tacoma",
    "Akron",
    "Toledo",
    "Dayton",
    "Mobile",
    "Shreveport",
    "Lubbock",
    "Amarillo",
];

const NICKNAMES: [&str; 16] = [
    "Expansion",
    "Vinegaroons",
    "Dust",
    "Ironbacks",
    "Stampede",
    "Drifters",
    "Roughnecks",
    "Wranglers",
    "Outlaws",
    "Gamblers",
    "Pioneers",
    "Ravens",
    "Comets",
    "Foundry",
    "Thunder",
    "Storm",
];

fn step_index(i: usize, len: usize, d: i8) -> usize {
    ((i as isize + d as isize).rem_euclid(len as isize)) as usize
}

fn nearest_palette(c: Rgb) -> usize {
    nopads_core::custom::PALETTE
        .iter()
        .enumerate()
        .min_by_key(|(_, (_, p))| {
            let d = (p.r as i32 - c.r as i32).abs()
                + (p.g as i32 - c.g as i32).abs()
                + (p.b as i32 - c.b as i32).abs();
            d
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn pattern_index(p: nopads_core::team::JerseyPattern) -> usize {
    nopads_core::custom::PATTERNS
        .iter()
        .position(|q| *q == p)
        .unwrap_or(0)
}

/// Two choices: a club to make, or a kit to repaint.
fn customize_screen(view: &mut View, pad: &mut Option<&mut Pad>) -> Option<()> {
    const ITEMS: [(&str, &str); 3] = [
        ("CREATE A TEAM", "FIFTY SLOTS FOR A CLUB OF YOUR OWN, IN ANY MODE"),
        ("EDIT A UNIFORM", "REPAINT THE HOME AND AWAY KITS OF ANY CLUB"),
        ("DESIGN A PLAYBOOK", "DRAW FORMATIONS, PLAYS, FRONTS AND CALLS"),
    ];
    let mut pick = 0usize;
    let mut nav = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        if let d = nav.step(input.stick.y, dt) {
            if d != 0 {
                pick = wrap(pick, ITEMS.len(), -d);
            }
        }
        slide.step(pick, dt);
        if input.pressed.a {
            match pick {
                0 => {
                    team_slots(view, pad)?;
                }
                1 => {
                    uniform_pick(view, pad)?;
                }
                _ => {
                    design_hub(view, pad)?;
                }
            }
            view.reset();
            last = Instant::now();
            continue;
        }
        let rows: Vec<(&str, String)> = ITEMS
            .iter()
            .map(|(name, _)| (*name, String::new()))
            .collect();
        let mut frame = Frame::new(UI_W, UI_H);
        draw_option_screen(
            &mut frame,
            "CUSTOMIZATION",
            "MAKE SOMETHING OF YOUR OWN",
            &rows,
            &slide,
            ITEMS[pick].1,
            "[UP/DOWN] CHOOSE   [A] SELECT   [B] BACK",
        );
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

/// The fifty slots, five across and ten down. A opens the slot, X empties it.
fn team_slots(view: &mut View, pad: &mut Option<&mut Pad>) -> Option<()> {
    let mut sel = 0usize;
    let mut nav_x = Repeat::default();
    let mut nav_y = Repeat::default();
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        if let d = nav_x.step(input.stick.x, dt) {
            if d != 0 {
                sel = step_index(sel % 5, 5, d) + (sel / 5) * 5;
            }
        }
        if let d = nav_y.step(input.stick.y, dt) {
            if d != 0 {
                let row = step_index(sel / 5, 10, -d);
                sel = row * 5 + sel % 5;
            }
        }
        if input.pressed.x {
            nopads_core::custom::set_team(sel, None);
            save_custom();
        }
        if input.pressed.a {
            if team_editor(view, pad, sel)? {
                save_custom();
            }
            view.reset();
            last = Instant::now();
        }
        let saved = nopads_core::custom::saved_teams();
        let mut frame = Frame::new(UI_W, UI_H);
        night(&mut frame);
        ui::heading(&mut frame, UI_W as i32 / 2, 16, "CREATE A TEAM", ui::T_BODY, ui::ACCENT);
        ui::caption(
            &mut frame,
            UI_W as i32 / 2 - ui::tracked_width("FIFTY SLOTS. PICK ONE AND BUILD A CLUB.", ui::T_CAPTION, 1) / 2,
            40,
            "FIFTY SLOTS. PICK ONE AND BUILD A CLUB.",
            dim(),
        );
        for i in 0..nopads_core::custom::TEAM_SLOTS {
            let col = (i % 5) as i32;
            let row = (i / 5) as i32;
            let x = 40 + col * 112;
            let y = 62 + row * 24;
            let on = i == sel;
            let filled = saved.get(i).and_then(|t| t.as_ref());
            frame.rounded_rect(
                x,
                y,
                104,
                20,
                5,
                if on { panel().lerp(accent(), 0.10) } else { panel() },
            );
            if on {
                frame.fill_rect(x, y, 104, 2, accent());
            }
            let text = match filled {
                Some(t) => format!("{:02} {}", i + 1, t.abbr),
                None => format!("{:02} --", i + 1),
            };
            frame.text_shadowed(
                x + 8,
                y + 6,
                &text,
                1,
                if on { paper() } else { dim() },
                1,
            );
        }
        footer(&mut frame, "[STICK] SLOT   [A] EDIT   [X] CLEAR   [B] BACK");
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

/// One club, a row at a time. Returns true when it was saved.
fn team_editor(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    slot: usize,
) -> Option<bool> {
    use nopads_core::custom::{CustomTeam, PALETTE, PATTERNS};
    let saved = nopads_core::custom::team(slot);
    let mut city = saved
        .as_ref()
        .and_then(|t| CITIES.iter().position(|c| c.eq_ignore_ascii_case(&t.city)))
        .unwrap_or(slot % CITIES.len());
    let mut name = saved
        .as_ref()
        .and_then(|t| NICKNAMES.iter().position(|c| c.eq_ignore_ascii_case(&t.name)))
        .unwrap_or(slot % NICKNAMES.len());
    let mut division = saved.as_ref().map(|t| t.division).unwrap_or(Division::North);
    let mut primary = saved.as_ref().map(|t| nearest_palette(t.primary)).unwrap_or(2);
    let mut accent = saved.as_ref().map(|t| nearest_palette(t.accent)).unwrap_or(4);
    let mut pattern = saved
        .as_ref()
        .map(|t| pattern_index(t.pattern))
        .unwrap_or(0);
    let mut level = saved.as_ref().map(|t| t.level).unwrap_or(4);
    let mut book = saved.as_ref().map(|t| t.book).unwrap_or(Book::Balanced);
    if !selectable_books().contains(&book) {
        book = Book::Balanced;
    }

    const ROWS: [&str; 8] = [
        "CITY",
        "TEAM NAME",
        "DIVISION",
        "PRIMARY COLOUR",
        "ACCENT COLOUR",
        "JERSEY PATTERN",
        "PLAYBOOK",
        "ATTRIBUTES",
    ];
    let mut row = 0usize;
    let mut nav_y = Repeat::default();
    let mut nav_x = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(false);
        }
        if let d = nav_y.step(input.stick.y, dt) {
            if d != 0 {
                row = wrap(row, ROWS.len(), -d);
            }
        }
        if let d = nav_x.step(input.stick.x, dt) {
            if d != 0 {
                match row {
                    0 => city = step_index(city, CITIES.len(), d),
                    1 => name = step_index(name, NICKNAMES.len(), d),
                    2 => {
                        let i = Division::ALL
                            .iter()
                            .position(|x| *x == division)
                            .unwrap_or(0);
                        division = Division::ALL[step_index(i, Division::ALL.len(), d)];
                    }
                    3 => primary = step_index(primary, PALETTE.len(), d),
                    4 => accent = step_index(accent, PALETTE.len(), d),
                    5 => pattern = step_index(pattern, PATTERNS.len(), d),
                    6 => book = step_book(book, d),
                    _ => level = (level + d as i32).clamp(-2, 12),
                }
            }
        }
        slide.step(row, dt);
        if input.pressed.a {
            let mut t = CustomTeam::blank(slot);
            t.city = CITIES[city].to_string();
            t.name = NICKNAMES[name].to_string();
            t.abbr = "EXP".to_string();
            t.division = division;
            t.primary = PALETTE[primary].1;
            t.accent = PALETTE[accent].1;
            t.secondary = PALETTE[12].1;
            t.pattern = PATTERNS[pattern];
            t.book = book;
            t.level = level;
            nopads_core::custom::set_team(slot, Some(t));
            return Some(true);
        }
        let values = [
            CITIES[city].to_uppercase(),
            NICKNAMES[name].to_uppercase(),
            division.label().to_string(),
            PALETTE[primary].0.to_string(),
            PALETTE[accent].0.to_string(),
            nopads_core::custom::pattern_name(PATTERNS[pattern]).to_uppercase(),
            book.label().to_string(),
            format!("{} / 10", level.clamp(1, 10)),
        ];
        let rows: Vec<(&str, String)> = ROWS
            .iter()
            .zip(values.iter())
            .map(|(label, value)| (*label, value.clone()))
            .collect();
        let mut frame = Frame::new(UI_W, UI_H);
        draw_option_screen(
            &mut frame,
            &format!("SLOT {:02}", slot + 1),
            "A NEW CLUB GETS A FRESH SET OF MEN OF ITS OWN",
            &rows,
            &slide,
            "FRESH PLAYERS EVERY TIME. [LEFT/RIGHT] CHANGE THE ROW.",
            "[UP/DOWN] ROW   [LEFT/RIGHT] CHANGE   [A] SAVE   [B] BACK",
        );
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

/// Which club's kit is being repainted.
fn uniform_pick(view: &mut View, pad: &mut Option<&mut Pad>) -> Option<()> {
    let all = clubs();
    let mut sel = 0usize;
    let mut nav = Repeat::default();
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        if let d = nav.step(input.stick.x, dt) {
            if d != 0 {
                sel = wrap(sel, all.len(), d);
            }
        }
        if input.pressed.a {
            kit_editor(view, pad, &all[sel])?;
            view.reset();
            last = Instant::now();
        }
        let club = &all[sel];
        let mut frame = Frame::new(UI_W, UI_H);
        night(&mut frame);
        ui::heading(&mut frame, UI_W as i32 / 2, 16, "EDIT A UNIFORM", ui::T_BODY, ui::ACCENT);
        ui::caption(
            &mut frame,
            UI_W as i32 / 2 - ui::tracked_width("HOME AND AWAY KITS, FOR ANY CLUB", ui::T_CAPTION, 1) / 2,
            40,
            "HOME AND AWAY KITS, FOR ANY CLUB",
            dim(),
        );
        badge(&mut frame, 320, 120, 64, club, true);
        centered_in(&mut frame, 320, 166, &club.city.to_uppercase(), 2, paper());
        centered_in(&mut frame, 320, 186, &club.name.to_uppercase(), 1, club.accent);
        let painted = nopads_core::custom::kit(club.abbr).is_some();
        centered_in(
            &mut frame,
            320,
            210,
            if painted { "REPAINTED" } else { "AS IT SHIPPED" },
            1,
            if painted { accent() } else { dim() },
        );
        footer(&mut frame, "[LEFT/RIGHT] CLUB   [A] EDIT THE KIT   [B] BACK");
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

/// One club's two kits, a colour at a time.
fn kit_editor(view: &mut View, pad: &mut Option<&mut Pad>, club: &Club) -> Option<()> {
    use nopads_core::custom::{KitEdit, PALETTE, PATTERNS};
    use nopads_core::team::JerseyPattern;

    /// The editor works in palette slots rather than colours: a pad steps
    /// through a short list of names, and the slot is what a name stands for.
    #[derive(Clone, Copy)]
    struct KitDraft {
        home_jersey: usize,
        home_shorts: usize,
        home_accent: usize,
        away_jersey: usize,
        away_shorts: usize,
        away_accent: usize,
        pattern: JerseyPattern,
    }

    let (home, away) = nopads_core::team::club_kits(club);
    let base = nopads_core::custom::kit(club.abbr);
    let mut draft = KitDraft {
        home_jersey: nearest_palette(base.map(|k| k.home_jersey).unwrap_or(home.jersey)),
        home_shorts: nearest_palette(base.map(|k| k.home_shorts).unwrap_or(home.shorts)),
        home_accent: nearest_palette(base.map(|k| k.home_accent).unwrap_or(home.jersey_accent)),
        away_jersey: nearest_palette(base.map(|k| k.away_jersey).unwrap_or(away.jersey)),
        away_shorts: nearest_palette(base.map(|k| k.away_shorts).unwrap_or(away.shorts)),
        away_accent: nearest_palette(base.map(|k| k.away_accent).unwrap_or(away.jersey_accent)),
        pattern: base.map(|k| k.pattern).unwrap_or(home.pattern),
    };
    const ROWS: [&str; 8] = [
        "HOME JERSEY",
        "HOME SHORTS",
        "HOME ACCENT",
        "AWAY JERSEY",
        "AWAY SHORTS",
        "AWAY ACCENT",
        "PATTERN",
        "RESTORE THE CLUB'S KIT",
    ];
    let mut row = 0usize;
    let mut nav_y = Repeat::default();
    let mut nav_x = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        if let d = nav_y.step(input.stick.y, dt) {
            if d != 0 {
                row = wrap(row, ROWS.len(), -d);
            }
        }
        if let d = nav_x.step(input.stick.x, dt) {
            if d != 0 {
                let set = |i: &mut usize| *i = step_index(*i, PALETTE.len(), d);
                match row {
                    0 => set(&mut draft.home_jersey),
                    1 => set(&mut draft.home_shorts),
                    2 => set(&mut draft.home_accent),
                    3 => set(&mut draft.away_jersey),
                    4 => set(&mut draft.away_shorts),
                    5 => set(&mut draft.away_accent),
                    6 => {
                        draft.pattern = PATTERNS[step_index(
                            PATTERNS.iter().position(|p| *p == draft.pattern).unwrap_or(0),
                            PATTERNS.len(),
                            d,
                        )]
                    }
                    _ => {}
                }
            }
        }
        slide.step(row, dt);
        if input.pressed.a {
            if row == ROWS.len() - 1 {
                nopads_core::custom::set_kit(club.abbr, None);
            } else {
                nopads_core::custom::set_kit(
                    club.abbr,
                    Some(KitEdit {
                        home_jersey: PALETTE[draft.home_jersey].1,
                        home_shorts: PALETTE[draft.home_shorts].1,
                        home_accent: PALETTE[draft.home_accent].1,
                        away_jersey: PALETTE[draft.away_jersey].1,
                        away_shorts: PALETTE[draft.away_shorts].1,
                        away_accent: PALETTE[draft.away_accent].1,
                        pattern: draft.pattern,
                    }),
                );
            }
            save_custom();
            return Some(());
        }
        // The rows read back through the palette, so the editor shows a name
        // rather than a number for the colour it is on.
        let values = [
            PALETTE[draft.home_jersey].0.to_string(),
            PALETTE[draft.home_shorts].0.to_string(),
            PALETTE[draft.home_accent].0.to_string(),
            PALETTE[draft.away_jersey].0.to_string(),
            PALETTE[draft.away_shorts].0.to_string(),
            PALETTE[draft.away_accent].0.to_string(),
            nopads_core::custom::pattern_name(draft.pattern).to_uppercase(),
            "BACK TO THE SHIPPED KIT".to_string(),
        ];
        let rows: Vec<(&str, String)> = ROWS
            .iter()
            .zip(values.iter())
            .map(|(label, value)| (*label, value.clone()))
            .collect();
        let mut frame = Frame::new(UI_W, UI_H);
        draw_option_screen(
            &mut frame,
            &format!("{} {}", club.city.to_uppercase(), club.name.to_uppercase()),
            "THE KIT IS WORN WHEREVER THIS CLUB PLAYS",
            &rows,
            &slide,
            "A KIT CHANGE IS SAVED AS SOON AS IT IS MADE.",
            "[UP/DOWN] ROW   [LEFT/RIGHT] COLOUR   [A] SAVE   [B] BACK",
        );
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

// ---------------------------------------------------------------------------
// The playbook editor: a shelf of books, formations of circles, fronts of X's,
// the plays drawn on them and the coverage drawn behind each call.
// ---------------------------------------------------------------------------

use nopads_core::design::{
    self, DesignBook, DesignCall, DesignFormation, DesignFront, DesignPlay, MAX_BOOKS,
    MAX_FORMATIONS, MAX_PLAYS,
};
use nopads_core::math::Vec2;
use nopads_core::playbook::{Coverage, DefRole, Formation, OffRole, PlayFamily};

/// The box the yard grid is drawn in.
const FIELD_L: i32 = 46;
const FIELD_R: i32 = 594;
const FIELD_T: i32 = 58;
const FIELD_B: i32 = 282;

fn field_px(x: f32, y: f32) -> (i32, i32) {
    let (x0, x1) = design::GRID_X;
    let (y0, y1) = design::GRID_Y;
    let fx = ((x - x0 as f32) / (x1 - x0) as f32).clamp(0.0, 1.0);
    let fy = ((y - y0 as f32) / (y1 - y0) as f32).clamp(0.0, 1.0);
    (
        FIELD_L + (fx * (FIELD_R - FIELD_L) as f32) as i32,
        FIELD_T + (fy * (FIELD_B - FIELD_T) as f32) as i32,
    )
}

fn disc(frame: &mut Frame, cx: i32, cy: i32, r: i32, color: Rgb) {
    for y in -r..=r {
        for x in -r..=r {
            if x * x + y * y <= r * r {
                frame.put(cx + x, cy + y, color);
            }
        }
    }
}

fn ring(frame: &mut Frame, cx: i32, cy: i32, r: i32, color: Rgb) {
    for y in -r..=r {
        for x in -r..=r {
            let d = x * x + y * y;
            if d <= r * r && d >= (r - 2) * (r - 2) {
                frame.put(cx + x, cy + y, color);
            }
        }
    }
}

fn cross(frame: &mut Frame, cx: i32, cy: i32, r: i32, color: Rgb) {
    frame.line(cx - r, cy - r, cx + r, cy + r, color, 2);
    frame.line(cx - r, cy + r, cx + r, cy - r, color, 2);
}

fn cursor_box(frame: &mut Frame, x: i8, y: i8) {
    let (px, py) = field_px(x as f32, y as f32);
    let r = 8;
    frame.fill_rect(px - r, py - r, r * 2, 1, ui::ACCENT);
    frame.fill_rect(px - r, py + r, r * 2, 1, ui::ACCENT);
    frame.fill_rect(px - r, py - r, 1, r * 2, ui::ACCENT);
    frame.fill_rect(px + r, py - r, 1, r * 2, ui::ACCENT);
}

/// The jobs on the seven spots, as the screen keeps them: the index into the
/// job's own list, so one board can be drawn for either side of the ball.
fn off_bits(roles: &[OffRole; 7]) -> [u8; 7] {
    let mut out = [0u8; 7];
    for (i, r) in roles.iter().enumerate() {
        out[i] = r.index() as u8;
    }
    out
}

fn def_bits(roles: &[DefRole; 7]) -> [u8; 7] {
    let mut out = [0u8; 7];
    for (i, r) in roles.iter().enumerate() {
        out[i] = r.index() as u8;
    }
    out
}

fn off_roles(bits: &[u8; 7]) -> [OffRole; 7] {
    let mut out = OffRole::ALL;
    for (i, b) in bits.iter().enumerate() {
        out[i] = OffRole::ALL[(*b as usize).min(6)];
    }
    out
}

fn def_roles(bits: &[u8; 7]) -> [DefRole; 7] {
    let mut out = DefRole::ALL;
    for (i, b) in bits.iter().enumerate() {
        out[i] = DefRole::ALL[(*b as usize).min(6)];
    }
    out
}

fn job_label(bits: &[u8; 7], slot: usize, defense: bool) -> &'static str {
    if defense {
        DefRole::ALL[(bits[slot] as usize).min(6)].label()
    } else {
        OffRole::ALL[(bits[slot] as usize).min(6)].label()
    }
}

fn draw_field(frame: &mut Frame, defense: bool) {
    night(frame);
    frame.fill_rect(
        FIELD_L,
        FIELD_T,
        FIELD_R - FIELD_L,
        FIELD_B - FIELD_T,
        Rgb::hex(0x101C15),
    );
    let (x0, x1) = design::GRID_X;
    let (y0, y1) = design::GRID_Y;
    // A line every four yards each way, so a man can be read off the grid
    // instead of guessed at from the middle of the field.
    for x in (x0..=x1).step_by(4) {
        let (px, _) = field_px(x as f32, 0.0);
        frame.fill_rect(px, FIELD_T, 1, FIELD_B - FIELD_T, Rgb::hex(0x1A2B20));
    }
    for y in (y0..=y1).step_by(4) {
        let (_, py) = field_px(0.0, y as f32);
        frame.fill_rect(FIELD_L, py, FIELD_R - FIELD_L, 1, Rgb::hex(0x16261C));
    }
    let (los, _) = field_px(0.0, 0.0);
    // The side the defense stands on is shaded, so the line reads as the line
    // and not as one more yard marker.
    if defense {
        frame.fill_rect_alpha(los, FIELD_T, FIELD_R - los, FIELD_B - FIELD_T, ui::INK, 0.30);
    }
    frame.fill_rect(los - 1, FIELD_T, 3, FIELD_B - FIELD_T, ui::EDGE);
}

fn draw_men(
    frame: &mut Frame,
    spots: &[Vec2; 7],
    roles: &[u8; 7],
    sel: Option<usize>,
    defense: bool,
) {
    for slot in 0..7 {
        let (px, py) = field_px(spots[slot].x, spots[slot].y);
        let lit = sel == Some(slot);
        let color = if lit { ui::ACCENT } else { ui::PAPER };
        if defense {
            cross(frame, px, py, if lit { 6 } else { 5 }, color);
        } else {
            disc(frame, px, py, if lit { 6 } else { 5 }, color);
        }
        if lit {
            ring(frame, px, py, 10, ui::ACCENT);
        }
        // His job, beside him: a circle is a man, and the lettering is which man.
        let label = job_label(roles, slot, defense);
        frame.text_shadowed(px + 13, py - 3, label, 1, color, 1);
    }
}

fn draw_routes(frame: &mut Frame, spots: &[Vec2; 7], routes: &[Vec<Vec2>; 7], sel: usize) {
    for slot in 0..7 {
        let spot = spots[slot];
        let color = if slot == sel { ui::ACCENT } else { ui::GOOD };
        let mut prev = field_px(spot.x, spot.y);
        for p in &routes[slot] {
            let next = field_px(p.x, p.y);
            frame.line(prev.0, prev.1, next.0, next.1, color, 2);
            disc(frame, next.0, next.1, 2, color);
            prev = next;
        }
    }
}

fn draw_drops(frame: &mut Frame, spots: &[Vec2; 7], drops: &[Vec<Vec2>; 7], sel: usize) {
    for slot in 0..7 {
        if drops[slot].is_empty() {
            continue;
        }
        let spot = spots[slot];
        let color = if slot == sel { ui::ACCENT } else { ui::GOOD };
        let mut prev = field_px(spot.x, spot.y);
        for p in &drops[slot] {
            let next = field_px(p.x, p.y);
            frame.line(prev.0, prev.1, next.0, next.1, color, 2);
            prev = next;
        }
        ring(frame, prev.0, prev.1, 4, color);
    }
}

/// Draw one formation or front on the grid. A picks up the man under the
/// cursor, the stick carries him, A sets him down, Y changes which job stands
/// on the spot he is on.
fn spots_editor(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    title: &str,
    spots: &mut [Vec2; 7],
    roles: &mut [u8; 7],
    defense: bool,
) -> Option<()> {
    let mut sel = 0usize;
    let mut cursor = (spots[0].x as i8, spots[0].y as i8);
    let mut held = false;
    let mut nav_x = Repeat::default();
    let mut nav_y = Repeat::default();
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        if let d = nav_x.step(input.stick.x, dt) {
            if d != 0 {
                cursor.0 = (cursor.0 + d).clamp(design::GRID_X.0, design::GRID_X.1);
            }
        }
        // The stick's up is the screen's up and the field's `y` grows down the
        // screen, so the sign flips here.
        if let d = nav_y.step(input.stick.y, dt) {
            if d != 0 {
                cursor.1 = (cursor.1 - d).clamp(design::GRID_Y.0, design::GRID_Y.1);
            }
        }
        let cell = Vec2::new(cursor.0 as f32, cursor.1 as f32);
        if input.pressed.a {
            if !held {
                if let Some(i) = spots.iter().position(|s| *s == cell) {
                    sel = i;
                    held = true;
                }
            } else if !spots.iter().enumerate().any(|(i, s)| i != sel && *s == cell) {
                spots[sel] = cell;
                held = false;
            }
        }
        // Y gives the man under the cursor the next job across, swapping with
        // whoever was doing it: the seven jobs stay seven jobs.
        if input.pressed.y {
            let under = if held {
                Some(sel)
            } else {
                spots.iter().position(|s| *s == cell)
            };
            if let Some(here) = under {
                let next = (roles[here] + 1) % 7;
                if let Some(other) = roles.iter().position(|r| *r == next) {
                    roles.swap(here, other);
                }
                sel = here;
            }
        }
        let problem = if defense {
            design::front_problem(spots)
        } else {
            design::formation_problem_roles(spots, &off_roles(roles))
        };
        let mut frame = Frame::new(UI_W, UI_H);
        draw_field(&mut frame, defense);
        draw_men(&mut frame, spots, roles, Some(sel), defense);
        cursor_box(&mut frame, cursor.0, cursor.1);
        ui::heading(&mut frame, UI_W as i32 / 2, 16, title, ui::T_BODY, ui::ACCENT);
        centered_in(
            &mut frame,
            320,
            40,
            &format!(
                "{} {}  -  [Y] CHANGES THE JOB",
                if held { "CARRYING" } else { "SET" },
                job_label(roles, sel, defense)
            ),
            1,
            if held { accent() } else { dim() },
        );
        match problem {
            Some(why) => centered_in(&mut frame, 320, FIELD_B + 10, why, 1, ui::DANGER),
            None => centered_in(
                &mut frame,
                320,
                FIELD_B + 10,
                if defense {
                    "EVERY MAN IS SET"
                } else {
                    "THE LINE IS LEGAL"
                },
                1,
                ui::GOOD,
            ),
        }
        footer(
            &mut frame,
            if held {
                "[STICK] CARRY HIM   [A] SET HIM DOWN   [Y] JOB   [B] DONE"
            } else {
                "[STICK] CURSOR   [A] PICK UP A MAN   [Y] JOB   [B] DONE"
            },
        );
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

/// Draw one man's path with the stick. A puts the pen down, every yard the stick
/// walks the path picks up a waypoint, A lifts the pen again.
fn play_editor(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    form: &DesignFormation,
    play: &mut DesignPlay,
) -> Option<()> {
    let bits = off_bits(&form.roles);
    let mut sel = 0usize;
    let mut cursor = (form.spots[0].x as i8, form.spots[0].y as i8);
    let mut pen = false;
    let mut nav_x = Repeat::default();
    let mut nav_y = Repeat::default();
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        let mut moved = false;
        if let d = nav_x.step(input.stick.x, dt) {
            if d != 0 {
                cursor.0 = (cursor.0 + d).clamp(design::GRID_X.0, design::GRID_X.1);
                moved = true;
            }
        }
        if let d = nav_y.step(input.stick.y, dt) {
            if d != 0 {
                cursor.1 = (cursor.1 - d).clamp(design::GRID_Y.0, design::GRID_Y.1);
                moved = true;
            }
        }
        let cell = Vec2::new(cursor.0 as f32, cursor.1 as f32);
        if input.pressed.x {
            sel = (sel + 1) % 7;
            let spot = form.spots[sel];
            cursor = (spot.x as i8, spot.y as i8);
            pen = false;
        }
        if input.pressed.y {
            play.family = if play.family.is_pass() {
                PlayFamily::RunInside
            } else {
                PlayFamily::QuickPass
            };
        }
        if input.pressed.select {
            play.routes[sel].clear();
            pen = false;
        }
        if input.pressed.a {
            pen = !pen;
            if pen {
                if play.routes[sel].is_empty() {
                    let spot = form.spots[sel];
                    play.routes[sel].push(Vec2::new(spot.x, spot.y));
                }
                if play.routes[sel].last() != Some(&cell) {
                    play.routes[sel].push(cell);
                }
            }
        }
        if pen && moved && play.routes[sel].last() != Some(&cell) {
            // One waypoint a yard as the stick walks: the path is what the thumb
            // drew, not a line between two far corners.
            play.routes[sel].push(cell);
        }
        let role = form.roles[sel];
        let drawn = play.has_path();
        let mut frame = Frame::new(UI_W, UI_H);
        draw_field(&mut frame, false);
        draw_routes(&mut frame, &form.spots, &play.routes, sel);
        draw_men(&mut frame, &form.spots, &bits, None, false);
        if let Some(p) = play.routes[sel].last() {
            let a = field_px(p.x, p.y);
            let b = field_px(cursor.0 as f32, cursor.1 as f32);
            frame.line(a.0, a.1, b.0, b.1, accent().lerp(backdrop(), 0.55), 1);
        }
        cursor_box(&mut frame, cursor.0, cursor.1);
        ui::heading(&mut frame, UI_W as i32 / 2, 16, &play.name, ui::T_BODY, ui::ACCENT);
        centered_in(
            &mut frame,
            320,
            40,
            &format!(
                "DRAWING {} - {} - {}",
                role.label(),
                if play.family.is_pass() { "PASS" } else { "RUN" },
                if pen { "PEN DOWN" } else { "PEN UP" }
            ),
            1,
            if pen { accent() } else { dim() },
        );
        match (drawn, play.family.is_pass()) {
            (false, _) => centered_in(
                &mut frame,
                320,
                FIELD_B + 10,
                "EVERY PLAY NEEDS A DRAWN PATH: [A] PUTS THE PEN DOWN",
                1,
                ui::DANGER,
            ),
            (true, true) => centered_in(
                &mut frame,
                320,
                FIELD_B + 10,
                "EVERY YARD YOU DRAW IS A WAYPOINT: THE ROUTE STARTS AT HIS FEET",
                1,
                dim(),
            ),
            (true, false) => centered_in(
                &mut frame,
                320,
                FIELD_B + 10,
                "ON A RUN THE BACK'S PATH IS THE BALL; THE OTHERS BLOCK ALONG THEIRS",
                1,
                dim(),
            ),
        }
        footer(
            &mut frame,
            "[STICK] DRAW   [A] PEN   [X] NEXT MAN   [Y] PASS/RUN   [SELECT] WIPE   [B] DONE",
        );
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

/// Draw the coverage behind a call: the same pen, one defender at a time, with
/// the press toggle on the button beside it.
fn call_editor(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    front: &DesignFront,
    call: &mut DesignCall,
) -> Option<()> {
    let bits = def_bits(&front.roles);
    let mut sel = 0usize;
    let mut cursor = (front.spots[0].x as i8, front.spots[0].y as i8);
    let mut pen = false;
    let mut nav_x = Repeat::default();
    let mut nav_y = Repeat::default();
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        let mut moved = false;
        if let d = nav_x.step(input.stick.x, dt) {
            if d != 0 {
                cursor.0 = (cursor.0 + d).clamp(0, design::GRID_X.1);
                moved = true;
            }
        }
        if let d = nav_y.step(input.stick.y, dt) {
            if d != 0 {
                cursor.1 = (cursor.1 - d).clamp(design::GRID_Y.0, design::GRID_Y.1);
                moved = true;
            }
        }
        let cell = Vec2::new(cursor.0 as f32, cursor.1 as f32);
        if input.pressed.x {
            sel = (sel + 1) % 7;
            let spot = front.spots[sel];
            cursor = (spot.x as i8, spot.y as i8);
            pen = false;
        }
        if input.pressed.y {
            call.press = !call.press;
        }
        if input.pressed.select {
            call.drops[sel].clear();
            pen = false;
        }
        if input.pressed.a {
            pen = !pen;
            if pen {
                if call.drops[sel].is_empty() {
                    let spot = front.spots[sel];
                    call.drops[sel].push(Vec2::new(spot.x, spot.y));
                }
                if call.drops[sel].last() != Some(&cell) {
                    call.drops[sel].push(cell);
                }
            }
        }
        if pen && moved && call.drops[sel].last() != Some(&cell) {
            call.drops[sel].push(cell);
        }
        let role = front.roles[sel];
        let mut frame = Frame::new(UI_W, UI_H);
        draw_field(&mut frame, true);
        draw_drops(&mut frame, &front.spots, &call.drops, sel);
        draw_men(&mut frame, &front.spots, &bits, Some(sel), true);
        if let Some(p) = call.drops[sel].last() {
            let a = field_px(p.x, p.y);
            let b = field_px(cursor.0 as f32, cursor.1 as f32);
            frame.line(a.0, a.1, b.0, b.1, accent().lerp(backdrop(), 0.55), 1);
        }
        cursor_box(&mut frame, cursor.0, cursor.1);
        ui::heading(&mut frame, UI_W as i32 / 2, 16, &call.name, ui::T_BODY, ui::ACCENT);
        centered_in(
            &mut frame,
            320,
            40,
            &format!(
                "{} - {} - {} - {}",
                role.label(),
                call.coverage.label(),
                if call.press { "PRESS" } else { "OFF" },
                if pen { "PEN DOWN" } else { "PEN UP" }
            ),
            1,
            if pen { accent() } else { dim() },
        );
        centered_in(
            &mut frame,
            320,
            FIELD_B + 10,
            if call.has_drop() {
                "DRAW WHERE HE DROPS: THE COVERAGE FOLLOWS THE PATH YOU DREW"
            } else {
                "NO DROP DRAWN: THE COVERAGE PICKS WHERE HE GOES"
            },
            1,
            if call.has_drop() { dim() } else { ui::DANGER },
        );
        footer(
            &mut frame,
            "[STICK] DRAW   [A] PEN   [X] NEXT MAN   [Y] PRESS/OFF   [SELECT] WIPE   [B] DONE",
        );
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

// ---------------------------------------------------------------------------
// Naming: a pad has no keyboard, so the letters are on the screen
// ---------------------------------------------------------------------------

const KEY_ROWS: [&str; 6] = ["ABCDEF", "GHIJKL", "MNOPQR", "STUVWX", "YZ0123", "456789"];
const KEY_ACTIONS: [&str; 3] = ["SPACE", "DEL", "DONE"];
const NAME_MAX: usize = 14;

/// The keyboard, drawn from the state the loop keeps, so a picture of it needs no
/// window and no pad.
fn draw_name_screen(frame: &mut Frame, title: &str, prompt: &str, text: &str, row: usize, col: usize) {
    night(frame);
    ui::heading(frame, UI_W as i32 / 2, 16, title, ui::T_BODY, ui::ACCENT);
    centered_in(frame, 320, 40, prompt, 1, dim());
    let shown = format!("[ {} ]", text);
    centered_in(frame, 320, 62, &shown, 1, ui::PAPER);
    let x0 = 110;
    let y0 = 96;
    let cw = 70;
    let chh = 30;
    for (r, keys) in KEY_ROWS.iter().enumerate() {
        for (c, ch) in keys.chars().enumerate() {
            let x = x0 + c as i32 * cw;
            let y = y0 + r as i32 * chh;
            let lit = row == r && col == c;
            frame.fill_rect(x, y, 60, 20, if lit { ui::ACCENT } else { Rgb::hex(0x1B2430) });
            frame.text_shadowed(
                x + 24,
                y + 4,
                &ch.to_string(),
                1,
                if lit { ui::INK } else { ui::PAPER },
                1,
            );
        }
    }
    for (c, action) in KEY_ACTIONS.iter().enumerate() {
        let x = x0 + c as i32 * 140;
        let y = y0 + 6 * chh;
        let lit = row == 6 && col == c;
        frame.fill_rect(
            x,
            y,
            130,
            20,
            if lit { ui::ACCENT } else { Rgb::hex(0x1B2430) },
        );
        frame.text_shadowed(
            x + 12,
            y + 4,
            action,
            1,
            if lit { ui::INK } else { ui::PAPER },
            1,
        );
    }
    footer(
        frame,
        "[STICK] LETTERS   [A] TYPE   [B] KEEP IT AS IT IS   [START] DONE",
    );
}

fn text_editor(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    title: &str,
    prompt: &str,
    text: &mut String,
) -> Option<()> {
    let mut row = 0usize;
    let mut col = 0usize;
    let mut nav_x = Repeat::default();
    let mut nav_y = Repeat::default();
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        if let d = nav_x.step(input.stick.x, dt) {
            if d != 0 {
                col = step_index(col, 6, d);
            }
        }
        if let d = nav_y.step(input.stick.y, dt) {
            if d != 0 {
                row = step_index(row, 7, -d);
            }
        }
        if input.pressed.start {
            return Some(());
        }
        if input.pressed.a {
            if row < KEY_ROWS.len() {
                if let Some(ch) = KEY_ROWS[row].chars().nth(col) {
                    if text.len() < NAME_MAX {
                        text.push(ch);
                    }
                }
            } else {
                match col {
                    0 => {
                        if text.len() < NAME_MAX && !text.is_empty() {
                            text.push(' ');
                        }
                    }
                    1 => {
                        text.pop();
                    }
                    _ => return Some(()),
                }
            }
        }
        let mut frame = Frame::new(UI_W, UI_H);
        draw_name_screen(&mut frame, title, prompt, text.as_str(), row, col);
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

fn name_something(view: &mut View, pad: &mut Option<&mut Pad>, what: &str, name: &mut String) -> Option<()> {
    text_editor(
        view,
        pad,
        &format!("NAME THE {}", what),
        "STICK TO A LETTER, [A] TO TYPE IT",
        name,
    )
}

// ---------------------------------------------------------------------------
// The shelf of books
// ---------------------------------------------------------------------------

fn keep_book(book: &DesignBook) {
    design::set(Some(book.clone()));
    save_custom();
}

fn book_status(book: &DesignBook) -> (String, Rgb) {
    match book.problems().first() {
        Some(why) => (why.clone(), ui::DANGER),
        None => (
            "READY: A CLUB CAN TAKE THIS BOOK INTO A GAME".to_string(),
            ui::GOOD,
        ),
    }
}

/// The shelf, drawn from a list of books so the picture can be made without a
/// store behind it or a pad in front of it.
fn draw_shelf(frame: &mut Frame, books: &[DesignBook], pick: usize, slide: &ui::Slide) {
    let mut rows: Vec<(&str, String)> = Vec::new();
    if books.is_empty() {
        rows.push(("START A PLAYBOOK", "BLANK OR FROM A BOOK".to_string()));
    } else {
        for b in books {
            let (status, _) = book_status(b);
            let note = match status.as_str() {
                "READY: A CLUB CAN TAKE THIS BOOK INTO A GAME" => "READY".to_string(),
                other => other.to_string(),
            };
            rows.push((b.name.as_str(), note));
        }
        if books.len() < MAX_BOOKS {
            rows.push(("NEW PLAYBOOK", String::new()));
        }
    }
    let note = if books.is_empty() {
        "NOTHING DRAWN YET".to_string()
    } else {
        let b = &books[pick.min(books.len().saturating_sub(1))];
        book_status(b).0
    };
    draw_option_screen(
        frame,
        "PLAYBOOKS",
        "EACH BOOK CARRIES ITS OWN PLAYS, FRONTS AND COVERAGE",
        &rows,
        slide,
        &note,
        "[UP/DOWN] PICK   [A] OPEN   [Y] RENAME   [X] DELETE   [B] BACK",
    );
}

fn draw_start_book(frame: &mut Frame, pick: usize, slide: &ui::Slide) {
    let books = Book::ALL;
    let mut rows: Vec<(&str, String)> =
        vec![("BLANK SHEET", "DRAW IT ALL YOURSELF".to_string())];
    for b in books.iter() {
        rows.push((b.label(), String::new()));
    }
    let note = if pick == 0 {
        "TWO FORMATIONS AND TWO FRONTS, WITH EVERY MAN ON THE FIELD"
    } else {
        books[pick - 1].blurb()
    };
    draw_option_rows(
        frame,
        "START A PLAYBOOK",
        "A COPY OF A BOOK OR A BLANK SHEET: BOTH ARE YOURS TO CHANGE",
        &rows,
        slide,
        note,
        "[UP/DOWN] PICK   [A] START   [B] BACK",
        24,
        3,
    );
}

fn draw_book_menu(frame: &mut Frame, book: &DesignBook, slide: &ui::Slide) {
    let (status, _) = book_status(book);
    let rows = [
        (
            "OFFENSE",
            format!(
                "{} FORMS - {} PLAYS",
                book.offense.len(),
                book.plays.iter().map(|p| p.len()).sum::<usize>()
            ),
        ),
        (
            "DEFENSE",
            format!(
                "{} FRONTS - {} CALLS",
                book.defense.len(),
                book.calls.iter().map(|c| c.len()).sum::<usize>()
            ),
        ),
        ("RENAME THE BOOK", String::new()),
        ("DELETE THE BOOK", String::new()),
    ];
    draw_option_screen(
        frame,
        &book.name,
        "CIRCLES FOR THE OFFENSE, X'S FOR THE DEFENSE",
        &rows,
        slide,
        &status,
        "[UP/DOWN] PICK   [A] OPEN   [B] BACK",
    );
}

fn counted(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn draw_offense_list(frame: &mut Frame, book: &DesignBook, slide: &ui::Slide) {
    let mut rows: Vec<(&str, String)> = Vec::new();
    for (i, f) in book.offense.iter().enumerate() {
        let n = book.plays_of(i).len();
        rows.push((f.name.as_str(), counted(n, "PLAY", "PLAYS")));
    }
    if book.offense.len() < MAX_FORMATIONS {
        rows.push(("NEW FORMATION", String::new()));
    }
    draw_option_screen(
        frame,
        "OFFENSE",
        "TWO LINEMEN ON THE LINE, THE QUARTERBACK BEHIND THEM",
        &rows,
        slide,
        "EACH FORMATION CARRIES ONE TO NINE PLAYS",
        "[UP/DOWN] FORMATION   [A] OPEN   [Y] RENAME   [X] DELETE   [B] BACK",
    );
}

fn draw_play_list(frame: &mut Frame, book: &DesignBook, fi: usize, slide: &ui::Slide) {
    let plays = book.plays_of(fi).len();
    let mut rows: Vec<(&str, String)> = Vec::new();
    for p in book.plays_of(fi) {
        rows.push((
            p.name.as_str(),
            format!(
                "{} - {} WAYPOINTS",
                if p.family.is_pass() { "PASS" } else { "RUN" },
                p.routes.iter().map(|r| r.len()).sum::<usize>()
            ),
        ));
    }
    if plays < MAX_PLAYS {
        rows.push(("NEW PLAY", String::new()));
    }
    draw_option_screen(
        frame,
        &book.offense[fi].name,
        "ONE TO NINE PLAYS. DRAW EACH MAN'S PATH WITH THE STICK.",
        &rows,
        slide,
        "A RUN'S BACK PATH IS THE BALL; A PASS'S PATHS ARE THE ROUTES",
        "[UP/DOWN] PLAY   [A] OPEN   [Y] RENAME   [X] DELETE   [B] BACK",
    );
}

fn draw_defense_list(frame: &mut Frame, book: &DesignBook, slide: &ui::Slide) {
    let mut rows: Vec<(&str, String)> = Vec::new();
    for (i, f) in book.defense.iter().enumerate() {
        let n = book.calls_of(i).len();
        rows.push((f.name.as_str(), counted(n, "CALL", "CALLS")));
    }
    if book.defense.len() < MAX_FORMATIONS {
        rows.push(("NEW FRONT", String::new()));
    }
    draw_option_screen(
        frame,
        "DEFENSE",
        "STAND SEVEN MEN WHERE YOU WANT THEM",
        &rows,
        slide,
        "EACH FRONT CARRIES ONE TO NINE CALLS",
        "[UP/DOWN] FRONT   [A] OPEN   [Y] RENAME   [X] DELETE   [B] BACK",
    );
}

fn draw_call_list(frame: &mut Frame, book: &DesignBook, fi: usize, slide: &ui::Slide) {
    let calls = book.calls_of(fi).len();
    let mut rows: Vec<(&str, String)> = Vec::new();
    for c in book.calls_of(fi) {
        rows.push((
            c.name.as_str(),
            format!(
                "{} - {}{}",
                c.coverage.label(),
                if c.press { "PRESS" } else { "OFF" },
                if c.has_drop() { " - DRAWN" } else { "" }
            ),
        ));
    }
    if calls < MAX_PLAYS {
        rows.push(("NEW CALL", String::new()));
    }
    draw_option_screen(
        frame,
        &book.defense[fi].name,
        "ONE TO NINE CALLS OFF THIS FRONT",
        &rows,
        slide,
        "LEFT/RIGHT CHANGES THE COVERAGE; [A] DRAWS THE DROPS",
        "[UP/DOWN] CALL   [LEFT/RIGHT] COVERAGE   [A] OPEN   [Y] RENAME   [X] DELETE",
    );
}

/// Where a designed book starts: a blank sheet or a copy of one the league runs.
fn start_book(view: &mut View, pad: &mut Option<&mut Pad>) -> Option<()> {
    let books: Vec<Book> = Book::ALL.to_vec();
    let mut pick = 0usize;
    let mut nav = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        if let d = nav.step(input.stick.y, dt) {
            if d != 0 {
                pick = wrap(pick, books.len() + 1, -d);
            }
        }
        slide.step(pick, dt);
        let mut frame = Frame::new(UI_W, UI_H);
        draw_start_book(&mut frame, pick, &slide);
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        if input.pressed.a {
            let made = if pick == 0 {
                DesignBook::blank()
            } else {
                DesignBook::from_built_in(books[pick - 1])
            };
            if design::count() >= MAX_BOOKS {
                return Some(());
            }
            let i = design::add(made);
            design::select(i);
            save_custom();
            return Some(());
        }
        beat();
    }
}

/// The shelf: every book the player has drawn, and the way to start another.
fn design_hub(view: &mut View, pad: &mut Option<&mut Pad>) -> Option<()> {
    let mut pick = 0usize;
    let mut nav = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        let books = design::shelf();
        if let d = nav.step(input.stick.y, dt) {
            if d != 0 {
                let count = if books.is_empty() {
                    1
                } else {
                    books.len() + usize::from(books.len() < MAX_BOOKS)
                };
                pick = wrap(pick, count, -d);
            }
        }
        slide.step(pick, dt);
        if input.pressed.a {
            if books.is_empty() || pick >= books.len() {
                start_book(view, pad)?;
            } else {
                design::select(pick);
                book_menu(view, pad, pick)?;
            }
            view.reset();
            last = Instant::now();
            continue;
        }
        if input.pressed.y && !books.is_empty() && pick < books.len() {
            let mut name = books[pick].name.clone();
            name_something(view, pad, "BOOK", &mut name)?;
            let mut b = books[pick].clone();
            b.name = name;
            design::select(pick);
            design::set(Some(b));
            save_custom();
            view.reset();
            last = Instant::now();
            continue;
        }
        if input.pressed.x && !books.is_empty() && pick < books.len() {
            design::remove(pick);
            save_custom();
            pick = pick.min(design::count().saturating_sub(1));
            view.reset();
            last = Instant::now();
            continue;
        }
        let mut frame = Frame::new(UI_W, UI_H);
        draw_shelf(&mut frame, &books, pick, &slide);
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

/// One book's three doors.
fn book_menu(view: &mut View, pad: &mut Option<&mut Pad>, index: usize) -> Option<()> {
    let mut pick = 0usize;
    let mut nav = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        let Some(book) = design::book_at(index) else {
            return Some(());
        };
        if let d = nav.step(input.stick.y, dt) {
            if d != 0 {
                pick = wrap(pick, 4, -d);
            }
        }
        slide.step(pick, dt);
        if input.pressed.a {
            match pick {
                0 => offense_list(view, pad)?,
                1 => defense_list(view, pad)?,
                2 => {
                    let mut name = book.name.clone();
                    name_something(view, pad, "BOOK", &mut name)?;
                    let mut b = book.clone();
                    b.name = name;
                    design::select(index);
                    design::set(Some(b));
                    save_custom();
                }
                _ => {
                    design::remove(index);
                    save_custom();
                    return Some(());
                }
            }
            view.reset();
            last = Instant::now();
            continue;
        }
        let mut frame = Frame::new(UI_W, UI_H);
        draw_book_menu(&mut frame, &book, &slide);
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

/// The formations of the book, and the plays hanging off each.
fn offense_list(view: &mut View, pad: &mut Option<&mut Pad>) -> Option<()> {
    let Some(mut book) = design::book() else {
        return Some(());
    };
    let mut sel = 0usize;
    let mut nav = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        let n = book.offense.len() + usize::from(book.offense.len() < MAX_FORMATIONS);
        if let d = nav.step(input.stick.y, dt) {
            if d != 0 {
                sel = wrap(sel, n.max(1), -d);
            }
        }
        if input.pressed.y && sel < book.offense.len() {
            let mut name = book.offense[sel].name.clone();
            name_something(view, pad, "FORMATION", &mut name)?;
            book.offense[sel].name = name;
            keep_book(&book);
            view.reset();
            last = Instant::now();
            continue;
        }
        if input.pressed.x && sel < book.offense.len() {
            book.offense.remove(sel);
            book.plays.remove(sel);
            keep_book(&book);
            sel = sel.min(book.offense.len().saturating_sub(1));
            view.reset();
            last = Instant::now();
            continue;
        }
        if input.pressed.a {
            if sel < book.offense.len() {
                let mut spots = book.offense[sel].spots;
                let mut roles = off_bits(&book.offense[sel].roles);
                spots_editor(
                    view,
                    pad,
                    &format!("OFFENSE {}", sel + 1),
                    &mut spots,
                    &mut roles,
                    false,
                )?;
                book.offense[sel].spots = spots;
                book.offense[sel].roles = off_roles(&roles);
                keep_book(&book);
                play_list(view, pad, &mut book, sel)?;
            } else if book.offense.len() < MAX_FORMATIONS {
                let f = DesignFormation::blank(book.offense.len(), Formation::Pistol);
                let play = DesignPlay::blank(format!("{} 1", f.name));
                book.offense.push(f);
                book.plays.push(vec![play]);
                keep_book(&book);
                sel = book.offense.len() - 1;
            }
            view.reset();
            last = Instant::now();
            continue;
        }
        slide.step(sel, dt);
        let mut frame = Frame::new(UI_W, UI_H);
        draw_offense_list(&mut frame, &book, &slide);
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

fn play_list(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    book: &mut DesignBook,
    fi: usize,
) -> Option<()> {
    let mut sel = 0usize;
    let mut nav = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        let plays = book.plays_of(fi).len();
        let n = plays + usize::from(plays < MAX_PLAYS);
        if let d = nav.step(input.stick.y, dt) {
            if d != 0 {
                sel = wrap(sel, n.max(1), -d);
            }
        }
        if input.pressed.y && sel < plays {
            let mut name = book.plays[fi][sel].name.clone();
            name_something(view, pad, "PLAY", &mut name)?;
            book.plays[fi][sel].name = name;
            keep_book(book);
            view.reset();
            last = Instant::now();
            continue;
        }
        if input.pressed.x && sel < plays {
            book.plays[fi].remove(sel);
            keep_book(book);
            sel = sel.min(book.plays[fi].len().saturating_sub(1));
            view.reset();
            last = Instant::now();
            continue;
        }
        if input.pressed.a {
            if sel >= plays {
                if plays >= MAX_PLAYS {
                    continue;
                }
                let name = format!("{} {}", book.offense[fi].name, plays + 1);
                book.plays[fi].push(DesignPlay::blank(name));
                keep_book(book);
            }
            let form = book.offense[fi].clone();
            let mut play = book.plays[fi][sel].clone();
            play_editor(view, pad, &form, &mut play)?;
            book.plays[fi][sel] = play;
            keep_book(book);
            view.reset();
            last = Instant::now();
            continue;
        }
        slide.step(sel, dt);
        let mut frame = Frame::new(UI_W, UI_H);
        draw_play_list(&mut frame, book, fi, &slide);
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

fn defense_list(view: &mut View, pad: &mut Option<&mut Pad>) -> Option<()> {
    let Some(mut book) = design::book() else {
        return Some(());
    };
    let mut sel = 0usize;
    let mut nav = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        let n = book.defense.len() + usize::from(book.defense.len() < MAX_FORMATIONS);
        if let d = nav.step(input.stick.y, dt) {
            if d != 0 {
                sel = wrap(sel, n.max(1), -d);
            }
        }
        if input.pressed.y && sel < book.defense.len() {
            let mut name = book.defense[sel].name.clone();
            name_something(view, pad, "FRONT", &mut name)?;
            book.defense[sel].name = name;
            keep_book(&book);
            view.reset();
            last = Instant::now();
            continue;
        }
        if input.pressed.x && sel < book.defense.len() {
            book.defense.remove(sel);
            book.calls.remove(sel);
            keep_book(&book);
            sel = sel.min(book.defense.len().saturating_sub(1));
            view.reset();
            last = Instant::now();
            continue;
        }
        if input.pressed.a {
            if sel < book.defense.len() {
                let mut spots = book.defense[sel].spots;
                let mut roles = def_bits(&book.defense[sel].roles);
                spots_editor(
                    view,
                    pad,
                    &format!("DEFENSE {}", sel + 1),
                    &mut spots,
                    &mut roles,
                    true,
                )?;
                book.defense[sel].spots = spots;
                book.defense[sel].roles = def_roles(&roles);
                keep_book(&book);
                call_list(view, pad, &mut book, sel)?;
            } else if book.defense.len() < MAX_FORMATIONS {
                let f = DesignFront::blank(book.defense.len());
                book.defense.push(f);
                book.calls.push(vec![DesignCall::blank(
                    "COVER 1".to_string(),
                    Coverage::ManFree,
                )]);
                keep_book(&book);
                sel = book.defense.len() - 1;
            }
            view.reset();
            last = Instant::now();
            continue;
        }
        slide.step(sel, dt);
        let mut frame = Frame::new(UI_W, UI_H);
        draw_defense_list(&mut frame, &book, &slide);
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

fn call_list(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    book: &mut DesignBook,
    fi: usize,
) -> Option<()> {
    const COVERAGES: [Coverage; 6] = [
        Coverage::ManFree,
        Coverage::Cover2,
        Coverage::Cover3,
        Coverage::Cover4,
        Coverage::ManPress,
        Coverage::GoalLine,
    ];
    let mut sel = 0usize;
    let mut nav = Repeat::default();
    let mut nav_x = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return Some(());
        }
        let calls = book.calls_of(fi).len();
        let n = calls + usize::from(calls < MAX_PLAYS);
        if let d = nav.step(input.stick.y, dt) {
            if d != 0 {
                sel = wrap(sel, n.max(1), -d);
            }
        }
        // Left and right change the coverage of the call under the bar, so the
        // front can be tried with every coverage without drawing it again.
        if let d = nav_x.step(input.stick.x, dt) {
            if d != 0 && sel < calls {
                let c = &mut book.calls[fi][sel];
                let idx = COVERAGES
                    .iter()
                    .position(|x| *x == c.coverage)
                    .unwrap_or(0);
                let next = COVERAGES[step_index(idx, COVERAGES.len(), d)];
                // A call still wearing its coverage's name is renamed with it.
                if c.name == c.coverage.label() {
                    c.name = next.label().to_string();
                }
                c.coverage = next;
                c.press = next == Coverage::ManPress;
                keep_book(book);
                view.reset();
                last = Instant::now();
                continue;
            }
        }
        if input.pressed.y && sel < calls {
            let mut name = book.calls[fi][sel].name.clone();
            name_something(view, pad, "CALL", &mut name)?;
            book.calls[fi][sel].name = name;
            keep_book(book);
            view.reset();
            last = Instant::now();
            continue;
        }
        if input.pressed.x && sel < calls {
            book.calls[fi].remove(sel);
            keep_book(book);
            sel = sel.min(book.calls[fi].len().saturating_sub(1));
            view.reset();
            last = Instant::now();
            continue;
        }
        if input.pressed.a {
            if sel >= calls {
                if calls >= MAX_PLAYS {
                    continue;
                }
                book.calls[fi].push(DesignCall::blank(
                    COVERAGES[0].label().to_string(),
                    COVERAGES[0],
                ));
                keep_book(book);
                sel = book.calls[fi].len() - 1;
            }
            let front = book.defense[fi].clone();
            let mut call = book.calls[fi][sel].clone();
            call_editor(view, pad, &front, &mut call)?;
            book.calls[fi][sel] = call;
            keep_book(book);
            view.reset();
            last = Instant::now();
            continue;
        }
        slide.step(sel, dt);
        let mut frame = Frame::new(UI_W, UI_H);
        draw_call_list(&mut frame, book, fi, &slide);
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

/// The two clubs and the side of the ball, which every mode asks for: home
/// first, then away, then which of them the player runs.
fn choose_sides(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    all: &[Club],
    cards: &[Card],
) -> Option<Sides> {
    // Both clubs first, and neither of them is "yours" yet: the side of the ball
    // is the screen after this one. Either screen can be left to a roll instead.
    let (first, first_kit) = pick_team(view, pad, all, cards, None, "HOME TEAM")?.club(all, None);
    let (second, second_kit) =
        pick_team(view, pad, all, cards, Some(first), "AWAY TEAM")?.club(all, Some(first));
    // The pad starts in the middle and slides to a side: the club on that side is
    // the one the player runs, and the other hosts.
    let (yours, theirs, player_home, kits) =
        pick_side(view, pad, all, first, first_kit, second, second_kit)?;
    Some(Sides {
        yours,
        theirs,
        player_home,
        kits,
    })
}

/// Every book a side can take into a game: the league's, and the player's own
/// once he has drawn a legal one.
fn selectable_books() -> Vec<Book> {
    // The league's books, then every book on the player's shelf that is legal
    // enough to run: each one is its own entry in the picker.
    let mut out: Vec<Book> = Book::ALL.to_vec();
    for (i, book) in nopads_core::design::shelf().iter().enumerate() {
        if book.is_legal() {
            out.push(Book::Custom(i as u8));
        }
    }
    out
}

fn step_book(current: Book, d: i8) -> Book {
    let all = selectable_books();
    let i = all.iter().position(|b| *b == current).unwrap_or(0);
    all[step_index(i, all.len(), d)]
}

/// Which book the player runs. His own club's is the one it opens on, and the
/// screen says which that is: a side is not stuck with the plays it was born
/// with.
fn pick_book(view: &mut View, pad: &mut Option<&mut Pad>, club: usize) -> Option<Book> {
    let all = clubs();
    let own = all[club].book;
    let books = selectable_books();
    let mut pick = books.iter().position(|b| *b == own).unwrap_or(0);
    let mut nav = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return None;
        }
        if let d = nav.step(input.stick.y, dt) {
            if d != 0 {
                pick = wrap(pick, books.len(), -d);
            }
        }
        slide.step(pick, dt);
        let book = books[pick];
        let rows: Vec<(&str, String)> = books
            .iter()
            .map(|b| (b.label(), if *b == own { "YOUR CLUB".to_string() } else { String::new() }))
            .collect();
        let mut frame = Frame::new(UI_W, UI_H);
        draw_option_screen(
            &mut frame,
            "PLAYBOOK",
            &format!("{} RUNS {}", all[club].abbr, own.label()),
            &rows,
            &slide,
            book.blurb(),
            "[UP/DOWN] BOOK   [A] TAKE IT   [B] BACK",
        );
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        if input.pressed.a {
            return Some(book);
        }
        beat();
    }
}

/// One of the drills under MORE MODES, set up on the same clubs, side and book
/// the exhibition screen asks for. A drill has no ground to pick and no clock to
/// set: it is played on the practice field. The toss belongs to the gauntlet, where
/// it decides who brings the first return back; a session that has already chosen
/// its side of the ball has nothing for a coin to decide.
fn drill_kickoff(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    kind: DrillKind,
    difficulty: usize,
) -> Option<(Kickoff, usize)> {
    let all = clubs();
    let cards: Vec<Card> = all
        .iter()
        .map(|c| {
            let (ratings, men) = c.profile(3);
            Card {
                ratings,
                stars: men
                    .into_iter()
                    .map(|(pos, name, rating)| {
                        (format!("{} {}", pos.abbr(), name.to_uppercase()), rating)
                    })
                    .collect(),
            }
        })
        .collect();
    let sides = choose_sides(view, pad, &all, &cards)?;
    // Two of the three run no book of the player's: the gauntlet is a return
    // drill with no plays in it, and the tutorial teaches off the club's own
    // plays so the lesson is the same one whatever is on the shelf. Practice
    // asks which book to run.
    let book = match kind {
        DrillKind::Gauntlet | DrillKind::Tutorial(_) => all[sides.yours].book,
        DrillKind::Practice(_) => pick_book(view, pad, sides.yours)?,
    };
    // How hard the other lot play is a question for the field, not a number
    // buried in the settings screen: the drills that play against them ask it.
    // The tutorial is a teaching session, so it keeps the settings' answer.
    let chosen = match kind {
        DrillKind::Practice(_) | DrillKind::Gauntlet => difficulty_screen(view, pad, difficulty)?,
        DrillKind::Tutorial(_) => difficulty,
    };
    // Only the gauntlet needs a toss: it decides who brings the first one back.
    // A tutorial or a practice session has already been told which side of the
    // ball it is playing, so the coin has nothing left to decide.
    let receive = match kind {
        DrillKind::Gauntlet => coin_flip(view, pad, &all, sides.yours, sides.theirs, 0)?,
        _ => true,
    };
    Some((
        Kickoff {
            yours: sides.yours,
            theirs: sides.theirs,
            player_home: sides.player_home,
            kits: sides.kits,
            receive,
            venue: 0,
            book,
        },
        chosen,
    ))
}

/// Everything about a club the picker shows that is not its badge: the numbers
/// and the three men who carry them.
struct Card {
    ratings: TeamRatings,
    stars: Vec<(String, u8)>,
}

/// The picker's grid: a picture of each ground, four across and three deep.
const THUMB_W: usize = 140;
const THUMB_H: usize = 72;
const GRID_COLS: usize = 4;
const GRID_ROWS: usize = 3;
const CELL_W: i32 = 148;
const CELL_H: i32 = 86;
const PAGE: usize = GRID_COLS * GRID_ROWS;

/// The ground. It opens on the home club's own park, because that is where the
/// game would be played, and shows a still of every ground, so the player picks
/// the one he wants to see rather than a name he has to remember.
fn pick_venue(view: &mut View, pad: &mut Option<&mut Pad>, home: usize) -> Option<usize> {
    let mut sel = (2 + home).min(venues::COUNT - 1);
    let mut nav_x = Repeat::slow();
    let mut nav_y = Repeat::slow();
    let mut last = Instant::now();
    let teams = league_teams();
    let preview = venue_preview();
    let mut thumbs: Vec<Option<Frame>> = (0..venues::COUNT).map(|_| None).collect();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b {
            return None;
        }
        // One ground a press: left and right walk the row, up and down the
        // column, so the cursor goes where it is pushed and stays there.
        if let d = nav_x.step(input.stick.x, dt) {
            if d != 0 {
                sel = wrap(sel, venues::COUNT, d);
            }
        }
        if let d = nav_y.step(input.stick.y, dt) {
            if d != 0 {
                // Up is up and down is down. The cursor walks one row and stops at
                // the top and bottom of the grid rather than arriving at the other
                // end of it, which is what the reversed step used to do.
                let next = if d > 0 {
                    sel.checked_sub(GRID_COLS)
                } else {
                    let n = sel + GRID_COLS;
                    (n < venues::COUNT).then_some(n)
                };
                if let Some(n) = next {
                    sel = n;
                }
            }
        }
        let page = sel / PAGE;
        // The pictures are made one a frame, so the screen fills in while the
        // player is already reading it instead of stalling on the way in.
        if let Some((game, record)) = preview.as_ref() {
            let from = page * PAGE;
            let to = (from + PAGE).min(venues::COUNT);
            if let Some(i) = (from..to).find(|&i| thumbs[i].is_none()) {
                thumbs[i] = Some(venue_thumb(
                    game,
                    record,
                    i,
                    [teams[0].clone(), teams[1].clone()],
                ));
            }
        }
        let venue = Venue::from_index(sel);
        let sky = venue.sky();
        let mut frame = Frame::new(UI_W, UI_H);
        frame.gradient_rect(0, 0, UI_W as i32, UI_H as i32, sky[0], sky[2]);
        frame.fill_rect_alpha(0, 0, UI_W as i32, UI_H as i32, Rgb::new(4, 6, 10), 0.62);
        ui::heading(
            &mut frame,
            UI_W as i32 / 2,
            8,
            "WHERE IS IT PLAYED",
            ui::T_LABEL,
            ui::ACCENT,
        );
        let note = format!("{} OF {}", sel + 1, venues::COUNT);
        frame.text_shadowed(
            UI_W as i32 - 14 - text_width(&note, 1),
            12,
            &note,
            1,
            dim(),
            1,
        );
        for row in 0..GRID_ROWS {
            for col in 0..GRID_COLS {
                let i = page * PAGE + row * GRID_COLS + col;
                if i >= venues::COUNT {
                    break;
                }
                let x = 24 + col as i32 * CELL_W;
                let y = 34 + row as i32 * CELL_H;
                let on = i == sel;
                // A rounded frame with a lit ring, and a shadow under the
                // selected still so it lifts off the grid it sits in.
                frame.rounded_rect_alpha(
                    x - 6,
                    y - 4,
                    THUMB_W as i32 + 12,
                    THUMB_H as i32 + 12,
                    8,
                    Rgb::BLACK,
                    if on { 0.50 } else { 0.26 },
                );
                frame.rounded_rect(
                    x - 3,
                    y - 3,
                    THUMB_W as i32 + 6,
                    THUMB_H as i32 + 6,
                    6,
                    if on { accent() } else { ui::EDGE },
                );
                match &thumbs[i] {
                    Some(shot) => blit(&mut frame, shot, x, y),
                    None => {
                        frame.fill_rect(x, y, THUMB_W as i32, THUMB_H as i32, sky[1].scale(0.45))
                    }
                }
            }
        }
        ui::card(&mut frame, 150, 282, 340, 42, sky[1], 1.0);
        centered(&mut frame, 292, &venue.name(), 2, paper());
        centered(&mut frame, 311, &venue.sub(), 1, sky[1].lerp(paper(), 0.5));
        footer(&mut frame, "[ARROWS] GROUND   [A] PLAY   [B] BACK");
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        if input.pressed.a {
            return Some(sel);
        }
        beat();
    }
}

/// A still of one ground, so the picker can show it rather than describe it.
fn venue_thumb(game: &GameState, record: &PlayRecord, index: usize, teams: [Team; 2]) -> Frame {
    let mut broadcast = Broadcast::with_venue(THUMB_W, THUMB_H, teams, Venue::from_index(index));
    broadcast.frame_at_clean(game, record, 0.8, CameraMode::Endzone)
}

/// The play every ground is photographed at: the first snap of a practice
/// game, so each still has the same men in the same places.
fn venue_preview() -> Option<(GameState, PlayRecord)> {
    let teams = league_teams();
    let mut game = GameState::new(teams[0].clone(), teams[1].clone(), 0xA7F1_5EED);
    for _ in 0..80 {
        let record = game.simulate_next()?;
        if matches!(record, PlayRecord::Scrimmage(_)) {
            return Some((game, record));
        }
    }
    None
}

/// A finished frame, drawn straight onto the canvas.
fn blit(frame: &mut Frame, src: &Frame, x: i32, y: i32) {
    for v in 0..src.h {
        for u in 0..src.w {
            frame.put(x + u as i32, y + v as i32, src.color[v * src.w + u]);
        }
    }
}

/// What the carousel came back with: a club and the shirt it will wear, or a
/// roll of the division it was showing when the player took the random card.
enum Pick {
    Club(usize, bool),
    Random(usize, bool),
}

impl Pick {
    /// The club, and whether it wears its home shirt.
    fn club(self, all: &[Club], avoid: Option<usize>) -> (usize, bool) {
        match self {
            Pick::Club(i, home) => (i, home),
            Pick::Random(div, home) => (
                roll_club(&ids_in(all, Division::ALL[div]), avoid, coin_seed()),
                home,
            ),
        }
    }
}

/// The two shirts a club owns, in the order the menu names them.
fn swatch_tone(all: &[Club], club: usize, home: bool) -> Rgb {
    let (h, a) = nopads_core::team::club_kits(&all[club]);
    if home { h.jersey } else { a.jersey }
}

/// One club out of a list, never the one already in the game. The extra pass is
/// for a one-club list, where the avoided club would otherwise be the answer.
fn roll_club(list: &[usize], avoid: Option<usize>, seed: u64) -> usize {
    let n = list.len().max(1);
    let mut at = (seed as usize) % n;
    for _ in 0..n {
        if Some(list[at]) != avoid {
            return list[at];
        }
        at = (at + 1) % n;
    }
    list[at]
}

/// One card of the carousel: a club's badge, or the roll.
fn slot(
    frame: &mut Frame,
    all: &[Club],
    cx: i32,
    cy: i32,
    size: i32,
    club: Option<usize>,
    lit: bool,
) {
    match club {
        Some(i) => badge(frame, cx, cy, size, &all[i], lit),
        None => roll_card(frame, cx, cy, size, lit),
    }
}

/// The random card: a shield with a die punched through it, so an empty slot
/// reads as a choice rather than a club that failed to load.
fn roll_card(frame: &mut Frame, cx: i32, cy: i32, size: i32, lit: bool) {
    let tone = |c: Rgb| if lit { c } else { c.lerp(backdrop(), 0.62) };
    let w = size;
    let h = size * 6 / 5;
    let x = cx - w / 2;
    let y = cy - h / 2;
    frame.rounded_rect(x - 3, y - 3, w + 6, h + 6, w / 4, tone(accent()));
    frame.rounded_rect(x, y, w, h, w / 5, tone(panel()));
    let dot = (w / 9).max(2);
    for (dx, dy) in [(0, 0), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
        frame.fill_rect(
            cx + dx * dot - dot / 2,
            cy + dy * dot - dot / 2,
            dot,
            dot,
            tone(accent()),
        );
    }
    centered_in(
        frame,
        cx,
        y + h + 6,
        "RANDOM",
        if size > 80 { 2 } else { 1 },
        tone(paper()),
    );
}

/// The carousel: the division along the top, the club in the middle, its city
/// and nickname underneath, and a random card after the last club in the
/// division. Left and right roll through the division.
fn pick_team(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    all: &[Club],
    cards: &[Card],
    avoid: Option<usize>,
    heading: &str,
) -> Option<Pick> {
    let mut div = 0usize;
    let mut sel = 0usize;
    let mut slide = 0.0f32;
    // The shirt the highlighted club would wear: home until the player says
    // otherwise, and kept per screen so a club and its kit are one choice.
    let mut kit_home = true;
    let mut nav = Repeat::default();
    let mut last = Instant::now();
    let mut said = 0.0f32;
    let mut t = 0.0f32;
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b || input.pressed.start {
            return None;
        }
        // The divisions are the bumpers, not the stick: up and down were doing
        // nothing else useful on this screen and are easy to hit by accident
        // while rolling clubs.
        if input.pressed.dummy || input.pressed.shoulder {
            let d = if input.pressed.shoulder { 1 } else { -1 };
            div = wrap(div, Division::ALL.len(), d);
            sel = 0;
            slide = 0.0;
        }
        if input.pressed.x {
            kit_home = !kit_home;
        }
        // The clubs of this division and one card past them that rolls for the
        // player, so he never has to know a club to pick one.
        let mut slots: Vec<Option<usize>> = ids_in(all, Division::ALL[div])
            .into_iter()
            .map(Some)
            .collect();
        slots.push(None);
        let n = slots.len();
        if let d = nav.step(input.stick.x, dt) {
            if d != 0 {
                sel = wrap(sel, n, d);
                slide = 90.0 * d as f32;
            }
        }
        t += dt;
        slide *= 1.0 - (dt * 9.0).min(1.0);
        said = (said - dt).max(0.0);
        if input.pressed.a {
            match slots[sel] {
                Some(pick) if Some(pick) == avoid => said = 1.6,
                Some(pick) => return Some(Pick::Club(pick, kit_home)),
                None => return Some(Pick::Random(div, kit_home)),
            }
        }

        let club = slots[sel];
        let prev = slots[wrap(sel, n, -1)];
        let next = slots[wrap(sel, n, 1)];
        let mut frame = Frame::new(UI_W, UI_H);
        night(&mut frame);
        ui::heading(
            &mut frame,
            UI_W as i32 / 2,
            14,
            heading,
            ui::T_LABEL,
            ui::ACCENT,
        );
        // The divisions are one segmented control, not four labels in a row.
        let widths: Vec<i32> = Division::ALL
            .iter()
            .map(|d| ui::chip_width(d.label()))
            .collect();
        let total: i32 = widths.iter().sum::<i32>() + ui::S2 * (widths.len() as i32 - 1);
        let mut tab_x = UI_W as i32 / 2 - total / 2;
        for (i, d) in Division::ALL.iter().enumerate() {
            ui::chip(
                &mut frame,
                tab_x,
                42,
                d.label(),
                if i == div { 1.0 } else { 0.0 },
                ui::ACCENT,
            );
            tab_x += widths[i] + ui::S2;
        }
        let accent_tone = match club {
            Some(i) => all[i].accent,
            None => ui::ACCENT,
        };
        ui::card(
            &mut frame,
            190,
            74,
            260,
            226,
            accent_tone,
            ui::enter(t, 0, 8),
        );
        slot(&mut frame, all, 92, 150, 54, prev, false);
        slot(&mut frame, all, 548, 150, 54, next, false);
        slot(&mut frame, all, 320 + slide as i32, 130, 88, club, true);
        let (city, name, tone) = match club {
            Some(i) => (
                all[i].city.to_uppercase(),
                all[i].name.to_uppercase(),
                all[i].accent,
            ),
            None => ("RANDOM".to_string(), "ROLL THE DICE".to_string(), accent()),
        };
        centered_in(&mut frame, 320, 184, &city, 3, paper());
        centered_in(&mut frame, 320, 206, &name, 2, tone);
        frame.fill_rect(210, 226, 220, 1, ui::EDGE);
        if let Some(i) = club {
            let card = &cards[i];
            let r = card.ratings;
            let bars = [("OVR", r.overall), ("OFF", r.offense), ("DEF", r.defense)];
            for (k, (label, value)) in bars.iter().enumerate() {
                let y = 234 + k as i32 * 15;
                ui::caption(&mut frame, 212, y + 1, label, dim());
                ui::meter(
                    &mut frame,
                    248,
                    y,
                    136,
                    6,
                    *value as f32 / 99.0,
                    accent_tone,
                );
                let v = format!("{value}");
                frame.text_shadowed(390, y - 1, &v, 1, paper(), 1);
            }
            if let Some((who, rating)) = card.stars.first() {
                let line = format!("{who}  {rating}");
                centered(&mut frame, 282, &line, 1, dim());
            }
        }
        // The counter keeps to the corner so the card has the middle of the
        // screen to itself.
        let note = if said > 0.0 {
            "ALREADY IN THIS GAME".to_string()
        } else {
            format!("{} OF {}", sel + 1, n)
        };
        let note_color = if said > 0.0 {
            Rgb::new(255, 120, 96)
        } else {
            dim()
        };
        frame.text_shadowed(
            UI_W as i32 - 14 - text_width(&note, 1),
            18,
            &note,
            1,
            note_color,
            1,
        );
        if let Some(i) = club {
            let kit = swatch_tone(all, i, kit_home);
            frame.rounded_rect(14, 296, 30, 30, 6, ui::EDGE);
            frame.rounded_rect(16, 298, 26, 26, 5, kit);
            frame.fill_rect(16, 304, 26, 4, if kit_home { all[i].accent } else { all[i].primary });
            ui::caption(
                &mut frame,
                52,
                300,
                if kit_home { "HOME KIT" } else { "AWAY KIT" },
                paper(),
            );
            ui::caption(&mut frame, 52, 312, "[X] CHANGE", dim());
        }
        footer(
            &mut frame,
            "[LEFT/RIGHT] ROLL   [LB/RB] DIVISION   [A] PICK   [X] KIT   [B] BACK",
        );
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

/// Which side of the ball the player is on, drawn as the pad sliding between two
/// plates the way NFL Street asks it: the choice of club was the last screen, and
/// this is the choice of whose side of the ball he plays it from.
///
/// The club on the plate he settles on is his. The other club hosts nothing and
/// wears nothing different: it simply takes the other side.
fn pick_side(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    all: &[Club],
    first: usize,
    first_kit: bool,
    second: usize,
    second_kit: bool,
) -> Option<(usize, usize, bool, [bool; 2])> {
    let mut side = 0i32;
    let mut at = 0.0f32;
    let mut t = 0.0f32;
    let mut nav = Repeat::default();
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b || input.pressed.start {
            return None;
        }
        if let d = nav.step(input.stick.x, dt) {
            if d != 0 {
                side = d.signum() as i32;
            }
        }
        // The icon travels to the plate rather than jumping to it, so the slide
        // is the thing being read on screen.
        at += (side as f32 - at) * (dt * 11.0).min(1.0);
        t += dt;
        let mut frame = Frame::new(UI_W, UI_H);
        draw_side_pick(&mut frame, all, first, first_kit, second, second_kit, at, t);
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        if input.pressed.a && side != 0 {
            return if side < 0 {
                Some((first, second, true, [first_kit, second_kit]))
            } else {
                Some((second, first, false, [first_kit, second_kit]))
            };
        }
        beat();
    }
}

/// The side screen: two plates, the pad between them, and the shirt each club
/// would wear on its plate.
fn draw_side_pick(
    frame: &mut Frame,
    all: &[Club],
    first: usize,
    first_kit: bool,
    second: usize,
    second_kit: bool,
    at: f32,
    t: f32,
) {
    night(frame);
    ui::heading(
        frame,
        UI_W as i32 / 2,
        14,
        "CHOOSE YOUR SIDE",
        ui::T_LABEL,
        ui::ACCENT,
    );
    let plates = [
        (first, first_kit, "HOME", 168, -1.0f32),
        (second, second_kit, "AWAY", 472, 1.0f32),
    ];
    for (club, kit_home, label, cx, sign) in plates {
        let lit = at * sign > 0.35;
        let tone = if lit { all[club].accent } else { ui::EDGE };
        ui::card(frame, cx - 128, 74, 256, 200, tone, ui::enter(t, 0, 8));
        centered_in(frame, cx, 86, label, ui::T_CAPTION as usize, if lit { all[club].accent } else { dim() });
        badge(frame, cx, 140, 54, &all[club], lit);
        centered_in(frame, cx, 190, &all[club].city.to_uppercase(), 2, paper());
        centered_in(frame, cx, 210, &all[club].name.to_uppercase(), 1, all[club].accent);
        // The shirt, where the plate can be read without leaving it.
        let kit = swatch_tone(all, club, kit_home);
        frame.rounded_rect(cx - 52, 232, 26, 26, 5, ui::EDGE);
        frame.rounded_rect(cx - 50, 234, 22, 22, 4, kit);
        frame.fill_rect(cx - 50, 239, 22, 3, if kit_home { all[club].accent } else { all[club].primary });
        ui::caption(
            frame,
            cx - 20,
            238,
            if kit_home { "HOME KIT" } else { "AWAY KIT" },
            paper(),
        );
    }
    // The pad itself, sliding between the plates and resting in the middle when
    // no side has been taken.
    let x = 320 + (at * 118.0) as i32;
    pad_icon(frame, x, 300);
    if at.abs() < 0.3 {
        centered_in(frame, 320, 316, "SLIDE TO A SIDE", 1, dim());
    }
    footer(
        frame,
        "[LEFT/RIGHT] SLIDE   [A] KICK OFF   [B] BACK",
    );
}

/// A controller, at menu size: a body with two grips, a d-pad and two buttons.
fn pad_icon(frame: &mut Frame, cx: i32, cy: i32) {
    frame.rounded_rect(cx - 18, cy - 9, 36, 18, 7, ui::EDGE);
    frame.rounded_rect(cx - 16, cy - 7, 32, 14, 5, ui::PAPER);
    frame.fill_rect(cx - 16, cy - 1, 4, 8, ui::EDGE);
    frame.fill_rect(cx + 12, cy - 1, 4, 8, ui::EDGE);
    frame.fill_rect(cx - 8, cy - 1, 6, 2, ui::INK);
    frame.fill_rect(cx - 6, cy - 3, 2, 6, ui::INK);
    frame.fill_rect(cx + 3, cy - 3, 3, 3, ui::INK);
    frame.fill_rect(cx + 7, cy + 1, 3, 3, ui::ACCENT);
}

// ---------------------------------------------------------------------------
// Season
// ---------------------------------------------------------------------------

/// Pick a club and a length, and the season is scheduled from both.
///
/// The length is a choice rather than a setting because it is the shape of the
/// thing: four weeks is an evening, a full thirty-one is a league campaign.
fn season_setup(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    _settings: &mut Settings,
) -> Option<Season> {
    let all = clubs();
    let cards: Vec<Card> = all
        .iter()
        .map(|c| {
            let (ratings, men) = c.profile(3);
            Card {
                ratings,
                stars: men
                    .into_iter()
                    .map(|(pos, name, rating)| {
                        (format!("{} {}", pos.abbr(), name.to_uppercase()), rating)
                    })
                    .collect(),
            }
        })
        .collect();
    let (you, _) = pick_team(view, pad, &all, &cards, None, "YOUR CLUB THIS SEASON")?.club(&all, None);
    // The book he runs all year, picked once before the first week rather than
    // before every one: a book is the season's plan, and the picker offers his
    // own shelf beside the league's.
    let book = pick_book(view, pad, you)?;
    let weeks = season_length(view, pad, you, &all)?;
    let mut season = Season::new(you, weeks, coin_seed());
    season.book = book;
    Some(season)
}

/// The lengths on offer. The last is the full round-robin: every club twice a
/// week for thirty-one weeks, which is the longest season the league can give.
const SEASON_LENGTHS: [usize; 9] = [4, 6, 8, 10, 12, 16, 20, 24, 31];

/// The length of a season, drawn from the pick: the number the player is on set
/// as the screen's own figure, and the lengths under it as one control rather
/// than nine loose labels.
fn draw_season_length(
    frame: &mut Frame,
    all: &[Club],
    you: usize,
    pick: usize,
    slide: &ui::Slide,
    t: f32,
) {
    night(frame);
    let w = frame.w as i32;
    let cx = w / 2;
    let weeks = SEASON_LENGTHS[pick];
    ui::heading(frame, cx, 16, "SEASON", ui::T_TITLE, ui::ACCENT);
    let sub = "HOW MANY WEEKS";
    ui::caption(
        frame,
        cx - ui::tracked_width(sub, ui::T_CAPTION, 1) / 2,
        52,
        sub,
        dim(),
    );

    let card_w = 360;
    let card_x = cx - card_w / 2;
    let card_y = 70;
    let card_h = 176;
    ui::card(
        frame,
        card_x,
        card_y,
        card_w,
        card_h,
        all[you].accent,
        ui::enter(t, 0, 4),
    );
    badge(frame, cx, card_y + 30, 40, &all[you], true);
    centered(
        frame,
        card_y + 58,
        &all[you].full_name().to_uppercase(),
        ui::T_LABEL,
        paper(),
    );
    frame.fill_rect_alpha(card_x + 24, card_y + 78, card_w - 48, 1, ui::EDGE, 0.9);
    let figure = weeks.to_string();
    frame.text(
        cx - text_width(&figure, ui::T_TITLE) / 2,
        card_y + 84,
        &figure,
        ui::T_TITLE,
        ui::ACCENT,
    );
    let unit = "WEEKS";
    ui::caption(
        frame,
        cx - ui::tracked_width(unit, ui::T_CAPTION, 2) / 2,
        card_y + 126,
        unit,
        dim(),
    );

    // The lengths, as one segmented control with the highlight travelling.
    let sizes: Vec<i32> = SEASON_LENGTHS
        .iter()
        .map(|w| ui::chip_width(&w.to_string()))
        .collect();
    let total: i32 = sizes.iter().sum::<i32>() + ui::S1 * (sizes.len() as i32 - 1);
    let mut x = cx - total / 2;
    for (i, size) in sizes.iter().enumerate() {
        ui::chip(
            frame,
            x,
            card_y + 142,
            &SEASON_LENGTHS[i].to_string(),
            slide.heat(i),
            ui::ACCENT,
        );
        x += size + ui::S1;
    }

    centered(
        frame,
        card_y + card_h + 12,
        &format!("{weeks} GAMES IN THE REGULAR SEASON"),
        ui::T_CAPTION,
        paper(),
    );
    centered(
        frame,
        card_y + card_h + 26,
        "THE TOP TWO IN EACH DIVISION MAKE THE PLAYOFFS",
        ui::T_CAPTION,
        dim(),
    );
    centered(
        frame,
        card_y + card_h + 40,
        if weeks == season::MAX_WEEKS {
            "A FULL ROUND ROBIN: EVERY CLUB HOME AND AWAY"
        } else {
            "EVERY CLUB PLAYS EVERY WEEK"
        },
        ui::T_CAPTION,
        dim(),
    );
    footer(
        frame,
        "[LEFT/RIGHT] LENGTH   [A] START THE SEASON   [B] BACK",
    );
}

fn season_length(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    you: usize,
    all: &[Club],
) -> Option<usize> {
    let mut pick = 4usize;
    let mut nav = Repeat::default();
    let mut slide = ui::Slide::new(4);
    let mut t = 0.0f32;
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        t += dt;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b || input.pressed.start {
            return None;
        }
        if let d = nav.step(input.stick.x, dt) {
            if d != 0 {
                pick = wrap(pick, SEASON_LENGTHS.len(), d);
            }
        }
        slide.step(pick, dt);
        let weeks = SEASON_LENGTHS[pick];
        let mut frame = Frame::new(UI_W, UI_H);
        draw_season_length(&mut frame, all, you, pick, &slide, t);
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        if input.pressed.a {
            return Some(weeks);
        }
        beat();
    }
}

/// What the season hub wants from the game itself.
pub enum SeasonStep {
    /// The player chose to play his own game, on this ground.
    Play(Kickoff),
    /// He is done with the season.
    Done,
}

/// The season hub: the week, the player's next game, the division table, and the
/// bracket once there is one. Every game that is not the player's is simulated
/// here, a week or a round at a time, so he can play his own or hand it to the
/// machine without leaving the season.
pub fn season_hub(
    view: &mut View,
    pad: Option<&mut Pad>,
    season: &mut Season,
) -> Option<SeasonStep> {
    let mut pad = pad;
    if season.phase == Phase::Regular && season.regular_done() {
        season.start_playoffs();
    }
    let all = clubs();
    let mut pick = 0usize;
    let mut page = 0usize;
    let mut nav = Repeat::default();
    let mut nav_x = Repeat::default();
    let mut last = Instant::now();
    let mut note = String::new();
    let mut note_t = 0.0f32;
    let mut slide = ui::Slide::new(0);
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        note_t = (note_t - dt).max(0.0);
        slide.step(pick, dt);
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.start {
            return Some(SeasonStep::Done);
        }
        let fixture = season.your_fixture();
        let mut rows: Vec<&str> = Vec::new();
        if season.phase == Phase::Done {
            rows.push("BACK TO THE MENU");
        } else if fixture.is_some() {
            rows.push("PLAY MY GAME");
            rows.push("SIM MY GAME");
        } else {
            rows.push("SIM THIS WEEK");
        }
        rows.push("STANDINGS");
        rows.push("BRACKET");
        if season.phase != Phase::Done {
            rows.push("BACK TO THE MENU");
        }
        if pick >= rows.len() {
            pick = rows.len() - 1;
        }
        if season.phase == Phase::Done {
            // One row, one key: the season is over.
            if input.pressed.a || input.pressed.b {
                return Some(SeasonStep::Done);
            }
        } else {
            if let d = nav.step(input.stick.y, dt) {
                if d != 0 {
                    pick = wrap(pick, rows.len(), -d);
                }
            }
            if let d = nav_x.step(input.stick.x, dt) {
                if d != 0 {
                    page = wrap(page, 2, d);
                }
            }
            if input.pressed.b {
                return Some(SeasonStep::Done);
            }
            if input.pressed.a {
                match rows[pick] {
                    "PLAY MY GAME" => {
                        if let Some((_, f)) = fixture {
                            let theirs = if f.home == season.you { f.away } else { f.home };
                            let championship = season.phase != Phase::Regular
                                && season.round_name() == "CHAMPIONSHIP";
                            // The final is played where the league plays it; any
                            // other game is played at the home club's park.
                            let venue = if championship {
                                venues::CHAMPIONSHIP
                            } else {
                                2 + f.home
                            };
                            return Some(SeasonStep::Play(Kickoff {
                                yours: season.you,
                                theirs,
                                player_home: f.home == season.you,
                                kits: [true, false],
                                receive: coin_seed() % 2 == 0,
                                venue,
                                book: season.book,
                            }));
                        }
                    }
                    "SIM MY GAME" | "SIM THIS WEEK" => {
                        let played =
                            season.phase != Phase::Regular && season.round_name() == "CHAMPIONSHIP";
                        season.sim_rest();
                        note = if played && season.champion.is_some() {
                            "THE SEASON IS OVER".to_string()
                        } else {
                            "SIMULATED".to_string()
                        };
                        note_t = 1.4;
                        pick = 0;
                    }
                    "STANDINGS" => page = 0,
                    "BRACKET" => page = 1,
                    _ => return Some(SeasonStep::Done),
                }
            }
        }

        let mut frame = Frame::new(UI_W, UI_H);
        night(&mut frame);
        // Header: where in the season this is.
        let head = match season.phase {
            Phase::Regular => format!(
                "WEEK {} OF {}",
                (season.week + 1).min(season.length()),
                season.length()
            ),
            Phase::Playoffs(_) => season.round_name().to_string(),
            Phase::Done => "COMPLETE".to_string(),
        };
        frame.text_shadowed(20, 12, "SEASON", 2, accent(), 1);
        frame.text_shadowed(132, 16, &head, 1, paper(), 1);
        let rec = season.record_of(season.you);
        let line = format!("{}  {}", all[season.you].abbr, rec.line());
        frame.text_shadowed(
            UI_W as i32 - 18 - text_width(&line, 1),
            16,
            &line,
            1,
            paper(),
            1,
        );
        frame.fill_rect(20, 34, 600, 1, Rgb::new(48, 92, 148));

        // Who is next.
        badge(&mut frame, 46, 66, 34, &all[season.you], true);
        frame.text_shadowed(70, 52, all[season.you].city, 2, paper(), 1);
        let short = all[season.you].name.to_uppercase();
        frame.text_shadowed(70, 70, &short, 2, all[season.you].accent, 1);
        match fixture {
            Some((_, f)) => {
                let home = f.home == season.you;
                let other = if home { f.away } else { f.home };
                let where_ = if home { "HOME" } else { "AWAY" };
                let next = format!(
                    "{} {}",
                    if home { "VS" } else { "AT" },
                    all[other].full_name().to_uppercase()
                );
                frame.text_shadowed(250, 52, &next, 2, paper(), 1);
                frame.text_shadowed(250, 70, where_, 1, dim(), 1);
            }
            None => {
                frame.text_shadowed(250, 52, "NO GAME THIS WEEK", 2, dim(), 1);
                frame.text_shadowed(250, 70, "THE LEAGUE PLAYS ON", 1, dim(), 1);
            }
        }

        // Left: the player's division table. Right: the bracket, or the picture.
        ui::card(&mut frame, 12, 90, 244, 198, ui::ACCENT, 1.0);
        frame.fill_rect(20, 96, 1, 200, Rgb::new(38, 58, 88));
        frame.text_shadowed(20, 96, all[season.you].division.label(), 1, accent(), 1);
        let table = season.table(all[season.you].division);
        for (n, (club, r)) in table.iter().enumerate() {
            let y = 112 + n as i32 * 18;
            let on = *club == season.you;
            if on {
                frame.fill_rect(16, y - 3, 232, 16, panel());
            }
            frame.text_shadowed(20, y, &format!("{}", n + 1), 1, dim(), 1);
            frame.text_shadowed(
                36,
                y,
                all[*club].abbr,
                1,
                if on { accent() } else { paper() },
                1,
            );
            frame.text_shadowed(80, y, &all[*club].city.to_uppercase(), 1, dim(), 1);
            let right = format!("{:>5}  {:>4}", r.line(), r.diff());
            frame.text_shadowed(
                248 - text_width(&right, 1),
                y,
                &right,
                1,
                if on { paper() } else { dim() },
                1,
            );
        }

        ui::card(&mut frame, 284, 90, 344, 198, ui::ACCENT, 1.0);
        // Right half: the bracket once it exists, the division leaders before it.
        if page == 1 && !season.rounds.is_empty() {
            frame.text_shadowed(290, 96, "THE BRACKET", 1, accent(), 1);
            for (r, round) in season.rounds.iter().enumerate() {
                let x = 290 + r as i32 * 112;
                frame.text_shadowed(x, 112, round.name, 1, dim(), 1);
                for (n, f) in round.games.iter().enumerate() {
                    let y = 128 + n as i32 * 30;
                    let mine = f.home == season.you || f.away == season.you;
                    let text = format!("{} {}", all[f.home].abbr, f.line());
                    let text2 = all[f.away].abbr;
                    frame.text_shadowed(x, y, &text, 1, if mine { accent() } else { paper() }, 1);
                    frame.text_shadowed(
                        x,
                        y + 10,
                        text2,
                        1,
                        if mine { accent() } else { dim() },
                        1,
                    );
                }
            }
        } else {
            frame.text_shadowed(290, 96, "PLAYOFF PICTURE", 1, accent(), 1);
            let seeds = if season.rounds.is_empty() {
                season.seeds()
            } else {
                Vec::new()
            };
            for (n, div) in Division::ALL.iter().enumerate() {
                let rows = season.table(*div);
                let y = 114 + n as i32 * 44;
                frame.text_shadowed(290, y, div.label(), 1, dim(), 1);
                for (k, (club, r)) in rows.iter().take(2).enumerate() {
                    let seed = seeds
                        .iter()
                        .position(|c| c == club)
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    let label = if seed > 0 {
                        format!("#{seed} {}", all[*club].abbr)
                    } else {
                        all[*club].abbr.to_string()
                    };
                    frame.text_shadowed(
                        290 + k as i32 * 96,
                        y + 12,
                        &label,
                        1,
                        if *club == season.you {
                            accent()
                        } else {
                            paper()
                        },
                        1,
                    );
                    frame.text_shadowed(290 + k as i32 * 96, y + 24, &r.line(), 1, dim(), 1);
                }
            }
        }

        // Actions.
        if season.phase == Phase::Done {
            let champ = season.champion;
            centered(&mut frame, 150, "CHAMPIONSHIP", 3, accent());
            if let Some(c) = champ {
                let yours = c == season.you;
                centered(
                    &mut frame,
                    200,
                    &all[c].full_name().to_uppercase(),
                    3,
                    all[c].accent,
                );
                centered(
                    &mut frame,
                    236,
                    if yours {
                        "YOU WON IT ALL"
                    } else {
                        "SEASON OVER"
                    },
                    2,
                    if yours { accent() } else { dim() },
                );
            }
        }
        // The actions sit in one row along the bottom, above the footer.
        let mut x = 24;
        for (i, label) in rows.iter().enumerate() {
            let heat = if season.phase == Phase::Done {
                0.0
            } else {
                slide.heat(i)
            };
            let w = ui::chip(&mut frame, x, 292, label, heat, ui::ACCENT);
            x += w + ui::S2;
        }
        if note_t > 0.0 {
            centered(&mut frame, 276, &note, 1, accent());
        }
        footer(
            &mut frame,
            "[UP/DOWN] ACTION   [LEFT/RIGHT] PAGE   [A] SELECT   [B] MENU",
        );
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

// ---------------------------------------------------------------------------
// Coin toss
// ---------------------------------------------------------------------------

/// A toss has to be made in the moment, not decided before the player sits down.
fn coin_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() ^ d.subsec_nanos() as u64)
        .unwrap_or(0xA7F1)
}

fn coin(frame: &mut Frame, cx: i32, cy: i32, width: i32, heads: bool, landed: bool) {
    let h = 92;
    let w = width.max(10);
    let rim = ui::ACCENT.lerp(ui::INK, 0.58);
    let face = ui::ACCENT.lerp(ui::PAPER, 0.10);
    let inner = ui::ACCENT.lerp(ui::INK, 0.26);
    frame.rounded_rect(
        cx - w / 2 - 5,
        cy - h / 2 - 5,
        w + 10,
        h + 10,
        w / 2 + 5,
        rim,
    );
    frame.rounded_rect(cx - w / 2, cy - h / 2, w, h, w / 2, face);
    if w > 26 {
        frame.rounded_rect(
            cx - w / 2 + 7,
            cy - h / 2 + 7,
            w - 14,
            h - 14,
            (w - 14) / 2,
            inner,
        );
    }
    if w > 34 {
        let mark = if heads { "H" } else { "T" };
        let scale = ((h as usize) / 16).max(1);
        let tw = text_width(mark, scale);
        // No drop shadow on the mark: at this size the offset reads as a
        // second letter rather than as depth.
        frame.text(
            cx - tw / 2,
            cy - (7 * scale as i32) / 2,
            mark,
            scale,
            if landed {
                paper()
            } else {
                Rgb::new(255, 240, 200)
            },
        );
    }
}

/// The four things the toss can be asking, in the order it asks them.
#[derive(Clone, Copy, PartialEq)]
enum CoinStep {
    Call,
    Flip,
    Won,
    Lost,
    Choose,
}

/// The toss, drawn from its state rather than from inside the loop that reads
/// the stick, so it can be looked at in the states that take a while to reach.
fn draw_coin_toss(
    frame: &mut Frame,
    all: &[Club],
    yours: usize,
    theirs: usize,
    venue: usize,
    step: CoinStep,
    pick: usize,
    t: f32,
    heads: bool,
    slide: &ui::Slide,
) {
    night(frame);
    let w = frame.w as i32;
    let cx = w / 2;
    ui::heading(frame, cx, 16, "COIN TOSS", ui::T_BODY, ui::ACCENT);
    let line = format!(
        "{} AT {}",
        all[yours].full_name().to_uppercase(),
        all[theirs].full_name().to_uppercase()
    );
    ui::caption(
        frame,
        cx - ui::tracked_width(&line, ui::T_CAPTION, 1) / 2,
        48,
        &line,
        dim(),
    );
    let place = format!("AT {}", Venue::from_index(venue).name().to_uppercase());
    ui::caption(
        frame,
        cx - ui::tracked_width(&place, ui::T_CAPTION, 1) / 2,
        60,
        &place,
        dim(),
    );

    let (show_heads, width, landed) = match step {
        CoinStep::Call => (pick == 0, 118, false),
        CoinStep::Flip => {
            let spin = (t * 13.0).cos().abs();
            (
                heads || ((t * 13.0) as i32 % 2 == 0),
                (34.0 + 84.0 * spin) as i32,
                false,
            )
        }
        CoinStep::Won | CoinStep::Choose | CoinStep::Lost => (heads, 118, true),
    };
    coin(frame, cx, 142, width, show_heads, landed);

    // Two choices, laid out as chips and measured before either is drawn so the
    // pair is centred whichever words they are.
    let pair = |frame: &mut Frame, y: i32, labels: [&str; 2]| {
        let sizes = [ui::chip_width(labels[0]), ui::chip_width(labels[1])];
        let total = sizes[0] + sizes[1] + ui::S5;
        let mut x = cx - total / 2;
        for (i, label) in labels.iter().enumerate() {
            ui::chip(frame, x, y, label, slide.heat(i), ui::ACCENT);
            x += sizes[i] + ui::S5;
        }
    };

    match step {
        CoinStep::Call => {
            centered(frame, 200, "CALL IT", ui::T_CAPTION, dim());
            pair(frame, 218, ["HEADS", "TAILS"]);
            centered(
                frame,
                252,
                "THE WINNER CHOOSES TO THROW IT OR RECEIVE IT",
                ui::T_CAPTION,
                dim(),
            );
            footer(frame, "[LEFT/RIGHT] CALL   [A] FLIP   [B] BACK");
        }
        CoinStep::Flip => {
            centered(frame, 208, "IN THE AIR", ui::T_LABEL, paper());
            footer(frame, "WAITING ON THE COIN");
        }
        CoinStep::Won => {
            centered(
                frame,
                204,
                if heads { "HEADS" } else { "TAILS" },
                ui::T_LABEL,
                accent(),
            );
            centered(
                frame,
                228,
                "YOU CALLED IT. YOU WON THE TOSS.",
                ui::T_CAPTION,
                paper(),
            );
            footer(frame, "[A] CHOOSE");
        }
        CoinStep::Lost => {
            centered(
                frame,
                204,
                if heads { "HEADS" } else { "TAILS" },
                ui::T_LABEL,
                dim(),
            );
            centered(frame, 228, "YOU LOST THE TOSS", ui::T_CAPTION, paper());
            centered(
                frame,
                242,
                &format!("{} WILL RECEIVE", all[theirs].full_name().to_uppercase()),
                ui::T_CAPTION,
                all[theirs].accent,
            );
            footer(frame, "THE COMPUTER TAKES THE BALL");
        }
        CoinStep::Choose => {
            centered(frame, 200, "THROW OR RECEIVE", ui::T_CAPTION, dim());
            pair(frame, 218, ["THROW IT OFF", "RECEIVE IT"]);
            footer(frame, "[LEFT/RIGHT] CHOOSE   [A] KICK OFF");
        }
    }
}

/// Call it, watch it come down, and let the winner say throw or receive.
/// Returns true when the player receives the opening throw-off.
fn coin_flip(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    all: &[Club],
    yours: usize,
    theirs: usize,
    venue: usize,
) -> Option<bool> {
    let mut step = CoinStep::Call;
    let mut pick = 0usize;
    let mut t = 0.0f32;
    let mut heads = true;
    let mut nav = Repeat::default();
    let mut slide = ui::Slide::new(0);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        match step {
            CoinStep::Call => {
                if let d = nav.step(input.stick.x, dt) {
                    if d != 0 {
                        pick = if d > 0 { 1 } else { 0 };
                    }
                }
                if input.pressed.b {
                    return None;
                }
                if input.pressed.a {
                    t = 0.0;
                    step = CoinStep::Flip;
                }
            }
            CoinStep::Flip => {
                t += dt;
                if t >= 1.05 {
                    t = 0.0;
                    heads = coin_seed() % 2 == 0;
                    let called_heads = pick == 0;
                    step = if heads == called_heads {
                        CoinStep::Won
                    } else {
                        CoinStep::Lost
                    };
                }
            }
            CoinStep::Won => {
                t += dt;
                if input.pressed.a && t > 0.3 {
                    pick = 0;
                    step = CoinStep::Choose;
                }
            }
            CoinStep::Lost => {
                t += dt;
                if t > 1.5 || input.pressed.a {
                    // The computer won the toss: it takes the ball.
                    return Some(false);
                }
            }
            CoinStep::Choose => {
                if let d = nav.step(input.stick.x, dt) {
                    if d != 0 {
                        pick = if d > 0 { 1 } else { 0 };
                    }
                }
                if input.pressed.a {
                    return Some(pick == 1);
                }
            }
        }
        slide.step(pick, dt);

        let mut frame = Frame::new(UI_W, UI_H);
        draw_coin_toss(
            &mut frame, all, yours, theirs, venue, step, pick, t, heads, &slide,
        );
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

#[cfg(test)]
mod tests {
    /// The list of modes, one picture per entry, so the notes can be read rather
    /// than argued about.
    #[test]
    fn the_modes_screen_can_be_rendered_to_pictures() {
        let slide = ui::Slide::new(0);
        for pick in 0..MODES.len() {
            let mut frame = Frame::new(UI_W, UI_H);
            draw_modes(&mut frame, pick, &slide);
            if std::env::var("NOPADS_DUMP_UI").is_ok() {
                // Tests run from the crate directory: the pictures belong with
                // the rest of them, at the root of the repository.
                for dir in ["../../out/ui"] {
                    std::fs::create_dir_all(dir).ok();
                    let path = format!("{dir}/modes_{}.png", pick + 1);
                    let _ = nopads_render::capture::write_png(&path, &frame);
                }
            }
        }
    }

    use super::*;
    use crate::view::UI_SCALE;

    #[test]
    fn every_mark_fits_inside_its_badge() {
        for club in clubs() {
            let w = 104i32;
            let scale = ((w as usize) / (club.abbr.chars().count() * 6 + 3)).max(1);
            assert!(
                text_width(club.abbr, scale) <= w,
                "{} overflows the badge: {} against {w}",
                club.abbr,
                text_width(club.abbr, scale)
            );
            assert!(
                (7 * scale as i32) <= w * 6 / 5,
                "{} is taller than the shield",
                club.abbr
            );
        }
    }

    /// A roll never hands the player the club already on the other side of the
    /// game, whatever card he took it from.
    #[test]
    fn a_random_pick_never_rolls_the_other_side() {
        let all = clubs();
        for div in 0..Division::ALL.len() {
            let list = ids_in(&all, Division::ALL[div]);
            for &avoid in &list {
                for seed in 0..40u64 {
                    let got = roll_club(&list, Some(avoid), seed);
                    assert_ne!(got, avoid, "rolled the club already in the game");
                    assert!(list.contains(&got), "rolled a club outside the division");
                }
            }
        }
        // And with nobody to avoid, every club in the list can come up.
        let list = ids_in(&all, Division::ALL[0]);
        let mut seen = vec![false; list.len()];
        for seed in 0..40u64 {
            let got = roll_club(&list, None, seed);
            seen[list
                .iter()
                .position(|&i| i == got)
                .expect("not in the list")] = true;
        }
        assert!(
            seen.iter().all(|s| *s),
            "the roll never reached every club: {seen:?}"
        );
    }

    #[test]
    fn a_menu_wraps_round_both_ways() {
        assert_eq!(wrap(0, 4, -1), 3);
        assert_eq!(wrap(3, 4, 1), 0);
        assert_eq!(wrap(0, 4, 1), 1);
    }

    #[test]
    fn the_start_menu_lands_in_the_middle_of_the_picture() {
        let mut screen = Frame::new(UI_W, UI_H);
        draw_start(&mut screen, 0, &ui::Slide::new(0), 1.0);
        // The card's top rule is the longest run of accent on the screen. It has
        // to be centred on the frame and narrower than it, because that is what
        // "the menu is in the middle and not a slab" means.
        let (mut best, mut best_row, mut x0, mut x1) = (0, 0, 0, 0);
        for y in 0..UI_H as i32 {
            let mut run = 0;
            let mut run_x0 = 0;
            for x in 0..UI_W as i32 {
                if screen.get(x, y) == ui::ACCENT {
                    if run == 0 {
                        run_x0 = x;
                    }
                    run += 1;
                    if run > best {
                        best = run;
                        best_row = y;
                        x0 = run_x0;
                        x1 = x;
                    }
                } else {
                    run = 0;
                }
            }
        }
        assert!(
            best > 120,
            "the card has no accent rule: the longest run is {best} px at row {best_row}"
        );
        assert!(
            best < UI_W as i32 - 40,
            "the rule spans the whole screen: {best} px"
        );
        let centre = (x0 + x1) / 2;
        assert!(
            (centre - UI_W as i32 / 2).abs() <= 4,
            "the card is off centre: its rule runs {x0}..{x1}"
        );
        // And it still goes through the whole-number scale onto the canvas.
        assert_eq!(UI_SCALE, 2);
    }

    /// The screens, rendered to PNGs so the layout can be looked at as well as
    /// measured. Only runs when asked: CI has nowhere to put the pictures.
    /// The side screen, in the three states it is looked at in: undecided, on
    /// the home plate, on the away plate. Dallas is on the home plate because
    /// its white is its home shirt, which the picture has to show.
    #[test]
    fn the_side_screen_can_be_rendered_to_pictures() {
        if std::env::var_os("NOPADS_DUMP_UI").is_none() {
            return;
        }
        let all = clubs();
        let find = |abbr: &str| {
            all.iter()
                .position(|c| c.abbr == abbr)
                .unwrap_or_else(|| panic!("no club {abbr}"))
        };
        let (first, second) = (find("DAL"), find("BOS"));
        let dir = format!("{}/../../out/ui", env!("CARGO_MANIFEST_DIR"));
        let frames = [
            ("side_middle", 0.0f32, 0.0f32),
            ("side_home", -1.0, 0.45),
            ("side_away", 1.0, 0.45),
        ];
        for (name, at, t) in frames {
            let mut frame = Frame::new(UI_W, UI_H);
            draw_side_pick(&mut frame, &all, first, true, second, false, at, t);
            nopads_render::write_png(format!("{dir}/{name}.png"), &frame).expect("write png");
        }
        println!("wrote the side screen to {dir}");
    }

    /// A picture of the drawing boards: a formation of circles with a job on
    /// every spot, a play drawn with the pen, a front of X's, and a coverage
    /// Every screen of the editor, so the whole flow can be read as pictures
    /// rather than walked with a pad.
    #[test]
    fn the_playbook_editor_can_be_rendered_screen_by_screen() {
        if std::env::var_os("NOPADS_DUMP_UI").is_none() {
            return;
        }
        let dir = format!("{}/../../out/ui", env!("CARGO_MANIFEST_DIR"));
        let save = |name: &str, screen: &Frame| {
            let mut canvas = Frame::new(crate::view::CANVAS_W, crate::view::CANVAS_H);
            canvas.blit_up(screen, UI_SCALE);
            nopads_render::write_png(format!("{dir}/{name}.png"), &canvas).expect("write png");
        };
        let mut first = DesignBook::from_built_in(Book::WestCoast);
        first.name = "SATURDAY BOOK".to_string();
        first.offense[0].name = "TRIPS RIGHT".to_string();
        first.plays[0][0].name = "MESH AND WHEEL".to_string();
        first.defense[0].name = "WIDE 9".to_string();
        first.calls[0][0].name = "COVER 2 CLOUD".to_string();
        let mut second = DesignBook::from_built_in(Book::Power);
        second.name = "GOAL LINE BOOK".to_string();
        let shelf = [first.clone(), second];
        let slide = ui::Slide::new(0);

        let mut frame = Frame::new(UI_W, UI_H);
        draw_shelf(&mut frame, &shelf, 0, &slide);
        save("design_shelf", &frame);

        let mut frame = Frame::new(UI_W, UI_H);
        draw_start_book(&mut frame, 1, &slide);
        save("design_start_book", &frame);

        let mut frame = Frame::new(UI_W, UI_H);
        draw_book_menu(&mut frame, &first, &slide);
        save("design_book_menu", &frame);

        let mut frame = Frame::new(UI_W, UI_H);
        draw_offense_list(&mut frame, &first, &slide);
        save("design_offense", &frame);

        let mut frame = Frame::new(UI_W, UI_H);
        draw_play_list(&mut frame, &first, 0, &slide);
        save("design_plays", &frame);

        let mut frame = Frame::new(UI_W, UI_H);
        draw_defense_list(&mut frame, &first, &slide);
        save("design_defense", &frame);

        let mut frame = Frame::new(UI_W, UI_H);
        draw_call_list(&mut frame, &first, 0, &slide);
        save("design_calls", &frame);

        let mut frame = Frame::new(UI_W, UI_H);
        draw_name_screen(
            &mut frame,
            "NAME THE PLAY",
            "STICK TO A LETTER, [A] TO TYPE IT",
            "MESH WHEEL",
            4,
            2,
        );
        save("design_name", &frame);
    }

    /// drawn behind a call.
    #[test]
    fn the_drawing_boards_can_be_rendered_to_pictures() {
        if std::env::var_os("NOPADS_DUMP_UI").is_none() {
            return;
        }
        let dir = format!("{}/../../out/ui", env!("CARGO_MANIFEST_DIR"));
        let save = |name: &str, screen: &Frame| {
            let mut canvas = Frame::new(crate::view::CANVAS_W, crate::view::CANVAS_H);
            canvas.blit_up(screen, UI_SCALE);
            nopads_render::write_png(format!("{dir}/{name}.png"), &canvas).expect("write png");
        };
        let mut book = DesignBook::from_built_in(Book::WestCoast);
        book.offense[0].name = "TRIPS RIGHT".to_string();
        let form = book.offense[0].clone();
        let bits = off_bits(&form.roles);
        let mut play = book.plays[0][0].clone();
        play.name = "MESH AND WHEEL".to_string();
        play.family = PlayFamily::QuickPass;
        let slot = form.slot_of(OffRole::Slot);
        let spot = form.spots[slot];
        play.routes[slot] = (1..=8)
            .map(|d| Vec2::new(spot.x + d as f32, spot.y + d as f32 * 0.5))
            .collect();
        let wide = form.slot_of(OffRole::WideL);
        let wspot = form.spots[wide];
        play.routes[wide] = (1..=10)
            .map(|d| Vec2::new(wspot.x + d as f32, wspot.y - d as f32 * 0.9))
            .collect();

        let mut frame = Frame::new(UI_W, UI_H);
        draw_field(&mut frame, false);
        draw_men(&mut frame, &form.spots, &bits, Some(0), false);
        cursor_box(&mut frame, form.spots[0].x as i8, form.spots[0].y as i8);
        ui::heading(&mut frame, UI_W as i32 / 2, 16, &form.name, ui::T_BODY, ui::ACCENT);
        centered_in(&mut frame, 320, 40, "SET QB - [Y] CHANGES THE JOB", 1, accent());
        centered_in(&mut frame, 320, FIELD_B + 10, "THE LINE IS LEGAL", 1, ui::GOOD);
        footer(&mut frame, "[STICK] CURSOR   [A] PICK UP A MAN   [Y] JOB   [B] DONE");
        save("play_formation", &frame);

        let mut frame = Frame::new(UI_W, UI_H);
        draw_field(&mut frame, false);
        draw_routes(&mut frame, &form.spots, &play.routes, slot);
        draw_men(&mut frame, &form.spots, &bits, None, false);
        cursor_box(&mut frame, 6, 3);
        ui::heading(&mut frame, UI_W as i32 / 2, 16, &play.name, ui::T_BODY, ui::ACCENT);
        centered_in(&mut frame, 320, 40, "DRAWING TE - PASS - PEN DOWN", 1, accent());
        centered_in(
            &mut frame,
            320,
            FIELD_B + 10,
            "EVERY YARD YOU DRAW IS A WAYPOINT: THE ROUTE STARTS AT HIS FEET",
            1,
            dim(),
        );
        footer(
            &mut frame,
            "[STICK] DRAW   [A] PEN   [X] NEXT MAN   [Y] PASS/RUN   [SELECT] WIPE   [B] DONE",
        );
        save("play_routes", &frame);

        let front = book.defense[0].clone();
        let fbits = def_bits(&front.roles);
        let mut frame = Frame::new(UI_W, UI_H);
        draw_field(&mut frame, true);
        draw_men(&mut frame, &front.spots, &fbits, Some(5), true);
        cursor_box(&mut frame, front.spots[5].x as i8, front.spots[5].y as i8);
        ui::heading(&mut frame, UI_W as i32 / 2, 16, &front.name, ui::T_BODY, ui::ACCENT);
        centered_in(&mut frame, 320, 40, "SET CB - [Y] CHANGES THE JOB", 1, accent());
        centered_in(&mut frame, 320, FIELD_B + 10, "EVERY MAN IS SET", 1, ui::GOOD);
        footer(&mut frame, "[STICK] CURSOR   [A] PICK UP A MAN   [Y] JOB   [B] DONE");
        save("play_front", &frame);

        let mut call = book.calls[0][0].clone();
        call.name = "COVER 2 CLOUD".to_string();
        call.coverage = Coverage::Cover2;
        let cb = front.slot_of(DefRole::CbL);
        let cspot = front.spots[cb];
        call.drops[cb] = (1..=9)
            .map(|d| Vec2::new(cspot.x + d as f32, cspot.y - d as f32 * 0.5))
            .collect();
        let mut frame = Frame::new(UI_W, UI_H);
        draw_field(&mut frame, true);
        draw_drops(&mut frame, &front.spots, &call.drops, cb);
        draw_men(&mut frame, &front.spots, &fbits, Some(cb), true);
        cursor_box(&mut frame, 8, -8);
        ui::heading(&mut frame, UI_W as i32 / 2, 16, &call.name, ui::T_BODY, ui::ACCENT);
        centered_in(&mut frame, 320, 40, "CB - COVER 2 - OFF - PEN DOWN", 1, accent());
        centered_in(
            &mut frame,
            320,
            FIELD_B + 10,
            "DRAW WHERE HE DROPS: THE COVERAGE FOLLOWS THE PATH YOU DREW",
            1,
            dim(),
        );
        footer(
            &mut frame,
            "[STICK] DRAW   [A] PEN   [X] NEXT MAN   [Y] PRESS/OFF   [SELECT] WIPE   [B] DONE",
        );
        save("play_coverage", &frame);
    }

    fn the_front_end_can_be_rendered_to_pictures() {
        if std::env::var_os("NOPADS_DUMP_UI").is_none() {
            return;
        }
        let dir = format!("{}/../../out/ui", env!("CARGO_MANIFEST_DIR"));
        let save = |name: &str, screen: &Frame| {
            let mut canvas = Frame::new(crate::view::CANVAS_W, crate::view::CANVAS_H);
            canvas.blit_up(screen, UI_SCALE);
            nopads_render::write_png(format!("{dir}/{name}.png"), &canvas).expect("write png");
        };
        let mut start = Frame::new(UI_W, UI_H);
        draw_start(&mut start, 1, &ui::Slide::new(1), 2.0);
        save("start", &start);

        let rows = [
            ("CAMERA", "BROADCAST".to_string()),
            ("LENS", "1.00 / +0.00".to_string()),
            ("CONTROLS", String::new()),
            ("RESOLUTION", "1280 X 720".to_string()),
            ("DISPLAY", "WINDOWED".to_string()),
            ("BACK", String::new()),
        ];
        let mut settings = Frame::new(UI_W, UI_H);
        draw_option_screen(
            &mut settings,
            "SETTINGS",
            "THE GAME'S OWN DEFAULTS",
            &rows,
            &ui::Slide::new(2),
            "CHANGES ARE SAVED AS YOU MAKE THEM",
            "[UP/DOWN] ROW   [LEFT/RIGHT] CHANGE   [B] BACK",
        );
        save("settings", &settings);
        for (i, _) in crate::controls::PAGES.iter().enumerate() {
            let mut page = Frame::new(UI_W, UI_H);
            night(&mut page);
            crate::controls::draw(&mut page, i);
            footer(&mut page, "[LEFT/RIGHT] PAGE   [A] DONE");
            save(&format!("controls_{}", i + 1), &page);
        }
        let all = clubs();
        let mut length = Frame::new(UI_W, UI_H);
        draw_season_length(&mut length, &all, 0, 4, &ui::Slide::new(4), 2.0);
        save("season_length", &length);

        // The playbook screen, with the club's own book marked, and the tutorial
        // submenu that opens off MORE MODES.
        let rows: Vec<(&str, String)> = Book::ALL
            .iter()
            .map(|b| {
                (
                    b.label(),
                    if *b == all[0].book {
                        "YOUR CLUB".to_string()
                    } else {
                        String::new()
                    },
                )
            })
            .collect();
        let mut books = Frame::new(UI_W, UI_H);
        draw_option_screen(
            &mut books,
            "PLAYBOOK",
            &format!("{} RUNS {}", all[0].abbr, all[0].book.label()),
            &rows,
            &ui::Slide::new(1),
            Book::ALL[1].blurb(),
            "[UP/DOWN] BOOK   [A] TAKE IT   [B] BACK",
        );
        save("playbook", &books);

        let mut tracks = Frame::new(UI_W, UI_H);
        draw_tutorial(&mut tracks, 1, &ui::Slide::new(1));
        save("tutorial_tracks", &tracks);

        let mut practice = Frame::new(UI_W, UI_H);
        draw_practice(&mut practice, 0, &ui::Slide::new(0));
        save("practice_tracks", &practice);

        let mut difficulty = Frame::new(UI_W, UI_H);
        draw_difficulty(&mut difficulty, 1, &ui::Slide::new(1));
        save("difficulty", &difficulty);

        let (depth, roster) = depth_chart(&all[0]);
        let pages = roster.len().div_ceil(CHART_ROWS).max(1);
        let mut chart = Frame::new(UI_W, UI_H);
        draw_roster(
            &mut chart,
            &all[0],
            &roster,
            &depth,
            (1, 0),
            0,
            pages,
            &ui::Slide::new(1),
            2.0,
        );
        save("roster", &chart);

        let qb = roster
            .iter()
            .find(|p| p.pos == Position::Qb)
            .expect("a club with a quarterback");
        let mut card = Frame::new(UI_W, UI_H);
        draw_player_card(&mut card, qb, 2.0);
        save("player_card", &card);

        let mut call = Frame::new(UI_W, UI_H);
        draw_coin_toss(
            &mut call,
            &all,
            0,
            1,
            0,
            CoinStep::Call,
            0,
            0.0,
            true,
            &ui::Slide::new(0),
        );
        save("coin_call", &call);
        let mut choose = Frame::new(UI_W, UI_H);
        draw_coin_toss(
            &mut choose,
            &all,
            0,
            1,
            0,
            CoinStep::Choose,
            1,
            0.0,
            true,
            &ui::Slide::new(1),
        );
        save("coin_choose", &choose);

        println!("wrote the front end to {dir}");
    }

    #[test]
    fn a_screen_of_badges_draws_inside_the_canvas() {
        let mut frame = Frame::new(UI_W, UI_H);
        night(&mut frame);
        for (i, club) in clubs().iter().enumerate() {
            let x = 40 + (i as i32 % 4) * 150;
            let y = 60 + (i as i32 / 4) * 70;
            badge(&mut frame, x, y, 40, club, i % 2 == 0);
        }
        centered(&mut frame, 300, "EXHIBITION", 2, accent());
    }
}

/// The lens screen: the shot the player is tuning, live, with the two numbers
/// that make it. Left and right zoom, up and down tilt, X puts it back.
fn lens_screen(view: &mut View, pad: &mut Option<&mut Pad>, settings: &mut Settings) {
    use nopads_core::playbook::{defensive_playbook, playbook};
    use nopads_core::sim::{PlaySim, SnapSetup};
    use nopads_core::team::league_teams;
    use nopads_render::broadcast::{Broadcast, CameraMode};

    let teams = league_teams();
    let home = teams[0].clone();
    let away = teams[1].clone();
    let setup = SnapSetup {
        los: 35.0,
        ball_y: 0.0,
        direction: 1,
        down: 1,
        to_go: 10.0,
        seed: 7,
        offense_team: 0,
        defense_team: 1,
        is_try: false,
    };
    let book = playbook();
    let dbook = defensive_playbook();
    let mut sim = PlaySim::new(&home, &away, &book[0], &dbook[0], &setup);
    // Hold at the line, so the preview is a formation rather than a play running
    // itself out while the player is still choosing a zoom.
    sim.await_snap(true);
    for _ in 0..20 {
        sim.step(1.0 / 60.0);
    }

    // The preview borrows the teams for its sim, so the picture gets its own copy.
    let mut broadcast = Broadcast::new(UI_W, UI_H, [home.clone(), away.clone()]);
    broadcast.set_lens(settings.lens_zoom, settings.lens_tilt);
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.b || input.pressed.start || input.pressed.a {
            settings.save();
            break;
        }
        let mut changed = false;
        if input.stick.x.abs() > 0.25 {
            settings.lens_zoom = (settings.lens_zoom + input.stick.x * dt * 0.8).clamp(0.6, 1.8);
            changed = true;
        }
        if input.stick.y.abs() > 0.25 {
            settings.lens_tilt = (settings.lens_tilt - input.stick.y * dt * 0.6).clamp(-0.35, 0.45);
            changed = true;
        }
        if input.pressed.x {
            settings.lens_zoom = 1.0;
            settings.lens_tilt = 0.0;
            changed = true;
        }
        if changed {
            broadcast.set_lens(settings.lens_zoom, settings.lens_tilt);
        }
        let mut frame = broadcast.live_frame(&sim, CameraMode::Classic);
        frame.fill_rect_alpha(10, 10, 190, 48, Rgb::new(8, 12, 18), 0.72);
        frame.text_shadowed(
            18,
            16,
            &format!("ZOOM {:.2}", settings.lens_zoom),
            2,
            paper(),
            1,
        );
        frame.text_shadowed(
            18,
            38,
            &format!("TILT {:+.2}", settings.lens_tilt),
            2,
            paper(),
            1,
        );
        let help = "[STICK] ZOOM / TILT   [X] RESET   [B] DONE".to_string();
        if !view.show_ui(&mut frame, &[help]) {
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    view.reset();
}

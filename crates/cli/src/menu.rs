//! The front of the game: the start menu, the settings screen and the
//! exhibition picker.
//!
//! Everything here is drawn on the same 640x360 canvas the broadcast uses, so
//! the menu looks like the game does. On a machine with no display the frames
//! are averaged down into terminal cells by `View`, exactly as live play is.

use std::time::{Duration, Instant};

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
use nopads_platform::pad::{self, Pad};
use nopads_platform::settings::{Settings, QUARTER_CHOICES, RESOLUTIONS};
use crate::view::{View, UI_H, UI_W};

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

/// What the player picked in exhibition: the two clubs and the side he runs.
pub struct Kickoff {
    /// The club the player picked.
    pub yours: usize,
    /// The computer's club, filled in by the menu.
    pub theirs: usize,
    /// True when the player takes the opening throw-off.
    pub receive: bool,
    /// The ground the game is played at: an index into the venue list.
    pub venue: usize,
}

/// What the player asked the front end for.
pub enum Front {
    /// One exhibition game.
    Friendly(Kickoff),
    /// A whole season, already scheduled.
    Season(Box<Season>),
}

enum Start {
    Exhibition,
    Season,
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
    frame.fill_rect(x + 3, y + h / 6, w - 6, (h / 10).max(3), tone(club.secondary));
    frame.fill_rect(x + 3, y + h / 6, (w / 16).max(2), h - h / 3, tone(club.accent));
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
        Repeat { held: 0.0, fired: false, delay: 0.30, gap: 0.16 }
    }
}

impl Repeat {
    /// A lazier repeat, for a screen where one press should move one item and
    /// holding the key should not run away past it.
    fn slow() -> Self {
        Repeat { delay: 0.45, gap: 0.24, ..Repeat::default() }
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
    const LABELS: [&str; 5] = ["EXHIBITION", "SEASON", "CUSTOMIZATION", "SETTINGS", "QUIT"];
    const NOTES: [&str; 5] = [
        "ONE GAME, PICKED IN A MINUTE",
        "A CLUB, A LEAGUE, A SEASON",
        "DEPTH CHARTS AND ROSTERS",
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
    let row_h = 26;
    let gap = 6;
    let pad = ui::S3;
    let n = LABELS.len() as i32;
    let card_h = pad * 2 + n * row_h + (n - 1) * gap;
    let card_x = cx - card_w / 2;
    let card_y = (h as f32 * 0.32) as i32;
    let lit = ui::enter(t, 1, LABELS.len() + 3);
    ui::card(frame, card_x, card_y, card_w, card_h, ui::ACCENT, lit);

    for (i, label) in LABELS.iter().enumerate() {
        let y = card_y + pad + i as i32 * (row_h + gap);
        let heat = slide.heat(i) * ui::enter(t, i + 2, LABELS.len() + 3);
        ui::row(frame, card_x + pad, y, card_w - pad * 2, row_h, label, None, heat, ui::ACCENT);
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
    const ITEMS: usize = 5;
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
                2 => Start::Customize,
                3 => Start::Settings,
                _ => Start::Quit,
            });
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
    let (row_h, gap) = (30, 6);
    let n = rows.len() as i32;
    let card_h = n * (row_h + gap) + ui::S3;
    ui::card(frame, x - ui::S3, 74, cw + ui::S3 * 2, card_h, ui::ACCENT, 1.0);
    for (i, (name, value)) in rows.iter().enumerate() {
        let y = 84 + i as i32 * (row_h + gap);
        let value = (!value.is_empty()).then_some(value.as_str());
        ui::row(frame, x, y, cw, row_h, name, value, slide.heat(i), ui::ACCENT);
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
    const ROWS: usize = 5;
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
                    let n = RESOLUTIONS.len() as isize;
                    let i = ((settings.resolution_index() as isize + dir).rem_euclid(n)) as usize;
                    settings.set_resolution(i);
                    changed = true;
                }
                3 => {
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
            ("CAMERA", CAMERA_LABELS[settings.camera % CAMERA_LABELS.len()].to_string()),
            ("LENS", settings.lens_label()),
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

// ---------------------------------------------------------------------------
// Game setup
// ---------------------------------------------------------------------------

/// The card between the ground and the coin: how long the game runs and how hard
/// the other sideline plays. Both lived on the settings screen, which is where a
/// player looks for the machine's defaults, not for the rules of the game he is
/// about to start.
fn game_setup(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    settings: &mut Settings,
) -> Option<()> {
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
            ("DIFFICULTY", DIFFICULTY[settings.difficulty % 3].to_string()),
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
// Customization: every club's depth chart and roster
// ---------------------------------------------------------------------------

/// The clubs, four across, with their ratings. A opens a club's roster.
fn customize_screen(view: &mut View, pad: &mut Option<&mut Pad>) -> Option<()> {
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
        centered(&mut frame, 12, "CUSTOMIZATION", 3, accent());
        centered(&mut frame, 44, "DEPTH CHART AND ROSTER FOR EVERY CLUB", 1, dim());
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
            frame.text_shadowed(cx - 12, cy - 14, all[i].abbr, 2, if on { accent() } else { dim() }, 1);
            let ovr = format!("{} OVR", ratings[i].overall);
            frame.text_shadowed(cx - 12, cy + 2, &ovr, 1, if on { paper() } else { dim() }, 1);
            frame.text_shadowed(cx - 12, cy + 14, all[i].city, 1, dim(), 1);
        }
        footer(&mut frame, "[STICK] CLUB   [A] ROSTER   [B] BACK");
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

/// One club's depth chart, best man first at every position, with the full roster
/// beside it. The depths are the same ordering the game uses to pick its seven,
/// and they can be edited: X walks a man up his row and Y walks him down, and the
/// chart is saved against the club, so the next game starts the men the player
/// picked.
fn roster_screen(view: &mut View, pad: &mut Option<&mut Pad>, club: &Club) -> Option<()> {
    const ROWS: usize = 16;
    const SHOWN: usize = 3;
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
        let mut men: Vec<usize> = (0..roster.len()).filter(|&i| roster[i].pos == pos).collect();
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
    let pages = roster.len().div_ceil(ROWS).max(1);
    let mut page = 0usize;
    let mut sel = (0usize, 0usize);
    let mut nav = Repeat::default();
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
                    nth += depth[r as usize].1.len().min(SHOWN).max(1) as i32;
                }
                while nth >= depth[r as usize].1.len().min(SHOWN).max(1) as i32 {
                    nth -= depth[r as usize].1.len().min(SHOWN).max(1) as i32;
                    r = (r + 1) % rows;
                }
                sel = (r as usize, nth as usize);
            }
        }
        let row = sel.0;
        let shown = depth[row].1.len().min(SHOWN).max(1);
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
        let mut frame = Frame::new(UI_W, UI_H);
        night(&mut frame);
        centered(&mut frame, 8, &club.full_name().to_uppercase(), 2, accent());
        frame.fill_rect(20, 30, 600, 1, Rgb::new(48, 92, 148));
        frame.text_shadowed(20, 38, "DEPTH CHART", 1, paper(), 1);
        let title = format!("ROSTER   {} / {}", page + 1, pages);
        frame.text_shadowed(330, 38, &title, 1, paper(), 1);
        for (r, (pos, men)) in depth.iter().enumerate() {
            let y = 58 + r as i32 * 30;
            frame.text_shadowed(20, y + 2, pos.abbr(), 2, accent(), 1);
            for (n, &i) in men.iter().take(SHOWN).enumerate() {
                let p = &roster[i];
                let on = r == row && n == sel.1;
                if on {
                    frame.rounded_rect(54, y - 2 + n as i32 * 9, 250, 11, 3, panel());
                }
                let line = format!(
                    "{:>2}  {:<12}{}",
                    p.number,
                    p.last_name().to_uppercase(),
                    p.overall()
                );
                let color = if n == 0 { paper() } else { dim() };
                frame.text_shadowed(
                    58,
                    y + n as i32 * 9,
                    &line,
                    1,
                    if on { accent() } else { color },
                    1,
                );
            }
        }
        frame.fill_rect(320, 52, 1, 268, Rgb::new(48, 92, 148));
        for (n, i) in (page * ROWS..(page * ROWS + ROWS).min(roster.len())).enumerate() {
            let p = &roster[i];
            let line = format!(
                "#{:>2} {:<13} {:<3} {}",
                p.number,
                p.last_name().to_uppercase(),
                p.pos.abbr(),
                p.overall()
            );
            frame.text_shadowed(334, 56 + n as i32 * 15, &line, 1, paper(), 1);
        }
        footer(
            &mut frame,
            "[UP/DOWN] MAN   [X] UP  [Y] DOWN   [A] CARD   [LEFT/RIGHT] ROSTER   [B] BACK",
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

/// One man's card: the ten numbers a roster is judged by, on the same 40..99 scale
/// the clubs are rated on.
fn player_card(view: &mut View, pad: &mut Option<&mut Pad>, player: &Player) -> Option<()> {
    view.reset();
    let mut last = Instant::now();
    loop {
        let now = Instant::now();
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.a || input.pressed.b || input.pressed.start {
            return Some(());
        }
        let mut frame = Frame::new(UI_W, UI_H);
        night(&mut frame);
        centered(
            &mut frame,
            10,
            &format!("#{} {}", player.number, player.full_name().to_uppercase()),
            2,
            accent(),
        );
        centered(
            &mut frame,
            30,
            &format!("{}   {} OVERALL", player.pos.abbr(), player.overall()),
            1,
            dim(),
        );
        for (n, (label, value)) in player.attrs.ratings().iter().enumerate() {
            let y = 54 + n as i32 * 26;
            frame.text_shadowed(110, y + 1, label, 1, paper(), 1);
            frame.fill_rect(260, y, 210, 9, Rgb::new(30, 40, 58));
            let filled = (210.0 * (*value as f32 / 99.0)) as i32;
            frame.fill_rect(260, y, filled.max(2), 9, accent());
            frame.text_shadowed(482, y + 1, &format!("{value}"), 1, paper(), 1);
        }
        footer(&mut frame, "[A/B] BACK");
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
        let _ = last;
    }
}

// ---------------------------------------------------------------------------
// Exhibition
// ---------------------------------------------------------------------------

fn ids_in(all: &[Club], division: Division) -> Vec<usize> {
    (0..all.len()).filter(|&i| all[i].division == division).collect()
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
    // Both sides are the player's to pick now, not just his own: the second
    // screen is the club he is playing against, from any division. Either screen
    // can be left to a roll instead.
    let yours = pick_team(view, pad, &all, &cards, None, "YOUR TEAM")?.club(&all, None);
    let theirs = pick_team(view, pad, &all, &cards, Some(yours), "THEIR TEAM")?
        .club(&all, Some(yours));
    // The ground comes before the toss: the player picks where the game is
    // played, and then stands on it to call the coin.
    let venue = pick_venue(view, pad, yours)?;
    // How long and how hard, then the coin: both are about this game, so they are
    // asked here rather than buried in the settings screen.
    game_setup(view, pad, settings)?;
    let receive = coin_flip(view, pad, &all, yours, theirs, venue)?;
    Some(Kickoff { yours, theirs, receive, venue })
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
        ui::heading(&mut frame, UI_W as i32 / 2, 8, "WHERE IS IT PLAYED", ui::T_LABEL, ui::ACCENT);
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
                    None => frame.fill_rect(
                        x,
                        y,
                        THUMB_W as i32,
                        THUMB_H as i32,
                        sky[1].scale(0.45),
                    ),
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

/// What the carousel came back with: a club, or a roll of the division it was
/// showing when the player took the random card.
enum Pick {
    Club(usize),
    Random(usize),
}

impl Pick {
    fn club(self, all: &[Club], avoid: Option<usize>) -> usize {
        match self {
            Pick::Club(i) => i,
            Pick::Random(div) => roll_club(&ids_in(all, Division::ALL[div]), avoid, coin_seed()),
        }
    }
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
    centered_in(frame, cx, y + h + 6, "RANDOM", if size > 80 { 2 } else { 1 }, tone(paper()));
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
    let mut nav = [Repeat::default(), Repeat::default()];
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
        if let d = nav[1].step(input.stick.y, dt) {
            if d != 0 {
                div = wrap(div, Division::ALL.len(), -d);
                sel = 0;
                slide = 0.0;
            }
        }
        // The clubs of this division and one card past them that rolls for the
        // player, so he never has to know a club to pick one.
        let mut slots: Vec<Option<usize>> = ids_in(all, Division::ALL[div])
            .into_iter()
            .map(Some)
            .collect();
        slots.push(None);
        let n = slots.len();
        if let d = nav[0].step(input.stick.x, dt) {
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
                Some(pick) => return Some(Pick::Club(pick)),
                None => return Some(Pick::Random(div)),
            }
        }

        let club = slots[sel];
        let prev = slots[wrap(sel, n, -1)];
        let next = slots[wrap(sel, n, 1)];
        let mut frame = Frame::new(UI_W, UI_H);
        night(&mut frame);
        ui::heading(&mut frame, UI_W as i32 / 2, 14, heading, ui::T_LABEL, ui::ACCENT);
        // The divisions are one segmented control, not four labels in a row.
        let widths: Vec<i32> = Division::ALL.iter().map(|d| ui::chip_width(d.label())).collect();
        let total: i32 = widths.iter().sum::<i32>() + ui::S2 * (widths.len() as i32 - 1);
        let mut tab_x = UI_W as i32 / 2 - total / 2;
        for (i, d) in Division::ALL.iter().enumerate() {
            ui::chip(&mut frame, tab_x, 42, d.label(), if i == div { 1.0 } else { 0.0 }, ui::ACCENT);
            tab_x += widths[i] + ui::S2;
        }
        let accent_tone = match club {
            Some(i) => all[i].accent,
            None => ui::ACCENT,
        };
        ui::card(&mut frame, 190, 74, 260, 226, accent_tone, ui::enter(t, 0, 8));
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
                ui::meter(&mut frame, 248, y, 136, 6, *value as f32 / 99.0, accent_tone);
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
        let note_color = if said > 0.0 { Rgb::new(255, 120, 96) } else { dim() };
        frame.text_shadowed(
            UI_W as i32 - 14 - text_width(&note, 1),
            18,
            &note,
            1,
            note_color,
            1,
        );
        footer(&mut frame, "[LEFT/RIGHT] ROLL   [UP/DOWN] DIVISION   [A] PICK   [B] BACK");
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
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
    let you = pick_team(view, pad, &all, &cards, None, "YOUR CLUB THIS SEASON")?.club(&all, None);
    let weeks = season_length(view, pad, you, &all)?;
    Some(Season::new(you, weeks, coin_seed()))
}

/// The lengths on offer. The last is the full round-robin: every club twice a
/// week for thirty-one weeks, which is the longest season the league can give.
const SEASON_LENGTHS: [usize; 9] = [4, 6, 8, 10, 12, 16, 20, 24, 31];

fn season_length(view: &mut View, pad: &mut Option<&mut Pad>, you: usize, all: &[Club]) -> Option<usize> {
    let mut pick = 4usize;
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
                pick = wrap(pick, SEASON_LENGTHS.len(), d);
            }
        }
        let weeks = SEASON_LENGTHS[pick];
        let mut frame = Frame::new(UI_W, UI_H);
        night(&mut frame);
        centered(&mut frame, 26, "SEASON", 3, accent());
        centered(&mut frame, 66, "HOW MANY WEEKS", 2, paper());
        badge(&mut frame, 320, 130, 44, &all[you], true);
        centered(&mut frame, 166, &all[you].full_name().to_uppercase(), 2, paper());
        // The number, big, with the season either side of it.
        let mut x = 320 - (SEASON_LENGTHS.len() as i32 * 34) / 2;
        for (i, w) in SEASON_LENGTHS.iter().enumerate() {
            let on = i == pick;
            if on {
                frame.rounded_rect(x - 4, 200, 32, 30, 6, panel());
                frame.fill_rect(x - 4, 228, 32, 2, accent());
            }
            centered_in(&mut frame, x + 12, 206, &w.to_string(), 2, if on { accent() } else { dim() });
            x += 34;
        }
        centered(&mut frame, 244, &format!("{weeks} GAMES IN THE REGULAR SEASON"), 1, paper());
        centered(
            &mut frame,
            260,
            "THE TOP TWO IN EACH DIVISION MAKE THE PLAYOFFS",
            1,
            dim(),
        );
        centered(
            &mut frame,
            274,
            if weeks == season::MAX_WEEKS {
                "A FULL ROUND ROBIN: EVERY CLUB HOME AND AWAY"
            } else {
                "EVERY CLUB PLAYS EVERY WEEK"
            },
            1,
            dim(),
        );
        footer(&mut frame, "[LEFT/RIGHT] LENGTH   [A] START THE SEASON   [B] BACK");
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
                                receive: coin_seed() % 2 == 0,
                                venue,
                            }));
                        }
                    }
                    "SIM MY GAME" | "SIM THIS WEEK" => {
                        let played = season.phase != Phase::Regular
                            && season.round_name() == "CHAMPIONSHIP";
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
            Phase::Regular => format!("WEEK {} OF {}", (season.week + 1).min(season.length()), season.length()),
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
                let next = format!("{} {}", if home { "VS" } else { "AT" }, all[other].full_name().to_uppercase());
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
            frame.text_shadowed(36, y, all[*club].abbr, 1, if on { accent() } else { paper() }, 1);
            frame.text_shadowed(80, y, &all[*club].city.to_uppercase(), 1, dim(), 1);
            let right = format!("{:>5}  {:>4}", r.line(), r.diff());
            frame.text_shadowed(248 - text_width(&right, 1), y, &right, 1, if on { paper() } else { dim() }, 1);
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
                    frame.text_shadowed(x, y + 10, text2, 1, if mine { accent() } else { dim() }, 1);
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
                    let seed = seeds.iter().position(|c| c == club).map(|i| i + 1).
                        unwrap_or(0);
                    let label = if seed > 0 { format!("#{seed} {}", all[*club].abbr) } else { all[*club].abbr.to_string() };
                    frame.text_shadowed(290 + k as i32 * 96, y + 12, &label, 1, if *club == season.you { accent() } else { paper() }, 1);
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
                    if yours { "YOU WON IT ALL" } else { "SEASON OVER" },
                    2,
                    if yours { accent() } else { dim() },
                );
            }
        }
        // The actions sit in one row along the bottom, above the footer.
        let mut x = 24;
        for (i, label) in rows.iter().enumerate() {
            let heat = if season.phase == Phase::Done { 0.0 } else { slide.heat(i) };
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
    let gold = Rgb::new(214, 168, 60);
    let rim = Rgb::new(150, 112, 36);
    let inner = Rgb::new(178, 138, 48);
    frame.rounded_rect(cx - w / 2 - 5, cy - h / 2 - 5, w + 10, h + 10, w / 2 + 5, rim);
    frame.rounded_rect(cx - w / 2, cy - h / 2, w, h, w / 2, gold);
    if w > 26 {
        frame.rounded_rect(cx - w / 2 + 7, cy - h / 2 + 7, w - 14, h - 14, (w - 14) / 2, inner);
    }
    if w > 34 {
        let face = if heads { "H" } else { "T" };
        let scale = ((h as usize) / 16).max(1);
        let tw = text_width(face, scale);
        frame.text_shadowed(
            cx - tw / 2,
            cy - (7 * scale as i32) / 2,
            face,
            scale,
            if landed { paper() } else { Rgb::new(255, 236, 178) },
            1,
        );
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
    #[derive(Clone, Copy, PartialEq)]
    enum Step {
        Call,
        Flip,
        Won,
        Lost,
        Choose,
    }
    let mut step = Step::Call;
    let mut pick = 0usize;
    let mut t = 0.0f32;
    let mut heads = true;
    let mut nav = Repeat::default();
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        match step {
            Step::Call => {
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
                    step = Step::Flip;
                }
            }
            Step::Flip => {
                t += dt;
                if t >= 1.05 {
                    t = 0.0;
                    heads = coin_seed() % 2 == 0;
                    let called_heads = pick == 0;
                    step = if heads == called_heads { Step::Won } else { Step::Lost };
                }
            }
            Step::Won => {
                t += dt;
                if input.pressed.a && t > 0.3 {
                    pick = 0;
                    step = Step::Choose;
                }
            }
            Step::Lost => {
                t += dt;
                if t > 1.5 || input.pressed.a {
                    // The computer won the toss: it takes the ball.
                    return Some(false);
                }
            }
            Step::Choose => {
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

        let mut frame = Frame::new(UI_W, UI_H);
        night(&mut frame);
        centered(&mut frame, 18, "COIN TOSS", 3, accent());
        centered(
            &mut frame,
            56,
            &format!(
                "{} AT {}",
                all[yours].full_name().to_uppercase(),
                all[theirs].full_name().to_uppercase()
            ),
            1,
            dim(),
        );

        centered(
            &mut frame,
            72,
            &format!(
                "AT {}",
                Venue::from_index(venue).name().to_uppercase()
            ),
            1,
            dim(),
        );

        let (show_heads, width, landed) = match step {
            Step::Call => (pick == 0, 118, false),
            Step::Flip => {
                let spin = (t * 13.0).cos().abs();
                (heads || ((t * 13.0) as i32 % 2 == 0), (34.0 + 84.0 * spin) as i32, false)
            }
            Step::Won | Step::Choose => (heads, 118, true),
            Step::Lost => (heads, 118, true),
        };
        coin(&mut frame, 320, 148, width, show_heads, landed);

        match step {
            Step::Call => {
                centered(&mut frame, 224, "CALL IT", 1, paper());
                let opts = ["HEADS", "TAILS"];
                for (i, label) in opts.iter().enumerate() {
                    let x = 190 + i as i32 * 150;
                    let on = i == pick;
                    if on {
                        frame.fill_rect(x - 24, 240, 118, 22, panel());
                        frame.fill_rect(x - 24, 260, 118, 2, accent());
                    }
                    centered_in(&mut frame, x + 35, 244, label, 2, if on { paper() } else { dim() });
                }
                footer(&mut frame, "[LEFT/RIGHT] CALL   [A] FLIP   [B] BACK");
            }
            Step::Flip => {
                centered(&mut frame, 224, "IN THE AIR", 2, paper());
                footer(&mut frame, "WAITING ON THE COIN");
            }
            Step::Won => {
                centered(&mut frame, 214, if heads { "HEADS" } else { "TAILS" }, 2, accent());
                centered(&mut frame, 240, "YOU CALLED IT. YOU WON THE TOSS.", 1, paper());
                footer(&mut frame, "[A] CHOOSE");
            }
            Step::Lost => {
                centered(&mut frame, 214, if heads { "HEADS" } else { "TAILS" }, 2, dim());
                centered(&mut frame, 240, "YOU LOST THE TOSS", 1, paper());
                centered(
                    &mut frame,
                    258,
                    &format!("{} WILL RECEIVE", all[theirs].full_name().to_uppercase()),
                    1,
                    all[theirs].accent,
                );
                footer(&mut frame, "THE COMPUTER TAKES THE BALL");
            }
            Step::Choose => {
                centered(&mut frame, 214, "THROW OR RECEIVE", 1, paper());
                let opts = ["THROW IT OFF", "RECEIVE IT"];
                for (i, label) in opts.iter().enumerate() {
                    let x = 176 + i as i32 * 176;
                    let on = i == pick;
                    if on {
                        frame.fill_rect(x - 40, 238, 160, 22, panel());
                        frame.fill_rect(x - 40, 258, 160, 2, accent());
                    }
                    centered_in(&mut frame, x + 38, 242, label, 2, if on { paper() } else { dim() });
                }
                footer(&mut frame, "[LEFT/RIGHT] CHOOSE   [A] KICK OFF");
            }
        }
        if !view.show_ui(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

#[cfg(test)]
mod tests {
    use crate::view::UI_SCALE;
    use super::*;

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
            seen[list.iter().position(|&i| i == got).expect("not in the list")] = true;
        }
        assert!(seen.iter().all(|s| *s), "the roll never reached every club: {seen:?}");
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
    #[test]
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
        frame.text_shadowed(18, 16, &format!("ZOOM {:.2}", settings.lens_zoom), 2, paper(), 1);
        frame.text_shadowed(18, 38, &format!("TILT {:+.2}", settings.lens_tilt), 2, paper(), 1);
        let help = "[STICK] ZOOM / TILT   [X] RESET   [B] DONE".to_string();
        if !view.show_ui(&mut frame, &[help]) {
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    view.reset();
}


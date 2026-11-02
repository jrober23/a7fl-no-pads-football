//! The front of the game: the start menu, the settings screen and the
//! exhibition picker.
//!
//! Everything here is drawn on the same 640x360 canvas the broadcast uses, so
//! the menu looks like the game does. On a machine with no display the frames
//! are averaged down into terminal cells by `View`, exactly as live play is.

use std::time::{Duration, Instant};

use a7fl_core::game::{GameState, PlayRecord};
use a7fl_core::math::Rgb;
use a7fl_core::team::{clubs, league_teams, Club, Division, Team, TeamRatings};
use a7fl_render::raster::Frame;
use a7fl_render::venues::{self, Venue};
use a7fl_render::{Broadcast, CameraMode};

use crate::arcade::CAMERA_LABELS;
use crate::pad::{self, Pad};
use crate::settings::{Settings, RESOLUTIONS};
use crate::view::{View, CANVAS_H, CANVAS_W};

fn paper() -> Rgb {
    Rgb::new(232, 238, 246)
}

fn dim() -> Rgb {
    Rgb::new(118, 132, 154)
}

fn accent() -> Rgb {
    Rgb::new(255, 196, 64)
}

fn backdrop() -> Rgb {
    Rgb::new(12, 16, 26)
}

fn panel() -> Rgb {
    Rgb::new(26, 38, 60)
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

enum Start {
    Exhibition,
    Settings,
    Quit,
}

/// Run the front end. `None` means the player quit (or closed the window).
pub fn run(view: &mut View, pad: Option<&mut Pad>, settings: &mut Settings) -> Option<Kickoff> {
    let mut pad = pad;
    loop {
        match start_screen(view, &mut pad, settings)? {
            Start::Exhibition => {
                if let Some(kick) = exhibition(view, &mut pad) {
                    return Some(kick);
                }
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
    let x = (CANVAS_W as i32 - text_width(text, scale)) / 2;
    frame.text_shadowed(x, y, text, scale, color, 1);
}

fn centered_in(frame: &mut Frame, cx: i32, y: i32, text: &str, scale: usize, color: Rgb) {
    frame.text_shadowed(cx - text_width(text, scale) / 2, y, text, scale, color, 1);
}

fn night(frame: &mut Frame) {
    frame.clear(backdrop());
    frame.gradient_rect(
        0,
        0,
        CANVAS_W as i32,
        CANVAS_H as i32,
        Rgb::new(20, 30, 50),
        Rgb::new(6, 9, 15),
    );
    // Two painted lines, so the screen reads as the same field the game is on.
    frame.fill_rect(0, 96, CANVAS_W as i32, 1, Rgb::new(30, 48, 76));
    frame.fill_rect(0, 272, CANVAS_W as i32, 1, Rgb::new(30, 48, 76));
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
    frame.fill_rect(0, CANVAS_H as i32 - 22, CANVAS_W as i32, 22, Rgb::new(8, 11, 18));
    frame.fill_rect(0, CANVAS_H as i32 - 22, CANVAS_W as i32, 1, Rgb::new(30, 48, 76));
    centered(frame, CANVAS_H as i32 - 15, text, 1, dim());
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

fn start_screen(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    _settings: &mut Settings,
) -> Option<Start> {
    const ITEMS: usize = 3;
    let labels = ["EXHIBITION", "SETTINGS", "QUIT"];
    let mut pick = 0usize;
    let mut nav = Repeat::default();
    let mut last = Instant::now();
    view.reset();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.start {
            return Some(Start::Quit);
        }
        if let d = nav.step(input.stick.y, dt) {
            if d != 0 {
                pick = wrap(pick, ITEMS, -d);
            }
        }

        let mut frame = Frame::new(CANVAS_W, CANVAS_H);
        night(&mut frame);
        centered(&mut frame, 34, "NO PADS", 7, accent());
        centered(&mut frame, 100, "NO PADS FOOTBALL", 2, paper());
        frame.fill_rect(150, 168, 340, 1, Rgb::new(48, 92, 148));
        for (i, label) in labels.iter().enumerate() {
            let y = 192 + i as i32 * 30;
            let on = i == pick;
            if on {
                frame.fill_rect(228, y - 6, 190, 24, panel());
                frame.fill_rect(228, y - 6, 4, 24, accent());
            }
            frame.text_shadowed(246, y, label, 2, if on { paper() } else { dim() }, 1);
        }
        footer(&mut frame, "[STICK] CHOOSE   [A] SELECT   [START] QUIT");

        if !view.show(&mut frame, &[]) {
            return None;
        }
        if input.pressed.a {
            return Some(match pick {
                0 => Start::Exhibition,
                1 => Start::Settings,
                _ => Start::Quit,
            });
        }
        beat();
    }
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

fn settings_screen(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    settings: &mut Settings,
) -> Option<()> {
    const ROWS: usize = 7;
    const DIFFICULTY_LABELS: [&str; 3] = ["EASY", "NORMAL", "HARD"];
    let mut pick = 0usize;
    let mut nav = Repeat::default();
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
                    settings.difficulty =
                        ((settings.difficulty as isize + dir).rem_euclid(3)) as usize;
                    changed = true;
                }
                2 => {
                    let n = a7fl_cli_quarter_count();
                    let i = ((settings.quarter_index() as isize + dir).rem_euclid(n)) as usize;
                    settings.set_quarter(i);
                    changed = true;
                }
                3 => {
                    // The lens opens its own live preview: two numbers are easier
                    // to judge against the picture than in a menu row.
                    if forward {
                        lens_screen(view, pad, settings);
                        last = Instant::now();
                        continue;
                    }
                }
                4 => {
                    let n = RESOLUTIONS.len() as isize;
                    let i = ((settings.resolution_index() as isize + dir).rem_euclid(n)) as usize;
                    settings.set_resolution(i);
                    changed = true;
                }
                5 => {
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
        let mut frame = Frame::new(CANVAS_W, CANVAS_H);
        night(&mut frame);
        centered(&mut frame, 30, "SETTINGS", 3, accent());
        let rows = [
            ("CAMERA", CAMERA_LABELS[settings.camera % CAMERA_LABELS.len()].to_string()),
            ("DIFFICULTY", DIFFICULTY_LABELS[settings.difficulty % 3].to_string()),
            ("QUARTERS", settings.quarter_label()),
            ("LENS", settings.lens_label()),
            ("RESOLUTION", settings.resolution_label()),
            ("DISPLAY", settings.display_label().to_string()),
            ("BACK", String::new()),
        ];
        for (i, (name, value)) in rows.iter().enumerate() {
            let y = 96 + i as i32 * 36;
            let on = i == pick;
            if on {
                frame.fill_rect(118, y - 8, 404, 30, panel());
                frame.fill_rect(118, y - 8, 4, 30, accent());
            }
            frame.text_shadowed(138, y, name, 2, if on { paper() } else { dim() }, 1);
            if !value.is_empty() {
                let vx = 500 - text_width(value, 2);
                frame.text_shadowed(vx, y, value, 2, if on { accent() } else { dim() }, 1);
            }
        }
        centered(&mut frame, 258, "CHANGES ARE SAVED AS YOU MAKE THEM", 1, dim());
        footer(&mut frame, "[UP/DOWN] ROW   [LEFT/RIGHT] CHANGE   [B] BACK");
        if !view.show(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

// ---------------------------------------------------------------------------
// Exhibition
// ---------------------------------------------------------------------------

fn ids_in(all: &[Club], division: Division) -> Vec<usize> {
    (0..all.len()).filter(|&i| all[i].division == division).collect()
}

fn exhibition(view: &mut View, pad: &mut Option<&mut Pad>) -> Option<Kickoff> {
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
    let receive = coin_flip(view, pad, &all, yours, theirs, venue)?;
    Some(Kickoff { yours, theirs, receive, venue })
}

/// Everything about a club the picker shows that is not its badge: the numbers
/// and the three men who carry them.
struct Card {
    ratings: TeamRatings,
    stars: Vec<(String, u8)>,
}

/// How many quarter lengths the settings screen can offer.
fn a7fl_cli_quarter_count() -> isize {
    crate::settings::QUARTER_CHOICES.len() as isize
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
                sel = wrap(sel, venues::COUNT, d * GRID_COLS as i8);
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
        let mut frame = Frame::new(CANVAS_W, CANVAS_H);
        frame.gradient_rect(0, 0, CANVAS_W as i32, CANVAS_H as i32, sky[0], sky[2]);
        frame.fill_rect_alpha(0, 0, CANVAS_W as i32, CANVAS_H as i32, Rgb::new(4, 6, 10), 0.62);
        centered(&mut frame, 8, "WHERE IS IT PLAYED", 2, accent());
        let note = format!("{} OF {}", sel + 1, venues::COUNT);
        frame.text_shadowed(
            CANVAS_W as i32 - 14 - text_width(&note, 1),
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
                frame.fill_rect(
                    x - 3,
                    y - 3,
                    THUMB_W as i32 + 6,
                    THUMB_H as i32 + 6,
                    if on { accent() } else { Rgb::new(28, 42, 62) },
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
        centered(&mut frame, 292, &venue.name(), 2, paper());
        centered(&mut frame, 314, &venue.sub(), 1, sky[1].lerp(paper(), 0.5));
        footer(&mut frame, "[ARROWS] GROUND   [A] PLAY   [B] BACK");
        if !view.show(&mut frame, &[]) {
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
        let mut frame = Frame::new(CANVAS_W, CANVAS_H);
        night(&mut frame);
        centered(&mut frame, 16, heading, 2, accent());
        for (i, d) in Division::ALL.iter().enumerate() {
            let x = 118 + i as i32 * 108;
            let on = i == div;
            if on {
                frame.fill_rect(x - 10, 44, 104, 20, panel());
                frame.fill_rect(x - 10, 62, 104, 2, accent());
            }
            frame.text_shadowed(x, 49, d.label(), 1, if on { paper() } else { dim() }, 1);
        }
        slot(&mut frame, all, 96, 150, 62, prev, false);
        slot(&mut frame, all, 544, 150, 62, next, false);
        slot(&mut frame, all, 320 + slide as i32, 152, 104, club, true);
        let (city, name, tone) = match club {
            Some(i) => (
                all[i].city.to_uppercase(),
                all[i].name.to_uppercase(),
                all[i].accent,
            ),
            None => ("RANDOM".to_string(), "ROLL THE DICE".to_string(), accent()),
        };
        centered(&mut frame, 238, &city, 3, paper());
        centered(&mut frame, 272, &name, 2, tone);
        if let Some(i) = club {
            let card = &cards[i];
            let r = card.ratings;
            centered(
                &mut frame,
                292,
                &format!(
                    "OVR {}    OFF {}    DEF {}",
                    r.overall, r.offense, r.defense
                ),
                2,
                paper(),
            );
            for (k, (who, rating)) in card.stars.iter().enumerate() {
                centered(
                    &mut frame,
                    314 + k as i32 * 10,
                    &format!("{who}  {rating}"),
                    1,
                    dim(),
                );
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
            CANVAS_W as i32 - 14 - text_width(&note, 1),
            18,
            &note,
            1,
            note_color,
            1,
        );
        footer(&mut frame, "[LEFT/RIGHT] ROLL   [UP/DOWN] DIVISION   [A] PICK   [B] BACK");
        if !view.show(&mut frame, &[]) {
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

        let mut frame = Frame::new(CANVAS_W, CANVAS_H);
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
        if !view.show(&mut frame, &[]) {
            return None;
        }
        beat();
    }
}

#[cfg(test)]
mod tests {
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
        let mut seen = [false; 4];
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
    fn a_screen_of_badges_draws_inside_the_canvas() {
        let mut frame = Frame::new(CANVAS_W, CANVAS_H);
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
    use a7fl_core::playbook::{defensive_playbook, playbook};
    use a7fl_core::sim::{PlaySim, SnapSetup};
    use a7fl_core::team::league_teams;
    use a7fl_render::broadcast::{Broadcast, CameraMode};

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
    let mut broadcast = Broadcast::new(CANVAS_W, CANVAS_H, [home.clone(), away.clone()]);
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
        if !view.show(&mut frame, &[help]) {
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    view.reset();
}


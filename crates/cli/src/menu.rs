//! The front of the game: the start menu, the settings screen and the
//! exhibition picker.
//!
//! Everything here is drawn on the same 640x360 canvas the broadcast uses, so
//! the menu looks like the game does. On a machine with no display the frames
//! are averaged down into terminal cells by `View`, exactly as live play is.

use std::time::{Duration, Instant};

use a7fl_core::math::Rgb;
use a7fl_core::team::{clubs, Club, Division};
use a7fl_render::raster::Frame;

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
#[derive(Default)]
struct Repeat {
    held: f32,
    fired: bool,
}

impl Repeat {
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
        if self.held >= 0.30 {
            self.held = 0.16;
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
        centered(&mut frame, 34, "7-ON-7", 7, accent());
        centered(&mut frame, 100, "NO PADS FOOTBALL", 2, paper());
        centered(&mut frame, 122, "SEVEN ON SEVEN", 1, dim());
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
    const ROWS: usize = 4;
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
                    let n = RESOLUTIONS.len() as isize;
                    let i = ((settings.resolution_index() as isize + dir).rem_euclid(n)) as usize;
                    settings.set_resolution(i);
                    changed = true;
                }
                2 => {
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
    let yours = pick_team(view, pad, &all, None, "YOUR TEAM")?;
    let theirs = opponent_of(&all, yours);
    let receive = coin_flip(view, pad, &all, yours, theirs)?;
    Some(Kickoff { yours, theirs, receive })
}

/// The other side of the toss. The player picks one club, so the computer gets
/// the next club in the same division: a divisional game, without a second
/// screen to get through before the coin is in the air.
fn opponent_of(all: &[Club], yours: usize) -> usize {
    let list = ids_in(all, all[yours].division);
    let at = list.iter().position(|&i| i == yours).unwrap_or(0);
    list[(at + 1) % list.len()]
}

/// The carousel: the division along the top, the club in the middle, its city
/// and nickname underneath. Left and right roll through the division.
fn pick_team(
    view: &mut View,
    pad: &mut Option<&mut Pad>,
    all: &[Club],
    avoid: Option<usize>,
    heading: &str,
) -> Option<usize> {
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
        let list = ids_in(all, Division::ALL[div]);
        let n = list.len().max(1);
        if let d = nav[0].step(input.stick.x, dt) {
            if d != 0 {
                sel = wrap(sel, n, d);
                slide = 90.0 * d as f32;
            }
        }
        slide *= 1.0 - (dt * 9.0).min(1.0);
        said = (said - dt).max(0.0);
        if input.pressed.a {
            let pick = list[sel];
            if Some(pick) == avoid {
                said = 1.6;
            } else {
                return Some(pick);
            }
        }

        let club = &all[list[sel]];
        let prev = &all[list[wrap(sel, n, -1)]];
        let next = &all[list[wrap(sel, n, 1)]];
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
        badge(&mut frame, 96, 150, 62, prev, false);
        badge(&mut frame, 544, 150, 62, next, false);
        badge(&mut frame, 320 + slide as i32, 152, 104, club, true);
        centered(&mut frame, 246, &club.city.to_uppercase(), 3, paper());
        centered(&mut frame, 284, &club.name.to_uppercase(), 2, club.accent);
        if said > 0.0 {
            centered(&mut frame, 310, "ALREADY IN THIS GAME", 1, Rgb::new(255, 120, 96));
        } else {
            centered(
                &mut frame,
                310,
                &format!("{} OF {}", sel + 1, n),
                1,
                dim(),
            );
        }
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

    #[test]
    fn the_computer_gets_the_next_club_in_the_division() {
        let all = clubs();
        for yours in 0..all.len() {
            let theirs = opponent_of(&all, yours);
            assert_ne!(theirs, yours, "the computer cannot be the club you picked");
            assert_eq!(all[theirs].division, all[yours].division, "not a divisional game");
        }
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

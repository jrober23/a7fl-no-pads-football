//! The design kit the front end and the presentation cards are built from.
//!
//! Everything the player reads outside a play used to be drawn a screen at a
//! time: two greens, one amber, a rectangle per row and the same block of code
//! copied between the settings and the game setup. That is how a menu ends up
//! looking like a list of numbers rather than a piece of software: no scale it
//! repeats, no light, nothing that moves when the stick does.
//!
//! So the surfaces, the type hierarchy and the motion live here instead, once,
//! and the screens are laid out with them. Three rules the whole kit keeps:
//!
//! * **Geometry comes from the frame.** Nothing is placed at a coordinate that
//!   only means something on one canvas: rows, cards and margins are given in
//!   grid pixels and positioned from the frame, so a screen designed at 640x360
//!   is still right if the grid is ever made taller or wider.
//! * **Selection moves.** A cursor that jumps from row to row is the single
//!   biggest reason a hand-drawn menu feels unresponsive: the eye has to find
//!   the thing it just pressed. [`Slide`] eases the highlight toward the row the
//!   stick went to, and the row it left cools down on the way out.
//! * **One accent, used sparingly.** Amber means "you are here, or this is the
//!   number that matters"; everything else is paper, muted or a club's colour.

use nopads_core::math::{clamp, Rgb};

use crate::font;
use crate::raster::Frame;

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

/// The page under everything.
pub const INK: Rgb = Rgb::hex(0x080B11);
/// Body text.
pub const PAPER: Rgb = Rgb::hex(0xE9EFF7);
/// Labels, counts and anything the player is not being asked to read closely.
pub const MUTED: Rgb = Rgb::hex(0x8494A9);
/// The one colour that means "here".
pub const ACCENT: Rgb = Rgb::hex(0xFFC440);
/// A surface: cards, rows, the panel behind a picker.
pub const SURFACE_TOP: Rgb = Rgb::hex(0x18222F);
pub const SURFACE_BOTTOM: Rgb = Rgb::hex(0x0E141D);
/// The hairline that separates a surface from the page.
pub const EDGE: Rgb = Rgb::hex(0x2B3A4C);
/// Warnings, penalties, the thing that went wrong.
pub const DANGER: Rgb = Rgb::hex(0xE4572E);
/// A made kick, a won down.
pub const GOOD: Rgb = Rgb::hex(0x4CD08A);

/// The spacing scale. Every gap on a screen is one of these, so two screens put
/// their contents the same distance apart without anyone counting pixels.
pub const S1: i32 = 4;
pub const S2: i32 = 8;
pub const S3: i32 = 12;
pub const S4: i32 = 16;
pub const S5: i32 = 24;
pub const S6: i32 = 32;

/// The type scale, in glyph sizes: caption, label, body, title, hero.
pub const T_CAPTION: usize = 1;
pub const T_LABEL: usize = 2;
pub const T_BODY: usize = 3;
pub const T_TITLE: usize = 5;
pub const T_HERO: usize = 7;

// ---------------------------------------------------------------------------
// Motion
// ---------------------------------------------------------------------------

/// Fast at the start, settling at the end: the curve a thing takes when it is
/// arriving rather than travelling.
pub fn ease_out(t: f32) -> f32 {
    let t = clamp(t, 0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t)
}

/// Eased at both ends, for something that grows out of where it already was.
pub fn ease_in_out(t: f32) -> f32 {
    let t = clamp(t, 0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let f = -2.0 * t + 2.0;
        1.0 - f * f * f / 2.0
    }
}

/// Walk `current` toward `target` at a rate in "fraction closed per second".
/// Framerate independent: the step is scaled by the frame's own time.
pub fn approach(current: f32, target: f32, rate: f32, dt: f32) -> f32 {
    let k = 1.0 - (-rate * dt.max(0.0)).exp();
    current + (target - current) * k
}

/// A highlight that travels to the row the player picked instead of jumping to
/// it. `at` is in rows, fractional while it is in flight, so a screen can draw
/// the highlight and the row it is leaving in the same frame.
#[derive(Clone, Copy, Debug)]
pub struct Slide {
    pub at: f32,
    /// Rows a second, at the speed the screens use for a stick.
    pub rate: f32,
}

impl Slide {
    /// Start already sitting on `row`: a screen that opens must not slide in
    /// from the top left every time.
    pub fn new(row: usize) -> Self {
        Slide { at: row as f32, rate: 15.0 }
    }

    pub fn step(&mut self, target: usize, dt: f32) {
        self.at = approach(self.at, target as f32, self.rate, dt);
    }

    /// Where the highlight sits in pixels, given the row height and the gap
    /// between rows.
    pub fn y(&self, row_h: i32, gap: i32) -> i32 {
        (self.at * (row_h + gap) as f32).round() as i32
    }

    /// How hot row `row` is: 1 while the highlight is on it, cooling as it
    /// leaves, warming as it arrives. This is what lets two rows be lit at once
    /// for the frame or two the highlight is between them.
    pub fn heat(&self, row: usize) -> f32 {
        (1.0 - (self.at - row as f32).abs()).clamp(0.0, 1.0)
    }
}

/// A staggered entrance: item `i` of `n` arrives over `SPAN` seconds, each one a
/// little after the last, so a list assembles instead of appearing.
pub fn enter(t: f32, i: usize, n: usize) -> f32 {
    const SPAN: f32 = 0.34;
    let n = n.max(1) as f32;
    let start = (i as f32 / n) * 0.14;
    ease_out((t - start) / SPAN)
}

// ---------------------------------------------------------------------------
// Surfaces
// ---------------------------------------------------------------------------

/// The page: a graded sky, a pool of light where the content sits, two faint
/// painted lines, and the edges shaded down.
///
/// Every screen sits on this instead of on a flat fill, which is what made the
/// front end read as a terminal rather than as the game's own front end.
pub fn backdrop(frame: &mut Frame) {
    let w = frame.w as i32;
    let h = frame.h as i32;
    if w <= 0 || h <= 0 {
        return;
    }
    frame.gradient_rect(0, 0, w, h, Rgb::hex(0x17212F), Rgb::hex(0x05080D));
    // The pool: a handful of very faint rounded bands, largest first, so the
    // middle of the screen is lit and the corners fall away.
    for i in (0..9).rev() {
        let t = i as f32 / 9.0;
        let pw = (w as f32 * (0.34 + 0.07 * t)) as i32;
        let ph = (h as f32 * (0.26 + 0.08 * t)) as i32;
        frame.rounded_rect_alpha(
            (w - pw) / 2,
            (h - ph) / 2 - h / 16,
            pw,
            ph,
            (ph / 2).max(8),
            Rgb::hex(0x2F5078),
            0.030,
        );
    }
    // The lines of paint the game is played between, kept quiet enough to read
    // as a background rather than as content.
    frame.fill_rect_alpha(0, (h as f32 * 0.26) as i32, w, 1, Rgb::hex(0x4C6E96), 0.16);
    frame.fill_rect_alpha(0, (h as f32 * 0.74) as i32, w, 1, Rgb::hex(0x4C6E96), 0.16);
    // Vignette: columns then rows, squared so the middle is untouched and the
    // corners take the most.
    for col in 0..w {
        let d = (col as f32 / w as f32 - 0.5).abs() * 2.0;
        let a = d * d * 0.20;
        if a > 0.004 {
            frame.fill_rect_alpha(col, 0, 1, h, Rgb::BLACK, a);
        }
    }
    for row in 0..h {
        let d = (row as f32 / h as f32 - 0.5).abs() * 2.0;
        let a = d * d * 0.24;
        if a > 0.004 {
            frame.fill_rect_alpha(0, row, w, 1, Rgb::BLACK, a);
        }
    }
}

/// A card: a shadow, a hairline, a graded face, and an accent rule across the
/// top. `lit` is 0..1 and lets a card arrive or a selection warm it up.
pub fn card(frame: &mut Frame, x: i32, y: i32, w: i32, h: i32, accent: Rgb, lit: f32) {
    if w <= 0 || h <= 0 {
        return;
    }
    let lit = clamp(lit, 0.0, 1.0);
    let r = 10.min(w / 2).min(h / 2);
    frame.panel(x, y, w, h, r, SURFACE_TOP, SURFACE_BOTTOM, EDGE, 0.34 + 0.2 * lit);
    if lit > 0.01 && w > 24 {
        let bar = ((w - 24) as f32 * lit) as i32;
        frame.fill_rect(x + 12, y + 1, bar, 3, accent);
    }
}

/// A sheet of glass over a live picture: the pause menu and the introduction
/// cards. Translucent on purpose, so the game is still behind them.
pub fn glass(frame: &mut Frame, x: i32, y: i32, w: i32, h: i32, accent: Rgb, alpha: f32) {
    if w <= 0 || h <= 0 {
        return;
    }
    let r = 12;
    let a = clamp(alpha, 0.0, 1.0);
    frame.rounded_rect_alpha(x - 2, y - 1, w + 4, h + 5, r + 2, Rgb::BLACK, a * 0.30);
    frame.rounded_rect_alpha(x - 1, y - 1, w + 2, h + 2, r + 1, EDGE, a * 0.55);
    frame.rounded_rect_alpha(x, y, w, h, r, Rgb::hex(0x1C2533), a);
    frame.fill_rect(x + 12, y + 5, w - 24, 4, accent);
}

/// A hairline, inset from both ends of the row it separates.
pub fn divider(frame: &mut Frame, x: i32, y: i32, w: i32) {
    frame.fill_rect_alpha(x, y, w, 1, EDGE, 0.9);
}

// ---------------------------------------------------------------------------
// Rows, chips and meters
// ---------------------------------------------------------------------------

/// One row of a settings list: a label on the left, a value on the right, and a
/// highlight whose heat comes from [`Slide::heat`].
pub fn row(
    frame: &mut Frame,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    label: &str,
    value: Option<&str>,
    heat: f32,
    accent: Rgb,
) {
    let heat = clamp(heat, 0.0, 1.0);
    if heat > 0.01 {
        frame.rounded_rect_alpha(x, y, w, h, 8, Rgb::hex(0x2A3A4D), 0.50 + 0.40 * heat);
        frame.rounded_rect_alpha(x, y, w, h, 8, accent, 0.045 * heat);
    }
    if heat > 0.02 {
        let bar = (h - 12).max(4);
        frame.rounded_rect(x, y + (h - bar) / 2, 3, bar, 1, accent.lerp(SURFACE_TOP, 1.0 - heat));
    }
    let ty = y + (h - font::text_height(T_LABEL) as i32) / 2;
    let ink = MUTED.lerp(PAPER, heat);
    font::draw_styled(frame, x + S4, ty, label, T_LABEL, ink, 1, false);
    if let Some(v) = value {
        if !v.is_empty() {
            let vx = x + w - S4 - font::text_width(v, T_LABEL) as i32;
            let vink = MUTED.lerp(accent, heat);
            font::draw_styled(frame, vx, ty, v, T_LABEL, vink, 1, false);
        }
    }
}

/// The width [`chip`] will take, so a row of chips can be laid out before any
/// of them is drawn.
pub fn chip_width(label: &str) -> i32 {
    font::text_width(label, T_CAPTION) as i32 + (S2 + S1 / 2) * 2
}

/// An action pill. Returns the width it took, so a caller can lay a row of them
/// out by asking for each in turn.
pub fn chip(frame: &mut Frame, x: i32, y: i32, label: &str, heat: f32, accent: Rgb) -> i32 {
    let heat = clamp(heat, 0.0, 1.0);
    let pad = S2 + S1 / 2;
    let w = chip_width(label);
    let h = 18;
    if heat > 0.01 {
        frame.rounded_rect(x, y, w, h, h / 2, accent.lerp(SURFACE_TOP, 1.0 - heat));
        frame.rounded_rect_alpha(x, y, w, h, h / 2, accent, 0.10 * heat);
    } else {
        frame.rounded_rect(x, y, w, h, h / 2, SURFACE_TOP);
        frame.rounded_rect_alpha(x, y, w, h, h / 2, EDGE, 0.9);
        frame.rounded_rect(x + 1, y + 1, w - 2, h - 2, h / 2 - 1, SURFACE_BOTTOM);
    }
    let ink = if heat > 0.5 { Rgb::hex(0x11161D) } else { MUTED.lerp(PAPER, heat) };
    font::draw_styled(frame, x + pad, y + (h - font::text_height(T_CAPTION) as i32) / 2, label, T_CAPTION, ink, 1, false);
    w
}

/// A small label with a hairline and no fill, for counts and states.
pub fn tag(frame: &mut Frame, x: i32, y: i32, text: &str, color: Rgb) -> i32 {
    let w = font::text_width(text, T_CAPTION) as i32 + S3;
    let h = font::text_height(T_CAPTION) as i32 + 6;
    frame.rounded_rect_alpha(x, y, w, h, h / 2, color, 0.16);
    font::draw_styled(frame, x + S3 / 2, y + 3, text, T_CAPTION, color, 1, false);
    w
}

/// A bar that fills to `t` (0..1): a rating, a play clock, a season's progress.
pub fn meter(frame: &mut Frame, x: i32, y: i32, w: i32, h: i32, t: f32, color: Rgb) {
    if w <= 0 || h <= 0 {
        return;
    }
    let t = clamp(t, 0.0, 1.0);
    frame.rounded_rect(x, y, w, h, h / 2, Rgb::hex(0x22303F));
    let fw = (w as f32 * t).round() as i32;
    if fw >= 2 {
        frame.rounded_rect(x, y, fw.max(h), h, h / 2, color);
    }
}

// ---------------------------------------------------------------------------
// Type
// ---------------------------------------------------------------------------

/// The width of `text` with `track` extra pixels between each pair of letters.
pub fn tracked_width(text: &str, scale: usize, track: i32) -> i32 {
    let n = text.chars().count() as i32;
    if n == 0 {
        0
    } else {
        font::text_width(text, scale) as i32 + track * (n - 1)
    }
}

/// A heading, letter-spaced and centred on `cx`. Wide tracking on short words is
/// what makes a caption read as a designed label rather than as a sentence.
pub fn heading(frame: &mut Frame, cx: i32, y: i32, text: &str, scale: usize, color: Rgb) {
    let track = (scale as i32 + 1) / 2;
    let mut pen = cx - tracked_width(text, scale, track) / 2;
    for ch in text.chars() {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        font::draw_styled(frame, pen, y, s, scale, color, 1, false);
        pen += font::text_width(s, scale) as i32 + track;
    }
}

/// Small caps above a block: muted, tracked, and the thing that tells the eye
/// what the numbers under it are.
pub fn caption(frame: &mut Frame, x: i32, y: i32, text: &str, color: Rgb) {
    let track = 1;
    let mut pen = x;
    for ch in text.chars() {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        font::draw_styled(frame, pen, y, s, T_CAPTION, color, 1, false);
        pen += font::text_width(s, T_CAPTION) as i32 + track;
    }
}

// ---------------------------------------------------------------------------
// The key bar
// ---------------------------------------------------------------------------

/// One hint: what to press, and what it does.
pub struct Hint<'a> {
    pub key: &'a str,
    pub label: &'a str,
}

/// How much room a set of hints takes, so a caller can decide whether to drop
/// the last one rather than let the bar run off the edge.
pub fn hints_width(hints: &[Hint<'_>]) -> i32 {
    let mut total = 0;
    for (i, h) in hints.iter().enumerate() {
        if i > 0 {
            total += S3 + S2;
        }
        total += hint_width(h);
    }
    total
}

fn hint_width(h: &Hint<'_>) -> i32 {
    let label = font::text_width(h.label, T_CAPTION) as i32;
    if h.key.is_empty() {
        // A phrase with no control on it ("WAITING ON THE COIN") is a label,
        // not an empty keycap with a gap where the key should be.
        return label;
    }
    font::text_width(h.key, T_CAPTION) as i32 + S2 + S1 / 2 + S1 + label
}

/// Split a footer written the way a manual writes a control -- "[STICK] CHOOSE
/// [A] SELECT" -- into keycaps for the bar. Every screen already writes its
/// controls that way, so they all get the new bar from one parser.
pub fn parse_hints(text: &str) -> Vec<Hint<'_>> {
    let mut out = Vec::new();
    for chunk in text.split("   ") {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        if let Some(rest) = chunk.strip_prefix('[') {
            if let Some((key, label)) = rest.split_once(']') {
                out.push(Hint { key: key.trim(), label: label.trim() });
                continue;
            }
        }
        out.push(Hint { key: "", label: chunk });
    }
    out
}

/// The bar along the bottom of a screen: a keycap per control, and its label.
/// Centred, and it never draws a hint it cannot fit.
pub fn hints(frame: &mut Frame, cx: i32, y: i32, hints: &[Hint<'_>]) {
    // Trim from the end until the set fits the frame.
    let room = frame.w as i32 - S6 * 2;
    let mut shown = hints.len();
    while shown > 0 && hints_width(&hints[..shown]) > room {
        shown -= 1;
    }
    let hints = &hints[..shown];
    let mut pen = cx - hints_width(hints) / 2;
    for (i, h) in hints.iter().enumerate() {
        if i > 0 {
            pen += S3 + S2;
        }
        if !h.key.is_empty() {
            let kw = font::text_width(h.key, T_CAPTION) as i32 + S2 + S1 / 2;
            let kh = font::text_height(T_CAPTION) as i32 + 6;
            frame.rounded_rect(pen, y, kw, kh, 4, Rgb::hex(0x273445));
            frame.rounded_rect(pen + 1, y + 1, kw - 2, kh - 2, 3, Rgb::hex(0x1A2431));
            font::draw_styled(
                frame,
                pen + (kw - font::text_width(h.key, T_CAPTION) as i32) / 2,
                y + 3,
                h.key,
                T_CAPTION,
                PAPER,
                1,
                false,
            );
            pen += kw + S1;
        }
        font::draw_styled(frame, pen, y + 3, h.label, T_CAPTION, MUTED, 1, false);
        pen += font::text_width(h.label, T_CAPTION) as i32;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_highlight_travels_and_passes_between_the_rows() {
        let mut slide = Slide::new(0);
        let mut between = false;
        for _ in 0..30 {
            slide.step(3, 1.0 / 60.0);
            if slide.at > 0.2 && slide.at < 2.8 {
                between = true;
            }
        }
        assert!(between, "the highlight jumped instead of travelling");
        assert!(slide.heat(3) > 0.95, "it never arrived: {}", slide.at);
        assert!(slide.heat(0) < 0.05, "the old row is still lit");
        // Mid-flight both rows are lit, which is the point of the heat model.
        let mut mid = Slide::new(0);
        for _ in 0..5 {
            mid.step(1, 1.0 / 60.0);
        }
        assert!(mid.heat(0) > 0.0 && mid.heat(1) > 0.0, "no crossfade");
    }

    #[test]
    fn a_slide_is_framerate_independent() {
        let mut fast = Slide::new(0);
        for _ in 0..120 {
            fast.step(5, 1.0 / 120.0);
        }
        let mut slow = Slide::new(0);
        for _ in 0..60 {
            slow.step(5, 1.0 / 60.0);
        }
        assert!(
            (fast.at - slow.at).abs() < 0.02,
            "{} against {} after the same second",
            fast.at,
            slow.at
        );
    }

    #[test]
    fn a_list_assembles_instead_of_appearing() {
        let n = 5;
        assert_eq!(enter(0.0, 0, n), 0.0);
        assert!(enter(0.02, 0, n) > 0.0, "the first row never arrives");
        assert!(
            enter(0.02, 0, n) > enter(0.02, n - 1, n),
            "the rows do not stagger"
        );
        assert!(enter(1.0, n - 1, n) > 0.99, "the last row never lands");
    }

    #[test]
    fn a_row_is_lit_only_where_the_highlight_is() {
        let mut frame = Frame::new(320, 120);
        frame.clear(SURFACE_BOTTOM);
        for i in 0..3 {
            let heat = if i == 1 { 1.0 } else { 0.0 };
            row(&mut frame, 20, 10 + i as i32 * 30, 280, 26, "CAMERA", Some("BROADCAST"), heat, ACCENT);
        }
        // The lit row carries the accent bar at its left edge; the others do not.
        // Row 1 sits at y 40, and its bar is the middle 14 pixels of the 26.
        assert_eq!(frame.get(20, 48), ACCENT, "the selected row has no accent");
        assert_ne!(frame.get(24, 48), ACCENT, "the bar is wider than it should be");
        assert_ne!(frame.get(20, 18), ACCENT, "an unselected row is lit");
        // And the label of the plain row is still legible against the surface.
        assert!(MUTED.luma() > 60.0 && PAPER.luma() > 200.0);
    }

    #[test]
    fn the_key_bar_drops_hints_it_cannot_fit() {
        let bar = [
            Hint { key: "STICK", label: "CHOOSE" },
            Hint { key: "LEFT/RIGHT", label: "CHANGE" },
            Hint { key: "A", label: "SELECT" },
            Hint { key: "B", label: "BACK" },
            Hint { key: "START", label: "QUIT" },
        ];
        let mut wide = Frame::new(640, 360);
        wide.clear(SURFACE_BOTTOM);
        hints(&mut wide, 320, 336, &bar);
        let painted = |f: &Frame| {
            (330..350)
                .flat_map(|y| (0..640).map(move |x| (y, x)))
                .filter(|(y, x)| f.get(*x as i32, *y as i32) != SURFACE_BOTTOM)
                .count()
        };
        let full = painted(&wide);
        assert!(full > 400, "the key bar is empty: {full} px");

        let mut narrow = Frame::new(220, 120);
        narrow.clear(SURFACE_BOTTOM);
        hints(&mut narrow, 110, 96, &bar);
        // Nothing is drawn past either edge: the bar trimmed itself instead.
        for x in [0i32, 219] {
            for y in 90..120 {
                assert_eq!(narrow.get(x, y), SURFACE_BOTTOM, "the bar ran off at {x}");
            }
        }
    }

    #[test]
    fn a_card_sits_on_the_page_with_its_accent_rule() {
        let mut frame = Frame::new(300, 200);
        frame.clear(INK);
        card(&mut frame, 40, 40, 220, 120, ACCENT, 1.0);
        // The rule runs across the top, inset from both corners.
        assert_eq!(frame.get(60, 42), ACCENT, "the accent rule is missing");
        assert_eq!(frame.get(40 + 220 - 20, 42), ACCENT, "the rule stops short");
        // The face is a surface, not the page.
        assert_ne!(frame.get(150, 100), INK, "the card is a hole");
        // The shadow falls outside the card and inside the frame.
        assert_ne!(frame.get(41, 41 + 120 + 2), INK, "no shadow under the card");
    }

    #[test]
    fn the_backdrop_is_darker_at_the_corners_than_in_the_middle() {
        let mut frame = Frame::new(320, 180);
        backdrop(&mut frame);
        let luma = |x: i32, y: i32| frame.get(x, y).luma();
        let middle = luma(160, 90);
        let corner = luma(2, 2);
        assert!(
            middle > corner + 3.0,
            "the page is flat: middle {middle:.1} corner {corner:.1}"
        );
    }

    #[test]
    fn a_meter_fills_from_the_left_and_never_past_its_track() {
        let mut frame = Frame::new(140, 30);
        frame.clear(SURFACE_BOTTOM);
        meter(&mut frame, 10, 10, 100, 8, 0.5, GOOD);
        assert_eq!(frame.get(11, 14), GOOD, "the fill is missing");
        assert_ne!(frame.get(90, 14), GOOD, "it filled past its value");
        assert_eq!(frame.get(109, 14), SURFACE_BOTTOM, "it left its track");
    }
}

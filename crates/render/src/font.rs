//! A 5x7 bitmap font used for the broadcast overlay and the painted field numbers.
//!
//! Deliberately tiny and dependency free: glyphs are 7 rows of 5 bits so a whole
//! font fits in a few hundred bytes and scales crisply to any size.

use a7fl_core::math::Rgb;

use crate::raster::Frame;

pub const GLYPH_W: usize = 5;
pub const GLYPH_H: usize = 7;

/// Rows are 5 bits wide, most significant bit leftmost.
pub fn glyph(c: char) -> [u8; GLYPH_H] {
    match c.to_ascii_uppercase() {
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        '-' => [0, 0, 0, 0b11111, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 0b11111],
        '.' => [0, 0, 0, 0, 0, 0b01100, 0b01100],
        ',' => [0, 0, 0, 0, 0b01100, 0b01100, 0b01000],
        ':' => [0, 0b01100, 0b01100, 0, 0b01100, 0b01100, 0],
        ';' => [0, 0b01100, 0b01100, 0, 0b01100, 0b01100, 0b01000],
        '/' => [0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000],
        '\\' => [0b10000, 0b01000, 0b01000, 0b00100, 0b00010, 0b00010, 0b00001],
        '%' => [0b11000, 0b11001, 0b00010, 0b00100, 0b01000, 0b10011, 0b00011],
        '!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0, 0b00100],
        '?' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0, 0b00100],
        '&' => [0b01100, 0b10010, 0b10100, 0b01000, 0b10101, 0b10010, 0b01101],
        '(' => [0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010],
        ')' => [0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000],
        '+' => [0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0],
        '=' => [0, 0, 0b11111, 0, 0b11111, 0, 0],
        '<' => [0b00010, 0b00100, 0b01000, 0b10000, 0b01000, 0b00100, 0b00010],
        '>' => [0b01000, 0b00100, 0b00010, 0b00001, 0b00010, 0b00100, 0b01000],
        '"' => [0b01010, 0b01010, 0, 0, 0, 0, 0],
        '\'' => [0b00100, 0b00100, 0, 0, 0, 0, 0],
        '#' => [0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010],
        '*' => [0, 0b00100, 0b10101, 0b01110, 0b10101, 0b00100, 0],
        '@' => [0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01110],
        '$' => [0b00100, 0b01111, 0b10100, 0b01110, 0b00101, 0b11110, 0b00100],
        '^' => [0b00100, 0b01010, 0b10001, 0, 0, 0, 0],
        '|' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        '~' => [0, 0, 0b01000, 0b10101, 0b00010, 0, 0],
        '[' => [0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110],
        ']' => [0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110],
        _ => [0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111],
    }
}

/// Text rotated 90 degrees, used for painted yard numbers and end zone wordmarks.
/// `dir` of 1 runs the text toward `+y`, -1 mirrors it.
pub fn draw_rot90(
    frame: &mut Frame,
    x: i32,
    y: i32,
    text: &str,
    scale: usize,
    color: Rgb,
    dir: i32,
    weight: usize,
) {
    let scale = scale.max(1) as i32;
    let mut pen = y;
    for ch in text.chars() {
        let rows = glyph(ch);
        for (ry, bits) in rows.iter().enumerate() {
            for rx in 0..GLYPH_W {
                if bits & (1 << (GLYPH_W - 1 - rx)) != 0 {
                    let (px, py) = if dir >= 0 {
                        (x + (GLYPH_H as i32 - 1 - ry as i32) * scale, pen + rx as i32 * scale)
                    } else {
                        (x + ry as i32 * scale, pen + (GLYPH_W as i32 - 1 - rx as i32) * scale)
                    };
                    for w in 0..weight.max(1) as i32 {
                        frame.fill_rect(px + w, py, scale, scale, color);
                    }
                }
            }
        }
        pen += (GLYPH_W as i32 + 1) * scale;
    }
}

pub fn text_width(text: &str, scale: usize) -> usize {
    if text.is_empty() {
        return 0;
    }
    text.chars().count() * (GLYPH_W + 1) * scale - scale
}

pub fn text_height(scale: usize) -> usize {
    GLYPH_H * scale
}

/// Draw `text` with its top left corner at `(x, y)`.
pub fn draw(frame: &mut Frame, x: i32, y: i32, text: &str, scale: usize, color: Rgb) {
    draw_styled(frame, x, y, text, scale, color, 1, false);
}

/// `weight` of 2 or more thickens strokes; `italic` slants the glyph.
pub fn draw_styled(
    frame: &mut Frame,
    x: i32,
    y: i32,
    text: &str,
    scale: usize,
    color: Rgb,
    weight: usize,
    italic: bool,
) {
    let scale = scale.max(1);
    let mut pen = x;
    for ch in text.chars() {
        let rows = glyph(ch);
        for (ry, bits) in rows.iter().enumerate() {
            for rx in 0..GLYPH_W {
                if bits & (1 << (GLYPH_W - 1 - rx)) != 0 {
                    let slant = if italic { (GLYPH_H - 1 - ry) as i32 * scale as i32 / 3 } else { 0 };
                    let px = pen + rx as i32 * scale as i32 + slant;
                    let py = y + ry as i32 * scale as i32;
                    for oy in 0..weight as i32 {
                        for ox in 0..weight as i32 {
                            frame.fill_rect(
                                px + ox * scale as i32,
                                py,
                                scale as i32,
                                scale as i32,
                                color,
                            );
                            let _ = oy;
                        }
                    }
                }
            }
        }
        pen += (GLYPH_W + 1) as i32 * scale as i32;
    }
}

/// Text with a soft drop shadow, used all over the broadcast package.
pub fn draw_shadowed(
    frame: &mut Frame,
    x: i32,
    y: i32,
    text: &str,
    scale: usize,
    color: Rgb,
    weight: usize,
) {
    let shadow = Rgb::new(0, 0, 0);
    draw_styled(frame, x + scale as i32 * 2, y + scale as i32 * 2, text, scale, shadow, weight, false);
    draw_styled(frame, x, y, text, scale, color, weight, false);
}

pub fn draw_centered(frame: &mut Frame, cx: i32, y: i32, text: &str, scale: usize, color: Rgb, weight: usize) {
    let w = text_width(text, scale) as i32;
    draw_styled(frame, cx - w / 2, y, text, scale, color, weight, false);
}

pub fn draw_right(frame: &mut Frame, right: i32, y: i32, text: &str, scale: usize, color: Rgb, weight: usize) {
    let w = text_width(text, scale) as i32;
    draw_styled(frame, right - w, y, text, scale, color, weight, false);
}

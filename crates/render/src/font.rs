//! A 5x7 bitmap font used for the broadcast overlay and the painted field numbers.
//!
//! Deliberately tiny and dependency free: glyphs are 7 rows of 5 bits so a whole
//! font fits in a few hundred bytes and scales crisply to any size.

use nopads_core::math::Rgb;

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

/// Text laid flat on the ground: `dir` runs the word along `x` (+1 or -1) and
/// `up` points the top of the letters along `y` (+1 or -1).
///
/// The painted yard numbers use it. They stand with their feet on the sideline
/// and their tops toward midfield, running down the field, which is what a fan
/// sitting in that stand reads left to right.
pub fn draw_flat(
    frame: &mut Frame,
    x: i32,
    y: i32,
    text: &str,
    scale: usize,
    color: Rgb,
    dir: i32,
    up: i32,
    weight: usize,
) {
    let scale = scale.max(1) as i32;
    let mut pen = 0i32;
    for ch in text.chars() {
        let rows = glyph(ch);
        for (ry, bits) in rows.iter().enumerate() {
            for rx in 0..GLYPH_W {
                if bits & (1 << (GLYPH_W - 1 - rx)) != 0 {
                    let gx = pen + rx as i32 * scale;
                    let gy = (GLYPH_H as i32 - 1 - ry as i32) * scale;
                    let px = if dir >= 0 { x + gx } else { x - gx - scale };
                    let py = if up >= 0 { y + gy } else { y - gy - scale };
                    for w in 0..weight.max(1) as i32 {
                        let off = if dir >= 0 { w } else { -w };
                        frame.fill_rect(px + off, py, scale, scale, color);
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
///
/// Anything above one pixel a stroke goes through the smoothed path: a 5x7 glyph
/// blown up three times is a staircase of whole pixels, and a staircase of whole
/// pixels is what made this read as an eight-bit menu rather than a broadcast.
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
    if scale > 1 && !text.is_empty() {
        draw_antialiased(frame, x, y, text, scale, color, weight, italic);
        return;
    }
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


#[cfg(test)]
mod tests {
    use super::*;

    fn ink(frame: &Frame, x: i32, y: i32) -> bool {
        frame.get(x, y) != Rgb::new(0, 0, 0)
    }

    #[test]
    fn flat_text_stands_on_its_feet_and_runs_the_way_it_is_told() {
        let mut frame = Frame::new(40, 40);
        // Reading toward -x with the tops of the letters toward -y: how a number
        // painted for the near sideline is laid on the turf.
        draw_flat(&mut frame, 30, 20, "L", 2, Rgb::new(255, 0, 0), -1, -1, 1);
        // The stem is the glyph's first column, at the far end of the word.
        assert!(ink(&frame, 28, 6), "the top of the stem is missing");
        // The foot runs along the baseline back toward where the word starts.
        assert!(ink(&frame, 20, 18), "the foot of the L is missing");
        assert!(!ink(&frame, 20, 6), "the L painted a foot it does not have");
    }
}

/// Draw a glyph run through a bilinear filter.
///
/// Supersampling cannot smooth a bitmap: every block edge lands exactly on a
/// destination pixel boundary, so every sample is all-ink or all-empty and the
/// coverage comes back 0 or 1. Filtering can. The run is laid out at one pixel per
/// glyph pixel and sampled at each destination pixel's centre, which leaves
/// two-level ramps along every edge — the difference between type that reads as a
/// blown-up bitmap and type that reads as drawn.
fn draw_antialiased(
    frame: &mut Frame,
    x: i32,
    y: i32,
    text: &str,
    scale: usize,
    color: Rgb,
    weight: usize,
    italic: bool,
) {
    let chars = text.chars().count();
    if chars == 0 {
        return;
    }
    let gw = chars * (GLYPH_W + 1) - 1;
    let gh = GLYPH_H;
    let mut ink = vec![0f32; gw * gh];

    let mut pen = 0usize;
    for ch in text.chars() {
        let rows = glyph(ch);
        for (ry, bits) in rows.iter().enumerate() {
            for rx in 0..GLYPH_W {
                if bits & (1 << (GLYPH_W - 1 - rx)) == 0 {
                    continue;
                }
                let slant = if italic { (gh - 1 - ry) / 3 } else { 0 };
                for w in 0..weight.max(1) {
                    let cx = pen + rx + slant + w;
                    if cx < gw {
                        ink[ry * gw + cx] = 1.0;
                    }
                }
            }
        }
        pen += GLYPH_W + 1;
    }

    let w = text_width(text, scale);
    let h = text_height(scale);
    let sample = |ix: i32, iy: i32| -> f32 {
        if ix < 0 || iy < 0 || ix >= gw as i32 || iy >= gh as i32 {
            0.0
        } else {
            ink[iy as usize * gw + ix as usize]
        }
    };
    let step = scale as f32;
    // Hard coverage first: the glyph pixel under each destination pixel.
    let mut cov = vec![0f32; w * h];
    for dy in 0..h {
        let gy = ((dy as f32 + 0.5) / step).floor() as i32;
        for dx in 0..w {
            let gx = ((dx as f32 + 0.5) / step).floor() as i32;
            cov[dy * w + dx] = sample(gx, gy);
        }
    }
    // Then soften only the boundary. Area sampling a bitmap at a whole-number
    // scale cannot give partial coverage on its own (the blocks line up), and
    // blurring the whole run turns a one-pixel stroke into grey soup. Smoothing
    // just the pixels that sit on a coverage edge leaves the stroke solid and
    // softens its outline, which is what drawn type looks like.
    let hard = cov.clone();
    let at = |x: i32, y: i32| -> f32 {
        if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
            0.0
        } else {
            hard[y as usize * w + x as usize]
        }
    };
    for dy in 0..h as i32 {
        for dx in 0..w as i32 {
            let c = at(dx, dy);
            let around =
                (at(dx - 1, dy) + at(dx + 1, dy) + at(dx, dy - 1) + at(dx, dy + 1)) * 0.25;
            if (around - c).abs() > 0.01 {
                cov[dy as usize * w + dx as usize] = c + (around - c) * 0.5;
            }
        }
    }
    for dy in 0..h {
        for dx in 0..w {
            let coverage = cov[dy * w + dx];
            if coverage <= 0.02 {
                continue;
            }
            frame.blend(x + dx as i32, y + dy as i32, color, coverage.min(1.0));
        }
    }
}

#[cfg(test)]
mod smooth_tests {
    use super::*;

    #[test]
    fn a_blown_up_glyph_lands_between_full_and_empty_on_its_edges() {
        let mut frame = Frame::new(160, 40);
        let ink = Rgb::new(255, 255, 255);
        draw_styled(&mut frame, 4, 4, "AX", 3, ink, 1, false);
        let partial = frame
            .color
            .iter()
            .filter(|c| c.r > 0 && c.r < 255)
            .count();
        assert!(
            partial > 8,
            "the type is still a staircase: only {partial} partial pixels"
        );
        // The inside of a stroke is still solid ink, not a grey wash.
        let solid = frame.color.iter().filter(|c| c.r == 255).count();
        assert!(solid > 40, "the strokes went grey: {solid} solid pixels");
    }

    #[test]
    fn smoothing_keeps_the_text_where_the_crisp_path_puts_it() {
        let mut smooth = Frame::new(120, 30);
        let mut crisp = Frame::new(120, 30);
        draw_styled(&mut smooth, 6, 6, "BOS 12", 3, Rgb::new(255, 0, 0), 1, false);
        // The same string at 1x, then tripled, is where the layout came from.
        draw_styled(&mut crisp, 6, 6, "BOS 12", 1, Rgb::new(255, 0, 0), 1, false);
        let painted = |f: &Frame| f.color.iter().filter(|c| c.r > 60).count();
        assert!(painted(&smooth) > painted(&crisp) * 4, "the layout drifted");
        assert_eq!(text_width("BOS 12", 3), 6 * 6 * 3 - 3);
    }
}

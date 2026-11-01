//! Terminal output: a frame as ANSI truecolour blocks, so the game can be
//! watched — and played — over a plain terminal, with no window and no GPU.
//!
//! Each character cell shows two pixels stacked vertically using the upper half
//! block, with the top pixel as the foreground colour and the bottom pixel as the
//! background. Two pixels per cell keeps the picture roughly square, because a
//! character cell is about twice as tall as it is wide.

use std::fmt::Write as _;

use a7fl_core::math::Rgb;

use crate::raster::Frame;

/// Terminal dimensions in character cells.
#[derive(Clone, Copy, Debug)]
pub struct Screen {
    pub cols: usize,
    pub rows: usize,
}

impl Screen {
    /// Read `COLUMNS` and `LINES`, falling back to a 110x40 window.
    pub fn fit() -> Self {
        let cols = std::env::var("COLUMNS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(110);
        let rows = std::env::var("LINES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(40);
        Screen { cols: cols.max(48), rows: rows.max(18) }
    }

    /// How many cell rows the 16:9 picture gets, leaving room for a text HUD.
    pub fn image_rows(&self) -> usize {
        let ideal = self.cols * 9 / 32;
        ideal.min(self.rows.saturating_sub(7)).max(8)
    }
}

/// Hide the cursor and clear the screen.
pub fn enter() -> &'static str {
    "\x1b[?25l\x1b[2J\x1b[H"
}

/// Show the cursor again and leave the screen clean.
pub fn leave() -> &'static str {
    "\x1b[0m\x1b[?25h\x1b[2J\x1b[H"
}

/// Move the cursor home without clearing (cheaper flicker-free redraw).
pub fn home() -> &'static str {
    "\x1b[H"
}

/// Rasterise `frame` into `cols` x `rows` cells of escape-coded blocks.
pub fn to_ansi(frame: &Frame, cols: usize, rows: usize) -> String {
    let cols = cols.max(2);
    let rows = rows.max(1);
    let mut out = String::with_capacity(cols * rows * 24);
    for row in 0..rows {
        for col in 0..cols {
            let top = sample_box(frame, cols, rows, col, row, 0);
            let bottom = sample_box(frame, cols, rows, col, row, 1);
            let _ = write!(
                out,
                "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m\u{2580}",
                top.r, top.g, top.b, bottom.r, bottom.g, bottom.b
            );
        }
        out.push_str("\x1b[0m");
        if row + 1 < rows {
            out.push_str("\x1b[K\r\n");
        }
    }
    out
}

/// Average colour of the sub-cell for character cell (`col`, `row`). `half` is 0
/// for the upper pixel of the cell and 1 for the lower one.
fn sample_box(frame: &Frame, cols: usize, rows: usize, col: usize, row: usize, half: usize) -> Rgb {
    let cw = frame.w as f32 / cols as f32;
    let ch = frame.h as f32 / (rows * 2) as f32;
    let x0 = col as f32 * cw;
    let y0 = (row * 2 + half) as f32 * ch;
    let x1 = (x0 + cw).max(x0 + 1.0);
    let y1 = (y0 + ch).max(y0 + 1.0);
    // At most an 8x8 box, stepping if the cell covers more pixels than that.
    let step_x = (((x1 - x0) / 8.0).ceil()).max(1.0);
    let step_y = (((y1 - y0) / 8.0).ceil()).max(1.0);
    let mut acc = [0u32; 3];
    let mut n = 0u32;
    let mut y = y0;
    while y < y1 {
        let mut x = x0;
        while x < x1 {
            let c = frame.get(x as i32, y as i32);
            acc[0] += c.r as u32;
            acc[1] += c.g as u32;
            acc[2] += c.b as u32;
            n += 1;
            x += step_x;
        }
        y += step_y;
    }
    let n = n.max(1);
    Rgb::new((acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8)
}

/// A still picture with a text HUD underneath, ready to print.
pub fn picture_and_text(frame: &Frame, screen: Screen, lines: &[String]) -> String {
    let mut out = String::from(home());
    out.push_str(&to_ansi(frame, screen.cols, screen.image_rows()));
    out.push_str("\x1b[K\r\n");
    for line in lines {
        let clipped: String = line.chars().take(screen.cols).collect();
        out.push_str(&clipped);
        out.push_str("\x1b[K\r\n");
    }
    out.push_str("\x1b[J");
    out
}

/// Text only: menus, results and prompts.
pub fn text_page(screen: Screen, lines: &[String]) -> String {
    let mut out = String::from(home());
    for line in lines {
        let clipped: String = line.chars().take(screen.cols).collect();
        out.push_str(&clipped);
        out.push_str("\x1b[K\r\n");
    }
    out.push_str("\x1b[J");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_grid_matches_requested_shape() {
        let mut frame = Frame::new(40, 20);
        frame.clear(Rgb::hex(0x336633));
        let ansi = to_ansi(&frame, 10, 5);
        assert_eq!(ansi.matches("\x1b[38;2;").count(), 50);
        assert!(ansi.contains('\u{2580}'));
        assert!(ansi.contains("51;102;51"));
        // Four separators between five rows.
        assert_eq!(ansi.matches("\r\n").count(), 4);
        let first = ansi.find('\x1b').unwrap();
        assert_eq!(first, 0);
    }

    #[test]
    fn picture_page_keeps_text_within_the_width() {
        let frame = Frame::new(32, 18);
        let screen = Screen { cols: 20, rows: 20 };
        let page = picture_and_text(&frame, screen, &["x".repeat(60)]);
        let last = page.lines().last().unwrap_or("");
        assert!(last.len() <= 20);
    }
}

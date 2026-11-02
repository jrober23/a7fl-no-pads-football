//! Where the game is drawn: a real window when the machine has a display, and
//! ANSI colour blocks in the terminal when it does not.
//!
//! Everything is drawn on a fixed 640x360 canvas. The window scales that canvas
//! up to whatever the desktop gives us (2x by default, resizable), the terminal
//! averages it down into character cells. Neither path needs a GPU.

use std::io::{self, Write};

use a7fl_core::math::{Rgb, Vec2};
use a7fl_render::raster::Frame;
use a7fl_render::terminal::{self, Screen};

use crate::pad::{Buttons, Input};
use crate::settings::Settings;

pub const CANVAS_W: usize = 640;
pub const CANVAS_H: usize = 360;

fn paper() -> Rgb {
    Rgb::new(232, 238, 246)
}

fn accent() -> Rgb {
    Rgb::new(255, 196, 64)
}

fn backdrop() -> Rgb {
    Rgb::new(12, 16, 26)
}

pub enum View {
    Window(WindowView),
    Term(Screen),
}

impl View {
    /// Try a window first; fall back to the terminal when there is no display.
    pub fn open(force_terminal: bool, settings: &Settings) -> View {
        if !force_terminal {
            if let Some(window) = WindowView::open(settings) {
                return View::Window(window);
            }
        }
        View::Term(Screen::fit())
    }

    pub fn is_window(&self) -> bool {
        matches!(self, View::Window(_))
    }

    /// Rebuild the window at the size and mode the settings ask for. minifb has
    /// no resize call, so the window is made again rather than nudged.
    pub fn apply(&mut self, settings: &Settings) {
        if !self.is_window() {
            return;
        }
        if let Some(window) = WindowView::open(settings) {
            *self = View::Window(window);
        }
    }

    pub fn describe(&self) -> String {
        match self {
            View::Window(w) => w.describe(),
            View::Term(screen) => format!("terminal {}x{}", screen.cols, screen.rows),
        }
    }

    /// Show a frame with a text bar along the bottom. Returns false when the
    /// player closed the window.
    pub fn show(&mut self, frame: &mut Frame, lines: &[String]) -> bool {
        match self {
            View::Window(window) => {
                if frame.w == CANVAS_W && frame.h == CANVAS_H {
                    draw_hud(frame, lines);
                }
                window.blit(frame)
            }
            View::Term(screen) => {
                print!("{}", terminal::picture_and_text(frame, *screen, lines));
                io::stdout().flush().ok();
                true
            }
        }
    }

    /// A full screen of text: the call sheet, a prompt, the box score.
    pub fn page(&mut self, lines: &[String]) -> bool {
        match self {
            View::Window(window) => {
                let mut frame = Frame::new(CANVAS_W, CANVAS_H);
                draw_page(&mut frame, lines);
                window.blit(&frame)
            }
            View::Term(screen) => {
                print!("{}", terminal::text_page(*screen, lines));
                io::stdout().flush().ok();
                true
            }
        }
    }

    /// Keyboard input, so the game is playable with no controller at all.
    pub fn poll(&mut self) -> Input {
        match self {
            View::Window(window) => window.poll(),
            View::Term(_) => Input::default(),
        }
    }

    pub fn reset(&mut self) {
        if let View::Window(window) = self {
            window.prev = Buttons::default();
        }
    }
}

pub struct WindowView {
    window: minifb::Window,
    buf: Vec<u32>,
    prev: Buttons,
}

impl WindowView {
    fn open(settings: &Settings) -> Option<WindowView> {
        let mut window = minifb::Window::new(
            "A7FL: No Pads Football",
            settings.width,
            settings.height,
            minifb::WindowOptions {
                resize: !settings.fullscreen,
                scale: minifb::Scale::X1,
                scale_mode: minifb::ScaleMode::AspectRatioStretch,
                borderless: settings.fullscreen,
                ..Default::default()
            },
        )
        .ok()?;
        window.set_target_fps(0);
        Some(WindowView { window, buf: Vec::new(), prev: Buttons::default() })
    }

    fn describe(&self) -> String {
        let (w, h) = self.window.get_size();
        format!("window {w}x{h}")
    }

    /// Nearest-neighbour blit of the canvas, letterboxed into the window.
    fn blit(&mut self, frame: &Frame) -> bool {
        if !self.window.is_open() || self.window.is_key_down(minifb::Key::Escape) {
            return false;
        }
        let (w, h) = self.window.get_size();
        if w < 32 || h < 32 {
            return true;
        }
        if self.buf.len() != w * h {
            self.buf = vec![0; w * h];
        }
        let scale = (w as f32 / frame.w as f32).min(h as f32 / frame.h as f32);
        let dw = ((frame.w as f32 * scale) as usize).min(w);
        let dh = ((frame.h as f32 * scale) as usize).min(h);
        let ox = (w - dw) / 2;
        let oy = (h - dh) / 2;
        self.buf.fill(0);
        for y in 0..dh {
            let sy = (y * frame.h / dh).min(frame.h - 1);
            let row = sy * frame.w;
            let out_row = (oy + y) * w + ox;
            for x in 0..dw {
                let sx = (x * frame.w / dw).min(frame.w - 1);
                let c = frame.color[row + sx];
                self.buf[out_row + x] =
                    ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32;
            }
        }
        self.window.update_with_buffer(&self.buf, w, h).is_ok()
    }

    fn poll(&mut self) -> Input {
        let key = |k| self.window.is_key_down(k);
        let mut held = Buttons {
            a: key(minifb::Key::Space) || key(minifb::Key::Enter),
            b: key(minifb::Key::X),
            x: key(minifb::Key::Z),
            y: key(minifb::Key::C),
            throw: key(minifb::Key::LeftShift) || key(minifb::Key::RightShift),
            select: key(minifb::Key::Tab) || key(minifb::Key::Backspace),
            preview: key(minifb::Key::R),
            dummy: key(minifb::Key::Q),
            shoulder: key(minifb::Key::E),
            start: key(minifb::Key::Escape),
        };

        let axis = |plus, minus| {
            let p = if self.window.is_key_down(plus) { 1.0 } else { 0.0 };
            let m = if self.window.is_key_down(minus) { 1.0 } else { 0.0 };
            p - m
        };
        let stick = Vec2::new(
            axis(minifb::Key::Right, minifb::Key::Left)
                + axis(minifb::Key::D, minifb::Key::A),
            axis(minifb::Key::Up, minifb::Key::Down)
                + axis(minifb::Key::W, minifb::Key::S),
        );
        let stick = Vec2::new(stick.x.clamp(-1.0, 1.0), stick.y.clamp(-1.0, 1.0));

        let pressed = Buttons {
            preview: held.preview && !self.prev.preview,
            a: held.a && !self.prev.a,
            b: held.b && !self.prev.b,
            x: held.x && !self.prev.x,
            y: held.y && !self.prev.y,
            throw: held.throw && !self.prev.throw,
            select: held.select && !self.prev.select,
            dummy: held.dummy && !self.prev.dummy,
            shoulder: held.shoulder && !self.prev.shoulder,
            start: held.start && !self.prev.start,
        };
        self.prev = held;
        Input { stick, held, pressed }
    }
}

/// Lay the text bar over a canvas-sized frame. The benchmark has no window to
/// draw into, so it uses this to make a picture of exactly what live play puts
/// on screen.
pub fn with_hud(mut frame: Frame, lines: &[String]) -> Frame {
    draw_hud(&mut frame, lines);
    frame
}

/// Text bar drawn straight into the picture, in the broadcast font.
fn draw_hud(frame: &mut Frame, lines: &[String]) {
    let count = lines.len() as i32;
    if count == 0 {
        return;
    }
    let width = frame.w as i32;
    let height = count * 10 + 6;
    let top = frame.h as i32 - height;
    frame.fill_rect_alpha(0, top, width, height, backdrop(), 0.78);
    frame.fill_rect(0, top, width, 1, Rgb::new(48, 92, 148));
    for (i, line) in lines.iter().enumerate() {
        let color = if line.contains(">>") || line.starts_with("TARGET") {
            accent()
        } else {
            paper()
        };
        frame.text(8, top + 4 + i as i32 * 10, line, 1, color);
    }
}

/// A page of text: title rule, then wrapped lines in the same font.
fn draw_page(frame: &mut Frame, lines: &[String]) {
    use a7fl_render::font;

    frame.clear(backdrop());
    frame.fill_rect(0, 0, CANVAS_W as i32, 2, Rgb::new(52, 104, 168));
    let mut y = 20i32;
    for line in lines {
        for chunk in wrap(line, 50) {
            if y > CANVAS_H as i32 - 14 {
                return;
            }
            let color = if chunk.contains(">>") {
                accent()
            } else if chunk.starts_with("  ·") {
                Rgb::new(150, 164, 184)
            } else {
                paper()
            };
            let weight = if chunk.starts_with('>') || chunk.contains("FINAL") { 2 } else { 1 };
            font::draw_styled(frame, 24, y, &chunk, 2, color, weight, false);
            y += font::text_height(2) as i32 + 5;
        }
    }
}

/// Break `line` on word boundaries so it fits `width` characters.
fn wrap(line: &str, width: usize) -> Vec<String> {
    if line.chars().count() <= width {
        return vec![line.to_string()];
    }
    let indent: String = line.chars().take_while(|c| *c == ' ').collect();
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in line.split_whitespace() {
        let too_long = !current.is_empty()
            && current.chars().count() + 1 + word.chars().count() > width;
        if too_long {
            out.push(current);
            current = format!("{indent}   {word}");
        } else if current.is_empty() {
            current = format!("{indent}{word}");
        } else {
            current.push(' ');
            current.push_str(word);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_keeps_the_indent_and_the_words() {
        let line = "  [B] NEXT PLAY   [X] PREVIOUS   [Y] ASK COACH   [A] SNAP   [START] QUIT";
        let parts = wrap(line, 30);
        assert!(parts.len() > 1);
        for part in &parts {
            assert!(part.chars().count() <= 30, "{part:?} is too long");
            assert!(part.starts_with("  "), "{part:?} lost its indent");
        }
        let joined = parts.join(" ");
        for word in line.split_whitespace() {
            assert!(joined.contains(word), "{word:?} went missing");
        }
    }

    #[test]
    fn the_text_page_stays_inside_the_canvas() {
        let lines: Vec<String> = (0..40).map(|i| format!("  line {i} of a very long page indeed")).collect();
        let mut frame = Frame::new(CANVAS_W, CANVAS_H);
        draw_page(&mut frame, &lines);
        assert_eq!(frame.color.len(), CANVAS_W * CANVAS_H);
    }
}

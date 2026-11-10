//! Where the game is drawn: a real window when the machine has a display, and
//! ANSI colour blocks in the terminal when it does not.
//!
//! Everything is drawn on a fixed 1280x720 canvas and the window scales that to
//! whatever the desktop gives us, the terminal averages it down into character
//! cells. Neither path needs a GPU.
//!
//! The canvas was 640x360, which was a quarter of the pixels the overlay was laid
//! out for: the score bug filled a third of the picture and every edge stepped a
//! whole pixel. The men were drawn at twice the canvas and boxed back down to
//! hide it, which bought them nothing the canvas could not give them directly.

use std::io::{self, Write};

use nopads_core::math::{Rgb, Vec2};
use nopads_render::hud;
use nopads_render::raster::Frame;
use nopads_render::terminal::{self, Screen};

#[cfg(feature = "bevy_view")]
use crate::view_bevy::BevyView;
use nopads_platform::pad::{Buttons, Input};
use nopads_platform::settings::Settings;

/// The picture is drawn at 720p: the size the broadcast overlay was always laid
/// out for.
pub const CANVAS_W: usize = 1280;
pub const CANVAS_H: usize = 720;

/// The grid the front end and the call sheets are laid out on.
///
/// Those screens were drawn for 640x360, corner to corner, and they are kept
/// there: the interface is built on this grid and scaled up whole onto the
/// canvas, so every screen keeps the proportions it was designed with instead of
/// being re-laid-out for a bigger picture a coordinate at a time.
pub const UI_W: usize = 640;
pub const UI_H: usize = 360;

/// How many canvas pixels one grid pixel becomes.
pub const UI_SCALE: usize = CANVAS_W / UI_W;

/// The size a page of text is set in: the call sheets, which are read closely.
const PAGE_TEXT: usize = 2;
/// One step larger, for a page that only reports: a flag, a score, the final.
const PAGE_BIG: usize = 3;

fn paper() -> Rgb {
    Rgb::new(232, 238, 246)
}

fn accent() -> Rgb {
    Rgb::new(255, 196, 64)
}

fn backdrop() -> Rgb {
    Rgb::new(12, 16, 26)
}

/// A frame of the picture kept for the halftime reel: half the canvas, colour
/// only.
///
/// A full 720p frame is five and a half megabytes of colour and depth. The reel
/// needs the colour, at a size where a few seconds of play is a dozen megabytes
/// rather than hundreds: half the canvas each way is what the tape can afford.
pub struct Shot {
    w: usize,
    h: usize,
    color: Vec<Rgb>,
}

impl Shot {
    /// A picture of `frame` at `factor` fewer pixels each way.
    pub fn of(frame: &Frame, factor: usize) -> Shot {
        let small = frame.downsample(factor.max(1));
        Shot {
            w: small.w,
            h: small.h,
            color: small.color,
        }
    }

    /// The shot as a full canvas frame, scaled up in whole pixels.
    pub fn frame(&self) -> Frame {
        let mut small = Frame::new(self.w, self.h);
        small.color.clone_from(&self.color);
        let mut out = Frame::new(CANVAS_W, CANVAS_H);
        out.blit_up(&small, (CANVAS_W / self.w.max(1)).max(1));
        out
    }
}

/// Where the picture goes.
enum Surface {
    Window(WindowView),
    /// The same game through Bevy's window. Built only with `--features bevy`,
    /// and only chosen when `--bevy` asked for it.
    #[cfg(feature = "bevy_view")]
    Bevy(BevyView),
    Term(Screen),
}

pub struct View {
    surface: Surface,
    /// The last full-canvas picture, kept for the card that goes over it. Only
    /// the colour is kept: a card has nothing to say about depth.
    last: Option<Frame>,
    /// The film of the last play, when that play earned one.
    film: Option<Vec<Shot>>,
}

impl View {
    /// Try a window first; fall back to the terminal when there is no display.
    pub fn open(force_terminal: bool, settings: &Settings) -> View {
        if !force_terminal {
            #[cfg(feature = "bevy_view")]
            if let Some(bevy) = BevyView::attach(settings) {
                return View {
                    surface: Surface::Bevy(bevy),
                    last: None,
                    film: None,
                };
            }
            if let Some(window) = WindowView::open(settings) {
                return View {
                    surface: Surface::Window(window),
                    last: None,
                    film: None,
                };
            }
        }
        View {
            surface: Surface::Term(Screen::fit()),
            last: None,
            film: None,
        }
    }

    /// Keep the film of the play that just finished, for the halftime reel.
    pub fn set_film(&mut self, shots: Vec<Shot>) {
        self.film = Some(shots);
    }

    /// Take the film, if the play that just finished left one.
    pub fn take_film(&mut self) -> Option<Vec<Shot>> {
        self.film.take()
    }

    /// Keep the picture the game just put up, so a card over it has something to
    /// be over. Only the colour is copied, and into the same buffer each time.
    fn remember(&mut self, frame: &Frame) {
        if frame.w != CANVAS_W || frame.h != CANVAS_H {
            return;
        }
        match &mut self.last {
            Some(last) => last.color.clone_from(&frame.color),
            None => {
                let mut copy = Frame::new(CANVAS_W, CANVAS_H);
                copy.color.clone_from(&frame.color);
                self.last = Some(copy);
            }
        }
    }

    /// A card over the last picture: what happened, or what the game is asking.
    ///
    /// The read-out after a play and the conversion question take the same card:
    /// small, over the shot rather than instead of it, and see-through enough
    /// that the field is still the picture.
    pub fn card(&mut self, title: &str, subtitle: &str, lines: &[String], accent: Rgb) -> bool {
        let mut frame = Frame::new(CANVAS_W, CANVAS_H);
        match &self.last {
            Some(last) => frame.color.clone_from(&last.color),
            None => frame.clear(backdrop()),
        }
        hud::draw_notice(&mut frame, title, subtitle, lines, accent);
        match &mut self.surface {
            Surface::Window(window) => window.blit(&frame),
            #[cfg(feature = "bevy_view")]
            Surface::Bevy(view) => view.blit(&frame),
            Surface::Term(screen) => {
                print!("{}", terminal::picture_and_text(&frame, *screen, &[]));
                io::stdout().flush().ok();
                true
            }
        }
    }

    pub fn is_window(&self) -> bool {
        match &self.surface {
            Surface::Window(_) => true,
            #[cfg(feature = "bevy_view")]
            Surface::Bevy(_) => true,
            Surface::Term(_) => false,
        }
    }

    /// Rebuild the window at the size and mode the settings ask for. minifb has
    /// no resize call, so the window is made again rather than nudged.
    ///
    /// Only a different size or display mode needs that: rebuilding it for a row
    /// that does not touch the window made the picture vanish and the window come
    /// back somewhere else on the desktop every time difficulty was changed.
    pub fn apply(&mut self, settings: &Settings) {
        #[cfg(feature = "bevy_view")]
        if let Surface::Bevy(view) = &mut self.surface {
            view.apply(settings);
            return;
        }
        let Surface::Window(window) = &mut self.surface else {
            return;
        };
        if window.matches(settings) {
            return;
        }
        if let Some(opened) = WindowView::open(settings) {
            self.surface = Surface::Window(opened);
        }
    }

    pub fn describe(&self) -> String {
        match &self.surface {
            Surface::Window(w) => w.describe(),
            #[cfg(feature = "bevy_view")]
            Surface::Bevy(view) => view.describe(),
            Surface::Term(screen) => format!("terminal {}x{}", screen.cols, screen.rows),
        }
    }

    /// Show a frame with a text bar along the bottom. Returns false when the
    /// player closed the window.
    pub fn show(&mut self, frame: &mut Frame, lines: &[String]) -> bool {
        // Kept before it is drawn over, so the card that follows a play goes over
        // the play rather than over a blank page.
        self.remember(frame);
        match &mut self.surface {
            Surface::Window(window) => {
                if frame.w == CANVAS_W && frame.h == CANVAS_H {
                    draw_hud(frame, lines);
                }
                window.blit(frame)
            }
            #[cfg(feature = "bevy_view")]
            Surface::Bevy(view) => {
                if frame.w == CANVAS_W && frame.h == CANVAS_H {
                    draw_hud(frame, lines);
                }
                view.blit(frame)
            }
            Surface::Term(screen) => {
                print!("{}", terminal::picture_and_text(frame, *screen, lines));
                io::stdout().flush().ok();
                true
            }
        }
    }

    /// Show a screen drawn on the interface grid, scaled onto the canvas.
    ///
    /// The menus and the call sheets are laid out in `UI_W`x`UI_H` pixels and
    /// come out through here: a whole-number scale up, so a screen sits where it
    /// was designed to sit instead of collecting in the corner of a canvas twice
    /// the size it was drawn for.
    pub fn show_ui(&mut self, ui: &mut Frame, lines: &[String]) -> bool {
        let mut frame = Frame::new(CANVAS_W, CANVAS_H);
        frame.blit_up(ui, UI_SCALE);
        self.show(&mut frame, lines)
    }

    /// A full screen of text: the call sheet, a prompt, the box score.
    pub fn page(&mut self, lines: &[String]) -> bool {
        self.page_at(lines, PAGE_TEXT)
    }

    /// The same page a step larger: a card that only reports what happened is
    /// read at a glance, from across the room, not studied like a call sheet.
    pub fn page_big(&mut self, lines: &[String]) -> bool {
        self.page_at(lines, PAGE_BIG)
    }

    fn page_at(&mut self, lines: &[String], scale: usize) -> bool {
        match &mut self.surface {
            Surface::Window(window) => {
                let mut frame = Frame::new(CANVAS_W, CANVAS_H);
                draw_page(&mut frame, lines, scale);
                window.blit(&frame)
            }
            #[cfg(feature = "bevy_view")]
            Surface::Bevy(view) => {
                let mut frame = Frame::new(CANVAS_W, CANVAS_H);
                draw_page(&mut frame, lines, scale);
                view.blit(&frame)
            }
            Surface::Term(screen) => {
                print!("{}", terminal::text_page(*screen, lines));
                io::stdout().flush().ok();
                true
            }
        }
    }

    /// Keyboard input, so the game is playable with no controller at all.
    pub fn poll(&mut self) -> Input {
        match &mut self.surface {
            Surface::Window(window) => window.poll(),
            #[cfg(feature = "bevy_view")]
            Surface::Bevy(view) => view.poll(),
            Surface::Term(_) => Input::default(),
        }
    }

    pub fn reset(&mut self) {
        if let Surface::Window(window) = &mut self.surface {
            window.prev = Buttons::default();
        }
        #[cfg(feature = "bevy_view")]
        if let Surface::Bevy(view) = &mut self.surface {
            view.reset();
        }
    }
}

pub struct WindowView {
    window: minifb::Window,
    buf: Vec<u32>,
    prev: Buttons,
    /// What this window was asked for, so `apply` can tell a real change from a
    /// row that never touched the window.
    w: usize,
    h: usize,
    fullscreen: bool,
}

impl WindowView {
    fn open(settings: &Settings) -> Option<WindowView> {
        let mut window = minifb::Window::new(
            "7 on 7 No Pads Football",
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
        Some(WindowView {
            window,
            buf: Vec::new(),
            prev: Buttons::default(),
            w: settings.width,
            h: settings.height,
            fullscreen: settings.fullscreen,
        })
    }

    /// True when this window already is the one the settings describe.
    fn matches(&self, settings: &Settings) -> bool {
        self.w == settings.width
            && self.h == settings.height
            && self.fullscreen == settings.fullscreen
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
        let held = Buttons {
            a: key(minifb::Key::Space) || key(minifb::Key::Enter),
            b: key(minifb::Key::X),
            x: key(minifb::Key::Z),
            y: key(minifb::Key::C),
            throw: key(minifb::Key::LeftShift) || key(minifb::Key::RightShift),
            select: key(minifb::Key::Tab) || key(minifb::Key::Backspace),
            preview: key(minifb::Key::R),
            dummy: key(minifb::Key::Q),
            shoulder: key(minifb::Key::E),
            truck: key(minifb::Key::T),
            stop: key(minifb::Key::G),
            juke_l: key(minifb::Key::F),
            juke_r: key(minifb::Key::H),
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

        // The press edges are the platform's, not this window's. The Bevy window
        // already asks the pad for them, and a second hand-written copy here was
        // one new button away from disagreeing with it.
        let pressed = held.edges(self.prev);
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
fn draw_page(frame: &mut Frame, lines: &[String], scale: usize) {
    use nopads_render::font;

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
            font::draw_styled(frame, 24, y, &chunk, scale, color, weight, false);
            y += font::text_height(scale) as i32 + 5;
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
    fn a_shot_keeps_half_the_picture_and_gives_the_whole_frame_back() {
        let mut frame = Frame::new(CANVAS_W, CANVAS_H);
        frame.clear(Rgb::new(10, 90, 40));
        frame.fill_rect(0, 0, CANVAS_W as i32, 40, Rgb::new(200, 40, 30));
        let shot = Shot::of(&frame, 2);
        // Half the canvas each way is what makes a few seconds of play worth
        // keeping in front of a half; the frame it came out of is five megabytes.
        assert_eq!(shot.color.len(), (CANVAS_W / 2) * (CANVAS_H / 2));
        let back = shot.frame();
        assert_eq!((back.w, back.h), (CANVAS_W, CANVAS_H), "the shot lost the canvas");
        assert_eq!(
            back.color[10 * CANVAS_W + 10],
            frame.color[10 * CANVAS_W + 10],
            "the reel band at the top of the shot changed colour"
        );
        assert_eq!(
            back.color[400 * CANVAS_W + 640],
            frame.color[400 * CANVAS_W + 640],
            "the field under the band changed colour"
        );
    }

    #[test]
    fn the_text_page_stays_inside_the_canvas() {
        let lines: Vec<String> = (0..40).map(|i| format!("  line {i} of a very long page indeed")).collect();
        let mut frame = Frame::new(CANVAS_W, CANVAS_H);
        draw_page(&mut frame, &lines, PAGE_TEXT);
        assert_eq!(frame.color.len(), CANVAS_W * CANVAS_H);
    }
}

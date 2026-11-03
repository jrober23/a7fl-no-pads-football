//! The same game through Bevy's window instead of minifb's.
//!
//! Bevy owns the main thread and the event loop, and the game is a blocking
//! script on its own thread; the two meet over `nopads_platform::link`. This file
//! is the game's end of that: `View` calls it exactly like `WindowView`, so
//! nothing in `arcade.rs` or `menu.rs` knows which window it is drawing into.
//!
//! Built only with `--features bevy_view`, and only used when `--bevy` asked for it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use nopads_platform::link::{Link, Shot};
use nopads_platform::pad::{Buttons, Input};
use nopads_platform::settings::Settings;
use nopads_render::raster::Frame;

/// How long the game waits for the window to take the frame it just posted.
/// Short: a window that is a frame behind is better than a game that stalls, and
/// the next post replaces the stale one anyway.
const HANDOFF: Duration = Duration::from_millis(120);

/// How many frames in a row may go untaken before the window is called dead.
/// At 120 ms each this is a bit over a minute: long enough for a hitch or a
/// dragged window, short enough that a machine with no working GPU does not sit
/// there drawing into nothing forever.
const MISSES_BEFORE_GONE: u32 = 600;

/// The presenter the game talks to, installed once by `main` when `--bevy` is on.
static SEAM: Mutex<Option<Arc<Link>>> = Mutex::new(None);

/// Hand the game's end of the link to the view that is about to be opened, and
/// keep one for the caller: if the player closes the window, `main` needs it to
/// tell the game to stop.
pub fn install(link: Link) -> Arc<Link> {
    let link = Arc::new(link);
    *SEAM.lock().expect("seam poisoned") = Some(Arc::clone(&link));
    link
}

pub struct BevyView {
    link: Arc<Link>,
    prev: Buttons,
    width: usize,
    height: usize,
    fullscreen: bool,
    misses: u32,
}

impl BevyView {
    /// Attach to the running presenter, if `main` started one.
    pub fn attach(settings: &Settings) -> Option<BevyView> {
        let link = SEAM.lock().expect("seam poisoned").clone()?;
        Some(BevyView {
            link,
            prev: Buttons::default(),
            width: settings.width,
            height: settings.height,
            fullscreen: settings.fullscreen,
            misses: 0,
        })
    }

    pub fn describe(&self) -> String {
        format!("bevy window {}x{}", self.width, self.height)
    }

    /// The settings screen changed the window. Bevy owns the real window, so the
    /// request goes over the link rather than being applied here.
    pub fn apply(&mut self, settings: &Settings) {
        if self.width == settings.width
            && self.height == settings.height
            && self.fullscreen == settings.fullscreen
        {
            return;
        }
        self.width = settings.width;
        self.height = settings.height;
        self.fullscreen = settings.fullscreen;
        self.link
            .request_resize(settings.width, settings.height, settings.fullscreen);
    }

    /// Show a frame. False when the window is gone, exactly as `WindowView::blit`.
    pub fn blit(&mut self, frame: &Frame) -> bool {
        self.link.post(Shot {
            width: frame.w,
            height: frame.h,
            pixels: frame.color.clone(),
        });
        if self.link.wait_for_take(HANDOFF) {
            self.misses = 0;
        } else {
            self.misses += 1;
        }
        if self.link.closed() {
            return false;
        }
        self.misses < MISSES_BEFORE_GONE
    }

    pub fn poll(&mut self) -> Input {
        let now = self.link.input();
        let pressed = now.held.edges(self.prev);
        self.prev = now.held;
        Input {
            stick: now.stick,
            held: now.held,
            pressed,
        }
    }

    pub fn reset(&mut self) {
        self.prev = Buttons::default();
    }
}

impl Drop for BevyView {
    fn drop(&mut self) {
        // The game is over: let the window close behind it.
        self.link.close();
    }
}

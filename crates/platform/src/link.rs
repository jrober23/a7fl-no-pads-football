//! The hand-off between the game's blocking loop and whatever is presenting it.
//!
//! The client is a script: it draws a frame, polls the pad, and blocks. A window
//! toolkit like Bevy wants the opposite — it owns the main thread and calls back
//! into you. Rather than rewrite 4,700 lines of script into a scheduler just to
//! change the window, the two sides meet here: a single frame slot one way and an
//! input snapshot the other.
//!
//! Only one frame is ever in flight. If the game draws two frames before the
//! presenter takes one, the older frame is dropped, because showing a stale
//! picture late is worse than showing the current one on time.
//!
//! None of this needs a GPU, so all of it is tested here even on a machine that
//! cannot open a window.

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use nopads_core::math::Rgb;

use crate::pad::Input;

/// One canvas, ready to be shown.
#[derive(Clone)]
pub struct Shot {
    pub width: usize,
    pub height: usize,
    /// Row-major, `width * height` pixels.
    pub pixels: Vec<Rgb>,
}

#[derive(Default)]
struct Shared {
    pending: Option<Shot>,
    /// Frames the game has handed over, and frames the presenter has picked up.
    posted: u64,
    taken: u64,
    input: Input,
    resize: Option<Resize>,
    game_closed: bool,
    window_closed: bool,
}

/// A window size the player picked on the settings screen.
#[derive(Clone, Copy, Debug)]
pub struct Resize {
    pub width: usize,
    pub height: usize,
    pub fullscreen: bool,
}

/// The game's end of the hand-off.
pub struct Link {
    shared: Arc<(Mutex<Shared>, Condvar)>,
}

/// The presenter's end: whatever owns the window and the main thread.
pub struct Host {
    shared: Arc<(Mutex<Shared>, Condvar)>,
}

/// The two ends, joined.
pub fn pair() -> (Link, Host) {
    let shared = Arc::new((Mutex::new(Shared::default()), Condvar::new()));
    let link = Link {
        shared: Arc::clone(&shared),
    };
    (link, Host { shared })
}

impl Link {
    /// Hand a frame to the presenter, replacing one it has not picked up yet.
    pub fn post(&self, shot: Shot) {
        let (lock, doorbell) = &*self.shared;
        let mut shared = lock.lock().expect("link poisoned");
        shared.pending = Some(shot);
        shared.posted += 1;
        doorbell.notify_all();
    }

    /// Wait until the frame just posted has been taken, or the wait runs out.
    ///
    /// False means the frame never landed: the caller should carry on rather than
    /// stop drawing, since a slow presenter is a dropped frame and not a dead game.
    pub fn wait_for_take(&self, timeout: Duration) -> bool {
        let (lock, doorbell) = &*self.shared;
        let mut shared = lock.lock().expect("link poisoned");
        let deadline = Instant::now() + timeout;
        while shared.taken < shared.posted && !shared.window_closed {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let (guard, _) = doorbell
                .wait_timeout(shared, deadline - now)
                .expect("link poisoned");
            shared = guard;
        }
        // Taken, and the window is still there. A frame that timed out never
        // landed, and a closed window will not take any more.
        shared.taken >= shared.posted && !shared.window_closed
    }

    /// The pad as the presenter last saw it.
    pub fn input(&self) -> Input {
        self.shared.0.lock().expect("link poisoned").input
    }

    /// Ask the presenter for a different window size.
    pub fn request_resize(&self, width: usize, height: usize, fullscreen: bool) {
        let (lock, doorbell) = &*self.shared;
        lock.lock().expect("link poisoned").resize = Some(Resize {
            width,
            height,
            fullscreen,
        });
        doorbell.notify_all();
    }

    /// Tell the presenter the game is over, so its window can close.
    pub fn close(&self) {
        let (lock, doorbell) = &*self.shared;
        lock.lock().expect("link poisoned").game_closed = true;
        doorbell.notify_all();
    }

    /// True once the player closed the window.
    pub fn closed(&self) -> bool {
        self.shared.0.lock().expect("link poisoned").window_closed
    }
}

impl Host {
    /// The next frame to show, waiting up to `timeout` for one to arrive.
    ///
    /// None means the game is finished or nothing arrived in time.
    pub fn take(&self, timeout: Duration) -> Option<Shot> {
        let (lock, doorbell) = &*self.shared;
        let mut shared = lock.lock().expect("link poisoned");
        let deadline = Instant::now() + timeout;
        loop {
            if shared.game_closed {
                return None;
            }
            if let Some(shot) = shared.pending.take() {
                shared.taken = shared.posted;
                doorbell.notify_all();
                return Some(shot);
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let (guard, _) = doorbell
                .wait_timeout(shared, deadline - now)
                .expect("link poisoned");
            shared = guard;
        }
    }

    /// Give the game the pad state this frame.
    pub fn post_input(&self, input: Input) {
        let (lock, doorbell) = &*self.shared;
        lock.lock().expect("link poisoned").input = input;
        doorbell.notify_all();
    }

    /// The window size the game last asked for, if it has not been applied.
    pub fn take_resize(&self) -> Option<Resize> {
        self.shared.0.lock().expect("link poisoned").resize.take()
    }

    /// The player closed the window.
    pub fn close(&self) {
        let (lock, doorbell) = &*self.shared;
        lock.lock().expect("link poisoned").window_closed = true;
        doorbell.notify_all();
    }

    /// True once the game has finished and wants the window gone.
    pub fn closed(&self) -> bool {
        self.shared.0.lock().expect("link poisoned").game_closed
    }
}

#[cfg(test)]
mod tests {
    use nopads_core::math::Vec2;

    use super::*;

    fn shot(value: u8) -> Shot {
        Shot {
            width: 2,
            height: 1,
            pixels: vec![Rgb::new(value, 0, 0); 2],
        }
    }

    #[test]
    fn a_frame_crosses_the_seam() {
        let (game, host) = pair();
        game.post(shot(7));
        let got = host.take(Duration::from_secs(1)).expect("no frame arrived");
        assert_eq!((got.width, got.height), (2, 1));
        assert_eq!(got.pixels[0].r, 7, "the pixels came across unchanged");
        // The game is told the frame landed instead of waiting out its timeout.
        assert!(game.wait_for_take(Duration::from_millis(50)));
    }

    #[test]
    fn the_presenter_gets_the_newest_frame_not_a_queue() {
        let (game, host) = pair();
        game.post(shot(1));
        game.post(shot(2));
        let got = host.take(Duration::from_secs(1)).expect("no frame arrived");
        assert_eq!(got.pixels[0].r, 2, "a stale frame was shown");
    }

    #[test]
    fn the_game_does_not_hang_when_nobody_is_presenting() {
        let (game, _host) = pair();
        game.post(shot(1));
        let started = Instant::now();
        assert!(!game.wait_for_take(Duration::from_millis(30)));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the wait outlived its timeout"
        );
    }

    #[test]
    fn closing_the_window_tells_the_game() {
        let (game, host) = pair();
        game.post(shot(1));
        host.close();
        assert!(game.closed());
        assert!(
            !game.wait_for_take(Duration::from_secs(1)),
            "a closed window told the game to carry on"
        );
    }

    #[test]
    fn a_resize_waits_until_the_presenter_takes_it() {
        let (game, host) = pair();
        assert!(host.take_resize().is_none());
        game.request_resize(1600, 900, true);
        let got = host.take_resize().expect("the resize went missing");
        assert_eq!((got.width, got.height, got.fullscreen), (1600, 900, true));
        // It is a request, not a setting: the presenter takes it once.
        assert!(host.take_resize().is_none());
    }

    #[test]
    fn the_pad_crosses_the_other_way() {
        let (game, host) = pair();
        let mut input = Input::default();
        input.stick = Vec2::new(0.5, -0.25);
        input.pressed.a = true;
        host.post_input(input);
        let got = game.input();
        assert_eq!((got.stick.x, got.stick.y), (0.5, -0.25));
        assert!(got.pressed.a);
    }

    #[test]
    fn a_finished_game_closes_the_presenter() {
        let (game, host) = pair();
        assert!(!host.closed());
        game.close();
        assert!(host.closed());
        assert!(host.take(Duration::from_millis(1)).is_none());
    }
}

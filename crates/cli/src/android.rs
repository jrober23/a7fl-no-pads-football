//! The Android platform layer: a NativeActivity surface where the desktop has a
//! window, and the pad's events where the desktop has a keyboard.
//!
//! It slots in as `WindowView`, so the game, the menus and the arcade never learn
//! that they are on a phone. The renderer already draws into a CPU framebuffer, so
//! there is no graphics API here at all: the frame is letterboxed onto the
//! activity's surface byte by byte, the way it was letterboxed into a window.

use std::sync::OnceLock;
use std::time::Duration;

use android_activity::input::{Axis, InputEvent, KeyAction};
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use nopads_core::math::Vec2;
use nopads_platform::pad::{Buttons, Input};
use nopads_platform::settings::Settings;
use nopads_render::raster::Frame;

/// The activity this process is running in. Android calls `android_main` once, so
/// the slot is filled once, before any view is opened.
static APP: OnceLock<AndroidApp> = OnceLock::new();

/// Run the game in the activity the OS just handed us.
pub fn run(app: AndroidApp) {
    let _ = APP.set(app.clone());
    // Settings live in the app's private directory, which is HOME as far as the
    // rest of the game is concerned; the desktop default (".") is not writable.
    if let Some(data) = app.internal_data_path() {
        std::fs::create_dir_all(&data).ok();
        std::env::set_var("HOME", &data);
    }
    let settings = Settings::load();
    let view = crate::view::View::open(false, &settings);
    let teams = nopads_core::team::league_teams();
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(7);
    let mut game = nopads_core::game::GameState::new(teams[0].clone(), teams[1].clone(), seed);
    game.set_difficulty(nopads_core::difficulty::Difficulty::from_index(settings.difficulty));
    game.set_quarter_seconds(settings.quarter_minutes as f32 * 60.0);
    crate::arcade::Arcade::new(game, None, view)
        .with_camera(settings.camera)
        .with_lens(settings.lens_zoom, settings.lens_tilt)
        .with_settings(settings)
        .with_venue(0)
        .with_my_team(0)
        .run();
}

/// The stick and triggers, as the last input event left them.
///
/// A joystick motion event carries one axis at a time, so the state is kept here
/// and the sticks are read back from it each time the game polls.
#[derive(Default)]
struct Axes {
    x: f32,
    y: f32,
    hat_x: f32,
    hat_y: f32,
    right_x: f32,
    right_y: f32,
    right_trigger: f32,
}

pub struct WindowView {
    /// The buttons the game saw last time it polled, for the press edges.
    pub prev: Buttons,
    held: Buttons,
    axes: Axes,
    w: usize,
    h: usize,
    fullscreen: bool,
}

impl WindowView {
    pub fn open(settings: &Settings) -> Option<WindowView> {
        // Nothing can be drawn until the activity exists, which is why the entry
        // point fills `APP` before it opens the view.
        APP.get()?;
        Some(WindowView {
            prev: Buttons::default(),
            held: Buttons::default(),
            axes: Axes::default(),
            w: settings.width,
            h: settings.height,
            fullscreen: settings.fullscreen,
        })
    }

    /// The surface is whatever the device gave us, so there is nothing to resize
    /// and nothing to reopen: the settings screen changes nothing here.
    pub fn matches(&self, settings: &Settings) -> bool {
        self.w == settings.width && self.h == settings.height && self.fullscreen == settings.fullscreen
    }

    pub fn describe(&self) -> String {
        "android surface".to_string()
    }

    /// Show a frame on the surface. Returns false once the activity is gone.
    pub fn blit(&mut self, frame: &Frame) -> bool {
        let Some(app) = APP.get() else { return false };
        loop {
            if destroyed(app) {
                return false;
            }
            if let Some(window) = app.native_window() {
                return present(&window, frame);
            }
            // The activity is in the background or still starting: hold the frame
            // rather than running the clock on a picture nobody can see.
            std::thread::sleep(Duration::from_millis(16));
        }
    }

    pub fn poll(&mut self) -> Input {
        if let Some(app) = APP.get() {
            pump(app, &mut self.held, &mut self.axes);
        }
        // The right trigger is the throw and the preview at the line, exactly as
        // it is on the desktop pad.
        if self.axes.right_trigger > 0.5 {
            self.held.throw = true;
            self.held.preview = true;
        } else {
            self.held.throw = false;
            self.held.preview = false;
        }
        // The right stick is the runner's body: up is a shoulder, down is a dead
        // stop, the sides are the jump cut.
        self.held.truck = self.axes.right_y > 0.55;
        self.held.stop = self.axes.right_y < -0.55;
        self.held.juke_r = self.axes.right_x > 0.55;
        self.held.juke_l = self.axes.right_x < -0.55;

        // Android reports Y up as negative, and the game's stick is Y up.
        let raw = Vec2::new(
            self.axes.x + self.axes.hat_x,
            -(self.axes.y + self.axes.hat_y),
        );
        let length = raw.length();
        let stick = if length < 0.22 {
            Vec2::ZERO
        } else if length > 1.0 {
            Vec2::new(raw.x / length, raw.y / length)
        } else {
            raw
        };
        let pressed = self.held.edges(self.prev);
        self.prev = self.held;
        Input { stick, held: self.held, pressed }
    }
}

/// True once the OS has destroyed the activity, which is the cue to stop the game.
fn destroyed(app: &AndroidApp) -> bool {
    let mut gone = false;
    app.poll_events(Some(Duration::ZERO), |event| {
        if let PollEvent::Main(MainEvent::Destroy) = event {
            gone = true;
        }
    });
    gone
}

/// Letterbox the canvas into the surface, nearest neighbour, then post it.
fn present(window: &ndk::native_window::NativeWindow, frame: &Frame) -> bool {
    let Ok(mut buffer) = window.lock(None) else {
        return true;
    };
    let (w, h, stride) = (buffer.width(), buffer.height(), buffer.stride());
    if w < 32 || h < 32 || frame.w == 0 || frame.h == 0 {
        return true;
    }
    let per_pixel = buffer.format().bytes_per_pixel().unwrap_or(4).max(3);
    let bits = buffer.bits().cast::<u8>();
    if bits.is_null() {
        return true;
    }
    let scale = (w as f32 / frame.w as f32).min(h as f32 / frame.h as f32);
    let dw = ((frame.w as f32 * scale) as usize).clamp(1, w);
    let dh = ((frame.h as f32 * scale) as usize).clamp(1, h);
    let ox = (w - dw) / 2;
    let oy = (h - dh) / 2;
    // SAFETY: the lock guard gives us `stride * height` pixels of at least
    // `per_pixel` bytes each, and every write below is inside that range.
    unsafe {
        std::ptr::write_bytes(bits, 0, stride * h * per_pixel);
        for y in 0..dh {
            let sy = (y * frame.h / dh).min(frame.h - 1);
            let row = sy * frame.w;
            let out = (oy + y) * stride + ox;
            for x in 0..dw {
                let sx = (x * frame.w / dw).min(frame.w - 1);
                let c = frame.color[row + sx];
                let p = bits.add((out + x) * per_pixel);
                p.write(c.r);
                p.add(1).write(c.g);
                p.add(2).write(c.b);
                if per_pixel >= 4 {
                    p.add(3).write(255);
                }
            }
        }
    }
    // Dropping the guard unlocks and posts the buffer.
    true
}

/// Drain the activity's input queue into the pad state.
///
/// Buttons are matched on Android's own key codes rather than on the crate's
/// enum names: the numbers are the NDK's and do not move between releases.
fn pump(app: &AndroidApp, held: &mut Buttons, axes: &mut Axes) {
    let Ok(mut events) = app.input_events_iter() else { return };
    while events.next(|event| {
        match event {
            InputEvent::KeyEvent(key) => {
                let down = matches!(key.action(), KeyAction::Down);
                button(held, key.key_code().into(), down);
                InputStatus::Handled
            }
            InputEvent::MotionEvent(motion) => {
                // A touch is not the pad. Without this the finger's screen
                // coordinates would be read as a stick position.
                if motion.source().is_joystick_class() && motion.pointer_count() > 0 {
                    let point = motion.pointer_at_index(0);
                    axes.x = point.axis_value(Axis::X);
                    axes.y = point.axis_value(Axis::Y);
                    axes.hat_x = point.axis_value(Axis::HatX);
                    axes.hat_y = point.axis_value(Axis::HatY);
                    // Android puts the right stick across on Rz and up on Z.
                    axes.right_x = point.axis_value(Axis::Rz);
                    axes.right_y = point.axis_value(Axis::Z);
                    axes.right_trigger = point.axis_value(Axis::Rtrigger);
                }
                InputStatus::Handled
            }
            _ => InputStatus::Unhandled,
        }
    }) {}
}

/// `AKEYCODE_BUTTON_A` and the rest, straight from the NDK.
const BUTTON_A: u32 = 96;
const BUTTON_B: u32 = 97;
const BUTTON_X: u32 = 99;
const BUTTON_Y: u32 = 100;
const BUTTON_L1: u32 = 102;
const BUTTON_R1: u32 = 103;
const BUTTON_L2: u32 = 104;
const BUTTON_R2: u32 = 105;
const BUTTON_THUMBL: u32 = 106;
const BUTTON_START: u32 = 108;
const BUTTON_SELECT: u32 = 109;

/// The pad's buttons, named for what they do in this game rather than where they
/// sit: an Xbox pad and a DualSense land on the same mapping, and it is the same
/// mapping the desktop gamepad uses.
fn button(held: &mut Buttons, code: u32, down: bool) {
    match code {
        BUTTON_A => held.a = down,
        BUTTON_B => held.b = down,
        BUTTON_X => held.x = down,
        BUTTON_Y => held.y = down,
        // Left bumper is the dummy count; the left trigger does it too.
        BUTTON_L1 | BUTTON_L2 => held.dummy = down,
        // Right bumper is the defensive check, right trigger the throw.
        BUTTON_R1 => held.shoulder = down,
        BUTTON_R2 => {
            held.throw = down;
            held.preview = down;
        }
        BUTTON_SELECT | BUTTON_THUMBL => held.select = down,
        BUTTON_START => held.start = down,
        _ => {}
    }
}

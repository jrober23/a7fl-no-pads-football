//! The Android platform layer: a NativeActivity surface where the desktop has a
//! window, and the pad's events where the desktop has a keyboard.
//!
//! It slots in as `WindowView`, so the game, the menus and the arcade never learn
//! that they are on a phone. The renderer already draws into a CPU framebuffer, so
//! there is no graphics API here at all: the frame is letterboxed onto the
//! activity's surface byte by byte, the way it was letterboxed into a window.

use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use android_activity::input::{Axis, InputEvent, KeyAction};
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use nopads_core::difficulty::Difficulty;
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
    log("android_main: started");
    // Settings live in the app's private directory, which is HOME as far as the
    // rest of the game is concerned; the desktop default (".") is not writable.
    if let Some(data) = app.internal_data_path() {
        std::fs::create_dir_all(&data).ok();
        std::env::set_var("HOME", &data);
        log(&format!("android_main: home is {}", data.display()));
    }
    let mut settings = Settings::load();
    // A phone has no settings screen, and a first run has no settings file, so it
    // would take the desktop default: Easy, the one tier the 0.25.116 buffs left
    // alone, which plays exactly like the build before them. Start on Normal
    // instead, and keep whatever a settings file asks for.
    if !Settings::path().exists() {
        settings.difficulty = Difficulty::Normal.index();
        log("android_main: no settings file, so starting on Normal");
    }
    log(&format!(
        "android_main: settings {}x{} fullscreen={} difficulty={}",
        settings.width, settings.height, settings.fullscreen, settings.difficulty
    ));
    let view = crate::view::View::open(false, &settings);
    log(&format!("android_main: view is {}", view.describe()));
    let teams = nopads_core::team::league_teams();
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(7);
    let mut game = nopads_core::game::GameState::new(teams[0].clone(), teams[1].clone(), seed);
    game.set_difficulty(nopads_core::difficulty::Difficulty::from_index(settings.difficulty));
    game.set_quarter_seconds(settings.quarter_minutes as f32 * 60.0);
    log("android_main: kicking off");
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
        if APP.get().is_none() {
            log("view: opened before the activity existed");
            return None;
        }
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
        let mut waited = 0u32;
        loop {
            if destroyed(app) {
                return false;
            }
            if let Some(window) = app.native_window() {
                return present(&window, frame);
            }
            // The activity is in the background or still starting: hold the frame
            // rather than running the clock on a picture nobody can see.
            if waited % 120 == 0 {
                log("surface: waiting for the activity's window");
            }
            waited += 1;
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
///
/// The surface is not always four bytes a pixel: some devices hand back RGB_565,
/// and writing three bytes into those would run past the end of the buffer. So
/// the pixel width comes from the format, and a format this cannot write is left
/// alone rather than guessed at.
fn present(window: &ndk::native_window::NativeWindow, frame: &Frame) -> bool {
    let Ok(mut buffer) = window.lock(None) else {
        return true;
    };
    // Only the work after the buffer arrives is the frame's cost; waiting for a
    // buffer is the display's clock, not ours.
    let started = Instant::now();
    let (w, h, stride) = (buffer.width(), buffer.height(), buffer.stride());
    if w < 32 || h < 32 || frame.w == 0 || frame.h == 0 {
        return true;
    }
    let Some(per_pixel) = buffer.format().bytes_per_pixel() else {
        log("surface: this device's format has no pixel width");
        return true;
    };
    if per_pixel != 4 && per_pixel != 2 {
        log(&format!("surface: {} bytes per pixel is not one this draws", per_pixel));
        return true;
    }
    let bits = buffer.bits().cast::<u8>();
    if bits.is_null() {
        return true;
    }
    if !PRESENTED.swap(true, Ordering::Relaxed) {
        log(&format!(
            "surface: {}x{} stride {} {} bytes per pixel",
            w, h, stride, per_pixel
        ));
    }
    let scale = (w as f32 / frame.w as f32).min(h as f32 / frame.h as f32);
    let dw = ((frame.w as f32 * scale) as usize).clamp(1, w);
    let dh = ((frame.h as f32 * scale) as usize).clamp(1, h);
    let ox = (w - dw) / 2;
    let oy = (h - dh) / 2;
    // Which source column each output column takes. One divide per column rather
    // than one per pixel: at 1080 wide that is a thousand divides a frame instead
    // of a million.
    let columns = column_map(dw, frame.w);
    // SAFETY: the lock guard exposes `stride * height` pixels of `per_pixel` bytes
    // each, and every write below stays inside that span.
    unsafe {
        // Only what the picture does not cover needs clearing: the bars above and
        // below it, and the margins either side. The whole-surface clear this
        // replaces wrote ten megabytes a frame on a 1080x2340 phone.
        if oy > 0 {
            std::ptr::write_bytes(bits, 0, oy * stride * per_pixel);
        }
        let below = oy + dh;
        if below < h {
            std::ptr::write_bytes(
                bits.add(below * stride * per_pixel),
                0,
                (h - below) * stride * per_pixel,
            );
        }
        if ox > 0 || ox + dw < w {
            for y in 0..dh {
                let row = bits.add((oy + y) * stride * per_pixel);
                std::ptr::write_bytes(row, 0, ox * per_pixel);
                std::ptr::write_bytes(
                    row.add((ox + dw) * per_pixel),
                    0,
                    (w - ox - dw) * per_pixel,
                );
            }
        }
        // The pixel format is settled once for the frame rather than once for each
        // pixel, and a four-byte surface is written a word at a time.
        if per_pixel == 4 {
            for y in 0..dh {
                let sy = (y * frame.h / dh).min(frame.h - 1) * frame.w;
                let out = bits.add(((oy + y) * stride + ox) * 4).cast::<u32>();
                for x in 0..dw {
                    let c = frame.color[sy + columns[x]];
                    let pixel =
                        0xFF00_0000 | ((c.b as u32) << 16) | ((c.g as u32) << 8) | c.r as u32;
                    out.add(x).write_unaligned(pixel);
                }
            }
        } else {
            // RGB_565: five bits of red, six of green, five of blue.
            for y in 0..dh {
                let sy = (y * frame.h / dh).min(frame.h - 1) * frame.w;
                let out = bits.add(((oy + y) * stride + ox) * 2).cast::<u16>();
                for x in 0..dw {
                    let c = frame.color[sy + columns[x]];
                    let pixel = (((c.r as u16) >> 3) << 11)
                        | (((c.g as u16) >> 2) << 5)
                        | ((c.b as u16) >> 3);
                    out.add(x).write_unaligned(pixel);
                }
            }
        }
    }
    count_frame(started.elapsed().as_secs_f32() * 1000.0);
    // Dropping the guard unlocks and posts the buffer.
    true
}

/// True once a frame has actually reached the screen, so the surface geometry is
/// reported once rather than every frame.
static PRESENTED: AtomicBool = AtomicBool::new(false);

/// Frames drawn since the last report, and when that report was due.
static FRAMES: AtomicU32 = AtomicU32::new(0);
static REPORT_AT_MS: AtomicU64 = AtomicU64::new(0);

/// One source column per output column, for the nearest-neighbour stretch.
fn column_map(dw: usize, source_width: usize) -> Vec<usize> {
    (0..dw)
        .map(|x| (x * source_width / dw).min(source_width - 1))
        .collect()
}

/// Report the frame rate to logcat every couple of seconds, with the cost of the
/// blit alone in it.
///
/// A phone cannot be profiled from here, so the game says how it is doing: `fps`
/// is what reached the screen, and `blit` separates drawing the picture from
/// making it. Low fps with a small blit is the renderer; a large blit is this.
fn count_frame(blit_ms: f32) {
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    let now_ms = start.elapsed().as_millis() as u64;
    let frames = FRAMES.fetch_add(1, Ordering::Relaxed) + 1;
    let since = REPORT_AT_MS.load(Ordering::Relaxed);
    if now_ms < since + 2000 {
        return;
    }
    let span = if since == 0 { now_ms } else { now_ms - since }.max(1);
    log(&format!(
        "fps {:.1}, blit {:.1} ms",
        frames as f64 * 1000.0 / span as f64,
        blit_ms
    ));
    FRAMES.store(0, Ordering::Relaxed);
    REPORT_AT_MS.store(now_ms, Ordering::Relaxed);
}

/// Write a line to logcat.
///
/// Android sends a `NativeActivity`'s stdout nowhere, so a device we cannot debug
/// directly can still say what it did: `adb logcat -s nopads`.
pub fn log(message: &str) {
    #[link(name = "log")]
    extern "C" {
        fn __android_log_print(
            priority: i32,
            tag: *const std::os::raw::c_char,
            format: *const std::os::raw::c_char,
            ...
        ) -> i32;
    }
    const INFO: i32 = 4;
    let text = message.replace('\0', " ");
    let (Ok(tag), Ok(text)) = (CString::new("nopads"), CString::new(text)) else {
        return;
    };
    unsafe {
        __android_log_print(INFO, tag.as_ptr(), b"%s\0".as_ptr().cast(), text.as_ptr());
    }
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

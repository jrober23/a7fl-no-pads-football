//! A spike: Bevy as the window, input and presentation shell for 7 on 7.
//!
//! The game already keeps its platform at arm's length. `crates/cli/src/view.rs`
//! has a `View` enum with a window arm (`minifb`) and a terminal arm, and the rest
//! of the client only ever calls `show`, `page`, `poll`, `reset` and `apply` on it.
//! That enum is the one place an engine can be bolted on without touching the game
//! itself, so this crate is a second window arm, built on Bevy.
//!
//! What is deliberately *not* here: Bevy's renderer. The picture stays the game's
//! own software rasteriser, drawn into a 640x360 `Frame` and uploaded as one
//! texture. Bevy owns the window, the swap chain, the event loop, the keyboard and
//! the scaling, which is exactly what `minifb` owns today. See README.md for what
//! the other kind of port — moving the 9,000 lines of geometry onto Bevy meshes —
//! would actually cost.

use bevy::prelude::*;
use nopads_core::math::Vec2;
use nopads_render::raster::Frame;

/// The canvas the game draws on, same as `view::CANVAS_W` / `CANVAS_H`.
pub const CANVAS_W: usize = 640;
pub const CANVAS_H: usize = 360;

/// The buttons the game cares about.
///
/// This is a copy of `crates/cli/src/pad.rs`, which lives inside a binary crate
/// and so cannot be imported. A real port would move `pad.rs` and `settings.rs`
/// into a small shared crate and delete this copy; the spike keeps it local so the
/// shipping game is untouched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Buttons {
    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,
    pub throw: bool,
    pub select: bool,
    pub preview: bool,
    pub dummy: bool,
    pub shoulder: bool,
    pub start: bool,
}

impl Buttons {
    /// The buttons that went down between `prev` and `held`.
    pub fn edges(self, prev: Buttons) -> Buttons {
        Buttons {
            a: self.a && !prev.a,
            b: self.b && !prev.b,
            x: self.x && !prev.x,
            y: self.y && !prev.y,
            throw: self.throw && !prev.throw,
            select: self.select && !prev.select,
            preview: self.preview && !prev.preview,
            dummy: self.dummy && !prev.dummy,
            shoulder: self.shoulder && !prev.shoulder,
            start: self.start && !prev.start,
        }
    }
}

/// One poll of the keyboard, in the shape the game already asks `View::poll` for.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub stick: Vec2,
    pub held: Buttons,
    pub pressed: Buttons,
}

/// The canvas as RGBA8, which is what a Bevy `Image` wants. Straight pass-through:
/// the game's `Rgb` is already 8-bit and the picture is already what it should be.
pub fn frame_to_rgba(frame: &Frame) -> Vec<u8> {
    let mut out = Vec::with_capacity(frame.color.len() * 4);
    for c in &frame.color {
        out.extend_from_slice(&[c.r, c.g, c.b, 255]);
    }
    out
}

/// Keyboard bindings, the same ones `WindowView::poll` uses so keyboard muscle
/// memory survives the switch: Space/Enter snap and confirm, X/Z/C the face
/// buttons, Shift throws, Tab and Backspace select, R previews, Q dummies, E
/// audibles, Escape is start, and both the arrows and WASD drive the stick.
pub fn keys_held(keys: &ButtonInput<KeyCode>) -> Buttons {
    let any = |list: &[KeyCode]| keys.any_pressed(list.iter().copied());
    Buttons {
        a: any(&[KeyCode::Space, KeyCode::Enter]),
        b: any(&[KeyCode::KeyX]),
        x: any(&[KeyCode::KeyZ]),
        y: any(&[KeyCode::KeyC]),
        throw: any(&[KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        select: any(&[KeyCode::Tab, KeyCode::Backspace]),
        preview: any(&[KeyCode::KeyR]),
        dummy: any(&[KeyCode::KeyQ]),
        shoulder: any(&[KeyCode::KeyE]),
        start: any(&[KeyCode::Escape]),
    }
}

/// The stick, from the arrows and WASD, clamped to the unit square the game
/// expects. Up the screen is positive, as in `pad.rs`.
pub fn keys_stick(keys: &ButtonInput<KeyCode>) -> Vec2 {
    let axis = |plus: &[KeyCode], minus: &[KeyCode]| {
        let p = plus.iter().any(|k| keys.pressed(*k)) as i32;
        let m = minus.iter().any(|k| keys.pressed(*k)) as i32;
        (p - m) as f32
    };
    let x = axis(&[KeyCode::ArrowRight, KeyCode::KeyD], &[KeyCode::ArrowLeft, KeyCode::KeyA]);
    let y = axis(&[KeyCode::ArrowUp, KeyCode::KeyW], &[KeyCode::ArrowDown, KeyCode::KeyS]);
    Vec2::new(x.clamp(-1.0, 1.0), y.clamp(-1.0, 1.0))
}

/// A whole poll: held buttons, their edges, and the stick.
pub fn poll(keys: &ButtonInput<KeyCode>, prev: Buttons) -> Input {
    let held = keys_held(keys);
    Input {
        stick: keys_stick(keys),
        held,
        pressed: held.edges(prev),
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::ButtonInput;
    use nopads_core::math::Rgb;

    use super::*;

    fn down(codes: &[KeyCode]) -> ButtonInput<KeyCode> {
        let mut keys = ButtonInput::<KeyCode>::default();
        for k in codes {
            keys.press(*k);
        }
        keys
    }

    #[test]
    fn the_canvas_becomes_rgba_in_order() {
        let mut frame = Frame::new(3, 2);
        frame.color[0] = Rgb::new(10, 20, 30);
        frame.color[5] = Rgb::new(200, 100, 50);
        let rgba = frame_to_rgba(&frame);
        assert_eq!(rgba.len(), 3 * 2 * 4);
        assert_eq!(&rgba[0..4], &[10, 20, 30, 255]);
        assert_eq!(&rgba[20..24], &[200, 100, 50, 255]);
        // Every pixel is opaque: the canvas has no alpha channel to lose.
        assert!(rgba.chunks(4).all(|p| p[3] == 255));
    }

    #[test]
    fn the_keys_fall_where_they_fell_on_minifb() {
        let keys = down(&[KeyCode::Space, KeyCode::KeyX, KeyCode::ShiftLeft, KeyCode::Tab]);
        let held = keys_held(&keys);
        assert!(held.a, "Space is still snap");
        assert!(held.b, "X is still the second face button");
        assert!(held.throw, "either Shift still throws");
        assert!(held.select, "Tab still selects");
        assert!(!held.x && !held.y && !held.start);
        // Enter is the other snap key, and Escape is still start.
        let other = keys_held(&down(&[KeyCode::Enter, KeyCode::Escape]));
        assert!(other.a && other.start);
    }

    #[test]
    fn the_stick_reads_both_handed_sets_and_clamps() {
        let right = keys_stick(&down(&[KeyCode::ArrowRight]));
        assert_eq!((right.x, right.y), (1.0, 0.0));
        let up = keys_stick(&down(&[KeyCode::KeyW]));
        assert_eq!((up.x, up.y), (0.0, 1.0), "W drives the stick up, as before");
        // Both sets at once is still no faster than a full push.
        let both = keys_stick(&down(&[KeyCode::ArrowRight, KeyCode::KeyD]));
        assert_eq!(both.x, 1.0);
        // Opposite keys cancel, as they should.
        let cancelled = keys_stick(&down(&[KeyCode::ArrowLeft, KeyCode::ArrowRight]));
        assert_eq!(cancelled.x, 0.0);
    }

    #[test]
    fn a_press_fires_once_and_a_hold_does_not() {
        let space = down(&[KeyCode::Space]);
        let first = poll(&space, Buttons::default());
        assert!(first.pressed.a, "the first poll with Space down is a press");
        let second = poll(&space, first.held);
        assert!(second.held.a, "still held");
        assert!(!second.pressed.a, "but no longer a press");
        let released = poll(&down(&[]), second.held);
        assert!(!released.held.a && !released.pressed.a);
    }
}

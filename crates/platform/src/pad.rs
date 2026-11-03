//! Gamepad input for the arcade client.
//!
//! Backed by `gilrs`, which speaks XInput on Windows, IOKit on macOS and evdev on
//! Linux, so an 8BitDo pad works in XInput, DirectInput or Switch mode without any
//! per-controller code here. Built with `--no-default-features` the crate has no
//! controller support at all and [`open`] simply returns `None`.

use nopads_core::math::Vec2;

/// Buttons the game cares about, named for what they do rather than where they
/// sit, so an 8BitDo in Switch layout still throws with the shoulder trigger.
#[derive(Clone, Copy, Debug, Default)]
pub struct Buttons {
    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,
    pub throw: bool,
    /// The Select / Back button, which cycles the camera.
    pub select: bool,
    /// The left trigger, held: the play preview at the line.
    pub preview: bool,
    /// The left bumper: a dummy count at the line.
    pub dummy: bool,
    /// The right shoulder: the defensive audible at the line, and the break to
    /// the right once the ball is in a runner's hands.
    pub shoulder: bool,
    /// Right stick up: put a shoulder into the man in front of him.
    pub truck: bool,
    /// Right stick down: sit down on both feet.
    pub stop: bool,
    /// Right stick left and right: the jump cut.
    pub juke_l: bool,
    pub juke_r: bool,
    pub start: bool,
}

impl Buttons {
    /// The buttons that went down between `prev` and `self`.
    ///
    /// A window backend should hand over the buttons that are *held* and let this
    /// work out the edges at the moment the game polls, because the two do not
    /// poll at the same rate: an edge computed per window frame is an edge the
    /// game can miss entirely.
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
            truck: self.truck && !prev.truck,
            stop: self.stop && !prev.stop,
            juke_l: self.juke_l && !prev.juke_l,
            juke_r: self.juke_r && !prev.juke_r,
            start: self.start && !prev.start,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    /// Left stick, dead zoned, `x` across the screen and `y` up the screen.
    pub stick: Vec2,
    /// Down right now.
    pub held: Buttons,
    /// Went down since the previous poll.
    pub pressed: Buttons,
}

/// Open the first connected controller.
pub fn open() -> Option<Pad> {
    Pad::open()
}

/// Combine the controller with the keyboard: whichever one is being used wins.
pub fn mix(pad: Option<&mut Pad>, keys: Input) -> Input {
    let Some(pad) = pad else { return keys };
    let stick = pad.poll();
    let stick_live = stick.stick.length() > 0.0;
    Input {
        stick: if stick_live { stick.stick } else { keys.stick },
        held: Buttons {
            a: stick.held.a || keys.held.a,
            b: stick.held.b || keys.held.b,
            x: stick.held.x || keys.held.x,
            y: stick.held.y || keys.held.y,
            throw: stick.held.throw || keys.held.throw,
            select: stick.held.select || keys.held.select,
            preview: stick.held.preview || keys.held.preview,
            dummy: stick.held.dummy || keys.held.dummy,
            shoulder: stick.held.shoulder || keys.held.shoulder,
            truck: stick.held.truck || keys.held.truck,
            stop: stick.held.stop || keys.held.stop,
            juke_l: stick.held.juke_l || keys.held.juke_l,
            juke_r: stick.held.juke_r || keys.held.juke_r,
            start: stick.held.start || keys.held.start,
        },
        pressed: Buttons {
            a: stick.pressed.a || keys.pressed.a,
            b: stick.pressed.b || keys.pressed.b,
            x: stick.pressed.x || keys.pressed.x,
            y: stick.pressed.y || keys.pressed.y,
            throw: stick.pressed.throw || keys.pressed.throw,
            select: stick.pressed.select || keys.pressed.select,
            preview: stick.pressed.preview || keys.pressed.preview,
            dummy: stick.pressed.dummy || keys.pressed.dummy,
            shoulder: stick.pressed.shoulder || keys.pressed.shoulder,
            truck: stick.pressed.truck || keys.pressed.truck,
            stop: stick.pressed.stop || keys.pressed.stop,
            juke_l: stick.pressed.juke_l || keys.pressed.juke_l,
            juke_r: stick.pressed.juke_r || keys.pressed.juke_r,
            start: stick.pressed.start || keys.pressed.start,
        },
    }
}

#[cfg(feature = "gamepad")]
mod backend {
    use gilrs::{Axis, Button, Gilrs};

    use super::{Buttons, Input};
    use nopads_core::math::Vec2;

    pub struct Pad {
        gilrs: Gilrs,
        prev: Buttons,
        name: String,
    }

    impl Pad {
        pub fn open() -> Option<Pad> {
            let mut gilrs = Gilrs::new().ok()?;
            // A wireless pad can take a moment to appear after it wakes up, and
            // a machine with no controller at all must not start a game.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            let mut name = None;
            loop {
                while gilrs.next_event().is_some() {}
                if let Some((_, gp)) = gilrs.gamepads().next() {
                    name = Some(gp.name().to_string());
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(80));
            }
            let name = name?;
            Some(Pad { gilrs, prev: Buttons::default(), name })
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        /// True when at least one controller is still connected.
        pub fn connected(&self) -> bool {
            self.gilrs.gamepads().next().is_some()
        }

        pub fn poll(&mut self) -> Input {
            // Draining the event queue is what keeps gilrs' device state current;
            // the buttons and axes are read back from that state below.
            while self.gilrs.next_event().is_some() {}

            let mut held = Buttons::default();
            let mut raw = Vec2::ZERO;
            if let Some((_, gamepad)) = self.gilrs.gamepads().next() {
                raw = Vec2::new(
                    gamepad.value(Axis::LeftStickX),
                    gamepad.value(Axis::LeftStickY),
                );
                // An 8BitDo reports the d-pad as buttons rather than as an axis, so
                // fold it into the stick: the call sheet and the quarterback both
                // read one value.
                let press = |b: Button| if gamepad.is_pressed(b) { 1.0 } else { 0.0 };
                raw.x += press(Button::DPadRight) - press(Button::DPadLeft);
                raw.y += press(Button::DPadUp) - press(Button::DPadDown);
                raw = Vec2::new(raw.x.clamp(-1.0, 1.0), raw.y.clamp(-1.0, 1.0));
                // The right stick is the runner's body, and it is the same body
                // on every play: up is a shoulder, down is a dead stop, and the
                // sides are the jump cut.
                let rx = gamepad.value(Axis::RightStickX);
                let ry = gamepad.value(Axis::RightStickY);
                held.truck = ry > 0.55;
                held.stop = ry < -0.55;
                held.juke_r = rx > 0.55;
                held.juke_l = rx < -0.55;
                held.a = gamepad.is_pressed(Button::South);
                held.b = gamepad.is_pressed(Button::East);
                held.x = gamepad.is_pressed(Button::West);
                held.y = gamepad.is_pressed(Button::North);
                // Right trigger throws; on an 8BitDo in Switch mode that is ZR.
                // The right bumper is the defensive check, not a second throw.
                held.throw = gamepad.is_pressed(Button::RightTrigger2);
                held.select = gamepad.is_pressed(Button::Select);
                // The right trigger is the preview at the line and the throw once
                // the ball is out; the left bumper is the dummy count and the right
                // bumper is the defensive check.
                held.preview = gamepad.is_pressed(Button::RightTrigger2);
                held.dummy = gamepad.is_pressed(Button::LeftTrigger);
                held.shoulder = gamepad.is_pressed(Button::RightTrigger);
                held.start = gamepad.is_pressed(Button::Start);
            }

            // Dead zone, so a resting stick does not make the quarterback drift.
            let stick = if raw.length() < 0.22 { Vec2::ZERO } else { raw };

            let mut pressed = Buttons::default();
            edge(&mut pressed.a, held.a, self.prev.a);
            edge(&mut pressed.b, held.b, self.prev.b);
            edge(&mut pressed.x, held.x, self.prev.x);
            edge(&mut pressed.y, held.y, self.prev.y);
            edge(&mut pressed.throw, held.throw, self.prev.throw);
            edge(&mut pressed.select, held.select, self.prev.select);
            edge(&mut pressed.preview, held.preview, self.prev.preview);
            edge(&mut pressed.dummy, held.dummy, self.prev.dummy);
            edge(&mut pressed.shoulder, held.shoulder, self.prev.shoulder);
            edge(&mut pressed.start, held.start, self.prev.start);
            self.prev = held;

            Input { stick, held, pressed }
        }
    }

    fn edge(pressed: &mut bool, now: bool, before: bool) {
        *pressed = now && !before;
    }
}

#[cfg(not(feature = "gamepad"))]
mod backend {
    use super::{Buttons, Input};

    /// Placeholder used when the crate is built without controller support.
    pub struct Pad {
        held: Buttons,
        prev: Buttons,
    }

    impl Pad {
        pub fn open() -> Option<Pad> {
            None
        }

        pub fn name(&self) -> &str {
            "none"
        }

        pub fn connected(&self) -> bool {
            false
        }

        pub fn poll(&mut self) -> Input {
            self.prev = self.held;
            Input::default()
        }
    }
}

pub use backend::Pad;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edge_is_the_frame_the_button_went_down() {
        let mut held = Buttons::default();
        held.a = true;
        assert!(held.edges(Buttons::default()).a, "a fresh press is an edge");
        assert!(!held.edges(held).a, "a held button is not a second press");
        held.a = false;
        assert!(!held.edges(held).a, "letting go is not a press");
    }

    #[test]
    fn every_button_reports_its_own_edge() {
        let all = Buttons {
            truck: true,
            stop: true,
            juke_l: true,
            juke_r: true,
            a: true, b: true, x: true, y: true, throw: true,
            select: true, preview: true, dummy: true, shoulder: true, start: true,
        };
        let edges = all.edges(Buttons::default());
        assert_eq!(
            [edges.a, edges.b, edges.x, edges.y, edges.throw, edges.select,
             edges.preview, edges.dummy, edges.shoulder, edges.start],
            [true; 10]
        );
        // Holding everything for one more poll is no further edges at all.
        let again = all.edges(all);
        assert!(![again.a, again.b, again.x, again.y, again.throw, again.select,
                  again.preview, again.dummy, again.shoulder, again.start]
            .iter()
            .any(|e| *e));
    }
}

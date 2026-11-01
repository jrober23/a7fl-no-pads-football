//! The passing overlay: which men can be thrown to, where the ball is going to
//! come down, and how hard it is being thrown.
//!
//! Everything is drawn into the finished frame through the same camera the scene
//! was rendered with, so a badge sits on a man's head wherever he is on the field
//! and the aim guide runs along the grass rather than across the screen.

use a7fl_core::math::{Rgb, Vec3};

use crate::font;
use crate::raster::Frame;
use crate::scene::{Camera, Projector};

const INK: Rgb = Rgb::hex(0x0C0F14);
const WHITE: Rgb = Rgb::hex(0xF4F6F8);
const OPEN: Rgb = Rgb::hex(0x46C46E);
const TIGHT: Rgb = Rgb::hex(0xF2C14E);
const COVERED: Rgb = Rgb::hex(0xE4572E);
const MUTED: Rgb = Rgb::hex(0x9BA8B6);

/// How much room a receiver has, as the badge reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Open,
    Tight,
    Covered,
}

impl Tone {
    fn color(self) -> Rgb {
        match self {
            Tone::Open => OPEN,
            Tone::Tight => TIGHT,
            Tone::Covered => COVERED,
        }
    }
}

/// A controller button floating over a receiver's head.
#[derive(Clone, Copy, Debug)]
pub struct Badge {
    /// World position of the top of his head.
    pub at: Vec3,
    pub label: &'static str,
    pub tone: Tone,
    /// The man the pass is currently aimed at.
    pub aimed: bool,
}

/// The throw being wound up: where it will land, and where the man is.
#[derive(Clone, Copy, Debug)]
pub struct Guide {
    pub from: Vec3,
    pub receiver: Vec3,
    pub landing: Vec3,
}

#[derive(Clone, Debug)]
pub struct PassUi {
    pub badges: Vec<Badge>,
    pub guide: Option<Guide>,
    /// Meter reading 0..1 while a throw button is held.
    pub power: Option<f32>,
    /// The meter cannot carry the ball to the spot being asked for.
    pub short: bool,
    /// Meter reading that would just reach it.
    pub needed: Option<f32>,
}

impl PassUi {
    pub fn draw(&self, frame: &mut Frame, camera: &Camera, projector: &Projector) {
        let project = |p: Vec3| -> Option<(i32, i32)> {
            projector
                .project(camera.view(p))
                .map(|(x, y, _)| (x.round() as i32, y.round() as i32))
        };
        if let Some(guide) = &self.guide {
            draw_guide(frame, &project, guide, self.short);
        }
        for badge in &self.badges {
            draw_badge(frame, &project, badge);
        }
        if let Some(power) = self.power {
            draw_meter(frame, power, self.short, self.needed);
        }
    }
}

fn shade_for(color: Rgb) -> Rgb {
    if color.luma() > 150.0 {
        INK
    } else {
        WHITE
    }
}

fn draw_badge(frame: &mut Frame, project: &impl Fn(Vec3) -> Option<(i32, i32)>, badge: &Badge) {
    let Some((x, y)) = project(badge.at) else {
        return;
    };
    // Above the head, so the man himself is never covered.
    let (x, y) = (x, y - 34);
    let (w, h) = (24, 20);
    let color = badge.tone.color();

    // A stem down to the head, so it is clear whose button this is.
    frame.line(x, y + h, x, y + h + 12, color, 1);
    frame.fill_rect_alpha(x - w / 2 - 3, y - 3, w + 6, h + 6, INK, 0.45);
    if badge.aimed {
        frame.rounded_rect(x - w / 2 - 2, y - 2, w + 4, h + 4, 5, WHITE);
    }
    frame.rounded_rect(x - w / 2, y, w, h, 4, color);
    let text_y = y + (h - font::text_height(2) as i32) / 2;
    font::draw_centered(frame, x, text_y, badge.label, 2, shade_for(color), 2);
}

fn draw_guide(
    frame: &mut Frame,
    project: &impl Fn(Vec3) -> Option<(i32, i32)>,
    guide: &Guide,
    short: bool,
) {
    let Some((fx, fy)) = project(guide.from) else {
        return;
    };
    let Some((rx, ry)) = project(guide.receiver) else {
        return;
    };
    let Some((lx, ly)) = project(guide.landing) else {
        return;
    };

    // The man, and the gap between him and where the ball is going: that gap is
    // the lead, which is the whole point of aiming the throw.
    frame.line(rx, ry, lx, ly, MUTED.scale(0.75), 1);

    // Dotted flight path, thinning out as it goes.
    let steps = 14;
    for i in 1..=steps {
        if i % 2 == 0 {
            continue;
        }
        let t = i as f32 / steps as f32;
        let x = fx as f32 + (lx - fx) as f32 * t;
        let y = fy as f32 + (ly - fy) as f32 * t;
        let color = if short { COVERED } else { WHITE };
        frame.fill_rect_alpha(x as i32 - 1, y as i32 - 1, 3, 3, color, 0.65);
    }

    // Landing marker: a ring with crosshairs, which is where the ball comes down.
    let color = if short { COVERED } else { WHITE };
    frame.rounded_rect(lx - 9, ly - 4, 18, 8, 4, color);
    frame.fill_rect(lx - 1, ly - 8, 3, 3, color);
    frame.fill_rect(lx - 1, ly + 6, 3, 3, color);
    if short {
        font::draw_centered(frame, lx, ly + 12, "SHORT", 1, COVERED, 2);
    }
}

fn draw_meter(frame: &mut Frame, power: f32, short: bool, needed: Option<f32>) {
    let w = 220;
    let h = 10;
    let x = (frame.w as i32 - w) / 2;
    // Clear of the text bar the client draws along the bottom of the picture.
    let y = frame.h as i32 - 76;
    frame.fill_rect_alpha(x - 4, y - 16, w + 8, h + 22, INK, 0.70);
    font::draw_centered(frame, frame.w as i32 / 2, y - 13, "PASS POWER", 1, MUTED, 1);
    frame.fill_rect(x, y, w, h, Rgb::hex(0x28313A));
    let fill = (w as f32 * power.clamp(0.0, 1.0)) as i32;
    let color = if power < 0.5 {
        OPEN
    } else if power < 0.8 {
        TIGHT
    } else {
        COVERED
    };
    frame.fill_rect(x, y, fill, h, color);
    // Notch at the weight that would just carry the ball to the man.
    if let Some(needed) = needed {
        let nx = x + (w as f32 * needed.clamp(0.0, 1.0)) as i32;
        frame.fill_rect(nx - 1, y - 2, 2, h + 4, if short { COVERED } else { WHITE });
    }
    for i in 1..4 {
        frame.fill_rect(x + w * i / 4, y, 1, h, Rgb::hex(0x0C0F14));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a7fl_core::math::Vec3;

    fn camera() -> Camera {
        let mut cam = Camera::new(Vec3::new(0.0, 20.0, 14.0), 0.0, 0.0);
        cam.look_at(Vec3::new(0.0, 0.0, 0.0));
        cam
    }

    #[test]
    fn badges_and_the_meter_draw_onto_the_frame() {
        let cam = camera();
        let projector = Projector::new(320, 180, cam.fov);
        let mut frame = Frame::new(320, 180);
        frame.clear(Rgb::BLACK);
        let ui = PassUi {
            badges: vec![Badge {
                at: Vec3::new(2.0, 1.0, 2.1),
                label: "A",
                tone: Tone::Open,
                aimed: true,
            }],
            guide: Some(Guide {
                from: Vec3::new(-4.0, 0.0, 1.8),
                receiver: Vec3::new(6.0, 1.0, 1.0),
                landing: Vec3::new(8.0, 2.0, 0.1),
            }),
            power: Some(0.7),
            short: true,
            needed: Some(0.9),
        };
        ui.draw(&mut frame, &cam, &projector);
        let painted = frame.color.iter().filter(|c| **c != Rgb::BLACK).count();
        assert!(painted > 100, "the overlay drew almost nothing: {painted} px");
    }

    #[test]
    fn a_point_behind_the_camera_is_skipped_instead_of_wrapping() {
        let mut cam = Camera::new(Vec3::new(0.0, 0.0, 2.0), 0.0, 0.0);
        cam.look_at(Vec3::new(10.0, 0.0, 0.0));
        let projector = Projector::new(320, 180, cam.fov);
        let mut frame = Frame::new(320, 180);
        frame.clear(Rgb::BLACK);
        let ui = PassUi {
            badges: vec![Badge {
                at: Vec3::new(-40.0, 0.0, 2.1),
                label: "A",
                tone: Tone::Covered,
                aimed: false,
            }],
            guide: None,
            power: None,
            short: false,
            needed: None,
        };
        ui.draw(&mut frame, &cam, &projector);
        assert!(frame.color.iter().all(|c| *c == Rgb::BLACK), "drew a badge behind the camera");
    }
}

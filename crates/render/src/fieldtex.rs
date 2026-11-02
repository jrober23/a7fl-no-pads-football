//! Painted turf: a top-down texture of the field (100 x 37.5 yards of play,
//! two 10-yard end zones) that the 3D ground pass samples.
//!
//! Everything is painted at 12 pixels per yard: mown stripes, yard lines, hash
//! marks, painted numbers, end zone wordmarks and the home club's midfield mark.

use a7fl_core::field::{
    ENDZONE_DEPTH, FIELD_HALF_WIDTH, FIELD_LENGTH, HASH_Y,
};
use a7fl_core::math::{Rgb, Rng};
use a7fl_core::team::Team;

use crate::font::{self, draw_flat, draw_rot90};
use crate::raster::{Frame, Texture};

pub const TEX_X0: f32 = -15.0;
pub const TEX_X1: f32 = 115.0;
pub const TEX_Y0: f32 = -22.0;
pub const TEX_Y1: f32 = 22.0;
pub const PX_PER_YARD: f32 = 16.0;

pub const TURF_LIGHT: Rgb = Rgb::hex(0x3A7C41);
pub const TURF_DARK: Rgb = Rgb::hex(0x2E6735);
pub const PAINT: Rgb = Rgb::hex(0xF6F7F2);

fn px(x: f32) -> f32 {
    (x - TEX_X0) * PX_PER_YARD
}

fn py(y: f32) -> f32 {
    (y - TEX_Y0) * PX_PER_YARD
}

fn tx(yards: f32) -> i32 {
    (yards * PX_PER_YARD).round() as i32
}

/// Build the field texture. `left_team` paints the left end zone, `right_team`
/// the right one.
pub fn build_field_texture(left_team: &Team, right_team: &Team) -> Texture {
    let w = ((TEX_X1 - TEX_X0) * PX_PER_YARD) as usize;
    let h = ((TEX_Y1 - TEX_Y0) * PX_PER_YARD) as usize;
    let mut frame = Frame::new(w, h);
    let mut rng = Rng::new(0xF13E_D0);

    // Surrounding apron.
    frame.clear(Rgb::hex(0x1B241D));

    // Turf inside the end zones and between the goal lines.
    let turf_x0 = px(-ENDZONE_DEPTH);
    let turf_x1 = px(FIELD_LENGTH + ENDZONE_DEPTH);
    let turf_y0 = py(-FIELD_HALF_WIDTH);
    let turf_y1 = py(FIELD_HALF_WIDTH);
    frame.fill_rect(
        turf_x0 as i32,
        turf_y0 as i32,
        (turf_x1 - turf_x0) as i32,
        (turf_y1 - turf_y0) as i32,
        TURF_DARK,
    );

    // Mown stripes every five yards, including through the end zones. The edge
    // between two passes of the mower is a soft line on real turf, so it is
    // blended over a yard instead of being a hard seam.
    let mut band = -ENDZONE_DEPTH;
    let mut i = 0;
    while band < FIELD_LENGTH + ENDZONE_DEPTH {
        let x0 = px(band);
        let x1 = px(band + 5.0);
        let color = if i % 2 == 0 { TURF_LIGHT } else { TURF_DARK };
        frame.fill_rect(
            x0 as i32,
            turf_y0 as i32,
            (x1 - x0) as i32,
            (turf_y1 - turf_y0) as i32,
            color,
        );
        // Soft mow edge: a couple of pixels of the neighbouring tone.
        let feather = tx(0.9);
        for px_i in 0..feather {
            let t = px_i as f32 / feather as f32;
            frame.fill_rect_alpha(
                (x0 as i32) + px_i,
                turf_y0 as i32,
                1,
                (turf_y1 - turf_y0) as i32,
                color,
                1.0 - t,
            );
        }
        band += 5.0;
        i += 1;
    }

    // Turf grain: a little blade-to-blade variation so the surface isn't flat,
    // plus faint mower swirls that only show up close.
    for y in (turf_y0 as i32)..(turf_y1 as i32) {
        for x in (turf_x0 as i32)..(turf_x1 as i32) {
            let n = rng.range(-0.045, 0.045);
            let swirl = ((x as f32 * 0.35 + y as f32 * 0.9).sin() * 0.012)
                + ((y as f32 * 0.7).sin() * 0.008);
            let c = frame.get(x, y);
            frame.put(x, y, c.scale(1.0 + n + swirl));
        }
    }

    // Painted end zones.
    let left_color = left_team.primary.scale(0.9);
    let right_color = right_team.primary.scale(0.9);
    frame.fill_rect_alpha(
        turf_x0 as i32,
        turf_y0 as i32,
        (px(0.0) - turf_x0) as i32,
        (turf_y1 - turf_y0) as i32,
        left_color,
        0.55,
    );
    frame.fill_rect_alpha(
        px(FIELD_LENGTH) as i32,
        turf_y0 as i32,
        (turf_x1 - px(FIELD_LENGTH)) as i32,
        (turf_y1 - turf_y0) as i32,
        right_color,
        0.55,
    );

    // Boundary and yard lines.
    let line_w = 2;
    let draw_vline = |frame: &mut Frame, yards: f32, wide: i32| {
        let x = px(yards) as i32;
        frame.fill_rect(x, turf_y0 as i32, wide, (turf_y1 - turf_y0) as i32, PAINT);
    };
    // End lines and goal lines.
    draw_vline(&mut frame, -ENDZONE_DEPTH, line_w + 1);
    draw_vline(&mut frame, 0.0, line_w + 2);
    draw_vline(&mut frame, FIELD_LENGTH, line_w + 2);
    draw_vline(&mut frame, FIELD_LENGTH + ENDZONE_DEPTH, line_w + 1);
    // Sidelines.
    let sideline = 2;
    frame.fill_rect(
        turf_x0 as i32,
        turf_y0 as i32,
        (turf_x1 - turf_x0) as i32,
        sideline,
        PAINT,
    );
    frame.fill_rect(
        turf_x0 as i32,
        (turf_y1 as i32) - sideline,
        (turf_x1 - turf_x0) as i32,
        sideline,
        PAINT,
    );

    // Yard lines every five yards, heavier on the tens.
    for yl in 0..=100 {
        if yl % 5 != 0 {
            continue;
        }
        let wide = if yl % 10 == 0 { 3 } else { 1 };
        draw_vline(&mut frame, yl as f32, wide);
    }

    // Hash marks every yard, one yard long.
    for yl in 1..100 {
        if yl % 5 == 0 {
            continue;
        }
        let x = px(yl as f32) as i32;
        for side in [-1.0f32, 1.0] {
            let y = py(side * HASH_Y) as i32;
            frame.fill_rect(x, y - 4, 2, 9, PAINT);
        }
    }

    // Inbound lines, five yards in from each sideline: the coaching boxes.
    for side in [-1.0f32, 1.0] {
        let y = py(side * (FIELD_HALF_WIDTH - 5.0)) as i32;
        frame.fill_rect(turf_x0 as i32, y - 1, (turf_x1 - turf_x0) as i32, 1, PAINT);
    }

    // Painted numbers every ten yards, on both sides of the field. A number's
    // feet stand on the sideline it belongs to and its top points at midfield, so
    // a fan in that stand reads it left to right; the two sets are half a turn
    // apart, the way they are painted on a real field. They used to be turned
    // through the width of the field, which no stand could read.
    let numbers: [(f32, u32); 9] = [
        (10.0, 40),
        (20.0, 30),
        (30.0, 20),
        (40.0, 10),
        (50.0, 50),
        (60.0, 10),
        (70.0, 20),
        (80.0, 30),
        (90.0, 40),
    ];
    for (yard, value) in numbers {
        let label = format!("{value}");
        let scale = 4;
        let wide = font::text_width(&label, scale) as i32;
        for side in [-1.0f32, 1.0] {
            // Left to right for a fan on `side` runs toward `-side` in x, and the
            // top of the letters points away from that stand, at midfield.
            let dir = if side > 0.0 { -1 } else { 1 };
            let up = if side > 0.0 { -1 } else { 1 };
            let cx = px(yard) as i32;
            let x = if dir > 0 { cx - wide / 2 } else { cx + wide / 2 };
            // The feet sit three yards in from the paint, between the sideline and
            // the inbound line, so the number is clear of both.
            let y = py(side * (FIELD_HALF_WIDTH - 3.0)) as i32;
            draw_flat(&mut frame, x, y, &label, scale, Rgb::hex(0xE8EAE2), dir, up, 1);
        }
    }

    // End zone wordmarks, reading across the width of the field.
    let paint_endzone = |frame: &mut Frame, team: &Team, x_center: f32| {
        let name = team.name.to_uppercase();
        let scale = 5;
        let len = font::text_width(&name, scale) as f32;
        let y = (py(0.0) - len * 0.5) as i32;
        let x = px(x_center) as i32 - 18;
        let color = Rgb::hex(0xF6F7F2);
        draw_rot90(frame, x, y, &name, scale, color, 1, 2);
        // The club's own mark underneath.
        let mark = team.abbr.to_uppercase();
        let mlen = font::text_width(&mark, 3) as f32;
        draw_rot90(
            frame,
            x + 34,
            (py(0.0) - mlen * 0.5) as i32,
            &mark,
            3,
            team.accent,
            -1,
            1,
        );
    };
    paint_endzone(&mut frame, left_team, -ENDZONE_DEPTH * 0.5);
    paint_endzone(&mut frame, right_team, FIELD_LENGTH + ENDZONE_DEPTH * 0.5);

    // Midfield mark: a painted circle with the league wordmark.
    let cx = px(50.0) as i32;
    let cy = py(0.0) as i32;
    let radius = tx(6.0);
    for ring in 0..radius {
        let t = ring as f32 / radius as f32;
        let alpha = 0.55 * (1.0 - t);
        for a in 0..180 {
            let ang = a as f32 / 180.0 * std::f32::consts::TAU;
            let x = cx + (ang.cos() * ring as f32) as i32;
            let y = cy + (ang.sin() * ring as f32 * 0.62) as i32;
            frame.blend(x, y, Rgb::hex(0xE8EAE2), alpha * 0.35);
        }
    }
    // The home club's mark at midfield, not a league wordmark.
    let home = left_team.abbr.to_uppercase();
    font::draw_centered(&mut frame, cx, cy - 16, &home, 4, Rgb::hex(0xE8EAE2), 1);
    font::draw_centered(&mut frame, cx, cy + 18, "NO PADS", 2, Rgb::hex(0xE8EAE2), 1);

    // Goal post-less, just like the rulebook says. Nothing to draw.

    Texture {
        w: frame.w,
        h: frame.h,
        px: frame.color,
        wrap: false,
    }
}

/// UV of a field position inside the texture.
pub fn uv(x: f32, y: f32) -> (f32, f32) {
    ((x - TEX_X0) / (TEX_X1 - TEX_X0), (y - TEX_Y0) / (TEX_Y1 - TEX_Y0))
}

pub fn coverage() -> (f32, f32, f32, f32) {
    (TEX_X0, TEX_X1, TEX_Y0, TEX_Y1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use a7fl_core::team::league_teams;

    /// The numbers are painted for the stand they belong to: feet toward that
    /// sideline, top toward midfield, and straddling the yard line between the
    /// two digits.
    #[test]
    fn the_yard_numbers_stand_with_their_feet_on_their_sideline() {
        let teams = league_teams();
        let tex = build_field_texture(&teams[0], &teams[1]);
        for side in [-1.0f32, 1.0] {
            let (mut lo, mut hi, mut x_lo, mut x_hi) =
                (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
            let mut x = 28.8f32;
            while x <= 31.2 {
                // The yard line is white and crosses the whole width; the digits
                // sit either side of it.
                if (x - 30.0).abs() > 0.3 {
                    let mut y = 14.4f32;
                    while y <= 18.0 {
                        let u = (x - TEX_X0) / (TEX_X1 - TEX_X0);
                        let v = (y * side - TEX_Y0) / (TEX_Y1 - TEX_Y0);
                        let c = tex.sample(u, v);
                        if c.r > 170 && c.g > 170 {
                            lo = lo.min(y);
                            hi = hi.max(y);
                            x_lo = x_lo.min(x);
                            x_hi = x_hi.max(x);
                        }
                        y += 0.05;
                    }
                }
                x += 0.05;
            }
            assert!(hi > 0.0, "no number painted on side {side}");
            let feet = FIELD_HALF_WIDTH - hi;
            assert!(
                (2.6..3.5).contains(&feet),
                "the feet of the numbers are {feet:.2} yards from the sideline"
            );
            assert!(
                hi - lo > 1.0,
                "the number is only {:.2} yards tall",
                hi - lo
            );
            assert!(
                x_lo < 29.6 && x_hi > 30.4,
                "the number does not straddle the 30-yard line: {x_lo}..{x_hi}"
            );
            // The top of the letters is the far side of the band from the paint.
            assert!(
                FIELD_HALF_WIDTH - lo > 4.0,
                "the numbers are not turning their tops to midfield: {lo:.2}..{hi:.2}"
            );
        }
    }

    #[test]
    fn texture_is_painted_and_sized_sensibly() {
        let teams = league_teams();
        let tex = build_field_texture(&teams[0], &teams[1]);
        assert!(tex.w > 1000 && tex.h > 400);
        assert_eq!(tex.px.len(), tex.w * tex.h);
        // Turf green should dominate the middle of the field.
        let (u, v) = uv(50.0, 0.0);
        let c = tex.sample(u, v);
        assert!(c.g > c.r && c.g > c.b, "midfield should be green: {c:?}");
    }
}

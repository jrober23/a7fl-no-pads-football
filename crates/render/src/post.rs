//! The film treatment: what turns a clean wireframe-crisp render into a picture.
//!
//! Three passes, all of them cheap enough to run on a 640x360 canvas:
//!
//! * **Soft edges.** A hard polygon edge stepped a whole pixel at a time is the
//!   single strongest "early 3D" tell. This finds luma edges and blends across
//!   them, more where the two sides agree and less inside texture detail.
//! * **Glow.** Bright things bleed a little: the score bars, white lines in sun,
//!   floodlit jerseys. The bright pass is downsampled four times before it is
//!   blurred, so it costs a fraction of the frame.
//! * **Grade.** A gentle vignette and a touch more contrast, which is most of
//!   what separates a broadcast still from a raw render.
//!
//! It is deliberately applied to the *world* and never to the overlay: type and
//! panels are drawn afterwards, so they stay crisp while the picture goes soft.

use crate::raster::Frame;
use nopads_core::math::Rgb;

const BLOOM_THRESHOLD: f32 = 208.0;
const BLOOM_STRENGTH: f32 = 0.18;
const EDGE_SOFTNESS: f32 = 0.30;
const VIGNETTE: f32 = 0.12;

/// Run the whole treatment over a finished world picture.
pub fn finish(frame: &mut Frame) {
    if frame.w < 4 || frame.h < 4 {
        return;
    }
    soften_edges(frame);
    glow(frame);
    grade(frame);
}

fn luma(c: Rgb) -> f32 {
    0.299 * c.r as f32 + 0.587 * c.g as f32 + 0.114 * c.b as f32
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    Rgb::new(
        (a.r as f32 + (b.r as f32 - a.r as f32) * t) as u8,
        (a.g as f32 + (b.g as f32 - a.g as f32) * t) as u8,
        (a.b as f32 + (b.b as f32 - a.b as f32) * t) as u8,
    )
}

/// One pass over the picture, blending across luma edges.
fn soften_edges(frame: &mut Frame) {
    let (w, h) = (frame.w, frame.h);
    let src = frame.color.clone();
    let at = |x: usize, y: usize| src[y * w + x];
    let l = |x: usize, y: usize| luma(at(x, y));

    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let n = l(x, y - 1);
            let s = l(x, y + 1);
            let e = l(x + 1, y);
            let west = l(x - 1, y);
            let vertical = (n - s).abs();
            let horizontal = (e - west).abs();
            let edge = vertical.max(horizontal);
            if edge < 12.0 {
                continue;
            }
            // Across the edge, or along it? The stronger difference says which
            // way the edge runs, and the neighbour pair across it is the pair
            // that should be averaged.
            let (a, b) = if horizontal >= vertical {
                (at(x - 1, y), at(x + 1, y))
            } else {
                (at(x, y - 1), at(x, y + 1))
            };
            let t = (edge / 90.0).min(1.0) * EDGE_SOFTNESS;
            if t <= 0.02 {
                continue;
            }
            let average = mix(a, b, 0.5);
            frame.color[y * w + x] = mix(at(x, y), average, t);
        }
    }
}

/// Soft glow on the brightest parts of the picture.
fn glow(frame: &mut Frame) {
    let (w, h) = (frame.w, frame.h);
    const STEP: usize = 4;
    let bw = (w / STEP).max(1);
    let bh = (h / STEP).max(1);
    let mut bright = vec![[0f32; 3]; bw * bh];

    for by in 0..bh {
        for bx in 0..bw {
            let mut acc = [0f32; 3];
            let mut n = 0f32;
            for oy in 0..STEP {
                for ox in 0..STEP {
                    let x = (bx * STEP + ox).min(w - 1);
                    let y = (by * STEP + oy).min(h - 1);
                    let c = frame.color[y * w + x];
                    acc[0] += c.r as f32;
                    acc[1] += c.g as f32;
                    acc[2] += c.b as f32;
                    n += 1.0;
                }
            }
            let c = [acc[0] / n, acc[1] / n, acc[2] / n];
            let l = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
            let keep = ((l - BLOOM_THRESHOLD) / (255.0 - BLOOM_THRESHOLD)).clamp(0.0, 1.0);
            bright[by * bw + bx] = [c[0] * keep, c[1] * keep, c[2] * keep];
        }
    }

    // Two box blurs on the small buffer: a wide soft falloff for a few thousand
    // samples instead of the half million a full-size blur would touch.
    let mut blurred = bright.clone();
    for _ in 0..2 {
        for by in 0..bh {
            for bx in 0..bw {
                let mut acc = [0f32; 3];
                let mut n = 0f32;
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let x = bx as i32 + dx;
                        let y = by as i32 + dy;
                        if x < 0 || y < 0 || x >= bw as i32 || y >= bh as i32 {
                            continue;
                        }
                        let s = blurred[y as usize * bw + x as usize];
                        acc[0] += s[0];
                        acc[1] += s[1];
                        acc[2] += s[2];
                        n += 1.0;
                    }
                }
                bright[by * bw + bx] = [acc[0] / n, acc[1] / n, acc[2] / n];
            }
        }
        blurred.clone_from_slice(&bright);
    }

    for y in 0..h {
        let by = (y / STEP).min(bh - 1);
        for x in 0..w {
            let bx = (x / STEP).min(bw - 1);
            let s = blurred[by * bw + bx];
            if s[0] <= 0.0 && s[1] <= 0.0 && s[2] <= 0.0 {
                continue;
            }
            let c = frame.color[y * w + x];
            frame.color[y * w + x] = Rgb::new(
                (c.r as f32 + s[0] * BLOOM_STRENGTH).min(255.0) as u8,
                (c.g as f32 + s[1] * BLOOM_STRENGTH).min(255.0) as u8,
                (c.b as f32 + s[2] * BLOOM_STRENGTH).min(255.0) as u8,
            );
        }
    }
}

/// Contrast and a vignette: the last thing before the overlay goes on.
fn grade(frame: &mut Frame) {
    let (w, h) = (frame.w, frame.h);
    let cx = w as f32 / 2.0;
    let cy = h as f32 / 2.0;
    let far = (cx * cx + cy * cy).sqrt().max(1.0);
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - cx;
            let dy = (y as f32 - cy) * 0.9;
            let d = ((dx * dx + dy * dy).sqrt() / far).min(1.0);
            let v = 1.0 - VIGNETTE * d * d;
            let c = frame.color[y * w + x];
            frame.color[y * w + x] = Rgb::new(
                (c.r as f32 * v).min(255.0) as u8,
                (c.g as f32 * v).min(255.0) as u8,
                (c.b as f32 * v).min(255.0) as u8,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(w: usize, h: usize, c: Rgb) -> Frame {
        let mut f = Frame::new(w, h);
        f.color.fill(c);
        f
    }

    #[test]
    fn a_hard_edge_comes_out_with_middle_values() {
        // A black half and a white half, the way a field line meets the turf.
        let mut f = blank(32, 32, Rgb::new(0, 0, 0));
        for y in 0..32 {
            for x in 16..32 {
                f.color[y * 32 + x] = Rgb::new(255, 255, 255);
            }
        }
        soften_edges(&mut f);
        let inside = f.color[16 * 32 + 15];
        let outside = f.color[16 * 32 + 16];
        assert!(
            inside.r > 0 && inside.r < 255,
            "the edge was not softened: {:?}",
            inside
        );
        assert!(
            outside.r > 0 && outside.r < 255,
            "the other side of the edge was not softened: {:?}",
            outside
        );
        // The middles of both halves are left alone.
        assert_eq!(f.color[16 * 32 + 2].r, 0);
        assert_eq!(f.color[16 * 32 + 29].r, 255);
    }

    #[test]
    fn a_bright_patch_glows_into_the_dark_beside_it() {
        let mut f = blank(64, 64, Rgb::new(0, 0, 0));
        for y in 24..40 {
            for x in 24..40 {
                f.color[y * 64 + x] = Rgb::new(255, 255, 255);
            }
        }
        glow(&mut f);
        assert_eq!(f.color[32 * 64 + 32].r, 255, "the bright patch lost its white");
        let beside = f.color[32 * 64 + 22];
        assert!(beside.r > 0, "nothing bled out of the bright patch");
        assert!(beside.r < 255, "the glow is a solid slab, not a falloff");
    }

    #[test]
    fn the_grade_leaves_the_middle_alone_and_darkens_the_corners() {
        let mut f = blank(64, 64, Rgb::new(200, 200, 200));
        grade(&mut f);
        let middle = f.color[32 * 64 + 32].r;
        let corner = f.color[0].r;
        assert!(middle > 190, "the middle was dimmed: {middle}");
        assert!(corner < middle, "the corners are not darker: {corner} vs {middle}");
        assert!(corner > 120, "the vignette is a black border: {corner}");
    }

    #[test]
    fn a_tiny_frame_is_left_alone_instead_of_panicking() {
        for (w, h) in [(1, 1), (2, 3), (3, 2)] {
            let mut f = blank(w, h, Rgb::new(9, 9, 9));
            finish(&mut f);
            assert_eq!(f.color.len(), w * h);
        }
    }
}

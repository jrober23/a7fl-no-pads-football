//! The film treatment: what turns a clean wireframe-crisp render into a picture.
//!
//! Three effects over the world picture, before the overlay goes on:
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
//!
//! At 720p each of these is a walk over a million pixels, so the picture is
//! walked as few times as the effects allow: the snapshot the edge pass reads
//! back from is taken in the same sweep that builds the luma table, and the glow
//! and the grade share one sweep instead of a read and a write each. What
//! remains is memory traffic, which is why the arithmetic here was cut to the
//! bone -- the vignette works from the distance squared, so the half million
//! square roots a frame that arrived where they started are gone.

use crate::raster::Frame;
use nopads_core::math::Rgb;

const BLOOM_THRESHOLD: f32 = 238.0;
const BLOOM_STRENGTH: f32 = 0.06;
const EDGE_SOFTNESS: f32 = 0.20;
const VIGNETTE: f32 = 0.12;

/// The buffers the treatment needs, kept between frames.
///
/// Each of these is megabytes at 720p and every one of them was being allocated
/// and freed sixty times a second. Post cost the same whether the arithmetic in
/// it was cheap or expensive, which is what a pass limited by getting the pixels
/// in and out of memory looks like; a reused buffer is one less thing to do to
/// them.
#[derive(Default)]
struct Scratch {
    src: Vec<Rgb>,
    luma: Vec<f32>,
    bright: Vec<[f32; 3]>,
    blurred: Vec<[f32; 3]>,
}

thread_local! {
    static SCRATCH: std::cell::RefCell<Scratch> =
        std::cell::RefCell::new(Scratch::default());
}

/// How many bands of the picture to work on at once.
fn bands() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(8)
}

/// Run the whole treatment over a finished world picture.
pub fn finish(frame: &mut Frame) {
    if frame.w < 4 || frame.h < 4 {
        return;
    }
    soften_edges(frame);
    glow_and_grade(frame);
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
    if w < 3 || h < 3 {
        return;
    }
    // The edge test asks whether there is an edge between two neighbours, and a
    // block of four pixels answers that as well as one does. The luma plane is
    // half the size of the picture; the blending is still per pixel, because
    // that is where the picture is.
    let (lw, lh) = (w.div_ceil(2), h.div_ceil(2));
    SCRATCH.with(|pad| {
    let mut pad = pad.borrow_mut();
    let Scratch { src, luma: luma_plane, .. } = &mut *pad;
    src.resize(w * h, Rgb::BLACK);
    src.clone_from_slice(&frame.color);
    luma_plane.resize(lw * lh, 0.0);
    {
        let color = &frame.color;
        let rows = lh.div_ceil(bands()).max(1);
        std::thread::scope(|scope| {
            for (i, band) in luma_plane.chunks_mut(rows * lw).enumerate() {
                scope.spawn(move || {
                    let y0 = i * rows;
                    for local in 0..band.len() / lw {
                        let y = (y0 + local) * 2;
                        for hx in 0..lw {
                            let x = hx * 2;
                            let mut sum = 0.0;
                            for oy in 0..2 {
                                for ox in 0..2 {
                                    let px = (x + ox).min(w - 1);
                                    let py = (y + oy).min(h - 1);
                                    sum += luma(color[py * w + px]);
                                }
                            }
                            band[local * lw + hx] = sum / 4.0;
                        }
                    }
                });
            }
        });
    }
    let src = &src;
    let luma_plane = &luma_plane;
    let rows = h.div_ceil(bands()).max(1);
    std::thread::scope(|scope| {
        for (i, band) in frame.color.chunks_mut(rows * w).enumerate() {
            scope.spawn(move || {
                let y0 = i * rows;
                for local in 0..band.len() / w {
                    let y = y0 + local;
                    if y == 0 || y >= h - 1 {
                        continue;
                    }
                    let hy = y / 2;
                    for x in 1..w - 1 {
                        let here = y * w + x;
                        let hx = x / 2;
                        let at = hy * lw + hx;
                        let up = if hy > 0 { luma_plane[at - lw] } else { luma_plane[at] };
                        let down = if hy + 1 < lh { luma_plane[at + lw] } else { luma_plane[at] };
                        let left = if hx > 0 { luma_plane[at - 1] } else { luma_plane[at] };
                        let right = if hx + 1 < lw { luma_plane[at + 1] } else { luma_plane[at] };
                        let vertical = (up - down).abs();
                        let horizontal = (right - left).abs();
                        let edge = vertical.max(horizontal);
                        if edge < 12.0 {
                            continue;
                        }
                        let (a, b) = if horizontal >= vertical {
                            (src[here - 1], src[here + 1])
                        } else {
                            (src[here - w], src[here + w])
                        };
                        let t = (edge / 90.0).min(1.0) * EDGE_SOFTNESS;
                        if t <= 0.02 {
                            continue;
                        }
                        let average = mix(a, b, 0.5);
                        band[local * w + x] = mix(src[here], average, t);
                    }
                }
            });
        }
    });
    });
}

/// The glow on the brightest parts of the picture, then the grade over it.
///
/// These are two effects with one sweep between them: the glow used to write
/// every pixel back to memory and the grade immediately read every pixel and
/// write it again, which at 720p is a dozen megabytes of traffic to do two
/// multiplications.
fn glow_and_grade(frame: &mut Frame) {
    let (w, h) = (frame.w, frame.h);
    const STEP: usize = 4;
    let bw = (w / STEP).max(1);
    let bh = (h / STEP).max(1);
    SCRATCH.with(|pad| {
    let mut pad = pad.borrow_mut();
    let Scratch { bright, blurred, .. } = &mut *pad;
    bright.resize(bw * bh, [0f32; 3]);
    let rows = bh.div_ceil(bands()).max(1);
    {
        let color = &frame.color;
        std::thread::scope(|scope| {
            for (i, band) in bright.chunks_mut(rows * bw).enumerate() {
                scope.spawn(move || {
                    let y0 = i * rows;
                    for local in 0..band.len() / bw {
                        let by = y0 + local;
                        for bx in 0..bw {
                            let mut acc = [0f32; 3];
                            let mut n = 0f32;
                            for oy in 0..STEP {
                                for ox in 0..STEP {
                                    let x = (bx * STEP + ox).min(w - 1);
                                    let y = (by * STEP + oy).min(h - 1);
                                    let c = color[y * w + x];
                                    acc[0] += c.r as f32;
                                    acc[1] += c.g as f32;
                                    acc[2] += c.b as f32;
                                    n += 1.0;
                                }
                            }
                            let c = [acc[0] / n, acc[1] / n, acc[2] / n];
                            let l = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
                            let keep = ((l - BLOOM_THRESHOLD) / (255.0 - BLOOM_THRESHOLD))
                                .clamp(0.0, 1.0);
                            band[local * bw + bx] = [c[0] * keep, c[1] * keep, c[2] * keep];
                        }
                    }
                });
            }
        });
    }

    // Two box blurs on the small buffer: a wide soft falloff for a few thousand
    // samples instead of the half million a full-size blur would touch.
    blurred.resize(bw * bh, [0f32; 3]);
    blurred.copy_from_slice(bright);
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

    // The vignette is a function of distance *squared*: the pass used to take a
    // square root and square the result straight back, half a million square
    // roots a frame to arrive where it started. The only place the root matters
    // is the clamp at the corners, which is a floor on the factor.
    let cx = w as f32 / 2.0;
    let cy = h as f32 / 2.0;
    let far = (cx * cx + cy * cy).sqrt().max(1.0);
    let far2 = far * far;
    let floor = (((1.0 - VIGNETTE) * 65536.0).round() as u32).min(65536);
    let mut across = vec![0f32; w];
    for (x, s) in across.iter_mut().enumerate() {
        let dx = x as f32 - cx;
        *s = dx * dx;
    }

    let blurred = &blurred;
    let across = &across;
    let rows = h.div_ceil(bands()).max(1);
    std::thread::scope(|scope| {
        for (i, band) in frame.color.chunks_mut(rows * w).enumerate() {
            scope.spawn(move || {
                let y0 = i * rows;
                for local in 0..band.len() / w {
                    let y = y0 + local;
                    let by = (y / STEP).min(bh - 1);
                    let dy = y as f32 - cy;
                    let down = dy * dy * 0.81;
                    for x in 0..w {
                        let mut c = band[local * w + x];
                        let s = blurred[by * bw + (x / STEP).min(bw - 1)];
                        if s[0] > 0.0 || s[1] > 0.0 || s[2] > 0.0 {
                            c = Rgb::new(
                                (c.r as f32 + s[0] * BLOOM_STRENGTH).min(255.0) as u8,
                                (c.g as f32 + s[1] * BLOOM_STRENGTH).min(255.0) as u8,
                                (c.b as f32 + s[2] * BLOOM_STRENGTH).min(255.0) as u8,
                            );
                        }
                        let d2 = (across[x] + down) / far2;
                        // Sixteen bits of factor, so the multiply stays an
                        // integer one and the colour a shift.
                        let v = if d2 >= 1.0 {
                            floor
                        } else {
                            (((1.0 - VIGNETTE * d2) * 65536.0).round() as u32).min(65536)
                        };
                        band[local * w + x] = Rgb::new(
                            ((c.r as u32 * v) >> 16) as u8,
                            ((c.g as u32 * v) >> 16) as u8,
                            ((c.b as u32 * v) >> 16) as u8,
                        );
                    }
                }
            });
        }
    });
    });
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
        glow_and_grade(&mut f);
        assert_eq!(f.color[32 * 64 + 32].r, 255, "the bright patch lost its white");
        let beside = f.color[32 * 64 + 22];
        assert!(beside.r > 0, "nothing bled out of the bright patch");
        assert!(beside.r < 255, "the glow is a solid slab, not a falloff");
    }

    #[test]
    fn the_grade_leaves_the_middle_alone_and_darkens_the_corners() {
        let mut f = blank(64, 64, Rgb::new(200, 200, 200));
        glow_and_grade(&mut f);
        let middle = f.color[32 * 64 + 32].r;
        let corner = f.color[0].r;
        assert!(middle > 190, "the middle was dimmed: {middle}");
        assert!(corner < middle, "the corners are not darker: {corner} vs {middle}");
        assert!(corner > 120, "the vignette is a black border: {corner}");
    }

    #[test]
    fn the_cheap_vignette_lands_where_the_square_root_did() {
        // The squared-distance grade has to be the same picture as the one that
        // took a root per pixel and squared it, not merely a similar one.
        let (w, h) = (128, 72);
        let mut fast = blank(w, h, Rgb::new(255, 255, 255));
        glow_and_grade(&mut fast);
        let cx = w as f32 / 2.0;
        let cy = h as f32 / 2.0;
        let far = (cx * cx + cy * cy).sqrt().max(1.0);
        for y in 0..h {
            for x in 0..w {
                let dx = x as f32 - cx;
                let dy = (y as f32 - cy) * 0.9;
                let d = ((dx * dx + dy * dy).sqrt() / far).min(1.0);
                let honest = (255.0 * (1.0 - VIGNETTE * d * d)).min(255.0) as u8;
                let got = fast.color[y * w + x].r;
                assert!(
                    got.abs_diff(honest) <= 1,
                    "at {x},{y}: {got} against {honest}"
                );
            }
        }
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

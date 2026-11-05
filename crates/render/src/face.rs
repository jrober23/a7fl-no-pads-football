//! A face painted instead of modelled.
//!
//! The head is a good surface and the features on it are honest geometry: a ball
//! for an eye, a bridge for a nose, a seam for a mouth. On the field nobody can
//! see any of it. A man is forty pixels tall and his whole face is six, so an eye
//! that cost a sphere, two lids and a socket to build lands as a single smudge --
//! and when the camera does come close, the beads read as a doll rather than a
//! face.
//!
//! This is the other trade, the one a LEGO minifigure makes: the features are ink
//! on skin, laid on the skull as one sheet. It is crisp at any size, costs a few
//! hundred texels instead of a few hundred triangles, and cannot fall apart into
//! separate pieces from any angle.
//!
//! `u` runs across the face and `v` from the chin end of the sheet up toward the
//! forehead, matching the rectangle the head lays it on.

use std::sync::{Arc, Mutex, OnceLock};

use nopads_core::math::Rgb;

use crate::raster::Texture;

/// Texels across the face. The sheet covers a head about twenty pixels wide in a
/// broadcast frame, so this is already more than the picture can spend.
pub const W: usize = 64;
pub const H: usize = 64;

/// The painted face for a man with this skin.
///
/// Painted once per tone and shared: a roster has a handful of tones between
/// them, and every man is rebuilt from scratch every frame.
pub fn face_texture(skin: Rgb) -> Arc<Texture> {
    static CACHE: OnceLock<Mutex<Vec<((u8, u8, u8), Arc<Texture>)>>> = OnceLock::new();
    let key = (skin.r, skin.g, skin.b);
    let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
    {
        let held = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, tex)) = held.iter().find(|(k, _)| *k == key) {
            return tex.clone();
        }
    }
    let tex = Arc::new(paint(skin));
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((key, tex.clone()));
    tex
}

fn paint(skin: Rgb) -> Texture {
    let mut tex = Texture::new(W, H, skin);
    let shadow = skin.scale(0.86);
    let ink = Rgb::hex(0x241A16);
    let white = Rgb::hex(0xF2EDE3);
    let iris = Rgb::hex(0x4A2F1E);

    // Socket and cheek shading. Not lighting -- the colour of the surface, which
    // is what stops a painted face reading as a poster stuck to a ball.
    for y in 0..H {
        let v = y as f32 / H as f32;
        for x in 0..W {
            let u = x as f32 / W as f32;
            let socket = 0.16 * bump(u, 0.34, 0.10) * bump(v, 0.60, 0.10)
                + 0.16 * bump(u, 0.66, 0.10) * bump(v, 0.60, 0.10);
            let jaw = 0.10 * bump(u, 0.50, 0.22) * bump(v, 0.10, 0.10);
            let temple = 0.07 * (bump(u, 0.04, 0.10) + bump(u, 0.96, 0.10));
            tex.set(x as i32, y as i32, skin.scale(1.0 - socket - jaw - temple));
        }
    }

    // The nose is a shadow and a highlight down the middle. Painted, it still
    // reads as a nose from the front, which is the only angle this face is for.
    for y in 20..40 {
        for x in 29..35 { tex.set(x, y, shadow); }
    }
    for x in 30..34 { tex.set(x, 40, skin); }

    // Eyes: an almond of white, an iris, a pupil, and a lid line over the top.
    for (cx, _side) in [(21i32, -1.0f32), (43, 1.0)] {
        for dy in -3i32..=3 {
            // Rounded ends, so an eye is an eye and not a slot.
            let wide = if dy.abs() == 3 { 3 } else { 5 };
            for dx in -wide..=wide {
                let d = (dx as f32 / wide as f32).powi(2) + (dy as f32 / 3.0f32).powi(2);
                if d <= 1.0 {
                    tex.set(cx + dx, 38 + dy, white);
                }
            }
        }
        for dy in -2i32..=2 {
            for dx in -2i32..=2 {
                if dx * dx + dy * dy <= 5 { tex.set(cx + dx, 38 + dy, iris); }
            }
        }
        tex.set(cx, 38, ink);
        tex.set(cx + 1, 38, ink);
        // The lid over the top: one line of ink is the whole difference between
        // an eye looking at you and an eye floating.
        for dx in -5..=5 { tex.set(cx + dx, 34, ink); }
        // Brow, thicker and angled in toward the bridge.
        for dx in -7i32..=7 {
            let lift = (dx.abs() as f32 / 7.0 * 1.6) as i32;
            for dy in 0..=1 {
                tex.set(cx + dx, 29 - lift + dy, ink.scale(0.85));
            }
        }
    }

    // Mouth: a line, with the lower lip a shade off the skin under it.
    for x in 25..=39 {
        tex.set(x, 17, ink.scale(0.9));
    }
    for x in 27..=37 {
        tex.set(x, 16, skin.scale(0.78));
        tex.set(x, 15, skin.scale(0.9));
    }
    tex
}

/// A smooth one-dimensional bump, for shading masks.
fn bump(x: f32, at: f32, width: f32) -> f32 {
    let d = (x - at) / width.max(1e-3);
    (-d * d).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_eyes_are_dark_and_the_skin_between_them_is_not() {
        let tex = paint(Rgb::hex(0x9A6242));
        // Inside an eye there is ink; between the eyes there is skin.
        let eye = tex.get(21, 38);
        let between = tex.get(32, 38);
        assert!(eye.luma() < between.luma() + 10.0, "the eye at {} is not darker than the bridge at {}", eye.luma(), between.luma());
        assert!(eye.luma() < 140.0, "the pupil is not dark: {}", eye.luma());
    }

    #[test]
    fn a_face_is_painted_once_per_tone() {
        let a = face_texture(Rgb::hex(0x6B4530));
        let b = face_texture(Rgb::hex(0x6B4530));
        assert!(Arc::ptr_eq(&a, &b), "the same tone was painted twice");
    }
}

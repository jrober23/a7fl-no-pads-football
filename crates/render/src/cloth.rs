//! The cloth a jersey is cut from.
//!
//! The shirt used to be vertex colour: every face of the torso loft was given
//! one flat shade, so the weave, the seams and the stitch along the hem had
//! nowhere to live and a jersey read as a painted tube. This paints the shirt
//! into a texture instead, once per club, and the torso is mapped onto it.
//!
//! `u` runs around the body and `v` up it, so the pattern is wrapped on like
//! cloth rather than guessed face by face. The texture carries what geometry
//! cannot afford: a knit two pixels wide, shadowed seams where the panels join,
//! a stitch line at the hem, and the club's pattern in the cloth itself.

use std::sync::{Arc, Mutex, OnceLock};

use nopads_core::math::Rgb;
use nopads_core::team::{JerseyPattern, Uniform};

use crate::raster::Texture;

/// Texels across the shirt. A man's chest is about thirty pixels wide on the
/// canvas, so anything past this is spent on a weave nobody can see.
pub const W: usize = 64;
/// Texels up the shirt: the knit runs vertically, so the extra resolution is all
/// in the direction the grain runs.
pub const H: usize = 128;

/// The jersey pattern for one band of the shirt at one place round the body.
///
/// This is the single table both paths read: the faces of a distant man are
/// coloured from it, and the cloth of a near one is painted with it, so a hoop
/// cannot land on a different band of the shirt depending on how far away he is
/// standing.
pub fn pattern_color(uniform: &Uniform, band: usize, bands: usize, lateral: f32) -> Rgb {
    let jersey = uniform.jersey;
    let accent = uniform.jersey_accent;
    match uniform.pattern {
        JerseyPattern::Solid => jersey,
        // A yoke across the top of the shoulders and the sleeves.
        JerseyPattern::ShoulderStripe => {
            if band + 1 >= bands && lateral > 0.45 {
                accent
            } else {
                jersey
            }
        }
        JerseyPattern::SidePanel => {
            if lateral > 0.84 {
                accent
            } else {
                jersey
            }
        }
        // A band across the chest, which is the sash reading at this distance.
        JerseyPattern::Sash => {
            if band == bands / 3 {
                accent
            } else {
                jersey
            }
        }
        JerseyPattern::Hoops => {
            if band % 2 == 1 {
                accent
            } else {
                jersey
            }
        }
    }
}

/// Paint a club's shirt.
pub fn paint(uniform: &Uniform, bands: usize) -> Texture {
    let mut tex = Texture::new(W, H, uniform.jersey);
    let salt = seed_of(uniform, bands);
    for y in 0..H {
        let v = (y as f32 + 0.5) / H as f32;
        let band = ((v * bands as f32) as usize).min(bands.saturating_sub(1));
        for x in 0..W {
            let u = (x as f32 + 0.5) / W as f32;
            let lateral = (u * std::f32::consts::TAU).sin().abs();
            let mut c = pattern_color(uniform, band, bands, lateral);

            // The knit: a jersey is a grid of loops, so brightness alternates
            // across the grain faster than it does up it.
            let knit = match x % 4 {
                0 => 0.962,
                1 => 1.0,
                2 => 1.018,
                _ => 0.984,
            };
            // Folds, soft and vertical: the geometry used to fake cloth with a
            // hard step per face, which read as faceting rather than fabric.
            let fold = 1.0 + 0.035 * (u * 9.0 * std::f32::consts::TAU).sin();
            // The seams where the panels are joined, one at each side.
            let seam = if x == 0 || x == W / 2 { 0.87 } else { 1.0 };
            // The hem: a stitch line, then the band of shirt turned under it.
            let hem = if y < 2 { 0.78 } else if y < 4 { 1.07 } else { 1.0 };
            // Baked from the bottom up: cloth tucked into a waistband is in its
            // own shadow however the sun falls.
            let baked = 0.93 + 0.07 * v;
            let grit = 0.985 + 0.03 * noise(x, y, salt);

            c = c.scale(knit * fold * seam * hem * baked * grit);
            tex.set(x as i32, y as i32, c);
        }
    }
    tex
}

/// The shirt for this club, painted once and shared by every man wearing it.
///
/// The figure is rebuilt every frame, so this has to be free the second time:
/// a club's shirt does not change between frames, and two clubs in the same
/// colours share one texture.
pub fn jersey_texture(uniform: &Uniform, bands: usize) -> Arc<Texture> {
    static CACHE: OnceLock<Mutex<Vec<(u64, Arc<Texture>)>>> = OnceLock::new();
    let key = seed_of(uniform, bands) as u64;
    let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
    let mut held = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, tex)) = held.iter().find(|(k, _)| *k == key) {
        return tex.clone();
    }
    let tex = Arc::new(paint(uniform, bands));
    held.push((key, tex.clone()));
    tex
}

fn pattern_byte(pattern: JerseyPattern) -> u8 {
    match pattern {
        JerseyPattern::Solid => 0,
        JerseyPattern::ShoulderStripe => 1,
        JerseyPattern::SidePanel => 2,
        JerseyPattern::Sash => 3,
        JerseyPattern::Hoops => 4,
    }
}

fn seed_of(uniform: &Uniform, bands: usize) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let bytes = [
        uniform.jersey.r,
        uniform.jersey.g,
        uniform.jersey.b,
        uniform.jersey_accent.r,
        uniform.jersey_accent.g,
        uniform.jersey_accent.b,
        pattern_byte(uniform.pattern),
        bands as u8,
    ];
    for b in bytes {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    h
}

/// Deterministic value noise. The shirt has to look the same on every replay of
/// every play, so this cannot be a random number generator with hidden state.
fn noise(x: usize, y: usize, salt: u32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(0x9E37_79B9)
        ^ (y as u32).wrapping_mul(0x85EB_CA6B)
        ^ salt;
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    (h & 0xFFFF) as f32 / 65535.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use nopads_core::team::league_teams;

    fn shirt(pattern: JerseyPattern) -> Texture {
        let mut uniform = league_teams()[0].home;
        uniform.pattern = pattern;
        paint(&uniform, 6)
    }

    /// The middle of a band of the shirt, well clear of the seams and the hem.
    fn at(tex: &Texture, band: usize, bands: usize) -> Rgb {
        let v = (band as f32 + 0.5) / bands as f32;
        tex.get((W as f32 * 0.3) as i32, (v * H as f32) as i32)
    }

    fn distance(a: Rgb, b: Rgb) -> i32 {
        (a.r as i32 - b.r as i32).abs() + (a.g as i32 - b.g as i32).abs() + (a.b as i32 - b.b as i32).abs()
    }

    #[test]
    fn a_hooped_shirt_bands_in_its_texture() {
        let tex = shirt(JerseyPattern::Hoops);
        let near = distance(at(&tex, 0, 6), at(&tex, 1, 6));
        assert!(near > 60, "the hoops are only {near} apart in the cloth");
    }

    #[test]
    fn a_solid_shirt_does_not_band() {
        let tex = shirt(JerseyPattern::Solid);
        let near = distance(at(&tex, 0, 6), at(&tex, 1, 6));
        assert!(near < 30, "a solid shirt varies by {near} between bands");
    }

    #[test]
    fn the_cloth_has_a_grain_the_flat_colour_never_had() {
        let tex = shirt(JerseyPattern::Solid);
        let row = (0..24).map(|x| tex.get(x * 2, 60)).collect::<Vec<_>>();
        let loudest = row
            .iter()
            .flat_map(|a| row.iter().map(move |b| distance(*a, *b)))
            .max()
            .unwrap_or(0);
        assert!(loudest > 10, "the weave is invisible: at most {loudest} between texels");
    }

    #[test]
    fn the_same_club_paints_its_shirt_once() {
        let uniform = league_teams()[0].home;
        let a = jersey_texture(&uniform, 6);
        let b = jersey_texture(&uniform, 6);
        assert!(Arc::ptr_eq(&a, &b), "the shirt was painted twice");
    }
}

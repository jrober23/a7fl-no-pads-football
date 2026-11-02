//! The head.
//!
//! Built as one lofted surface rather than a ball with parts stuck on it: the
//! profile runs from under the chin to the crown, and the cross-section at each
//! height is an ellipse that is flattened across the face, wider at the
//! cheekbones, pinched at the temples and cut back at the jaw. The features are
//! laid on that surface, at the depth the surface actually is, the same way the
//! jersey number is painted on the cloth: eyes set into sockets, a nose with a
//! bridge and wings, lips with a line between them, ears on the sides of the
//! skull, and hair that starts at a hairline instead of a bowl.
//!
//! Everything is in the figure's own frame: `+x` is the way he faces, `+y` his
//! left, `+z` up. The camera is the only thing that turns.

use a7fl_core::math::{Rgb, Vec3};
use a7fl_core::team::{HairStyle, Player};

use crate::scene::Mesh;

/// Where the face is on a head of radius `hr`, as `(height fraction, half width,
/// front depth, back depth)`, all but the height in units of `hr`.
///
/// These are the numbers that decide whether it reads as a man or a mannequin.
/// The jaw is narrow and the cranium is wide, the cheekbones are the widest point,
/// and the face is shallower than the back of the skull, which is what gives a
/// head a face and a profile instead of an egg.
const PROFILE: [(f32, f32, f32, f32); 14] = [
    (0.00, 0.28, 0.32, 0.24),
    (0.05, 0.49, 0.50, 0.36),
    (0.11, 0.60, 0.58, 0.47),
    (0.18, 0.65, 0.62, 0.55),
    (0.26, 0.685, 0.66, 0.61),
    (0.34, 0.710, 0.69, 0.66),
    (0.42, 0.735, 0.705, 0.70),
    (0.50, 0.745, 0.715, 0.735),
    (0.58, 0.740, 0.725, 0.765),
    (0.66, 0.725, 0.735, 0.78),
    (0.74, 0.695, 0.715, 0.775),
    (0.84, 0.635, 0.665, 0.735),
    (0.93, 0.525, 0.55, 0.63),
    (1.00, 0.320, 0.34, 0.40),
];

fn profile_at(v: f32) -> (f32, f32, f32) {
    let v = v.clamp(0.0, 1.0);
    let mut i = 0;
    while i + 2 < PROFILE.len() && PROFILE[i + 1].0 <= v {
        i += 1;
    }
    let (v0, w0, f0, b0) = PROFILE[i];
    let (v1, w1, f1, b1) = PROFILE[(i + 1).min(PROFILE.len() - 1)];
    let t = ((v - v0) / (v1 - v0).max(1e-4)).clamp(0.0, 1.0);
    (
        w0 + (w1 - w0) * t,
        f0 + (f1 - f0) * t,
        b0 + (b1 - b0) * t,
    )
}

fn gauss(x: f32, width: f32) -> f32 {
    (-(x / width).powi(2)).exp()
}

/// A head surface, built into the same mesh as the body so it moves with it.
pub struct Head<'a> {
    pub center: Vec3,
    pub radius: f32,
    pub player: &'a Player,
    pub skin: Rgb,
    pub accent: Rgb,
    /// The balaclava, in club colour. Every man wears one.
    pub mask: Option<Rgb>,
}

impl<'a> Head<'a> {
    fn hr(&self) -> f32 {
        self.radius
    }

    /// The surface itself: `u` runs around the head (0 is straight ahead),
    /// `v` from under the chin at 0 to the crown at 1.
    fn point(&self, u: f32, v: f32) -> Vec3 {
        let hr = self.hr();
        let (w, f, b) = profile_at(v);
        let (su, cu) = u.sin_cos();
        // Front and back have different depths, so the face is where it should be
        // and the skull still has a back to it.
        let mut x = if cu > 0.0 { f * cu } else { b * cu };
        let mut y = w * su;
        if cu > 0.0 {
            // The face is flattened across its width. A pure ellipse has the
            // cheeks curving away from the lens before the eyes, which is exactly
            // the look of a ball with a face drawn on it.
            x *= 1.0 - 0.16 * su * su;
            // Brow ridge over the eyes, the one hard edge on a human face.
            x += 0.060 * gauss(v - 0.700, 0.055) * cu * cu;
            // Chin, carried forward from the jaw.
            if v < 0.14 {
                x += (0.14 - v) / 0.14 * 0.055 * cu.max(0.0) * cu.max(0.0);
            }
        }
        // Cheekbones, the widest point of the face and the reason a face has a
        // structure under the skin.
        let cheek = 0.055 * gauss(v - 0.500, 0.075) * gauss(su.abs() - 0.72, 0.26);
        y += cheek * su.signum();
        // A slight temple hollow above them, so the shadow at the side of the
        // face is not a straight line down from the brow.
        let temple = 0.030 * gauss(v - 0.780, 0.070) * gauss(su.abs() - 0.85, 0.22);
        y -= temple * su.signum();
        // Under the jaw the skull tucks in toward the neck.
        if v < 0.10 {
            y *= 0.82 + 0.18 * (v / 0.10);
        }
        Vec3::new(
            self.center.x + x * hr,
            self.center.y + y * hr,
            self.center.z + (v * 1.95 - 0.97) * hr,
        )
    }

    fn outward(&self, p: Vec3) -> Vec3 {
        (p - self.center).normalize_or_zero()
    }

    /// A quadrilateral sheet over a parameter rectangle, with normals taken from
    /// the surface rather than the facet, so the light runs across the jaw instead
    /// of stepping around it.
    fn sheet(
        mesh: &mut Mesh,
        rings: usize,
        segs: usize,
        f: &dyn Fn(f32, f32) -> Vec3,
        color: &dyn Fn(f32, f32) -> Rgb,
        core: Vec3,
        shade: f32,
    ) {
        let d = 0.006f32;
        let normal = |u: f32, v: f32| -> Vec3 {
            let p = f(u, v);
            let du = f(u + d, v) - f(u - d, v);
            let dv = f(u, v + d) - f(u, v - d);
            let mut n = du.cross(dv).normalize_or_zero();
            if n.length() < 0.5 {
                n = (p - core).normalize_or_zero();
            }
            if n.dot(p - core) < 0.0 {
                n = n * -1.0;
            }
            n
        };
        for r in 0..rings {
            let v0 = r as f32 / rings as f32;
            let v1 = (r + 1) as f32 / rings as f32;
            for s in 0..segs {
                let u0 = s as f32 / segs as f32 * std::f32::consts::TAU;
                let u1 = (s + 1) as f32 / segs as f32 * std::f32::consts::TAU;
                mesh.quad_curved(
                    f(u0, v0),
                    normal(u0, v0),
                    f(u1, v0),
                    normal(u1, v0),
                    f(u1, v1),
                    normal(u1, v1),
                    f(u0, v1),
                    normal(u0, v1),
                    color(u0, v0),
                    shade,
                );
            }
        }
    }

    pub fn build(&self, mesh: &mut Mesh, quality: u8) {
        let hr = self.hr();
        let c = self.center;
        // The tiers: a man on the far hash gets a head, a man three yards from the
        // lens gets a face.
        let rings: usize = match quality {
            0 => 5,
            1 => 8,
            2 => 12,
            _ => 17,
        };
        let segs: usize = match quality {
            0 => 9,
            1 => 14,
            2 => 20,
            _ => 28,
        };
        let fine = quality >= 1;
        let face = quality >= 2;
        let portrait = quality >= 3;

        let skin = self.skin;
        let eye_white = Rgb::hex(0xEFE8DC);
        let iris = Rgb::hex(0x4A2F1E);
        let pupil = Rgb::hex(0x141010);
        let lip = skin.scale(0.66).lerp(Rgb::hex(0x77352E), 0.38);
        let lip_line = Rgb::hex(0x2C1512);

        // Socket and under-jaw shading. It is not lighting: it is the colour of
        // the surface, and it is what keeps a face from being one flat tone.
        let tone = |u: f32, v: f32| -> Rgb {
            let (su, cu) = u.sin_cos();
            let eye_socket = gauss(v - 0.615, 0.055) * gauss(su.abs() - 0.40, 0.20) * 0.12;
            let mouth_shadow = gauss(v - 0.350, 0.045) * gauss(su.abs() - 0.12, 0.22) * 0.10;
            let jaw = if v < 0.12 { (0.12 - v) / 0.12 * 0.16 } else { 0.0 };
            let temple = gauss(v - 0.760, 0.08) * gauss(su.abs() - 0.82, 0.22) * 0.06;
            let back = if cu < -0.2 { 0.04 } else { 0.0 };
            let side = if su.abs() > 0.55 { 0.05 } else { 0.0 };
            let brow_shadow = gauss(v - 0.668, 0.032) * cu.max(0.0) * 0.05;
            skin.scale(1.0 - eye_socket - mouth_shadow - jaw - temple - back - side - brow_shadow)
        };
        Self::sheet(mesh, rings, segs, &|u, v| self.point(u, v), &tone, c, 1.0);

        // Close the crown, so the top of the head is not a hole when the camera
        // is above the play.
        let crown = c + Vec3::new(0.0, 0.0, hr * 0.98);
        let ring: Vec<Vec3> = (0..segs)
            .map(|s| {
                let u = s as f32 / segs as f32 * std::f32::consts::TAU;
                self.point(u, 1.0)
            })
            .collect();
        for s in 0..segs {
            let s1 = (s + 1) % segs;
            mesh.tri_curved(
                ring[s],
                Vec3::new(0.0, 0.0, 1.0),
                ring[s1],
                Vec3::new(0.0, 0.0, 1.0),
                crown,
                Vec3::new(0.0, 0.0, 1.0),
                skin,
                1.0,
            );
        }

        // The underside of the jaw, filling the gap between the chin and the
        // neck. Without it the open bottom of the head loft shows as a ring of
        // ragged triangles around the throat.
        mesh.sphere(
            c + Vec3::new(0.0, 0.0, -hr * 0.86),
            Vec3::new(hr * 0.62, hr * 0.66, hr * 0.30),
            if fine { 4 } else { 2 },
            if fine { 10 } else { 6 },
            self.mask.unwrap_or_else(|| skin.scale(0.88)),
        );

        // ---------------- balaclava ----------------
        // The same surface as the skull, pushed out a few millimetres, with an
        // aperture left for the eyes and the brows. Cloth over the nose, mouth,
        // jaw and neck; nothing else changes underneath.
        if let Some(cloth) = self.mask {
            let m_rings = if portrait { rings } else if fine { 9 } else { 5 };
            let off = hr * 0.032;
            let s = |u: f32, v: f32| {
                let p = self.point(u, v);
                // The cloth follows the nose underneath: a ridge down the middle
                // of the face, so the mask is a face and not a sock pulled over
                // one. The nose itself is not built while the mask is on.
                let nose = 0.070 * gauss(v - 0.520, 0.085) * gauss(u, 0.26);
                p + self.outward(p) * (off + hr * nose)
            };
            let d = 0.006f32;
            let normal = |u: f32, v: f32| -> Vec3 {
                let p = self.point(u, v);
                let du = self.point(u + d, v) - self.point(u - d, v);
                let dv = self.point(u, v + d) - self.point(u, v - d);
                let mut n = du.cross(dv).normalize_or_zero();
                if n.length() < 0.5 {
                    n = (p - c).normalize_or_zero();
                }
                if n.dot(p - c) < 0.0 {
                    n = n * -1.0;
                }
                n
            };
            for r in 0..m_rings {
                let v0 = 0.012 + 0.988 * r as f32 / m_rings as f32;
                let v1 = 0.012 + 0.988 * (r + 1) as f32 / m_rings as f32;
                for seg in 0..segs {
                    let u0 = seg as f32 / segs as f32 * std::f32::consts::TAU;
                    let u1 = (seg + 1) as f32 / segs as f32 * std::f32::consts::TAU;
                    let mu = ((u0 + u1) * 0.5 + std::f32::consts::PI)
                        .rem_euclid(std::f32::consts::TAU)
                        - std::f32::consts::PI;
                    let mv = (v0 + v1) * 0.5;
                    let ex = mu / 0.76;
                    let ey = (mv - 0.632) / 0.064;
                    if ex * ex + ey * ey < 1.0 {
                        continue;
                    }
                    mesh.quad_curved(
                        s(u0, v0),
                        normal(u0, v0),
                        s(u1, v0),
                        normal(u1, v0),
                        s(u1, v1),
                        normal(u1, v1),
                        s(u0, v1),
                        normal(u0, v1),
                        cloth,
                        1.06,
                    );
                }
            }
            // Close the crown, so the top of the mask is not a hole from above.
            let top = c + Vec3::new(0.0, 0.0, hr * 1.018);
            let ring: Vec<Vec3> = (0..segs)
                .map(|i| s(i as f32 / segs as f32 * std::f32::consts::TAU, 1.0))
                .collect();
            for i in 0..segs {
                let j = (i + 1) % segs;
                mesh.tri_curved(
                    ring[i],
                    Vec3::new(0.0, 0.0, 1.0),
                    ring[j],
                    Vec3::new(0.0, 0.0, 1.0),
                    top,
                    Vec3::new(0.0, 0.0, 1.0),
                    cloth,
                    1.06,
                );
            }
            // The edge of the aperture, so the opening is cloth with a thickness
            // rather than a hole cut in paper.
            let rim: Vec<(Vec3, f32)> = (0..=16)
                .map(|i| {
                    let t = i as f32 / 16.0 * std::f32::consts::TAU;
                    (
                        s(0.76 * t.cos(), 0.632 + 0.064 * t.sin()),
                        hr * 0.024,
                    )
                })
                .collect();
            mesh.tube(&rim, if fine { 5 } else { 3 }, cloth.scale(0.82), false);
        }

        // ---------------- eyes ----------------
        let eye_v = 0.617;
        let eye_u = 0.400;
        for side in [-1.0f32, 1.0] {
            let u = side * eye_u;
            let surface = self.point(u, eye_v);
            let out = self.outward(surface);
            let eye = surface - out * hr * 0.050;
            // The ball, sitting proud of the socket by most of its radius.
            mesh.sphere(
                eye,
                Vec3::new(hr * 0.086, hr * 0.092, hr * 0.080),
                if fine { 4 } else { 2 },
                if fine { 9 } else { 5 },
                eye_white,
            );
            if fine {
                mesh.sphere(
                    eye + out * hr * 0.052,
                    Vec3::new(hr * 0.056, hr * 0.058, hr * 0.046),
                    2,
                    7,
                    iris,
                );
                mesh.sphere(
                    eye + out * hr * 0.055,
                    Vec3::new(hr * 0.024, hr * 0.025, hr * 0.021),
                    2,
                    5,
                    pupil,
                );
                if portrait {
                    mesh.sphere(
                        eye + out * hr * 0.060 + Vec3::new(0.0, 0.0, hr * 0.012),
                        Vec3::new(hr * 0.009, hr * 0.009, hr * 0.009),
                        2,
                        4,
                        Rgb::hex(0xFBF7F0),
                    );
                }
            }
            // Lids: two arcs of skin over the top and bottom of the ball. Without
            // them the eye is a bead glued to a face.
            for (lid_v, r, shade) in [(eye_v + 0.024, 0.024, 1.02), (eye_v - 0.024, 0.019, 0.97)] {
                let mut pts = Vec::new();
                for j in 0..=4 {
                    let uu = u + (j as f32 / 4.0 - 0.5) * 0.34 * side;
                    let p = self.point(uu, lid_v);
                    pts.push((p + self.outward(p) * hr * 0.003, hr * r));
                }
                mesh.tube(&pts, if fine { 7 } else { 4 }, skin.scale(shade), false);
            }
            // Brow, heavy and low: the sheets' faces are all brow.
            if fine {
                let mut pts = Vec::new();
                for j in 0..=5 {
                    let uu = u + (j as f32 / 5.0 - 0.5) * 0.46 * side;
                    let v = eye_v + 0.062 - 0.010 * (j as f32 / 5.0 - 0.5).abs();
                    let p = self.point(uu, v);
                    pts.push((p + self.outward(p) * hr * 0.012, hr * 0.032));
                }
                let brow = if self.player.hair == HairStyle::Bald {
                    skin.scale(0.72)
                } else {
                    self.player.hair_color
                };
                mesh.tube(&pts, 5, brow, false);
            }
        }

        // ---------------- nose ----------------
        // Covered by the balaclava, which carries its shape instead.
        if fine && self.mask.is_none() {
            let bridge = self.point(0.0, 0.665);
            let mid = self.point(0.0, 0.560);
            let lower = self.point(0.0, 0.480);
            let tip = self.point(0.0, 0.448);
            let fwd = Vec3::new(1.0, 0.0, 0.0);
            mesh.tube(
                &[
                    (bridge + fwd * hr * 0.008, hr * 0.032),
                    (mid + fwd * hr * 0.030, hr * 0.044),
                    (lower + fwd * hr * 0.055, hr * 0.050),
                    (tip + fwd * hr * 0.064, hr * 0.052),
                ],
                if fine { 8 } else { 5 },
                skin,
                false,
            );
            // Wings and the tip, so the nose ends in a nose and not a point.
            mesh.sphere(
                tip + fwd * hr * 0.048,
                Vec3::new(hr * 0.055, hr * 0.060, hr * 0.050),
                3,
                8,
                skin,
            );
            for side in [-1.0f32, 1.0] {
                let wing = tip + fwd * hr * 0.020 + Vec3::new(0.0, side * hr * 0.062, -hr * 0.012);
                mesh.sphere(
                    wing,
                    Vec3::new(hr * 0.038, hr * 0.032, hr * 0.030),
                    2,
                    7,
                    skin,
                );
                if face {
                    mesh.sphere(
                        wing + Vec3::new(hr * 0.010, 0.0, -hr * 0.024),
                        Vec3::new(hr * 0.014, hr * 0.013, hr * 0.008),
                        2,
                        5,
                        lip_line,
                    );
                }
            }
        }

        // ---------------- mouth ----------------
        if fine && self.mask.is_none() {
            let mouth_v = 0.352;
            let mut lip_tube = |v: f32, r: f32, color: Rgb, dark: f32| {
                let mut pts = Vec::new();
                for j in 0..=5 {
                    let u = (j as f32 / 5.0 - 0.5) * 0.52;
                    let p = self.point(u, v);
                    let with_dip = if j == 0 || j == 5 { r * 0.72 } else { r };
                    pts.push((p + self.outward(p) * hr * 0.005, hr * with_dip));
                }
                mesh.tube(&pts, if fine { 6 } else { 4 }, color.scale(dark), false);
            };
            lip_tube(mouth_v + 0.018, 0.017, lip, 1.08);
            lip_tube(mouth_v - 0.016, 0.022, lip, 0.92);
            // The line between the lips: a mouth closed looks closed.
            let mut line = Vec::new();
            for j in 0..=5 {
                let u = (j as f32 / 5.0 - 0.5) * 0.46;
                let p = self.point(u, mouth_v + 0.004);
                line.push((p + self.outward(p) * hr * 0.010, hr * 0.005));
            }
            mesh.tube(&line, 4, lip_line, false);
            // A chin crease under the lower lip.
            if portrait {
                let mut crease = Vec::new();
                for j in 0..=4 {
                    let u = (j as f32 / 4.0 - 0.5) * 0.30;
                    let p = self.point(u, mouth_v - 0.085);
                    crease.push((p + self.outward(p) * hr * 0.008, hr * 0.007));
                }
                mesh.tube(&crease, 4, skin.scale(0.80), false);
            }
        }

        // ---------------- ears ----------------
        for side in [-1.0f32, 1.0] {
            let u = if side > 0.0 {
                std::f32::consts::FRAC_PI_2
            } else {
                -std::f32::consts::FRAC_PI_2
            };
            let base = self.point(u, 0.560);
            let ear = base + Vec3::new(-hr * 0.012, side * hr * 0.034, -hr * 0.010);
            mesh.sphere(
                ear,
                Vec3::new(hr * 0.070, hr * 0.044, hr * 0.100),
                if fine { 4 } else { 2 },
                if fine { 8 } else { 5 },
                skin,
            );
            if face {
                // The bowl of the ear, pressed back into it.
                mesh.sphere(
                    ear + Vec3::new(hr * 0.012, side * hr * 0.030, -hr * 0.004),
                    Vec3::new(hr * 0.034, hr * 0.022, hr * 0.052),
                    3,
                    7,
                    skin.scale(0.62),
                );
                mesh.sphere(
                    ear + Vec3::new(hr * 0.004, side * hr * 0.044, -hr * 0.066),
                    Vec3::new(hr * 0.032, hr * 0.022, hr * 0.028),
                    2,
                    6,
                    skin,
                );
            }
        }

        // ---------------- beard ----------------
        let beard = self.player.beard.clamp(0.0, 1.0);
        if beard > 0.15 && fine && self.mask.is_none() {
            let hair_color = self.player.hair_color;
            // One shell over the whole jaw: from the sideburn, down the cheek,
            // round the chin and back up. Built as straps it reads as a mask.
            let b_rings = if portrait { 9 } else { 5 };
            let b_segs = if portrait { 20 } else { 12 };
            Self::sheet(
                mesh,
                b_rings,
                b_segs,
                &|s, t| {
                    let u = (s - 0.5) * 2.70;
                    let v = 0.020 + t * 0.415;
                    let p = self.point(u, v);
                    let out = self.outward(p);
                    // Fuller at the chin and jaw, shaved thin up the cheek.
                    let full = (1.0 - t).powf(0.7);
                    p + out * hr * (0.030 + 0.034 * beard) * (0.45 + 0.55 * full)
                },
                &|_, _| hair_color,
                c,
                1.0,
            );
            // Moustache across the top lip.
            let mut stache = Vec::new();
            for j in 0..=6 {
                let u = (j as f32 / 6.0 - 0.5) * 0.46;
                let p = self.point(u, 0.398);
                stache.push((p + self.outward(p) * hr * 0.026, hr * 0.024));
            }
            mesh.tube(&stache, 6, hair_color, false);
            // A strip up the jaw to the sideburn, on the full beards.
            if beard > 0.5 {
                for side in [-1.0f32, 1.0] {
                    let mut pts = Vec::new();
                    for j in 0..=3 {
                        let v = 0.46 - j as f32 * 0.11;
                        let u = side * (1.02 - j as f32 * 0.02);
                        let p = self.point(u, v);
                        pts.push((p + self.outward(p) * hr * 0.022, hr * 0.026));
                    }
                    mesh.tube(&pts, 5, hair_color, false);
                }
            }
        }

        // ---------------- hair ----------------
        if self.mask.is_none() && self.player.hair != HairStyle::Bald {
            let hair_color = self.player.hair_color;
            let (front_back, side_drop, thick) = match self.player.hair {
                HairStyle::Buzz => (0.845, 0.045, 0.030),
                HairStyle::Fade => (0.885, 0.075, 0.026),
                HairStyle::Locs => (0.800, 0.035, 0.052),
                HairStyle::TwistOut => (0.790, 0.030, 0.058),
                HairStyle::HighTop => (0.840, 0.040, 0.048),
                HairStyle::Bald => (1.0, 0.0, 0.0),
            };
            // The hairline: high across the forehead, lower at the back, and
            // notched at the temples like a real one.
            let hairline = |u: f32| -> f32 {
                let (su, cu) = u.sin_cos();
                let back = (-cu).max(0.0);
                front_back + side_drop - 0.085 * back - 0.055 * su.abs() * su.abs()
            };
            let cap = |u: f32, t: f32| -> Vec3 {
                let v0 = hairline(u);
                let v = v0 + (1.0 - v0) * t;
                let p = self.point(u, v);
                let out = self.outward(p);
                // Hair is thin at the hairline and stands off the skull at the
                // crown, which is what a fade and a high top both are.
                p + out * hr * thick * (0.30 + 0.70 * t)
            };
            let cap_rings = if portrait { 7 } else if fine { 5 } else { 3 };
            Self::sheet(
                mesh,
                cap_rings,
                segs,
                &cap,
                &|_, _| hair_color,
                c,
                1.0,
            );
            // The rim of the hair at the hairline, so its edge is a thickness of
            // hair rather than a line where the colour stops.
            let rim: Vec<(Vec3, f32)> = (0..=segs)
                .map(|s| {
                    let u = s as f32 / segs as f32 * std::f32::consts::TAU;
                    (cap(u, 0.02), hr * thick * 0.55)
                })
                .collect();
            mesh.tube(&rim, if fine { 6 } else { 4 }, hair_color, false);

            match self.player.hair {
                HairStyle::Locs if fine => {
                    // Locs down the back and the sides of the head.
                    let strands = if portrait { 11 } else { 7 };
                    for i in 0..strands {
                        let u = 1.35 + i as f32 / strands as f32 * 3.3;
                        let v = 0.62 + 0.10 * ((i as f32) * 1.7).sin();
                        let base = cap(u, 0.20);
                        let mid = base + Vec3::new(-hr * 0.30, 0.0, -hr * (0.30 + 0.08 * (i as f32).cos()));
                        let tip = mid + Vec3::new(-hr * 0.10, 0.0, -hr * 0.65);
                        mesh.tube(
                            &[
                                (base, hr * 0.028),
                                (mid, hr * 0.024),
                                (tip, hr * 0.012),
                            ],
                            5,
                            hair_color,
                            false,
                        );
                        let _ = v;
                    }
                }
                HairStyle::TwistOut if fine => {
                    // Twists are a field of ends, not a bigger ball.
                    let n = if portrait { 14 } else { 8 };
                    for i in 0..n {
                        let u = i as f32 / n as f32 * std::f32::consts::TAU;
                        let t = 0.18 + 0.42 * ((i as f32) * 2.3).sin().abs();
                        let p = cap(u, t);
                        mesh.sphere(
                            p,
                            Vec3::new(hr * 0.075, hr * 0.075, hr * 0.070),
                            2,
                            6,
                            hair_color,
                        );
                    }
                }
                HairStyle::HighTop if fine => {
                    // A high top is cut flat: a wall up from the crown, and a flat
                    // top on it, not a bun.
                    let top_z = c.z + hr * 0.98;
                    let wall = if portrait { 12 } else { 8 };
                    mesh.tube(
                        &[
                            (Vec3::new(c.x - hr * 0.03, c.y, top_z - hr * 0.14), hr * 0.36),
                            (Vec3::new(c.x - hr * 0.06, c.y, top_z + hr * 0.20), hr * 0.41),
                            (Vec3::new(c.x - hr * 0.08, c.y, top_z + hr * 0.52), hr * 0.40),
                        ],
                        wall,
                        hair_color,
                        false,
                    );
                    mesh.sphere(
                        Vec3::new(c.x - hr * 0.08, c.y, top_z + hr * 0.56),
                        Vec3::new(hr * 0.40, hr * 0.40, hr * 0.08),
                        3,
                        wall,
                        hair_color,
                    );
                }
                _ => {}
            }
        }

        // A headband, on the men the sheets give one to. It wraps the forehead
        // over the hair, so it is a band of cloth and not a stripe painted on.
        if self.mask.is_none() && self.player.id % 3 == 0 {
            let band = if self.player.id % 2 == 0 { self.accent } else { Rgb::hex(0xEDEAE0) };
            let v0 = 0.672;
            let v1 = 0.706;
            let band_segs = if fine { segs } else { 10 };
            Self::sheet(
                mesh,
                1,
                band_segs,
                &|u, t| {
                    let v = v0 + (v1 - v0) * t;
                    let p = self.point(u, v);
                    let out = self.outward(p);
                    let over_hair = if self.player.hair == HairStyle::Bald { 0.012 } else { 0.042 };
                    p + out * hr * over_hair
                },
                &|_, _| band,
                c,
                1.02,
            );
            if portrait {
                // The tail of it, tied at the back.
                let back = self.point(std::f32::consts::PI, 0.68) + Vec3::new(-hr * 0.04, 0.0, 0.0);
                mesh.tube(
                    &[
                        (back, hr * 0.020),
                        (back + Vec3::new(-hr * 0.10, hr * 0.06, -hr * 0.10), hr * 0.016),
                        (back + Vec3::new(-hr * 0.14, hr * 0.12, -hr * 0.22), hr * 0.010),
                    ],
                    4,
                    band,
                    false,
                );
            }
        }
    }
}

//! Small dependency-free math + RNG layer used by both the simulation and the renderer.

use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub, SubAssign};

pub const PI: f32 = std::f32::consts::PI;
pub const TAU: f32 = std::f32::consts::TAU;

pub fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn smoothstep(t: f32) -> f32 {
    let t = clamp(t, 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Wrap an angle into `(-PI, PI]`.
pub fn wrap_angle(a: f32) -> f32 {
    let mut a = a;
    while a > PI {
        a -= TAU;
    }
    while a <= -PI {
        a += TAU;
    }
    a
}

/// Move `current` toward `target` by at most `max_delta` (angles in radians).
pub fn approach_angle(current: f32, target: f32, max_delta: f32) -> f32 {
    let d = wrap_angle(target - current);
    current + clamp(d, -max_delta, max_delta)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    pub const X: Vec2 = Vec2 { x: 1.0, y: 0.0 };
    pub const Y: Vec2 = Vec2 { x: 0.0, y: 1.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Vec2 { x, y }
    }

    pub fn splat(v: f32) -> Self {
        Vec2 { x: v, y: v }
    }

    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    pub fn length_sq(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    pub fn normalize_or_zero(self) -> Self {
        let l = self.length();
        if l > 1e-6 {
            Vec2::new(self.x / l, self.y / l)
        } else {
            Vec2::ZERO
        }
    }

    pub fn dot(self, o: Vec2) -> f32 {
        self.x * o.x + self.y * o.y
    }

    pub fn cross(self, o: Vec2) -> f32 {
        self.x * o.y - self.y * o.x
    }

    pub fn dist(self, o: Vec2) -> f32 {
        (self - o).length()
    }

    pub fn dist_sq(self, o: Vec2) -> f32 {
        (self - o).length_sq()
    }

    pub fn perp(self) -> Vec2 {
        Vec2::new(-self.y, self.x)
    }

    pub fn lerp(self, o: Vec2, t: f32) -> Self {
        Vec2::new(lerp(self.x, o.x, t), lerp(self.y, o.y, t))
    }

    pub fn angle(self) -> f32 {
        self.y.atan2(self.x)
    }

    pub fn from_angle(a: f32) -> Self {
        Vec2::new(a.cos(), a.sin())
    }

    pub fn rotate(self, a: f32) -> Self {
        let (s, c) = a.sin_cos();
        Vec2::new(self.x * c - self.y * s, self.x * s + self.y * c)
    }

    pub fn with_len(self, len: f32) -> Self {
        self.normalize_or_zero() * len
    }

    pub fn clamp_len(self, max: f32) -> Self {
        let l = self.length();
        if l > max && l > 1e-6 {
            self * (max / l)
        } else {
            self
        }
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}
impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        self.x += o.x;
        self.y += o.y;
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}
impl SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Vec2) {
        self.x -= o.x;
        self.y -= o.y;
    }
}
impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f32) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}
impl MulAssign<f32> for Vec2 {
    fn mul_assign(&mut self, s: f32) {
        self.x *= s;
        self.y *= s;
    }
}
impl Div<f32> for Vec2 {
    type Output = Vec2;
    fn div(self, s: f32) -> Vec2 {
        Vec2::new(self.x / s, self.y / s)
    }
}
impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    pub const UP: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 1.0 };

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Vec3 { x, y, z }
    }

    pub fn from_vec2(v: Vec2, z: f32) -> Self {
        Vec3::new(v.x, v.y, z)
    }

    pub fn xy(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn length_sq(self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    pub fn normalize_or_zero(self) -> Self {
        let l = self.length();
        if l > 1e-6 {
            self * (1.0 / l)
        } else {
            Vec3::ZERO
        }
    }

    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn lerp(self, o: Vec3, t: f32) -> Self {
        Vec3::new(lerp(self.x, o.x, t), lerp(self.y, o.y, t), lerp(self.z, o.z, t))
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f32) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}

/// Linear colour, used for uniforms, field paint and lighting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }

    pub const fn hex(v: u32) -> Self {
        Rgb::new((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    pub const BLACK: Rgb = Rgb::new(0, 0, 0);
    pub const WHITE: Rgb = Rgb::new(255, 255, 255);

    pub fn lerp(self, o: Rgb, t: f32) -> Rgb {
        let t = clamp(t, 0.0, 1.0);
        Rgb::new(
            lerp(self.r as f32, o.r as f32, t) as u8,
            lerp(self.g as f32, o.g as f32, t) as u8,
            lerp(self.b as f32, o.b as f32, t) as u8,
        )
    }

    pub fn scale(self, s: f32) -> Rgb {
        Rgb::new(
            clamp(self.r as f32 * s, 0.0, 255.0) as u8,
            clamp(self.g as f32 * s, 0.0, 255.0) as u8,
            clamp(self.b as f32 * s, 0.0, 255.0) as u8,
        )
    }

    pub fn to_array(self) -> [u8; 3] {
        [self.r, self.g, self.b]
    }

    /// Perceived brightness 0..255, handy for picking readable overlay text.
    pub fn luma(self) -> f32 {
        0.299 * self.r as f32 + 0.587 * self.g as f32 + 0.114 * self.b as f32
    }
}

/// PCG-XSH-RR 64/32 generator. Deterministic across platforms, which is what makes
/// replays and the broadcast renderer line up exactly with the text simulation.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
    inc: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut r = Rng { state: 0, inc: (seed << 1) | 1 };
        r.next_u32();
        r.state = r.state.wrapping_add(seed ^ 0x9e37_79b9_7f4a_7c15);
        r.next_u32();
        r
    }

    pub fn seed(&self) -> u64 {
        self.state
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6364136223846793005).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in `[0, 1)`.
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16_777_216.0
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }

    /// Standard normal via Box-Muller.
    pub fn gauss(&mut self) -> f32 {
        let u1 = self.f32().max(1e-6);
        let u2 = self.f32();
        (-2.0 * u1.ln()).sqrt() * (TAU * u2).cos()
    }

    pub fn gauss_clamped(&mut self, sigma: f32, limit: f32) -> f32 {
        clamp(self.gauss() * sigma, -limit, limit)
    }

    pub fn index(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u32() as usize) % n
        }
    }

    pub fn range_i32(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            lo
        } else {
            lo + (self.next_u32() % ((hi - lo + 1) as u32)) as i32
        }
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            Some(&items[self.index(items.len())])
        }
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        if items.len() < 2 {
            return;
        }
        for i in (1..items.len()).rev() {
            let j = self.index(i + 1);
            items.swap(i, j);
        }
    }

    /// Derive an independent stream; useful so one subsystem's rolls can never
    /// shift another subsystem's randomness.
    pub fn fork(&mut self) -> Rng {
        Rng::new(self.next_u32() as u64 ^ ((self.next_u32() as u64) << 32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec_basics() {
        let a = Vec2::new(3.0, 4.0);
        assert!((a.length() - 5.0).abs() < 1e-5);
        assert!((a.normalize_or_zero().length() - 1.0).abs() < 1e-5);
        assert_eq!(Vec2::ZERO.normalize_or_zero(), Vec2::ZERO);
        let r = Vec2::new(1.0, 0.0).rotate(PI / 2.0);
        assert!((r.x).abs() < 1e-5 && (r.y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn rng_is_deterministic_and_in_range() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            let x = a.f32();
            assert_eq!(x, b.f32());
            assert!((0.0..1.0).contains(&x));
        }
        let mut c = Rng::new(7);
        let mut sum = 0.0;
        for _ in 0..20_000 {
            sum += c.gauss();
        }
        assert!(sum.abs() < 400.0, "gauss should be centred: {sum}");
    }

    #[test]
    fn color_ops() {
        let c = Rgb::hex(0x102030);
        assert_eq!((c.r, c.g, c.b), (0x10, 0x20, 0x30));
        assert_eq!(Rgb::BLACK.lerp(Rgb::WHITE, 1.0), Rgb::WHITE);
        assert!(Rgb::WHITE.luma() > 250.0);
    }
}

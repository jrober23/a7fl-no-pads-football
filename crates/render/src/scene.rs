//! Perspective camera, mesh primitives and the software 3D pipeline.

use a7fl_core::math::{clamp, Rgb, Vec3};

use crate::raster::{Frame, Texture};

#[derive(Clone, Debug)]
pub struct Camera {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// Vertical field of view in radians.
    pub fov: f32,
    pub near: f32,
    pub roll: f32,
}

impl Camera {
    pub fn new(pos: Vec3, yaw: f32, pitch: f32) -> Self {
        Camera {
            pos,
            yaw,
            pitch,
            fov: 52f32.to_radians(),
            near: 0.15,
            roll: 0.0,
        }
    }

    /// Right, up, forward basis vectors.
    pub fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let f = Vec3::new(cy * cp, sy * cp, sp);
        let r0 = Vec3::new(-sy, cy, 0.0);
        let u0 = f.cross(r0);
        let (sr, cr) = self.roll.sin_cos();
        let r = r0 * cr + u0 * sr;
        let u = u0 * cr - r0 * sr;
        (r, u, f)
    }

    pub fn view(&self, p: Vec3) -> Vec3 {
        let (r, u, f) = self.basis();
        let d = p - self.pos;
        Vec3::new(d.dot(r), d.dot(u), d.dot(f))
    }

    pub fn look_at(&mut self, target: Vec3) {
        let d = target - self.pos;
        self.yaw = d.y.atan2(d.x);
        let horiz = (d.x * d.x + d.y * d.y).sqrt();
        self.pitch = d.z.atan2(horiz);
    }

    pub fn forward(&self) -> Vec3 {
        self.basis().2
    }
}

pub struct Projector {
    pub w: f32,
    pub h: f32,
    pub focal: f32,
}

impl Projector {
    pub fn new(w: usize, h: usize, fov: f32) -> Self {
        let focal = (h as f32 * 0.5) / (fov * 0.5).tan();
        Projector { w: w as f32, h: h as f32, focal }
    }

    /// Projects a view-space point. Returns `(screen x, screen y, 1/z)`.
    pub fn project(&self, v: Vec3) -> Option<(f32, f32, f32)> {
        if v.z <= 0.02 {
            return None;
        }
        let iz = 1.0 / v.z;
        let x = self.w * 0.5 + v.x * self.focal * iz;
        let y = self.h * 0.5 - v.y * self.focal * iz;
        Some((x, y, iz))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Tri {
    pub a: Vec3,
    pub b: Vec3,
    pub c: Vec3,
    pub color: Rgb,
    /// Multiplier applied on top of the computed lighting (fake ambient occlusion).
    pub shade: f32,
}

#[derive(Clone, Default, Debug)]
pub struct Mesh {
    pub tris: Vec<Tri>,
}

impl Mesh {
    pub fn new() -> Self {
        Mesh { tris: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    pub fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Rgb) {
        self.tris.push(Tri { a, b, c, color, shade: 1.0 });
    }

    pub fn tri_shaded(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Rgb, shade: f32) {
        self.tris.push(Tri { a, b, c, color, shade });
    }

    pub fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Rgb) {
        self.tri(a, b, c, color);
        self.tri(a, c, d, color);
    }

    pub fn quad_shaded(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Rgb, shade: f32) {
        self.tri_shaded(a, b, c, color, shade);
        self.tri_shaded(a, c, d, color, shade);
    }

    /// Axis-aligned box rotated about the Z axis.
    pub fn box_yaw(&mut self, center: Vec3, half: Vec3, yaw: f32, color: Rgb) {
        self.box_shaded(center, half, yaw, color, 1.0);
    }

    pub fn box_shaded(&mut self, center: Vec3, half: Vec3, yaw: f32, color: Rgb, shade: f32) {
        let (s, c) = yaw.sin_cos();
        let mut corners = [Vec3::ZERO; 8];
        let mut n = 0;
        for &sx in &[-1.0f32, 1.0] {
            for &sy in &[-1.0f32, 1.0] {
                for &sz in &[-1.0f32, 1.0] {
                    let lx = sx * half.x;
                    let ly = sy * half.y;
                    corners[n] = Vec3::new(
                        center.x + lx * c - ly * s,
                        center.y + lx * s + ly * c,
                        center.z + sz * half.z,
                    );
                    n += 1;
                }
            }
        }
        // Corner index bits: bit0 = x, bit1 = y, bit2 = z.
        let idx = |x: usize, y: usize, z: usize| x + y * 2 + z * 4;
        let faces: [[usize; 4]; 6] = [
            [idx(0, 0, 0), idx(1, 0, 0), idx(1, 1, 0), idx(0, 1, 0)], // bottom
            [idx(0, 0, 1), idx(0, 1, 1), idx(1, 1, 1), idx(1, 0, 1)], // top
            [idx(0, 0, 0), idx(0, 1, 0), idx(0, 1, 1), idx(0, 0, 1)], // -x
            [idx(1, 0, 0), idx(1, 0, 1), idx(1, 1, 1), idx(1, 1, 0)], // +x
            [idx(0, 0, 0), idx(0, 0, 1), idx(1, 0, 1), idx(1, 0, 0)], // -y
            [idx(0, 1, 0), idx(1, 1, 0), idx(1, 1, 1), idx(0, 1, 1)], // +y
        ];
        for (fi, f) in faces.iter().enumerate() {
            let face_shade = shade
                * match fi {
                    1 => 1.06,
                    0 => 0.72,
                    2 | 3 => 0.94,
                    _ => 0.88,
                };
            self.quad_shaded(corners[f[0]], corners[f[1]], corners[f[2]], corners[f[3]], color, face_shade);
        }
    }

    pub fn cylinder(
        &mut self,
        base: Vec3,
        top: Vec3,
        r_base: f32,
        r_top: f32,
        sides: usize,
        color: Rgb,
    ) {
        let sides = sides.max(3);
        let axis = top - base;
        let len = axis.length().max(1e-4);
        let dir = axis * (1.0 / len);
        // Pick a perpendicular basis.
        let helper = if dir.z.abs() > 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 0.0, 1.0) };
        let u = dir.cross(helper).normalize_or_zero();
        let v = dir.cross(u);
        for i in 0..sides {
            let a0 = i as f32 / sides as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / sides as f32 * std::f32::consts::TAU;
            let p0 = base + u * (a0.cos() * r_base) + v * (a0.sin() * r_base);
            let p1 = base + u * (a1.cos() * r_base) + v * (a1.sin() * r_base);
            let q0 = top + u * (a0.cos() * r_top) + v * (a0.sin() * r_top);
            let q1 = top + u * (a1.cos() * r_top) + v * (a1.sin() * r_top);
            let shade = 0.82 + 0.18 * a0.cos().abs();
            self.quad_shaded(p0, p1, q1, q0, color, shade);
        }
    }

    pub fn sphere(&mut self, center: Vec3, radius: Vec3, rings: usize, segments: usize, color: Rgb) {
        let rings = rings.max(2);
        let segments = segments.max(3);
        let point = |r: usize, s: usize| -> Vec3 {
            let phi = std::f32::consts::PI * r as f32 / rings as f32;
            let theta = std::f32::consts::TAU * s as f32 / segments as f32;
            Vec3::new(
                center.x + radius.x * phi.sin() * theta.cos(),
                center.y + radius.y * phi.sin() * theta.sin(),
                center.z + radius.z * phi.cos(),
            )
        };
        for r in 0..rings {
            for s in 0..segments {
                let a = point(r, s);
                let b = point(r + 1, s);
                let c = point(r + 1, s + 1);
                let d = point(r, s + 1);
                let shade = 0.78 + 0.22 * (r as f32 / rings as f32);
                self.quad_shaded(a, b, c, d, color, shade);
            }
        }
    }

    /// Rotate about Z, scale, then translate every triangle.
    pub fn place(&mut self, offset: Vec3, yaw: f32, scale: f32) {
        let (s, c) = yaw.sin_cos();
        for t in self.tris.iter_mut() {
            for p in [&mut t.a, &mut t.b, &mut t.c] {
                let x = p.x * scale;
                let y = p.y * scale;
                let z = p.z * scale;
                *p = Vec3::new(offset.x + x * c - y * s, offset.y + x * s + y * c, offset.z + z);
            }
        }
    }

    pub fn append(&mut self, other: &Mesh) {
        self.tris.extend_from_slice(&other.tris);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Lighting {
    pub sun: Vec3,
    pub ambient: f32,
    pub fog: Rgb,
    pub fog_density: f32,
}

impl Default for Lighting {
    fn default() -> Self {
        Lighting {
            sun: Vec3::new(-0.35, 0.25, 0.9).normalize_or_zero(),
            ambient: 0.55,
            fog: Rgb::hex(0x8FA6B8),
            fog_density: 0.012,
        }
    }
}

fn clip_near(poly: &[Vec3], near: f32) -> Vec<Vec3> {
    let mut out = Vec::with_capacity(poly.len() + 2);
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let a_in = a.z >= near;
        let b_in = b.z >= near;
        if a_in {
            out.push(a);
        }
        if a_in != b_in {
            let t = (near - a.z) / (b.z - a.z);
            out.push(a.lerp(b, t));
        }
    }
    out
}

fn fog_amount(depth: f32, density: f32) -> f32 {
    clamp(1.0 - (-depth * density).exp(), 0.0, 0.92)
}

pub struct SceneRenderer {
    pub projector: Projector,
    pub lighting: Lighting,
}

impl SceneRenderer {
    pub fn new(w: usize, h: usize, fov: f32) -> Self {
        SceneRenderer { projector: Projector::new(w, h, fov), lighting: Lighting::default() }
    }

    pub fn draw_mesh(&self, frame: &mut Frame, mesh: &Mesh, cam: &Camera) {
        for tri in &mesh.tris {
            let n = (tri.b - tri.a).cross(tri.c - tri.a).normalize_or_zero();
            let ndl = n.dot(self.lighting.sun).abs();
            let shade = (self.lighting.ambient + ndl * (1.0 - self.lighting.ambient)) * tri.shade;

            let view = [cam.view(tri.a), cam.view(tri.b), cam.view(tri.c)];
            let clipped = clip_near(&view, cam.near);
            if clipped.len() < 3 {
                continue;
            }
            // Backface cull using the screen-space winding of the clipped polygon.
            let projected: Vec<(f32, f32, f32)> = clipped
                .iter()
                .filter_map(|v| self.projector.project(*v))
                .collect();
            if projected.len() < 3 {
                continue;
            }
            let (x0, y0, _) = projected[0];
            let (x1, y1, _) = projected[1];
            let (x2, y2, _) = projected[2];
            let area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
            if area.abs() < 0.01 {
                continue;
            }
            let depth = (view[0].z + view[1].z + view[2].z) / 3.0;
            let fog = fog_amount(depth, self.lighting.fog_density);
            for i in 1..projected.len() - 1 {
                frame.flat_tri(
                    [projected[0], projected[i], projected[i + 1]],
                    tri.color,
                    shade,
                    self.lighting.fog,
                    fog,
                );
            }
        }
    }

    /// Draw the turf as a textured grid. `world` is `(x0, x1, y0, y1)` in yards and
    /// the texture maps onto exactly that rectangle.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_ground(
        &self,
        frame: &mut Frame,
        tex: &Texture,
        cam: &Camera,
        x0: f32,
        x1: f32,
        y0: f32,
        y1: f32,
        step: f32,
    ) {
        let nx = (((x1 - x0) / step).ceil() as usize).max(1);
        let ny = (((y1 - y0) / step).ceil() as usize).max(1);
        let sun_z = self.lighting.sun.z.abs();
        let shade = self.lighting.ambient + sun_z * (1.0 - self.lighting.ambient);

        for i in 0..nx {
            for j in 0..ny {
                let xa = x0 + (x1 - x0) * i as f32 / nx as f32;
                let xb = x0 + (x1 - x0) * (i + 1) as f32 / nx as f32;
                let ya = y0 + (y1 - y0) * j as f32 / ny as f32;
                let yb = y0 + (y1 - y0) * (j + 1) as f32 / ny as f32;

                let u0 = (xa - x0) / (x1 - x0);
                let u1 = (xb - x0) / (x1 - x0);
                let v0 = (ya - y0) / (y1 - y0);
                let v1 = (yb - y0) / (y1 - y0);

                let p00 = Vec3::new(xa, ya, 0.0);
                let p10 = Vec3::new(xb, ya, 0.0);
                let p11 = Vec3::new(xb, yb, 0.0);
                let p01 = Vec3::new(xa, yb, 0.0);

                // Two triangles; clip each against the near plane.
                for tri in [
                    [(p00, u0, v0), (p10, u1, v0), (p11, u1, v1)],
                    [(p00, u0, v0), (p11, u1, v1), (p01, u0, v1)],
                ] {
                    let mut poly: Vec<(Vec3, f32, f32)> = Vec::new();
                    for k in 0..3 {
                        let (p, u, v) = tri[k];
                        poly.push((cam.view(p), u, v));
                    }
                    let clipped = clip_poly_near(&poly, cam.near);
                    if clipped.len() < 3 {
                        continue;
                    }
                    let projected: Vec<(f32, f32, f32, f32, f32)> = clipped
                        .iter()
                        .filter_map(|(v, u, vv)| {
                            self.projector.project(*v).map(|(x, y, iz)| (x, y, iz, *u, *vv))
                        })
                        .collect();
                    if projected.len() < 3 {
                        continue;
                    }
                    let depth = clipped.iter().map(|(v, _, _)| v.z).sum::<f32>() / clipped.len() as f32;
                    let fog = fog_amount(depth, self.lighting.fog_density);
                    for k in 1..projected.len() - 1 {
                        frame.textured_tri(
                            [projected[0], projected[k], projected[k + 1]],
                            tex,
                            shade,
                            self.lighting.fog,
                            fog,
                        );
                    }
                }
            }
        }
    }

    /// Soft elliptical shadow on the turf, drawn before the players.
    pub fn draw_shadow(&self, frame: &mut Frame, cam: &Camera, center: Vec3, radius: f32, alpha: f32) {
        let ring = 10;
        let mut pts: Vec<(f32, f32, f32)> = Vec::new();
        for i in 0..ring {
            let a = i as f32 / ring as f32 * std::f32::consts::TAU;
            let p = Vec3::new(center.x + a.cos() * radius, center.y + a.sin() * radius * 0.75, 0.02);
            match self.projector.project(cam.view(p)) {
                Some(v) => pts.push(v),
                None => return,
            }
        }
        for i in 1..pts.len() - 1 {
            frame.flat_tri_alpha(
                [pts[0], pts[i], pts[i + 1]],
                Rgb::hex(0x0B1A10),
                alpha,
            );
        }
    }
}

fn clip_poly_near(poly: &[(Vec3, f32, f32)], near: f32) -> Vec<(Vec3, f32, f32)> {
    let mut out = Vec::with_capacity(poly.len() + 2);
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let a_in = a.0.z >= near;
        let b_in = b.0.z >= near;
        if a_in {
            out.push(a);
        }
        if a_in != b_in {
            let t = (near - a.0.z) / (b.0.z - a.0.z);
            out.push((
                a.0.lerp(b.0, t),
                a.1 + (b.1 - a.1) * t,
                a.2 + (b.2 - a.2) * t,
            ));
        }
    }
    out
}

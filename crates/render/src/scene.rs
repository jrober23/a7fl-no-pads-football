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
    /// Normal at each corner. Three equal normals is a flat facet; three
    /// different ones have the light interpolated across the face, which is what
    /// makes a limb look round instead of like a stack of plates.
    pub na: Vec3,
    pub nb: Vec3,
    pub nc: Vec3,
    pub color: Rgb,
    /// Multiplier applied on top of the computed lighting (fake ambient occlusion).
    pub shade: f32,
}

#[inline]
fn face_normal(a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    (b - a).cross(c - a).normalize_or_zero()
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
        let n = face_normal(a, b, c);
        self.tris.push(Tri { a, b, c, na: n, nb: n, nc: n, color, shade: 1.0 });
    }

    pub fn tri_shaded(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Rgb, shade: f32) {
        let n = face_normal(a, b, c);
        self.tris.push(Tri { a, b, c, na: n, nb: n, nc: n, color, shade });
    }

    /// A triangle whose corners carry their own normals, so the shading runs
    /// across it. Builders that know the surface they are making (a loft, a
    /// cylinder, a sphere) use this; a box face stays flat on purpose.
    pub fn tri_curved(
        &mut self,
        a: Vec3,
        na: Vec3,
        b: Vec3,
        nb: Vec3,
        c: Vec3,
        nc: Vec3,
        color: Rgb,
        shade: f32,
    ) {
        self.tris.push(Tri { a, b, c, na, nb, nc, color, shade });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn quad_curved(
        &mut self,
        a: Vec3,
        na: Vec3,
        b: Vec3,
        nb: Vec3,
        c: Vec3,
        nc: Vec3,
        d: Vec3,
        nd: Vec3,
        color: Rgb,
        shade: f32,
    ) {
        self.tri_curved(a, na, b, nb, c, nc, color, shade);
        self.tri_curved(a, na, c, nc, d, nd, color, shade);
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
            // The normals come off the axis, not off the facet: that is the whole
            // difference between a tube and a nine-sided extrusion.
            let n0 = (u * a0.cos() + v * a0.sin()).normalize_or_zero();
            let n1 = (u * a1.cos() + v * a1.sin()).normalize_or_zero();
            self.quad_curved(p0, n0, p1, n1, q1, n1, q0, n0, color, 1.0);
        }
    }

    /// A smooth tube through a chain of `(point, radius)` rings, capped at both
    /// ends.
    ///
    /// `cylinder` is one ring pair, so a limb built from three of them is three
    /// tubes with a ring of disagreeing normals at every joint — the stepped,
    /// ballooned arm that reads as a toy however good the silhouette is. This
    /// lofts one surface through the whole chain: the frame is carried down the
    /// tube so it does not twist, each ring is mitered between the segments on
    /// either side of it, and the normals run along the limb, so the highlight is
    /// a line down the arm rather than a ring at every joint.
    pub fn tube(&mut self, points: &[(Vec3, f32)], sides: usize, color: Rgb, cap_ends: bool) {
        if points.len() < 2 {
            return;
        }
        let sides = sides.max(3);
        let mut tangents: Vec<Vec3> = Vec::with_capacity(points.len());
        for i in 0..points.len() {
            let prev = if i > 0 {
                (points[i].0 - points[i - 1].0).normalize_or_zero()
            } else {
                Vec3::ZERO
            };
            let next = if i + 1 < points.len() {
                (points[i + 1].0 - points[i].0).normalize_or_zero()
            } else {
                Vec3::ZERO
            };
            let t = if i == 0 {
                next
            } else if i + 1 == points.len() {
                prev
            } else {
                (prev + next).normalize_or_zero()
            };
            tangents.push(t);
        }
        // Carry one perpendicular across the chain instead of rebuilding it from
        // a fixed helper at every ring: two rings with independently chosen frames
        // are stitched with a twist in the surface between them.
        let mut u = {
            let t = tangents[0];
            let helper = if t.z.abs() > 0.9 {
                Vec3::new(1.0, 0.0, 0.0)
            } else {
                Vec3::new(0.0, 0.0, 1.0)
            };
            t.cross(helper).normalize_or_zero()
        };
        let mut rings: Vec<Vec<(Vec3, Vec3)>> = Vec::with_capacity(points.len());
        for (i, (center, radius)) in points.iter().enumerate() {
            let t = tangents[i];
            let carried = u - t * u.dot(t);
            u = if carried.length() > 0.5 {
                carried.normalize_or_zero()
            } else {
                let helper = if t.z.abs() > 0.9 {
                    Vec3::new(1.0, 0.0, 0.0)
                } else {
                    Vec3::new(0.0, 0.0, 1.0)
                };
                t.cross(helper).normalize_or_zero()
            };
            let v = t.cross(u).normalize_or_zero();
            let mut ring = Vec::with_capacity(sides);
            for k in 0..sides {
                let a = k as f32 / sides as f32 * std::f32::consts::TAU;
                let n = (u * a.cos() + v * a.sin()).normalize_or_zero();
                ring.push((*center + n * *radius, n));
            }
            rings.push(ring);
        }
        for i in 0..rings.len() - 1 {
            for k in 0..sides {
                let k1 = (k + 1) % sides;
                let (p0, n0) = rings[i][k];
                let (p1, n1) = rings[i][k1];
                let (q0, m0) = rings[i + 1][k];
                let (q1, m1) = rings[i + 1][k1];
                self.quad_curved(p0, n0, p1, n1, q1, m1, q0, m0, color, 1.0);
            }
        }
        if cap_ends {
            for (i, sign) in [(0usize, -1.0f32), (rings.len() - 1, 1.0)] {
                let c = points[i].0;
                let t = tangents[i] * sign;
                for k in 0..sides {
                    let k1 = (k + 1) % sides;
                    let a = rings[i][k].0;
                    let b = rings[i][k1].0;
                    // Wind each cap so its front face points out of the tube.
                    if sign > 0.0 {
                        self.tri_curved(c, t, b, t, a, t, color, 0.92);
                    } else {
                        self.tri_curved(c, t, a, t, b, t, color, 0.82);
                    }
                }
            }
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
        // Ellipsoid normals: the surface gradient, which is the point position
        // divided by the square of the radius on each axis.
        let normal = |p: Vec3| {
            Vec3::new(
                (p.x - center.x) / (radius.x * radius.x).max(1e-6),
                (p.y - center.y) / (radius.y * radius.y).max(1e-6),
                (p.z - center.z) / (radius.z * radius.z).max(1e-6),
            )
            .normalize_or_zero()
        };
        for r in 0..rings {
            for s in 0..segments {
                let a = point(r, s);
                let b = point(r + 1, s);
                let c = point(r + 1, s + 1);
                let d = point(r, s + 1);
                let shade = 0.78 + 0.22 * (r as f32 / rings as f32);
                self.quad_curved(
                    a,
                    normal(a),
                    b,
                    normal(b),
                    c,
                    normal(c),
                    d,
                    normal(d),
                    color,
                    shade,
                );
            }
        }
    }

    /// Rotate about Z, scale, then translate every triangle. Normals rotate with
    /// the vertices; a uniform scale leaves their lengths alone.
    pub fn place(&mut self, offset: Vec3, yaw: f32, scale: f32) {
        let (s, c) = yaw.sin_cos();
        for t in self.tris.iter_mut() {
            for p in [&mut t.a, &mut t.b, &mut t.c] {
                let x = p.x * scale;
                let y = p.y * scale;
                let z = p.z * scale;
                *p = Vec3::new(offset.x + x * c - y * s, offset.y + x * s + y * c, offset.z + z);
            }
            for n in [&mut t.na, &mut t.nb, &mut t.nc] {
                let (x, y) = (n.x, n.y);
                *n = Vec3::new(x * c - y * s, x * s + y * c, n.z);
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
    /// Light thrown back up off the turf. A green fill from below keeps the dark
    /// side of a body from going flat black, which is what makes a two-tone
    /// figure look like a cartoon instead of a silhouette.
    pub bounce: Vec3,
    pub bounce_strength: f32,
    /// Edge light that wraps the outline of everything: the wet-looking rim a
    /// Pixar frame has on a shoulder and a helmet and a face.
    pub rim: f32,
}

impl Default for Lighting {
    fn default() -> Self {
        Lighting {
            sun: Vec3::new(-0.35, 0.25, 0.9).normalize_or_zero(),
            ambient: 0.52,
            fog: Rgb::hex(0x8FA6B8),
            fog_density: 0.012,
            bounce: Vec3::new(0.1, 0.0, -1.0).normalize_or_zero(),
            bounce_strength: 0.10,
            rim: 0.16,
        }
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp((x - edge0) / (edge1 - edge0).max(1e-5), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Cel shading: the diffuse term is pushed onto three tones with a narrow blend
/// at each terminator, so a face is shadow, half tone or light — the broad, soft
/// steps of a painted figure — instead of the long grey ramp that reads as a
/// low-poly model. The blends stay a few degrees wide so nothing bands hard.
fn toon(x: f32) -> f32 {
    let mid = smoothstep(0.22, 0.40, x);
    let high = smoothstep(0.58, 0.78, x);
    (mid * 0.45 + high * 0.55).clamp(0.0, 1.0)
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
        // The stadium is a few thousand small triangles and most frames see a
        // fraction of them, so anything sitting wholly outside one side of the
        // lens is dropped before it is lit or projected.
        let (right, up, fwd) = cam.basis();
        let half_h = (self.projector.h * 0.5) / self.projector.focal;
        let half_w = (self.projector.w * 0.5) / self.projector.focal;
        let near = cam.near;
        for tri in &mesh.tris {
            let da = tri.a - cam.pos;
            let db = tri.b - cam.pos;
            let dc = tri.c - cam.pos;
            let va = Vec3::new(da.dot(right), da.dot(up), da.dot(fwd));
            let vb = Vec3::new(db.dot(right), db.dot(up), db.dot(fwd));
            let vc = Vec3::new(dc.dot(right), dc.dot(up), dc.dot(fwd));
            if va.z < near && vb.z < near && vc.z < near {
                continue;
            }
            // Wholly outside one side of the frame. Only tested for triangles
            // that are entirely in front of the lens, where the side planes are
            // a fair test.
            if va.z >= near && vb.z >= near && vc.z >= near
                && ((va.x > half_w * va.z
                    && vb.x > half_w * vb.z
                    && vc.x > half_w * vc.z)
                    || (va.x < -half_w * va.z
                        && vb.x < -half_w * vb.z
                        && vc.x < -half_w * vc.z)
                    || (va.y > half_h * va.z
                        && vb.y > half_h * vb.z
                        && vc.y > half_h * vc.z)
                    || (va.y < -half_h * va.z
                        && vb.y < -half_h * vb.z
                        && vc.y < -half_h * vc.z))
            {
                continue;
            }
            let view = [va, vb, vc];
            let n = (tri.b - tri.a).cross(tri.c - tri.a).normalize_or_zero();
            let centroid = (tri.a + tri.b + tri.c) * (1.0 / 3.0);
            let to_eye = (cam.pos - centroid).normalize_or_zero();
            let edge = 1.0 - n.dot(to_eye).abs();
            let rim = self.lighting.rim * edge * edge * edge;
            // Light each corner on its own normal. This is the line that turns a
            // nine-sided limb into a tube: the diffuse term no longer jumps from
            // one facet to the next, it runs around the surface, and the
            // rasteriser interpolates what is left between the corners.
            let corner = |cn: Vec3| -> f32 {
                let lit = toon(cn.dot(self.lighting.sun).abs());
                let bounce = cn.dot(self.lighting.bounce).abs() * self.lighting.bounce_strength;
                (self.lighting.ambient + lit * (1.0 - self.lighting.ambient)) * tri.shade + bounce
                    + rim
            };
            let corners = [corner(tri.na), corner(tri.nb), corner(tri.nc)];
            let flat_shade = (corners[0] + corners[1] + corners[2]) / 3.0;

            // Most triangles are entirely in front of the lens, so the clipper
            // only runs on the ones that straddle it: no allocation per triangle
            // in the middle of the picture, which is most of the picture.
            let whole = view[0].z >= cam.near && view[1].z >= cam.near && view[2].z >= cam.near;
            let clipped_storage;
            let clipped: &[Vec3] = if whole {
                &view
            } else {
                clipped_storage = clip_near(&view, cam.near);
                &clipped_storage
            };
            if clipped.len() < 3 {
                continue;
            }
            // Backface cull using the screen-space winding of the clipped polygon.
            let mut projected = [(0.0f32, 0.0f32, 0.0f32); 5];
            let mut n = 0;
            let mut lost = false;
            for v in clipped {
                match self.projector.project(*v) {
                    Some(p) => {
                        if n < projected.len() {
                            projected[n] = p;
                            n += 1;
                        }
                    }
                    None => lost = true,
                }
            }
            if lost || n < 3 {
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
            for i in 1..n - 1 {
                // A clipped triangle has new corners the original normals say
                // nothing about, and it is at most a hand's width from the lens,
                // so it falls back to the flat shade.
                let shades = if whole {
                    [corners[0], corners[i], corners[i + 1]]
                } else {
                    [flat_shade; 3]
                };
                frame.shade_tri(
                    [projected[0], projected[i], projected[i + 1]],
                    tri.color,
                    shades,
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

                // Two triangles; clip each against the near plane. This loop runs
                // a couple of thousand times a frame, so it takes the allocation
                // free path for tiles that are wholly in front of the lens.
                for tri in [
                    [(p00, u0, v0), (p10, u1, v0), (p11, u1, v1)],
                    [(p00, u0, v0), (p11, u1, v1), (p01, u0, v1)],
                ] {
                    let vs = [
                        (cam.view(tri[0].0), tri[0].1, tri[0].2),
                        (cam.view(tri[1].0), tri[1].1, tri[1].2),
                        (cam.view(tri[2].0), tri[2].1, tri[2].2),
                    ];
                    let mut clipped_storage;
                    let clipped: &[(Vec3, f32, f32)] = if vs[0].0.z >= cam.near
                        && vs[1].0.z >= cam.near
                        && vs[2].0.z >= cam.near
                    {
                        &vs
                    } else {
                        clipped_storage = clip_poly_near(&vs, cam.near);
                        &clipped_storage
                    };
                    if clipped.len() < 3 {
                        continue;
                    }
                    let mut projected = [(0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32); 5];
                    let mut n = 0;
                    let mut lost = false;
                    for (v, u, vv) in clipped {
                        match self.projector.project(*v) {
                            Some((x, y, iz)) => {
                                if n < projected.len() {
                                    projected[n] = (x, y, iz, *u, *vv);
                                    n += 1;
                                }
                            }
                            None => lost = true,
                        }
                    }
                    if lost || n < 3 {
                        continue;
                    }
                    let depth = clipped.iter().map(|(v, _, _)| v.z).sum::<f32>() / clipped.len() as f32;
                    let fog = fog_amount(depth, self.lighting.fog_density);
                    for k in 1..n - 1 {
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

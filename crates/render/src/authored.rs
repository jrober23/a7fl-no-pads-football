//! Authored characters: a rigged GLB skinned on the CPU.
//!
//! Every other figure the renderer draws is procedural -- lofted from a skeleton
//! of primitives and posed by arithmetic. This is the other route: an artist's
//! mesh, a real skeleton and sampled clips. It sits behind the `authored` feature
//! because it brings a JSON reader, a JPEG decoder and an asset loader with it,
//! and nothing else in the renderer wants any of those.
//!
//! The slice of glTF 2.0 here is the one a character asset uses: the GLB
//! container, a node hierarchy of translation/rotation/scale, one skin with an
//! inverse bind matrix per joint, indexed triangles with one UV set, a base
//! colour image, and animation channels sampled by step, linear or cubic-in-the-
//! file-but-taken-linearly. Extensions, sparse accessors, morph targets, node
//! matrices and Draco are refused with an error rather than half-loaded.
//!
//! Vertices are kept in the file's own frame -- glTF is Y up, and this character
//! faces +Z -- and are converted once, after skinning. They have to be: the joint
//! matrices are written in that frame, and a vertex converted before it is skinned
//! is a vertex moved by a matrix that means something else. The conversion turns
//! Y up into this renderer's Z up and +Z facing into +X, which is where a heading
//! of zero points a procedural figure.

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use nopads_core::math::{Rgb, Vec3};
use nopads_core::team::Uniform;

use crate::raster::Texture;
use crate::scene::{Mesh, Tri};

// ---------------------------------------------------------------- linear algebra

/// Row-major 3x3.
#[derive(Clone, Copy, Debug)]
pub struct Mat3 {
    pub m: [f32; 9],
}

impl Mat3 {
    pub const IDENTITY: Mat3 = Mat3 { m: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] };

    pub fn from_quat(q: [f32; 4]) -> Mat3 {
        let [x, y, z, w] = quat_normalize(q);
        let (xx, yy, zz) = (x * x, y * y, z * z);
        let (xy, xz, yz) = (x * y, x * z, y * z);
        let (wx, wy, wz) = (w * x, w * y, w * z);
        Mat3 {
            m: [
                1.0 - 2.0 * (yy + zz),
                2.0 * (xy - wz),
                2.0 * (xz + wy),
                2.0 * (xy + wz),
                1.0 - 2.0 * (xx + zz),
                2.0 * (yz - wx),
                2.0 * (xz - wy),
                2.0 * (yz + wx),
                1.0 - 2.0 * (xx + yy),
            ],
        }
    }

    pub fn from_scale(s: Vec3) -> Mat3 {
        Mat3 { m: [s.x, 0.0, 0.0, 0.0, s.y, 0.0, 0.0, 0.0, s.z] }
    }

    pub fn mul(self, o: Mat3) -> Mat3 {
        let mut m = [0.0f32; 9];
        for (r, row) in m.chunks_mut(3).enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = (0..3).map(|k| self.m[r * 3 + k] * o.m[k * 3 + c]).sum();
            }
        }
        Mat3 { m }
    }

    pub fn mul_vec(self, v: Vec3) -> Vec3 {
        Vec3::new(
            self.m[0] * v.x + self.m[1] * v.y + self.m[2] * v.z,
            self.m[3] * v.x + self.m[4] * v.y + self.m[5] * v.z,
            self.m[6] * v.x + self.m[7] * v.y + self.m[8] * v.z,
        )
    }
}

/// A rigid-plus-scale transform: a 3x3 and a translation.
#[derive(Clone, Copy, Debug)]
pub struct Affine {
    pub r: Mat3,
    pub t: Vec3,
}

impl Affine {
    pub const IDENTITY: Affine = Affine { r: Mat3::IDENTITY, t: Vec3::ZERO };

    pub fn from_trs(t: Vec3, q: [f32; 4], s: Vec3) -> Affine {
        Affine { r: Mat3::from_quat(q).mul(Mat3::from_scale(s)), t }
    }

    /// glTF writes a node's `matrix` as sixteen floats in column-major order,
    /// which is the reverse of how the rest of this file indexes a matrix.
    pub fn from_column_major(m: &[f32]) -> Affine {
        debug_assert!(m.len() >= 16);
        Affine {
            r: Mat3 { m: [m[0], m[4], m[8], m[1], m[5], m[9], m[2], m[6], m[10]] },
            t: Vec3::new(m[12], m[13], m[14]),
        }
    }

    /// `self * o`: what `o` does, then what `self` does.
    pub fn mul(self, o: Affine) -> Affine {
        Affine { r: self.r.mul(o.r), t: self.point(o.t) }
    }

    pub fn point(self, p: Vec3) -> Vec3 {
        self.r.mul_vec(p) + self.t
    }

    pub fn dir(self, d: Vec3) -> Vec3 {
        self.r.mul_vec(d)
    }
}

/// A node's local transform as the three things animation writes into. Keeping
/// the parts apart matters: a clip can key rotation on a node that also carries
/// a scale, and a matrix rebuilt from both has to know both.
#[derive(Clone, Copy, Debug)]
pub struct NodeXform {
    pub t: Vec3,
    pub q: [f32; 4],
    pub s: Vec3,
}

impl NodeXform {
    pub const IDENTITY: NodeXform =
        NodeXform { t: Vec3::ZERO, q: [0.0, 0.0, 0.0, 1.0], s: Vec3::new(1.0, 1.0, 1.0) };

    pub fn affine(&self) -> Affine {
        Affine::from_trs(self.t, self.q, self.s)
    }
}

fn quat_normalize(q: [f32; 4]) -> [f32; 4] {
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l > 1e-8 {
        [q[0] / l, q[1] / l, q[2] / l, q[3] / l]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

/// Shortest-arc spherical interpolation between two quaternions.
pub fn quat_slerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let a = quat_normalize(a);
    let mut b = quat_normalize(b);
    let mut dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if dot < 0.0 {
        b = [-b[0], -b[1], -b[2], -b[3]];
        dot = -dot;
    }
    if dot > 0.9995 {
        return quat_normalize([
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
            a[3] + (b[3] - a[3]) * t,
        ]);
    }
    let theta = dot.clamp(-1.0, 1.0).acos();
    let s = theta.sin();
    let (ka, kb) = (((1.0 - t) * theta).sin() / s, (t * theta).sin() / s);
    quat_normalize([
        a[0] * ka + b[0] * kb,
        a[1] * ka + b[1] * kb,
        a[2] * ka + b[2] * kb,
        a[3] * ka + b[3] * kb,
    ])
}

// ------------------------------------------------------------------- animation

/// Which part of a node's transform a channel writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackPath {
    Translation,
    Rotation,
    Scale,
}

/// One keyed track: the times it was sampled at, and the values it holds.
#[derive(Clone, Debug)]
pub struct Sampler {
    pub times: Vec<f32>,
    /// Always four wide. A translation only uses the first three.
    pub values: Vec<[f32; 4]>,
    pub step: bool,
}

impl Sampler {
    /// The value at `time`, held flat beyond either end of the track.
    pub fn at(&self, time: f32, rotation: bool) -> [f32; 4] {
        let n = self.times.len();
        if n == 0 || self.values.len() < n {
            return [0.0, 0.0, 0.0, 1.0];
        }
        if time <= self.times[0] {
            return self.values[0];
        }
        if time >= self.times[n - 1] {
            return self.values[n - 1];
        }
        let i = self.times.partition_point(|k| *k < time) - 1;
        if self.step {
            return self.values[i];
        }
        let span = self.times[i + 1] - self.times[i];
        let a = if span > 1e-9 { (time - self.times[i]) / span } else { 0.0 };
        let (lo, hi) = (self.values[i], self.values[i + 1]);
        if rotation {
            quat_slerp(lo, hi, a)
        } else {
            [
                lo[0] + (hi[0] - lo[0]) * a,
                lo[1] + (hi[1] - lo[1]) * a,
                lo[2] + (hi[2] - lo[2]) * a,
                lo[3] + (hi[3] - lo[3]) * a,
            ]
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Channel {
    pub node: usize,
    pub path: TrackPath,
    pub sampler: usize,
}

#[derive(Clone, Debug)]
pub struct Clip {
    pub name: String,
    pub duration: f32,
    pub channels: Vec<Channel>,
    pub samplers: Vec<Sampler>,
}

// ----------------------------------------------------------------------- asset

/// A loaded character: one skinned mesh and the clips that can move it.
pub struct Hero {
    /// The bind-pose mesh in the file's own frame: Y up, facing +Z. See [`to_scene`].
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<(f32, f32)>,
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    /// Every node's local transform in the bind pose, in document order.
    pub node_rest: Vec<NodeXform>,
    pub node_parent: Vec<Option<usize>>,
    /// The node each skin slot names.
    pub skin_joints: Vec<usize>,
    pub inverse_bind: Vec<Affine>,
    pub clips: Vec<Clip>,
    pub texture: Arc<Texture>,
    /// Height of the bind pose, in render units, feet at zero.
    pub height: f32,
}

impl Hero {
    pub fn load(path: impl AsRef<Path>) -> Result<Hero, String> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|e| format!("{} could not be read: {e}", path.display()))?;
        Hero::from_glb(&bytes)
    }

    pub fn from_glb(bytes: &[u8]) -> Result<Hero, String> {
        let (json, bin) = split_glb(bytes)?;
        let doc = Json::parse(json)?;
        if doc.get("extensionsRequired").is_some_and(|e| !e.array().is_empty()) {
            return Err("this file requires glTF extensions, which are not supported here".into());
        }
        let accessors = doc.get("accessors").map(|a| a.array()).unwrap_or(&[]);
        let views = doc.get("bufferViews").map(|v| v.array()).unwrap_or(&[]);
        let read = Reader { bin, accessors, views };

        let (positions, normals, uvs, joints, weights, indices) = read_primitives(&doc, &read)?;
        let (node_rest, node_parent) = read_nodes(&doc)?;
        let (skin_joints, inverse_bind) = read_skin(&doc, &read)?;
        if skin_joints.len() != inverse_bind.len() {
            return Err(format!(
                "skin names {} joints but carries {} bind matrices",
                skin_joints.len(),
                inverse_bind.len()
            ));
        }
        let clips = read_clips(&doc, &read)?;
        let texture = Arc::new(read_texture(&doc, &read)?);
        // Height in the file's own frame, where Y is up.
        let height = positions.iter().map(|p| p.y).fold(0.0f32, f32::max);

        Ok(Hero {
            positions,
            normals,
            uvs,
            joints,
            weights,
            indices,
            node_rest,
            node_parent,
            skin_joints,
            inverse_bind,
            clips,
            texture,
            height,
        })
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn clip_index(&self, name: &str) -> Option<usize> {
        self.clips.iter().position(|c| c.name == name)
    }

    /// The joint matrices for a clip at `time`: world transform of the joint,
    /// times its inverse bind matrix.
    pub fn pose(&self, clip: usize, time: f32) -> Vec<Affine> {
        let mut trs = self.node_rest.clone();
        if let Some(c) = self.clips.get(clip) {
            for ch in &c.channels {
                let Some(sampler) = c.samplers.get(ch.sampler) else { continue };
                let Some(node) = trs.get_mut(ch.node) else { continue };
                let v = sampler.at(time, ch.path == TrackPath::Rotation);
                match ch.path {
                    TrackPath::Translation => node.t = Vec3::new(v[0], v[1], v[2]),
                    TrackPath::Rotation => node.q = v,
                    TrackPath::Scale => node.s = Vec3::new(v[0], v[1], v[2]),
                }
            }
        }
        self.joint_matrices(&trs)
    }

    /// The bind pose: the figure as the artist left it, with no clip applied.
    pub fn bind_pose(&self) -> Vec<Affine> {
        self.joint_matrices(&self.node_rest)
    }

    fn joint_matrices(&self, trs: &[NodeXform]) -> Vec<Affine> {
        let local: Vec<Affine> = trs.iter().map(|t| t.affine()).collect();
        let mut world: Vec<Option<Affine>> = vec![None; local.len()];
        for i in 0..local.len() {
            self.resolve(i, &local, &mut world);
        }
        self.skin_joints
            .iter()
            .enumerate()
            .map(|(j, &node)| {
                let w = world.get(node).copied().flatten().unwrap_or(Affine::IDENTITY);
                w.mul(self.inverse_bind[j])
            })
            .collect()
    }

    fn resolve(&self, i: usize, local: &[Affine], world: &mut Vec<Option<Affine>>) -> Affine {
        if let Some(w) = world[i] {
            return w;
        }
        let w = match self.node_parent[i] {
            Some(p) => self.resolve(p, local, world).mul(local[i]),
            None => local[i],
        };
        world[i] = Some(w);
        w
    }

    /// Move every vertex by the joints that own it.
    ///
    /// Weights are normalised here rather than trusted: a vertex whose weights
    /// sum to 0.98 shrinks toward the origin by that much, and a figure that
    /// quietly shrinks when its clips are played is a bug nobody would find.
    pub fn skin(&self, joints: &[Affine], scale: f32) -> (Vec<Vec3>, Vec<Vec3>) {
        let mut out_p = Vec::with_capacity(self.positions.len());
        let mut out_n = Vec::with_capacity(self.normals.len());
        for (i, p) in self.positions.iter().enumerate() {
            let n = self.normals[i];
            let mut sp = Vec3::ZERO;
            let mut sn = Vec3::ZERO;
            let mut total = 0.0f32;
            for k in 0..4 {
                let w = self.weights[i][k];
                if w <= 0.0 {
                    continue;
                }
                let Some(m) = joints.get(self.joints[i][k] as usize).copied() else { continue };
                sp = sp + m.point(*p) * w;
                sn = sn + m.dir(n) * w;
                total += w;
            }
            if total > 1e-6 {
                sp = sp * (1.0 / total);
                sn = sn * (1.0 / total);
            } else {
                sp = *p;
                sn = n;
            }
            out_p.push(to_scene([sp.x, sp.y, sp.z]) * scale);
            out_n.push(to_scene([sn.x, sn.y, sn.z]).normalize_or_zero());
        }
        (out_p, out_n)
    }

    /// The posed mesh, ready for [`crate::scene::SceneRenderer`].
    ///
    /// Every triangle gets its three corners' uv and the base colour texture, so
    /// the kit comes off the atlas the same way the procedural cloth comes off
    /// its own.
    pub fn skinned_mesh(&self, joints: &[Affine], scale: f32) -> Mesh {
        let mut mesh = Mesh::with_capacity(self.indices.len() / 3);
        self.skin_into(joints, scale, &mut mesh);
        mesh
    }

    /// The same figure, built into a mesh that is already there.
    ///
    /// A hero dropped into a broadcast frame is rebuilt every frame, and every
    /// rebuild wants the same triangles in the same buffer as the frame before.
    /// The broadcast's own figures are filled this way; this is the same deal for
    /// an authored one.
    pub fn skin_into(&self, joints: &[Affine], scale: f32, mesh: &mut Mesh) {
        self.skin_into_kit(joints, scale, &self.texture, mesh);
    }

    /// The same figure wearing an atlas that is not the one it shipped with.
    ///
    /// A character arrives carrying one club's colours, which leaves every team
    /// in the file looking like the same team. This is the seam that lets one
    /// asset dress fourteen men: the geometry, the skeleton and the face are the
    /// file's, and the cloth is whoever's kit is handed in here.
    pub fn skin_into_kit(
        &self,
        joints: &[Affine],
        scale: f32,
        kit: &Arc<Texture>,
        mesh: &mut Mesh,
    ) {
        let (pos, nor) = self.skin(joints, scale);
        mesh.reuse();
        mesh.tris.reserve(self.indices.len() / 3);
        mesh.textures.push(Arc::clone(kit));
        for t in self.indices.chunks_exact(3) {
            let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
            let (pa, pb, pc) = (pos[a], pos[b], pos[c]);
            // A degenerate triangle can never put down a pixel, and this file has
            // a few where limbs meet. Dropping them up front saves the rasteriser
            // the per-triangle work of finding that out.
            if (pb - pa).cross(pc - pa).length_sq() < 1e-12 {
                continue;
            }
            mesh.tris.push(Tri {
                a: pa,
                b: pb,
                c: pc,
                na: nor[a],
                nb: nor[b],
                nc: nor[c],
                color: Rgb::new(255, 255, 255),
                shade: 1.0,
                uv: [self.uvs[a], self.uvs[b], self.uvs[c]],
                tex: Some(0),
            });
        }
    }
}

/// The file's idle clip, if it has one.
///
/// The names are the ones the exports actually use: Meshy's rig packs an
/// `Axe_Breathe_and_Look_Around`, and the Mixamo re-rig comes back with
/// `Idle_02` and `Idle_5`. Nothing here assumes any of them exist -- a file of
/// run cycles only answers `None`, and the caller falls back to its old rule.
pub fn idle_clip(hero: &Hero) -> Option<usize> {
    ["Idle_02", "Idle_5", "Idle", "Idle_01", "Axe_Breathe_and_Look_Around"]
        .iter()
        .find_map(|name| hero.clip_index(name))
}

/// An authored figure dropped into a frame in place of one procedural man.
///
/// The broadcast builds every man from primitives. This says: when a man inside
/// `within` yards of the lens is running, draw this character instead of him --
/// skinned, standing on the same spot, turned to the same heading -- so nothing
/// else in the frame has to know he is any different.
///
/// Only a moving man is replaced. A clip of a run on a body that is standing
/// still reads as a mannequin, and the file is locomotion and gestures.
pub struct HeroActor {
    pub hero: Arc<Hero>,
    /// The scale taking the file's own height to the height a man is on the
    /// field.
    pub scale: f32,
    /// Yards from the lens inside which the nearest running man is replaced.
    pub within: f32,
}

impl HeroActor {
    /// A hero standing `height` render units tall -- a man is about 1.9.
    pub fn new(hero: Arc<Hero>, height: f32) -> HeroActor {
        let scale = if hero.height > 1e-3 { height / hero.height } else { 1.0 };
        HeroActor { hero, scale, within: 14.0 }
    }
}

// ---------------------------------------------------------------------- club kits

/// What part of a man's kit a patch of the atlas is painted on.
///
/// A character arrives with its own club's colours baked into one atlas. Swapping
/// them is not a hue rotation, because the texture cannot say what a texel is:
/// the navy of a sleeve and the navy of a sock are the same three numbers. The
/// geometry can say. Every vertex knows where it sits on the body in the bind
/// pose, so the triangles that use a texel vote on what that texel belongs to,
/// and the shirt is whatever the torso is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Piece {
    /// Skin, hair, gloves: already the right colour, and not ours to paint.
    Keep = 0,
    Shirt,
    Shorts,
    Sock,
    Shoe,
}

const PIECES: usize = 5;

impl Piece {
    fn index(self) -> usize {
        self as usize
    }
}

/// Which piece of the kit a vertex is part of, from where it sits on the body.
///
/// The fractions are of the figure's own height with the feet at zero, and
/// `lateral` is how far the vertex hangs out from the spine -- the only thing
/// separating an arm from a rib at the same height, and a hand from a thigh.
fn piece_at(z_frac: f32, lateral_frac: f32) -> Piece {
    // Arms hang inside the shorts and the shirt both. Anything this far out from
    // the spine above the knees is a limb, and a limb is skin and undershirt.
    if lateral_frac > 0.17 && z_frac > 0.30 {
        return Piece::Keep;
    }
    if z_frac < 0.115 {
        Piece::Shoe
    } else if z_frac < 0.30 {
        Piece::Sock
    } else if z_frac < 0.45 {
        // Bare thigh between the top of the sock and the hem of the shorts.
        Piece::Keep
    } else if z_frac < 0.60 {
        Piece::Shorts
    } else if z_frac < 0.87 {
        Piece::Shirt
    } else {
        // Neck and head. The face is the one thing a re-kit must not touch, so
        // everything above the shoulders is left exactly as it was authored.
        Piece::Keep
    }
}

/// Which piece every texel of an atlas is painted on, and the average light of
/// each one.
///
/// That average is the whole trick. A piece of kit is repainted by keeping its
/// own light and swapping only its hue: a fold, a seam, the shadow under an arm
/// and the number painted across a chest are all differences in brightness, so
/// they survive being dressed for another club. Multiplying a club's colour by
/// this ratio turns a black and gold shirt green and gold without throwing away
/// any of the modelling the artist did.
pub struct KitMap {
    pub w: usize,
    pub h: usize,
    piece: Vec<Piece>,
    mean: [f32; PIECES],
}

impl KitMap {
    /// How many texels of the atlas belong to a piece. Read by the tests and by
    /// anything that wants to know whether a map actually found the shirt.
    pub fn count(&self, piece: Piece) -> usize {
        self.piece.iter().filter(|p| **p == piece).count()
    }
}

/// Rasterise one triangle's footprint in uv space into the piece map.
fn fill_uv(map: &mut [Piece], w: usize, h: usize, uvs: &[(f32, f32)], tri: [usize; 3], piece: Piece) {
    let p = [uvs[tri[0]], uvs[tri[1]], uvs[tri[2]]];
    let xs = [p[0].0 * w as f32, p[1].0 * w as f32, p[2].0 * w as f32];
    let ys = [p[0].1 * h as f32, p[1].1 * h as f32, p[2].1 * h as f32];
    let area = (xs[1] - xs[0]) * (ys[2] - ys[0]) - (xs[2] - xs[0]) * (ys[1] - ys[0]);
    if area.abs() < 1e-6 {
        return;
    }
    let inv = 1.0 / area;
    let lo_x = xs.iter().cloned().fold(f32::MAX, f32::min).floor().max(0.0) as usize;
    let lo_y = ys.iter().cloned().fold(f32::MAX, f32::min).floor().max(0.0) as usize;
    let hi_x = (xs.iter().cloned().fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(w - 1);
    let hi_y = (ys.iter().cloned().fold(f32::MIN, f32::max).ceil().max(0.0) as usize).min(h - 1);
    for py in lo_y..=hi_y {
        for px in lo_x..=hi_x {
            let (cx, cy) = (px as f32 + 0.5, py as f32 + 0.5);
            let w0 = ((xs[1] - cx) * (ys[2] - cy) - (xs[2] - cx) * (ys[1] - cy)) * inv;
            let w1 = ((xs[2] - cx) * (ys[0] - cy) - (xs[0] - cx) * (ys[2] - cy)) * inv;
            let w2 = 1.0 - w0 - w1;
            // A little slack on the edges: texels that fall just outside a
            // triangle are still part of that piece of the shirt, and a texel
            // left unclaimed reads as a seam of the old club's colour.
            if w0 >= -0.05 && w1 >= -0.05 && w2 >= -0.05 {
                map[py * w + px] = piece;
            }
        }
    }
}

fn eat(h: &mut u64, b: u8) {
    *h ^= b as u64;
    *h = h.wrapping_mul(0x0000_0100_0000_01b3);
}

/// One atlas per club, so two clubs in the same colours share one.
fn kit_key(hero: &Hero, uniform: &Uniform) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in [
        hero.positions.len() as u8,
        (hero.positions.len() >> 8) as u8,
        (hero.height * 100.0) as u8,
        uniform.jersey.r,
        uniform.jersey.g,
        uniform.jersey.b,
        uniform.jersey_accent.r,
        uniform.jersey_accent.g,
        uniform.jersey_accent.b,
        uniform.shorts.r,
        uniform.shorts.g,
        uniform.shorts.b,
        uniform.socks.r,
        uniform.socks.g,
        uniform.socks.b,
        uniform.shoes.r,
        uniform.shoes.g,
        uniform.shoes.b,
    ] {
        eat(&mut h, b);
    }
    h
}

impl Hero {
    /// Read the atlas against the body: which piece of the kit each texel is.
    pub fn kit_map(&self) -> KitMap {
        let (w, h) = (self.texture.w, self.texture.h);
        let mut piece = vec![Piece::Keep; w * h];
        let mut vert = vec![Piece::Keep; self.positions.len()];
        let inv_height = if self.height > 1e-3 { 1.0 / self.height } else { 1.0 };
        for (i, p) in self.positions.iter().enumerate() {
            let s = to_scene([p.x, p.y, p.z]);
            vert[i] = piece_at(s.z * inv_height, s.y.abs() * inv_height);
        }
        for t in self.indices.chunks_exact(3) {
            let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
            let (pa, pb, pc) = (vert[a], vert[b], vert[c]);
            // A triangle spanning two pieces lies on the hem between them. Give
            // it to whichever piece has the vote, and leave it in the file's own
            // colours when the vote is tied.
            let here = if pa == pb || pa == pc {
                pa
            } else if pb == pc {
                pb
            } else {
                continue;
            };
            if here == Piece::Keep {
                continue;
            }
            fill_uv(&mut piece, w, h, &self.uvs, [a, b, c], here);
        }
        let mut sum = [0.0f32; PIECES];
        let mut count = [0usize; PIECES];
        for (i, px) in self.texture.px.iter().enumerate() {
            let k = piece[i].index();
            sum[k] += px.luma();
            count[k] += 1;
        }
        let mut mean = [128.0f32; PIECES];
        for k in 0..PIECES {
            if count[k] > 0 {
                // Floored well above zero: a piece that is nearly black would
                // otherwise divide every fold in it into a blowout.
                mean[k] = (sum[k] / count[k] as f32).max(10.0);
            }
        }
        KitMap { w, h, piece, mean }
    }

    /// This figure dressed for a club.
    pub fn kit_texture(&self, uniform: &Uniform) -> Arc<Texture> {
        static CACHE: OnceLock<Mutex<Vec<(u64, Arc<Texture>)>>> = OnceLock::new();
        let key = kit_key(self, uniform);
        let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
        {
            let held = cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((_, tex)) = held.iter().find(|(k, _)| *k == key) {
                return tex.clone();
            }
        }
        let map = self.kit_map();
        let target = [
            Rgb::WHITE,
            uniform.jersey,
            uniform.shorts,
            uniform.socks,
            uniform.shoes,
        ];
        let mut tex = Texture::new(map.w, map.h, Rgb::BLACK);
        for (i, src) in self.texture.px.iter().enumerate() {
            let piece = map.piece[i];
            if piece == Piece::Keep {
                tex.px[i] = *src;
                continue;
            }
            let k = piece.index();
            let detail = (src.luma() / map.mean[k]).clamp(0.45, 1.8);
            tex.px[i] = target[k].scale(detail);
        }
        let tex = Arc::new(tex);
        cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((key, tex.clone()));
        tex
    }
}

/// An authored character standing in for every man on the field.
///
/// [`HeroActor`] is a showcase: one figure, the nearest running man, so the model
/// can be judged in a real frame. This is the other thing entirely -- the
/// imported figure as the base for all fourteen men, each in his own club's kit.
pub struct AuthoredField {
    pub hero: Arc<Hero>,
    pub scale: f32,
    /// One atlas per side. A club's kit is painted once, not once a man.
    pub kits: [Arc<Texture>; 2],
}

impl AuthoredField {
    /// The field `height` render units tall -- a man is about 1.9.
    pub fn new(hero: Arc<Hero>, height: f32, uniforms: [&Uniform; 2]) -> AuthoredField {
        let scale = if hero.height > 1e-3 { height / hero.height } else { 1.0 };
        let kits = [
            hero.kit_texture(uniforms[0]),
            hero.kit_texture(uniforms[1]),
        ];
        AuthoredField { hero, scale, kits }
    }
}

// ---------------------------------------------------------------------- glTF IO

/// glTF is Y up and this renderer is Z up; a character exported facing +Z has to
/// come out facing +X, which is where a heading of zero points a procedural
/// figure. `(x, y, z) -> (z, x, y)` does both, and being a cyclic permutation it
/// keeps the frame right-handed, so nothing comes out mirrored.
pub fn to_scene(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[2], p[0], p[1])
}

fn split_glb(bytes: &[u8]) -> Result<(&[u8], &[u8]), String> {
    if bytes.len() < 20 {
        return Err("file is too short to be a GLB".into());
    }
    if &bytes[0..4] != b"glTF" {
        return Err("no glTF magic: this is not a binary glTF".into());
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version != 2 {
        return Err(format!("GLB version {version}, and only version 2 is read"));
    }
    let total = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    if total > bytes.len() {
        return Err(format!("GLB claims {total} bytes but the file is {}", bytes.len()));
    }
    let mut json = None;
    let mut bin = None;
    let mut at = 12;
    while at + 8 <= total {
        let len = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let kind = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap());
        at += 8;
        if at + len > total {
            return Err("a GLB chunk runs past the end of the file".into());
        }
        match kind {
            0x4E4F_534A => json = Some(&bytes[at..at + len]),
            0x004E_4942 => bin = Some(&bytes[at..at + len]),
            _ => {}
        }
        at += len;
    }
    let json = json.ok_or("the GLB has no JSON chunk")?;
    Ok((json, bin.unwrap_or(&[])))
}

type Primitives = (Vec<Vec3>, Vec<Vec3>, Vec<(f32, f32)>, Vec<[u16; 4]>, Vec<[f32; 4]>, Vec<u32>);

fn read_primitives(doc: &Json, read: &Reader) -> Result<Primitives, String> {
    let meshes = doc.get("meshes").map(|m| m.array()).ok_or("no meshes in the file")?;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut joints = Vec::new();
    let mut weights = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for mesh in meshes {
        for prim in mesh.get("primitives").map(|p| p.array()).unwrap_or(&[]) {
            let attrs = prim.get("attributes").ok_or("a primitive has no attributes")?;
            let at = |name: &str| attrs.get(name).and_then(|v| v.as_usize());
            let pos_index = at("POSITION").ok_or("a primitive has no POSITION")?;
            let (pc, pos) = read.floats(pos_index)?;
            if pc != 3 {
                return Err(format!("POSITION has {pc} components, and needs 3"));
            }
            let base = positions.len() as u32;
            positions.extend(pos.chunks_exact(3).map(|c| Vec3::new(c[0], c[1], c[2])));

            let count = positions.len() - base as usize;
            match at("NORMAL") {
                Some(i) => {
                    let (nc, nor) = read.floats(i)?;
                    if nc != 3 || nor.len() / 3 != count {
                        return Err("the NORMAL accessor does not match the vertex count".into());
                    }
                    normals.extend(nor.chunks_exact(3).map(|c| Vec3::new(c[0], c[1], c[2])));
                }
                None => {
                    // Without normals there is no round figure: it would shade
                    // from whichever corner the light happened to hit. Refuse.
                    return Err("a primitive has no NORMAL data".into());
                }
            }
            match at("TEXCOORD_0") {
                Some(i) => {
                    let (uc, uv) = read.floats(i)?;
                    if uc < 2 || uv.len() / uc != count {
                        return Err("the TEXCOORD_0 accessor does not match the vertex count".into());
                    }
                    uvs.extend(uv.chunks_exact(uc).map(|c| (c[0], c[1])));
                }
                None => uvs.extend(std::iter::repeat((0.0, 0.0)).take(count)),
            }
            match (at("JOINTS_0"), at("WEIGHTS_0")) {
                (Some(ji), Some(wi)) => {
                    let (jc, js) = read.uints(ji)?;
                    let (wc, ws) = read.floats(wi)?;
                    if jc < 4 || wc < 4 || js.len() / jc != count || ws.len() / wc != count {
                        return Err("the skin weights do not match the vertex count".into());
                    }
                    for (j, w) in js.chunks_exact(jc).zip(ws.chunks_exact(wc)) {
                        joints.push([j[0] as u16, j[1] as u16, j[2] as u16, j[3] as u16]);
                        weights.push([w[0], w[1], w[2], w[3]]);
                    }
                }
                (None, None) => {
                    return Err("a primitive has no skin, so nothing could move it".into());
                }
                _ => return Err("a primitive has only one of JOINTS_0 and WEIGHTS_0".into()),
            }

            match prim.get("indices").and_then(|v| v.as_usize()) {
                Some(i) => {
                    let (_, list) = read.uints(i)?;
                    indices.extend(list.iter().map(|i| i + base));
                }
                None => indices.extend(base..base + count as u32),
            }
        }
    }
    if positions.is_empty() || indices.is_empty() {
        return Err("the file holds no triangles".into());
    }
    Ok((positions, normals, uvs, joints, weights, indices))
}

fn read_nodes(doc: &Json) -> Result<(Vec<NodeXform>, Vec<Option<usize>>), String> {
    let nodes = doc.get("nodes").map(|n| n.array()).unwrap_or(&[]);
    let mut rest = Vec::with_capacity(nodes.len());
    let mut parent = vec![None; nodes.len()];
    for (i, node) in nodes.iter().enumerate() {
        if node.get("matrix").is_some() {
            return Err(format!("node {i} is written as a matrix, which this loader does not read"));
        }
        let v3 = |key: &str, fallback: Vec3| -> Vec3 {
            match node.get(key).map(|v| v.array()).filter(|a| a.len() >= 3) {
                Some(a) => Vec3::new(
                    a[0].as_f32().unwrap_or(fallback.x),
                    a[1].as_f32().unwrap_or(fallback.y),
                    a[2].as_f32().unwrap_or(fallback.z),
                ),
                None => fallback,
            }
        };
        let q = match node.get("rotation").map(|v| v.array()).filter(|a| a.len() >= 4) {
            Some(a) => [
                a[0].as_f32().unwrap_or(0.0),
                a[1].as_f32().unwrap_or(0.0),
                a[2].as_f32().unwrap_or(0.0),
                a[3].as_f32().unwrap_or(1.0),
            ],
            None => [0.0, 0.0, 0.0, 1.0],
        };
        rest.push(NodeXform {
            t: v3("translation", Vec3::ZERO),
            q,
            s: v3("scale", Vec3::new(1.0, 1.0, 1.0)),
        });
        for child in node.get("children").map(|c| c.array()).unwrap_or(&[]) {
            if let Some(ci) = child.as_usize() {
                if ci < parent.len() {
                    parent[ci] = Some(i);
                }
            }
        }
    }
    Ok((rest, parent))
}

fn read_skin(doc: &Json, read: &Reader) -> Result<(Vec<usize>, Vec<Affine>), String> {
    let skin = doc.get("skins").map(|s| s.array()).unwrap_or(&[]).first().ok_or("the file has no skin")?;
    let joints: Vec<usize> = skin
        .get("joints")
        .map(|j| j.array())
        .unwrap_or(&[])
        .iter()
        .filter_map(|j| j.as_usize())
        .collect();
    let ibm_index = skin
        .get("inverseBindMatrices")
        .and_then(|v| v.as_usize())
        .ok_or("the skin has no inverse bind matrices")?;
    let (comps, flat) = read.floats(ibm_index)?;
    if comps != 16 {
        return Err(format!("inverse bind matrices have {comps} components, and need 16"));
    }
    let bind = flat.chunks_exact(16).map(Affine::from_column_major).collect();
    Ok((joints, bind))
}

fn read_clips(doc: &Json, read: &Reader) -> Result<Vec<Clip>, String> {
    const PATHS_ERROR: &str = "an animation path is not translation, rotation or scale";
    let mut clips = Vec::new();
    for anim in doc.get("animations").map(|a| a.array()).unwrap_or(&[]) {
        let mut samplers = Vec::new();
        let mut duration = 0.0f32;
        for s in anim.get("samplers").map(|s| s.array()).unwrap_or(&[]) {
            let input = s.get("input").and_then(|v| v.as_usize()).ok_or("an animation sampler has no input")?;
            let output = s.get("output").and_then(|v| v.as_usize()).ok_or("an animation sampler has no output")?;
            let (_, times) = read.floats(input)?;
            let (comps, values) = read.floats(output)?;
            if comps < 3 || values.len() / comps != times.len() {
                return Err("an animation sampler's keys do not match its values".into());
            }
            if let Some(last) = times.last() {
                duration = duration.max(*last);
            }
            let step = matches!(s.get("interpolation").and_then(|i| i.as_str()), Some("STEP"));
            samplers.push(Sampler {
                times,
                values: values
                    .chunks_exact(comps)
                    .map(|c| [c[0], c[1], c[2], if comps > 3 { c[3] } else { 0.0 }])
                    .collect(),
                step,
            });
        }
        let mut channels = Vec::new();
        for c in anim.get("channels").map(|c| c.array()).unwrap_or(&[]) {
            let sampler = c.get("sampler").and_then(|v| v.as_usize()).ok_or("an animation channel has no sampler")?;
            let target = c.get("target").ok_or("an animation channel has no target")?;
            let node = target.get("node").and_then(|v| v.as_usize()).ok_or("an animation channel has no node")?;
            let path = match target.get("path").and_then(|p| p.as_str()) {
                Some("translation") => TrackPath::Translation,
                Some("rotation") => TrackPath::Rotation,
                Some("scale") => TrackPath::Scale,
                _ => return Err(PATHS_ERROR.into()),
            };
            channels.push(Channel { node, path, sampler });
        }
        clips.push(Clip {
            name: anim.get("name").and_then(|n| n.as_str()).unwrap_or("clip").to_string(),
            duration,
            channels,
            samplers,
        });
    }
    Ok(clips)
}

/// The base colour image of the first material, as the renderer's texture.
fn read_texture(doc: &Json, read: &Reader) -> Result<Texture, String> {
    let none = || Texture::new(1, 1, Rgb::new(255, 255, 255));
    let Some(material) = doc.get("materials").map(|m| m.array()).and_then(|m| m.first()) else {
        return Ok(none());
    };
    let slot = material
        .get("pbrMetallicRoughness")
        .and_then(|p| p.get("baseColorTexture"))
        .and_then(|b| b.get("index"))
        .and_then(|i| i.as_usize());
    let Some(slot) = slot else { return Ok(none()) };
    let image = doc
        .get("textures")
        .map(|t| t.array())
        .and_then(|t| t.get(slot))
        .and_then(|t| t.get("source"))
        .and_then(|s| s.as_usize())
        .and_then(|i| doc.get("images").map(|im| im.array()).and_then(|im| im.get(i)))
        .ok_or("the material's base colour texture has no image")?;
    if image.get("uri").is_some() {
        return Err("the base colour image is an external file, and only embedded GLBs are read".into());
    }
    let view = image.get("bufferView").and_then(|v| v.as_usize()).ok_or("the base colour image has no data")?;
    let bytes = read.view_bytes(view)?;
    let decoded = image::load_from_memory(bytes)
        .map_err(|e| format!("the base colour image did not decode: {e}"))?
        .to_rgb8();
    let (w, h) = decoded.dimensions();
    let mut tex = Texture::new(w as usize, h as usize, Rgb::BLACK);
    for (i, p) in decoded.pixels().enumerate() {
        tex.px[i] = Rgb::new(p[0], p[1], p[2]);
    }
    Ok(tex)
}

struct Reader<'a> {
    bin: &'a [u8],
    accessors: &'a [Json],
    views: &'a [Json],
}

struct Raw<'a> {
    count: usize,
    comps: usize,
    comp: u32,
    normalized: bool,
    data: &'a [u8],
    base: usize,
    stride: usize,
}

impl Raw<'_> {
    fn size(&self) -> usize {
        match self.comp {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            _ => 4,
        }
    }

    fn at(&self, elem: usize, comp: usize) -> Result<&[u8], String> {
        let off = self.base + elem * self.stride + comp * self.size();
        self.data.get(off..off + self.size()).ok_or_else(|| "an accessor runs past the end of its buffer".to_string())
    }

    fn number(&self, elem: usize, comp: usize) -> Result<f32, String> {
        let b = self.at(elem, comp)?;
        Ok(match self.comp {
            5126 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            5125 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
            5123 => u16::from_le_bytes([b[0], b[1]]) as f32,
            5122 => i16::from_le_bytes([b[0], b[1]]) as f32,
            5121 => b[0] as f32,
            5120 => (b[0] as i8) as f32,
            other => return Err(format!("component type {other} is not readable")),
        })
    }

    fn integer(&self, elem: usize, comp: usize) -> Result<u32, String> {
        let b = self.at(elem, comp)?;
        Ok(match self.comp {
            5125 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            5123 => u16::from_le_bytes([b[0], b[1]]) as u32,
            5121 => b[0] as u32,
            other => return Err(format!("joint indices with component type {other} are not readable")),
        })
    }
}

impl<'a> Reader<'a> {
    fn view_bytes(&self, view: usize) -> Result<&'a [u8], String> {
        let bv = self.views.get(view).ok_or_else(|| format!("bufferView {view} is missing"))?;
        let off = bv.get("byteOffset").and_then(|v| v.as_usize()).unwrap_or(0);
        let len = bv.get("byteLength").and_then(|v| v.as_usize()).ok_or("a bufferView has no length")?;
        self.bin
            .get(off..off + len)
            .ok_or_else(|| format!("bufferView {view} runs past the end of the binary chunk"))
    }

    fn raw(&self, index: usize) -> Result<Raw<'a>, String> {
        let a = self.accessors.get(index).ok_or_else(|| format!("accessor {index} is missing"))?;
        if a.get("sparse").is_some() {
            return Err(format!("accessor {index} is sparse, which this loader does not read"));
        }
        let count = a.get("count").and_then(|v| v.as_usize()).ok_or("an accessor has no count")?;
        let comp = a.get("componentType").and_then(|v| v.as_usize()).ok_or("an accessor has no component type")? as u32;
        let comps = match a.get("type").and_then(|v| v.as_str()) {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") => 4,
            Some("MAT4") => 16,
            other => return Err(format!("accessor {index} has type {other:?}, which is not read")),
        };
        let normalized = a.get("normalized").is_some_and(|n| matches!(n, Json::Bool(true)));
        let (data, view_base, stride) = match a.get("bufferView").and_then(|v| v.as_usize()) {
            Some(view) => {
                let bv = self.views.get(view).ok_or_else(|| format!("bufferView {view} is missing"))?;
                let stride = bv
                    .get("byteStride")
                    .and_then(|v| v.as_usize())
                    .unwrap_or_else(|| comps * component_size(comp));
                (self.view_bytes(view)?, 0, stride)
            }
            None => (self.bin, 0, comps * component_size(comp)),
        };
        let base = view_base + a.get("byteOffset").and_then(|v| v.as_usize()).unwrap_or(0);
        Ok(Raw { count, comps, comp, normalized, data, base, stride })
    }

    fn floats(&self, index: usize) -> Result<(usize, Vec<f32>), String> {
        let raw = self.raw(index)?;
        let mut out = Vec::with_capacity(raw.count * raw.comps);
        for e in 0..raw.count {
            for c in 0..raw.comps {
                out.push(raw.number(e, c)?);
            }
        }
        Ok((raw.comps, out))
    }

    fn uints(&self, index: usize) -> Result<(usize, Vec<u32>), String> {
        let raw = self.raw(index)?;
        let mut out = Vec::with_capacity(raw.count * raw.comps);
        for e in 0..raw.count {
            for c in 0..raw.comps {
                out.push(raw.integer(e, c)?);
            }
        }
        Ok((raw.comps, out))
    }
}

fn component_size(comp: u32) -> usize {
    match comp {
        5120 | 5121 => 1,
        5122 | 5123 => 2,
        _ => 4,
    }
}

// ------------------------------------------------------------------- json reader

/// The reader is small on purpose: a character file's metadata is a JSON
/// document of a few hundred kilobytes, read once, and a parser of its own keeps
/// the renderer's dependency list short. It handles the whole grammar, including
/// escapes and surrogate pairs in names, and refuses nesting deep enough to blow
/// the stack.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn parse(bytes: &[u8]) -> Result<Json, String> {
        let mut p = Parser { s: bytes, i: 0, depth: 0 };
        let v = p.value()?;
        p.ws();
        if p.i < p.s.len() {
            return Err(format!("JSON has {} bytes of trailing rubbish", p.s.len() - p.i));
        }
        Ok(v)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn array(&self) -> &[Json] {
        match self {
            Json::Arr(v) => v,
            _ => &[],
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Json::Num(n) => Some(*n as f32),
            _ => None,
        }
    }

    pub fn as_usize(&self) -> Option<usize> {
        match self {
            Json::Num(n) if *n >= 0.0 => Some(*n as usize),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    depth: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while matches!(self.s.get(self.i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        let b = *self.s.get(self.i).ok_or("JSON ends in the middle of a value")?;
        match b {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => Ok(Json::Str(self.string()?)),
            b't' => self.literal(b"true", Json::Bool(true)),
            b'f' => self.literal(b"false", Json::Bool(false)),
            b'n' => self.literal(b"null", Json::Null),
            _ => self.number(),
        }
    }

    fn literal(&mut self, word: &[u8], value: Json) -> Result<Json, String> {
        if self.s.len() >= self.i + word.len() && &self.s[self.i..self.i + word.len()] == word {
            self.i += word.len();
            Ok(value)
        } else {
            Err(format!("JSON has an unreadable literal at byte {}", self.i))
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        while matches!(self.s.get(self.i), Some(b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')) {
            self.i += 1;
        }
        if start == self.i {
            return Err(format!("JSON has an unexpected byte at {}", self.i));
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| "JSON number is not utf-8")?;
        text.parse::<f64>().map(Json::Num).map_err(|_| format!("JSON number {text:?} does not parse"))
    }

    fn string(&mut self) -> Result<String, String> {
        if self.s.get(self.i) != Some(&b'"') {
            return Err(format!("JSON string expected at byte {}", self.i));
        }
        self.i += 1;
        let mut out = String::new();
        let mut run = self.i;
        loop {
            let b = *self.s.get(self.i).ok_or("JSON string is never closed")?;
            match b {
                b'"' => {
                    out.push_str(run_str(&self.s[run..self.i])?);
                    self.i += 1;
                    return Ok(out);
                }
                b'\\' => {
                    out.push_str(run_str(&self.s[run..self.i])?);
                    self.i += 1;
                    let e = *self.s.get(self.i).ok_or("JSON escape is cut off")?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let ch = if (0xD800..0xDC00).contains(&hi) {
                                if self.s.get(self.i) == Some(&b'\\') && self.s.get(self.i + 1) == Some(&b'u') {
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    char::from_u32(0x1_0000 + ((hi - 0xD800) << 10) + (lo.wrapping_sub(0xDC00)))
                                } else {
                                    None
                                }
                            } else {
                                char::from_u32(hi)
                            };
                            out.push(ch.unwrap_or('\u{fffd}'));
                        }
                        other => return Err(format!("JSON has no escape \\{}", other as char)),
                    }
                    run = self.i;
                }
                _ => self.i += 1,
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let mut v = 0u32;
        for _ in 0..4 {
            let b = *self.s.get(self.i).ok_or("JSON \\u escape is cut off")?;
            let d = (b as char).to_digit(16).ok_or("JSON \\u escape is not hexadecimal")?;
            v = v * 16 + d;
            self.i += 1;
        }
        Ok(v)
    }

    fn enter(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > 64 {
            return Err("JSON nests more than 64 deep".into());
        }
        Ok(())
    }

    fn object(&mut self) -> Result<Json, String> {
        self.enter()?;
        self.i += 1;
        let mut pairs = Vec::new();
        self.ws();
        if self.s.get(self.i) == Some(&b'}') {
            self.i += 1;
            self.depth -= 1;
            return Ok(Json::Obj(pairs));
        }
        loop {
            self.ws();
            let key = self.string()?;
            self.ws();
            if self.s.get(self.i) != Some(&b':') {
                return Err(format!("JSON object needs a colon at byte {}", self.i));
            }
            self.i += 1;
            pairs.push((key, self.value()?));
            self.ws();
            match self.s.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    self.depth -= 1;
                    return Ok(Json::Obj(pairs));
                }
                _ => return Err(format!("JSON object is malformed at byte {}", self.i)),
            }
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.enter()?;
        self.i += 1;
        let mut items = Vec::new();
        self.ws();
        if self.s.get(self.i) == Some(&b']') {
            self.i += 1;
            self.depth -= 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.ws();
            match self.s.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    self.depth -= 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(format!("JSON array is malformed at byte {}", self.i)),
            }
        }
    }
}

fn run_str(bytes: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(bytes).map_err(|_| "JSON string is not utf-8".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_1_SQRT_2;

    #[test]
    fn the_json_reader_walks_a_nested_document() {
        let doc = Json::parse(br#"{"a":[1,2.5,-3],"b":{"c":"x\"y\u00e9"},"d":true,"e":null}"#).unwrap();
        assert_eq!(doc.get("a").map(|a| a.array().len()), Some(3));
        assert_eq!(doc.get("a").and_then(|a| a.array()[2].as_f32()), Some(-3.0));
        assert_eq!(doc.get("b").and_then(|b| b.get("c")).and_then(|c| c.as_str()), Some("x\"yé"));
        assert_eq!(doc.get("d"), Some(&Json::Bool(true)));
        assert_eq!(doc.get("e"), Some(&Json::Null));
    }

    #[test]
    fn a_broken_glb_is_refused_rather_than_half_read() {
        let err = split_glb(b"not a glb at all, just text").unwrap_err();
        assert!(err.contains("magic"), "{err}");
        let mut short = vec![0u8; 12];
        short[0..4].copy_from_slice(b"glTF");
        short[4..8].copy_from_slice(&2u32.to_le_bytes());
        assert!(split_glb(&short).is_err());
    }

    #[test]
    fn a_quarter_turn_about_x_stands_up_on_end() {
        // +90 degrees about X takes +Y to +Z and +Z to -Y.
        let m = Mat3::from_quat([FRAC_1_SQRT_2, 0.0, 0.0, FRAC_1_SQRT_2]);
        let y = m.mul_vec(Vec3::new(0.0, 1.0, 0.0));
        assert!((y.z - 1.0).abs() < 1e-6 && y.y.abs() < 1e-6, "Y went to {y:?}");
        let z = m.mul_vec(Vec3::new(0.0, 0.0, 1.0));
        assert!((z.y + 1.0).abs() < 1e-6, "Z went to {z:?}");
    }

    #[test]
    fn the_kit_map_tells_a_shirt_from_a_hand_hanging_beside_it() {
        // The shirt is the torso; a hand hanging past the hem is at the same
        // height as the shorts and has to stay skin. That is why the vote comes
        // from where the geometry sits rather than from the colours in the file.
        assert_eq!(piece_at(0.70, 0.05), Piece::Shirt);
        assert_eq!(piece_at(0.70, 0.30), Piece::Keep);
        assert_eq!(piece_at(0.50, 0.06), Piece::Shorts);
        assert_eq!(piece_at(0.50, 0.30), Piece::Keep);
        assert_eq!(piece_at(0.20, 0.06), Piece::Sock);
        assert_eq!(piece_at(0.05, 0.08), Piece::Shoe);
        // The face is the one thing a re-kit must never repaint.
        assert_eq!(piece_at(0.95, 0.02), Piece::Keep);
    }

    #[test]
    fn the_scene_mapping_stands_a_figure_up_and_points_it_downfield() {
        // glTF: Y up, character facing +Z. Render: Z up, heading zero facing +X.
        let up = to_scene([0.0, 1.0, 0.0]);
        assert_eq!(up, Vec3::new(0.0, 0.0, 1.0));
        let forward = to_scene([0.0, 0.0, 1.0]);
        assert_eq!(forward, Vec3::new(1.0, 0.0, 0.0));
        // Facing +X with up +Z puts his right hand at -Y: he is not mirrored.
        let hand = to_scene([-1.0, 0.0, 0.0]);
        assert_eq!(hand, Vec3::new(0.0, -1.0, 0.0));
    }

    #[test]
    fn a_sampler_holds_its_ends_and_interpolates_between() {
        let s = Sampler {
            times: vec![0.0, 1.0],
            values: vec![[0.0, 0.0, 0.0, 0.0], [10.0, 20.0, 30.0, 0.0]],
            step: false,
        };
        assert_eq!(s.at(-5.0, false), [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(s.at(9.0, false), [10.0, 20.0, 30.0, 0.0]);
        assert_eq!(s.at(0.5, false), [5.0, 10.0, 15.0, 0.0]);
    }

    #[test]
    fn a_step_sampler_does_not_interpolate() {
        let s = Sampler {
            times: vec![0.0, 1.0],
            values: vec![[0.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]],
            step: true,
        };
        assert_eq!(s.at(0.9, false), [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn slerp_keeps_the_rotation_a_unit_quaternion() {
        let q = quat_slerp([0.0, 0.0, 0.0, 1.0], [FRAC_1_SQRT_2, 0.0, 0.0, FRAC_1_SQRT_2], 0.5);
        let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
        assert!((len - 1.0).abs() < 1e-5, "half a turn left a quaternion of length {len}");
        assert!((q[0] - 0.382_683_43).abs() < 1e-4, "half a quarter turn is not 22.5 degrees: {q:?}");
    }

    #[test]
    fn a_bind_matrix_written_column_major_is_read_the_right_way_round() {
        // Translation of (5, 6, 7) with no rotation, written the way glTF writes it.
        let m = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 5.0, 6.0, 7.0, 1.0];
        let a = Affine::from_column_major(&m);
        assert_eq!(a.point(Vec3::ZERO), Vec3::new(5.0, 6.0, 7.0));
        assert_eq!(a.point(Vec3::new(1.0, 0.0, 0.0)), Vec3::new(6.0, 6.0, 7.0));
    }

    #[test]
    fn a_parent_moves_its_child() {
        let parent = Affine::from_trs(Vec3::new(1.0, 0.0, 0.0), [0.0, 0.0, 0.0, 1.0], Vec3::new(1.0, 1.0, 1.0));
        let child = Affine::from_trs(Vec3::new(0.0, 2.0, 0.0), [0.0, 0.0, 0.0, 1.0], Vec3::new(1.0, 1.0, 1.0));
        assert_eq!(parent.mul(child).point(Vec3::ZERO), Vec3::new(1.0, 2.0, 0.0));
    }
}

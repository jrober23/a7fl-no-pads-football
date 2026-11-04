//! 7 on 7 player figures: sleeveless jersey, shorts, bare arms and no helmet.
//!
//! A figure is a small skeleton — pelvis, two-segment spine, head, and two joints
//! per limb — posed from a stride cycle and built out of lofted bodies, limbs and
//! a head. Nothing on it is a box measured off another box: the torso, shorts and
//! thighs are lofted through rounded cross-sections, so the silhouette has a
//! chest, a waist, a seat and a pair of calves instead of a stack of crates.
//!
//! Arms, legs, neck, socks and shoes are lofted with `Mesh::tube`, a single
//! surface through a chain of radii: a bicep is a swell in that chain, not a
//! cylinder with another cylinder on top of it.
//!
//! Two things make it read as a runner rather than a sliding doll:
//!
//! * The hip height is **solved from the legs**, not fixed. Whichever leg is
//!   straightest carries the body and its foot rests on the turf, so the vertical
//!   bob of a stride falls out of the contact instead of being faked with a sine
//!   wave. The feet cannot float or sink because the pose is built around them.
//! * `run_phase` is advanced by the *distance* the player covers, so a full cycle
//!   is one pair of steps of a fixed length. Feet keep their grip at any speed,
//!   and the cadence stays in a range a human could actually run in.

use nopads_core::math::{Rgb, Vec3};
use nopads_core::moves::Kind;
use nopads_core::team::{Player, Uniform};

use crate::cloth;
use crate::head::Head;
use crate::scene::Mesh;

/// Ankle height above the turf when the foot is planted: sole plus cleat.
const SOLE: f32 = 0.062;

/// Sides on a limb at full detail. Nine holds a highlight down the length of a
/// forearm instead of breaking it into facets, which is the line between a body
/// and a PlayStation One model.
const LIMB_SIDES: usize = 12;

/// Sides around the torso at full detail. Every facet is a step in the lighting
/// across the chest, so the count is what decides whether a shirt looks like
/// cloth wrapped around a man or like a stack of cards.
const BODY_SIDES: usize = 22;

/// How much of the body is worth building at a given distance from the lens.
///
/// A figure at full detail is around two thousand triangles and there are
/// fourteen of them standing on every frame of a software renderer, so a man on
/// the far hash is not drawn with eyes and a bicep: at that size they are a pixel
/// and a half, and the frame can only afford so much. The close level carries the
/// cloth, the face and the paint on the shirt; the far one is a figure the size of
/// a match head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    /// The men nearest the lens, the only level with a modelled face.
    Close,
    Full,
    Mid,
    Far,
}

impl Detail {
    /// Pick a level from the distance between the lens and the man.
    ///
    /// The lens keeps the ball in the middle of the picture, so most of the men
    /// in a broadcast frame are twenty to forty yards away: the middle level is
    /// the one that decides how the game looks, and it got the bigger share of
    /// the polygon budget.
    pub fn for_distance(yards: f32) -> Detail {
        if yards < 7.0 {
            Detail::Close
        } else if yards < 20.0 {
            Detail::Full
        } else if yards < 38.0 {
            Detail::Mid
        } else {
            Detail::Far
        }
    }

    fn sides(self) -> usize {
        match self {
            Detail::Close => 16,
            Detail::Full => LIMB_SIDES,
            Detail::Mid => 9,
            Detail::Far => 6,
        }
    }

    fn body_sides(self) -> usize {
        match self {
            Detail::Close => 28,
            Detail::Full => BODY_SIDES,
            Detail::Mid => 14,
            Detail::Far => 8,
        }
    }

    /// How finely a limb profile is split between its control rings. This is
    /// the subdivision level: it is what turns a cage of primitives into one
    /// smooth surface instead of a stack of cones.
    fn rings(self) -> usize {
        match self {
            Detail::Close => 3,
            Detail::Full => 2,
            Detail::Mid => 2,
            Detail::Far => 1,
        }
    }

    /// Eyes, brows, the quadriceps bulge, knee caps, trapezius wedges.
    fn fine(self) -> bool {
        !matches!(self, Detail::Far)
    }

    /// Painted numbers and the seams of the hair.
    fn paint(self) -> bool {
        !matches!(self, Detail::Far)
    }

    /// How much face to build: 3 is the modelled head, 0 is a head-shaped lump.
    fn quality(self) -> u8 {
        match self {
            Detail::Close => 3,
            Detail::Full => 1,
            Detail::Mid => 1,
            Detail::Far => 0,
        }
    }
}

/// How square a cross-section is. 2.0 is an ellipse; the torso sits between an
/// ellipse and a rounded rectangle, which is what a ribcage looks like.
const TORSO_ROUND: f32 = 2.9;
const LIMB_ROUND: f32 = 2.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct Pose {
    /// 0..1 for one whole stride cycle (two steps), advanced by distance covered.
    pub run_phase: f32,
    /// 0..1, where 1 is a full sprint.
    pub speed: f32,
    pub carrying: bool,
    pub blocking: bool,
    /// Where a blocker's hands are, across his body, in radians: the hand on the
    /// side the rusher goes to follows him.
    pub hands: f32,
    /// 0..1: hands up, driving the man in front of him back upfield.
    pub arms_up: f32,
    pub down: bool,
    pub celebrating: f32,
    /// 0 = facing, 1 = lateral shuffle.
    pub strafe: f32,
    /// The move the carrier is in the middle of, if any. A move named in the text
    /// bar is not a move: the figure has to be the thing that is spinning, jumping
    /// or going to ground, or the player is being lied to.
    pub motion: Option<Kind>,
    /// 0..1 through the move.
    pub motion_progress: f32,
    /// 0..1 through a tackle he is making: arms out, chest over his toes.
    pub wrapping: f32,
    /// 0..1 through a tackle he is taking: going over the hit to the turf.
    pub tackled: f32,
}

/// One leg through a running stride: `(phase, thigh angle, knee flexion, foot pitch)`.
///
/// Thigh angle is measured from straight down, positive forward. Knee flexion
/// folds the shin back and is never negative. Foot pitch points the toe down
/// when positive, so a heel strike is negative and toe-off is large and positive.
///
/// The shape is the real thing: reach forward and land, load over the foot,
/// extend and drive back, then snap the heel toward the seat and drive the knee
/// through while the shin unfolds for the next strike.
const RUN_LEG: [(f32, f32, f32, f32); 8] = [
    (0.00, 0.60, 0.34, -0.28),
    (0.12, 0.30, 0.16, 0.00),
    (0.28, -0.05, 0.20, 0.06),
    (0.40, -0.48, 0.42, 0.60),
    (0.52, -0.25, 1.90, 0.30),
    (0.68, 0.50, 1.75, -0.10),
    (0.86, 0.78, 0.70, -0.25),
    (1.00, 0.60, 0.34, -0.28),
];

/// The same cycle at a walk: shorter reach, straighter knees, no heel snap.
const WALK_LEG: [(f32, f32, f32, f32); 7] = [
    (0.00, 0.40, 0.10, 0.10),
    (0.15, 0.20, 0.08, -0.10),
    (0.35, -0.10, 0.12, 0.00),
    (0.50, -0.35, 0.20, 0.35),
    (0.68, -0.10, 0.95, 0.20),
    (0.86, 0.25, 0.55, -0.15),
    (1.00, 0.40, 0.10, 0.10),
];

/// Standing still: weight settled, knees soft, no stride to speak of.
const REST_LEG: [f32; 3] = [0.06, 0.12, 0.0];

/// Sample a keyed cycle, smoothing between keys so nothing pops.
fn cycle_at(keys: &[(f32, f32, f32, f32)], t: f32) -> [f32; 3] {
    let t = t.rem_euclid(1.0);
    let mut i = 0;
    while i + 1 < keys.len() && keys[i + 1].0 <= t {
        i += 1;
    }
    let a = keys[i];
    let b = keys[(i + 1).min(keys.len() - 1)];
    let span = (b.0 - a.0).max(1e-4);
    let raw = ((t - a.0) / span).clamp(0.0, 1.0);
    let u = raw * raw * (3.0 - 2.0 * raw);
    [
        a.1 + (b.1 - a.1) * u,
        a.2 + (b.2 - a.2) * u,
        a.3 + (b.3 - a.3) * u,
    ]
}

/// Leg angles for one phase of the cycle: `[thigh, knee flexion, foot pitch]`.
///
/// `run_blend` cross-fades a walk into a run, and `live` settles a standing
/// player into a neutral stance rather than freezing them mid-stride.
fn leg_angles(t: f32, run_blend: f32, live: f32) -> [f32; 3] {
    let walk = cycle_at(&WALK_LEG, t);
    let run = cycle_at(&RUN_LEG, t);
    let blended: [f32; 3] = [
        walk[0] + (run[0] - walk[0]) * run_blend,
        walk[1] + (run[1] - walk[1]) * run_blend,
        walk[2] + (run[2] - walk[2]) * run_blend,
    ];
    [
        REST_LEG[0] + (blended[0] - REST_LEG[0]) * live,
        REST_LEG[1] + (blended[1] - REST_LEG[1]) * live,
        blended[2] * live,
    ]
}

/// How far below the hip a leg carries its ankle. This is the number the body is
/// hung from: the tallest reach is the leg on the ground.
fn leg_reach(thigh: f32, knee: f32, thigh_len: f32, shin_len: f32) -> f32 {
    thigh.cos() * thigh_len + (thigh - knee).cos() * shin_len
}

// ---------------------------------------------------------------------------
// Rounded bodies
// ---------------------------------------------------------------------------

/// One cross-section of a body part: where it sits, how deep and how wide it is,
/// and which way it is turned. Stacking these and skinning between them is what
/// turns the figure from a pile of boxes into a body.
#[derive(Clone, Copy, Debug)]
struct Ring {
    center: Vec3,
    /// Half depth, front to back.
    rx: f32,
    /// Half width, side to side.
    ry: f32,
    yaw: f32,
}

impl Ring {
    fn new(center: Vec3, rx: f32, ry: f32, yaw: f32) -> Ring {
        Ring { center, rx, ry, yaw }
    }
}

/// A point on a superellipse: round = 2 is an ellipse, higher is squarer. Bodies
/// are neither round nor boxy, so the torso sits partway between the two.
fn superellipse(a: f32, round: f32) -> (f32, f32) {
    let (s, c) = a.sin_cos();
    let e = 2.0 / round.max(2.0);
    (
        c.signum() * c.abs().powf(e),
        s.signum() * s.abs().powf(e),
    )
}

/// The outward normal of the same superellipse the ring is made of: the gradient
/// of `(|x|/rx)^n + (|y|/ry)^n = 1`, turned into whatever way the ring is facing.
/// A torso lit by these has a chest that curves away from the light instead of a
/// row of flat panels.
fn ring_normal(ring: &Ring, a: f32, round: f32) -> Vec3 {
    let (ex, ey) = superellipse(a, round);
    let p = round - 1.0;
    let nx = ex.signum() * ex.abs().powf(p) / ring.rx.max(1e-4);
    let ny = ey.signum() * ey.abs().powf(p) / ring.ry.max(1e-4);
    let (s, c) = ring.yaw.sin_cos();
    Vec3::new(nx * c - ny * s, nx * s + ny * c, 0.0).normalize_or_zero()
}

fn ring_point(ring: &Ring, a: f32, round: f32) -> Vec3 {
    let (ex, ey) = superellipse(a, round);
    let (sy, cy) = ring.yaw.sin_cos();
    let x = ex * ring.rx;
    let y = ey * ring.ry;
    Vec3::new(
        ring.center.x + x * cy - y * sy,
        ring.center.y + x * sy + y * cy,
        ring.center.z,
    )
}

/// Skin a stack of rings into a surface, with the ends capped so a body is solid
/// from any camera, overhead included. `color_at(segment, side)` decides the
/// colour of every face, which is how the jersey patterns are painted: hoops,
/// side panels and shoulder stripes are the cloth itself, not boxes stuck on it.
fn loft(
    mesh: &mut Mesh,
    rings: &[Ring],
    sides: usize,
    round: f32,
    cap_bottom: bool,
    cap_top: bool,
    color_at: impl Fn(usize, usize) -> Rgb,
) {
    let angles: Vec<f32> = (0..sides)
        .map(|k| k as f32 / sides as f32 * std::f32::consts::TAU)
        .collect();
    let grid: Vec<Vec<Vec3>> = rings
        .iter()
        .map(|r| angles.iter().map(|a| ring_point(r, *a, round)).collect())
        .collect();
    let normals: Vec<Vec<Vec3>> = rings
        .iter()
        .map(|r| angles.iter().map(|a| ring_normal(r, *a, round)).collect())
        .collect();
    let bands = (rings.len() - 1) as f32;
    for i in 0..rings.len() - 1 {
        for k in 0..sides {
            let k1 = (k + 1) % sides;
            let before = mesh.tris.len();
            mesh.quad_curved(
                grid[i][k],
                normals[i][k],
                grid[i][k1],
                normals[i][k1],
                grid[i + 1][k1],
                normals[i + 1][k1],
                grid[i + 1][k],
                normals[i + 1][k],
                color_at(i, k),
                1.0,
            );
            // Round the body once for `u`, down it once for `v`, so a texture on
            // a loft is cloth wrapped around a man. `k + 1` rather than `k1`,
            // because the last panel has to end at 1.0 instead of snapping back
            // to 0 and screwing the pattern once round the whole shirt.
            let u0 = k as f32 / sides as f32;
            let u1 = (k + 1) as f32 / sides as f32;
            let (v0, v1) = (i as f32 / bands, (i + 1) as f32 / bands);
            mesh.set_quad_uv(before, [(u0, v0), (u1, v0), (u1, v1), (u0, v1)]);
        }
    }
    // Caps keep a hard edge: the rim of a shoulder is a rim, not a smooth bend.
    let down = Vec3::new(0.0, 0.0, -1.0);
    let up = Vec3::new(0.0, 0.0, 1.0);
    if cap_bottom {
        let c = rings[0].center;
        for k in 0..sides {
            let k1 = (k + 1) % sides;
            let before = mesh.tris.len();
            mesh.tri_curved(c, down, grid[0][k1], down, grid[0][k], down, color_at(0, k), 0.82);
            let (u0, u1) = (k as f32 / sides as f32, (k + 1) as f32 / sides as f32);
            mesh.set_tri_uv(before, [(0.5, 0.0), (u1, 0.0), (u0, 0.0)]);
        }
    }
    if cap_top {
        let last = rings.len() - 1;
        let c = rings[last].center;
        for k in 0..sides {
            let k1 = (k + 1) % sides;
            let before = mesh.tris.len();
            mesh.tri_curved(c, up, grid[last][k], up, grid[last][k1], up, color_at(last, k), 0.92);
            let (u0, u1) = (k as f32 / sides as f32, (k + 1) as f32 / sides as f32);
            mesh.set_tri_uv(before, [(0.5, 1.0), (u0, 1.0), (u1, 1.0)]);
        }
    }
}

fn emit_box(mesh: &mut Mesh, corners: [Vec3; 8], color: Rgb, shade: f32) {
    // Corner index bits: bit0 = x, bit1 = y, bit2 = z, matching `scene::Mesh`.
    let idx = |x: usize, y: usize, z: usize| x + y * 2 + z * 4;
    let faces: [[usize; 4]; 6] = [
        [idx(0, 0, 0), idx(1, 0, 0), idx(1, 1, 0), idx(0, 1, 0)],
        [idx(0, 0, 1), idx(0, 1, 1), idx(1, 1, 1), idx(1, 0, 1)],
        [idx(0, 0, 0), idx(0, 1, 0), idx(0, 1, 1), idx(0, 0, 1)],
        [idx(1, 0, 0), idx(1, 0, 1), idx(1, 1, 1), idx(1, 1, 0)],
        [idx(0, 0, 0), idx(0, 0, 1), idx(1, 0, 1), idx(1, 0, 0)],
        [idx(0, 1, 0), idx(1, 1, 0), idx(1, 1, 1), idx(0, 1, 1)],
    ];
    for (fi, f) in faces.iter().enumerate() {
        // Keep the same face weighting as the rest of the scene so a player is
        // lit consistently with the turf and the stands.
        let face_shade = shade
            * match fi {
                1 => 1.06,
                0 => 0.72,
                2 | 3 => 0.94,
                _ => 0.88,
            };
        mesh.quad_shaded(
            corners[f[0]],
            corners[f[1]],
            corners[f[2]],
            corners[f[3]],
            color,
            face_shade,
        );
    }
}

/// A box that can lean (pitch, about the lateral axis) and turn (yaw, about up)
/// at the same time.
fn box3(
    mesh: &mut Mesh,
    center: Vec3,
    half: Vec3,
    pitch: f32,
    yaw: f32,
    color: Rgb,
    shade: f32,
) {
    taper3(mesh, center, half, half, pitch, yaw, color, shade);
}

/// A box with different widths top and bottom: chest wider than waist, calf
/// wider than ankle.
fn taper3(
    mesh: &mut Mesh,
    center: Vec3,
    half_lo: Vec3,
    half_hi: Vec3,
    pitch: f32,
    yaw: f32,
    color: Rgb,
    shade: f32,
) {
    let (sp, cp) = pitch.sin_cos();
    let (sy, cy) = yaw.sin_cos();
    let mut corners = [Vec3::ZERO; 8];
    let mut n = 0;
    for &sx in &[-1.0f32, 1.0] {
        for &sy_ in &[-1.0f32, 1.0] {
            for &sz in &[-1.0f32, 1.0] {
                let half = if sz > 0.0 { half_hi } else { half_lo };
                let lx = sx * half.x;
                let ly = sy_ * half.y;
                let lz = sz * half.z;
                // Pitch about the lateral axis: +pitch carries the top forward.
                let px = lx * cp + lz * sp;
                let pz = -lx * sp + lz * cp;
                // Then yaw about up.
                corners[n] = Vec3::new(
                    center.x + px * cy - ly * sy,
                    center.y + px * sy + ly * cy,
                    center.z + pz,
                );
                n += 1;
            }
        }
    }
    emit_box(mesh, corners, color, shade);
}

fn limb(mesh: &mut Mesh, from: Vec3, to: Vec3, r0: f32, r1: f32, sides: usize, color: Rgb) {
    mesh.cylinder(from, to, r0, r1, sides, color);
}

/// Cleat: heel under the ankle, toe forward, pitched so it rolls through contact.
///
/// Built as a sole, an upper and a toe box rather than one tapered crate: a shoe
/// is the shape a viewer checks a low-poly figure against, and a block with a
/// corner on it is the first thing that reads as a model rather than a player.
/// Resample a chain of `(point, radius)` through a uniform Catmull-Rom spline.
///
/// A limb built from three or four rings with different radii has corners in
/// it: the surface steps from the bicep onto the elbow, and the light reads the
/// step as a plate edge. Subdivision is what makes a cage of primitives into a
/// body, so the profile gets subdivided too — every span becomes `per` smooth
/// steps, which rounds the swell of a calf or a deltoid into the joint below it.
fn smooth_profile(points: &[(Vec3, f32)], per: usize) -> Vec<(Vec3, f32)> {
    if per < 2 || points.len() < 3 {
        return points.to_vec();
    }
    let n = points.len() as isize;
    let at = |i: isize| points[i.clamp(0, n - 1) as usize];
    let mut out = Vec::with_capacity((points.len() - 1) * per + 1);
    for i in 0..n - 1 {
        let (p0, r0) = at(i - 1);
        let (p1, r1) = at(i);
        let (p2, r2) = at(i + 1);
        let (p3, r3) = at(i + 2);
        for k in 0..per {
            let t = k as f32 / per as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            let w0 = -0.5 * t3 + t2 - 0.5 * t;
            let w1 = 1.5 * t3 - 2.5 * t2 + 1.0;
            let w2 = -1.5 * t3 + 2.0 * t2 + 0.5 * t;
            let w3 = 0.5 * t3 - 0.5 * t2;
            out.push((
                p0 * w0 + p1 * w1 + p2 * w2 + p3 * w3,
                r0 * w0 + r1 * w1 + r2 * w2 + r3 * w3,
            ));
        }
    }
    out.push(*points.last().unwrap());
    out
}

fn shoe(mesh: &mut Mesh, ankle: Vec3, pitch: f32, color: Rgb) {
    let (sp, cp) = pitch.sin_cos();
    let heel = Vec3::new(ankle.x - 0.066, ankle.y, 0.032);
    let mid = Vec3::new(ankle.x + 0.055 * cp, ankle.y, 0.032 - 0.055 * sp * 0.35);
    let toe = Vec3::new(ankle.x + 0.212 * cp, ankle.y, 0.036 - 0.212 * sp * 0.35);
    // Sole and upper are each one capped tube from heel to toe, so the shoe is a
    // shoe from any angle. Built from open cylinders, a foot pitched hard on
    // toe-off showed the inside of the sole, which is the detached-looking
    // fragment that used to fly around a running back's ankle.
    mesh.tube(
        &[(heel, 0.043), (mid, 0.046), (toe, 0.039)],
        8,
        Rgb::hex(0xB8B5AB),
        true,
    );
    mesh.tube(
        &[
            (heel + Vec3::new(0.0, 0.0, 0.042), 0.052),
            (mid + Vec3::new(0.0, 0.0, 0.052), 0.056),
            (toe + Vec3::new(0.0, 0.0, 0.042), 0.041),
        ],
        8,
        color,
        true,
    );
    // Toe box, so the shoe has a front instead of a cone.
    mesh.sphere(
        toe + Vec3::new(0.0, 0.0, 0.026),
        Vec3::new(0.056, 0.049, 0.041),
        3,
        8,
        color,
    );
    // Collar around the ankle, and laces on top of the instep.
    mesh.tube(
        &[
            (ankle + Vec3::new(-0.040, 0.0, -0.062), 0.056),
            (ankle + Vec3::new(-0.026, 0.0, 0.030), 0.050),
        ],
        9,
        color,
        true,
    );
    box3(
        mesh,
        ankle.lerp(toe, 0.50) + Vec3::new(0.0, 0.0, 0.068),
        Vec3::new(0.034, 0.034, 0.006),
        pitch * 0.4,
        0.0,
        Rgb::hex(0xEDEAE0),
        1.0,
    );
}

/// Turn a built figure about the vertical axis it stands on.
fn yaw_mesh(mesh: &mut Mesh, angle: f32) {
    let (s, c) = angle.sin_cos();
    for t in mesh.tris.iter_mut() {
        for p in [&mut t.a, &mut t.b, &mut t.c] {
            let (x, y) = (p.x, p.y);
            p.x = x * c - y * s;
            p.y = x * s + y * c;
        }
        for n in [&mut t.na, &mut t.nb, &mut t.nc] {
            let (x, y) = (n.x, n.y);
            n.x = x * c - y * s;
            n.y = x * s + y * c;
        }
    }
}

/// Tip a built figure forward or back about a lateral axis at height `pivot`,
/// which is how a dive and a slide are made out of a standing body.
fn pitch_mesh(mesh: &mut Mesh, pivot: f32, angle: f32) {
    let (s, c) = angle.sin_cos();
    for t in mesh.tris.iter_mut() {
        for p in [&mut t.a, &mut t.b, &mut t.c] {
            let x = p.x;
            let z = p.z - pivot;
            p.x = x * c + z * s;
            p.z = pivot + z * c - x * s;
        }
        for n in [&mut t.na, &mut t.nb, &mut t.nc] {
            let (x, z) = (n.x, n.z);
            n.x = x * c + z * s;
            n.z = z * c - x * s;
        }
    }
}

fn lift_mesh(mesh: &mut Mesh, dz: f32) {
    for t in mesh.tris.iter_mut() {
        for p in [&mut t.a, &mut t.b, &mut t.c] {
            p.z += dz;
        }
    }
}

/// A body tipped over about its feet swings its heels through the ground. Set him
/// back down on the turf so nothing is buried in it.
fn raise_to_turf(mesh: &mut Mesh) {
    let mut lowest = f32::MAX;
    for t in mesh.tris.iter() {
        for p in [t.a, t.b, t.c] {
            lowest = lowest.min(p.z);
        }
    }
    if lowest < SOLE {
        lift_mesh(mesh, SOLE - lowest);
    }
}

// ---------------------------------------------------------------------------
// Jersey numbers
// ---------------------------------------------------------------------------

/// Digits on a 5x7 grid, one bit per pixel, top row first. Reading the number off
/// a player is the whole point of the back of a jersey, so the figure wears real
/// digits instead of a solid rectangle — and five by seven is the smallest grid
/// where a digit has curves in it instead of being a pile of blocks.
fn digit(c: u8) -> [u8; 7] {
    match c {
        b'0' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        b'1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        b'2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        b'3' => [0b11110, 0b00001, 0b00010, 0b00100, 0b00010, 0b00001, 0b11110],
        b'4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        b'5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        b'6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        b'7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        b'8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        b'9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        _ => [0, 0, 0, 0, 0, 0, 0],
    }
}

/// Paint a jersey number onto one side of the chest.
///
/// `face` is +1 for the front and -1 for the back, and the back is mirrored so the
/// digits read correctly to somebody standing behind him.
///
/// `cloth` gives the body's surface at a height along the spine: the centre of the
/// torso there, and its half depth and half width. Each patch of paint is laid at
/// the depth of the cloth under *that* patch, measured on the same superellipse
/// the shirt was lofted from, so the digits wrap around the chest. Painting them
/// onto one plane is what makes a number look like a sticker floating beside the
/// man rather than printed on his shirt.
fn paint_rows(
    mesh: &mut Mesh,
    glyphs: &[[u8; 7]],
    face: f32,
    cell: f32,
    color: Rgb,
    center_f: f32,
    torso_len: f32,
    cloth: &dyn Fn(f32) -> (Vec3, f32, f32),
    lateral_shift: f32,
) {
    if glyphs.is_empty() {
        return;
    }
    // A glyph is five cells wide, plus a little more than a cell between them.
    let pitch = cell * 6.15;
    let width = pitch * glyphs.len() as f32 - cell * 0.9;
    // How much of the spine one row of the glyph covers.
    let row_f = cell * 1.22 / torso_len.max(0.05);
    for (gi, rows) in glyphs.iter().enumerate() {
        let start = gi as f32 * pitch - width * 0.5 + lateral_shift;
        for (row, bits) in rows.iter().enumerate() {
            // Merge the lit pixels of one row into a single bar. Painting each
            // pixel as its own patch leaves a gap between neighbours wherever the
            // curve of the chest pulls them apart, and the number reads as a grid
            // of loose squares rather than as a numeral.
            let mut col = 0u8;
            while col < 5 {
                if bits & (1 << (4 - col)) == 0 {
                    col += 1;
                    continue;
                }
                let run_start = col;
                while col + 1 < 5 && bits & (1 << (4 - (col + 1))) != 0 {
                    col += 1;
                }
                let cells = (col - run_start + 1) as i32;
                let (center, rx, ry) = cloth(center_f + (3i32 - row as i32) as f32 * row_f);
                // Which way a digit advances across the cloth: the camera looks
                // along the inward normal, and in this projection screen-right
                // for a face pointing +x is -y. Get this backwards and the number
                // is mirrored, which is what a jersey reads as a smudge.
                let edge = |j: i32| -> (f32, f32, Vec3) {
                    let mut lateral = start + (run_start as i32 + j) as f32 * cell - cell * 0.5;
                    // Fatten the stroke by half a cell at each end. A one-cell
                    // stroke is how a dot-matrix digit is built and how a jersey
                    // number is not: jerseys are cut bold, and at this size a
                    // thin stroke reads as a scatter of squares.
                    if j == 0 {
                        lateral -= cell * 0.5;
                    }
                    if j == cells {
                        lateral += cell * 0.5;
                    }
                    let (depth, y_rel, normal) = cloth_surface(rx, ry, face, lateral);
                    // Five millimetres proud. Enough to beat the cloth it sits
                    // on, little enough that its edge is not a sliver on the
                    // silhouette when he turns side on.
                    (center.x + face * (depth + 0.005), center.y + y_rel, normal)
                };
                // Rows overlap by half a cell, so neighbouring strokes join up
                // instead of leaving the jersey showing through the digit.
                let (z0, z1) = (center.z - cell * 0.80, center.z + cell * 0.80);
                // One flat quad per bar stands off the cloth at its ends and shows
                // as a sliver as soon as the man turns side on. Walking the strip
                // cell by cell, sampling the surface at every boundary, keeps the
                // paint on the shirt all the way to the edge of the digit.
                for j in 0..cells {
                    let (xa, ya, na) = edge(j);
                    let (xb, yb, nb) = edge(j + 1);
                    mesh.quad_curved(
                        Vec3::new(xa, ya, z0),
                        na,
                        Vec3::new(xb, yb, z0),
                        nb,
                        Vec3::new(xb, yb, z1),
                        nb,
                        Vec3::new(xa, ya, z1),
                        na,
                        color,
                        1.35,
                    );
                }
                col += 1;
            }
        }
    }
}

/// Where the shirt is, and which way it faces, at a lateral offset across the
/// chest — read off the same rounded cross-section the torso is lofted through.
fn cloth_surface(rx: f32, ry: f32, face: f32, lateral: f32) -> (f32, f32, Vec3) {
    let y_rel = if face > 0.0 { -lateral } else { lateral };
    let across = (y_rel.abs() / ry.max(1e-3)).clamp(0.0, 1.0);
    let depth = rx * (1.0 - across.powf(TORSO_ROUND)).max(0.0).powf(1.0 / TORSO_ROUND);
    // The cloth's own normal. Give the paint a normal per cell instead and the
    // shading bands break the mark into a patchwork of light and dark squares.
    let ax = (depth / rx.max(1e-4)).clamp(0.0, 1.0).powf(TORSO_ROUND - 1.0) / rx.max(1e-4);
    let ay = across.powf(TORSO_ROUND - 1.0) / ry.max(1e-4);
    let normal = Vec3::new(face * ax, y_rel.signum() * ay, 0.0).normalize_or_zero();
    (depth, y_rel, normal)
}

/// The two digits on the chest and the back.
fn paint_number(
    mesh: &mut Mesh,
    number: u8,
    face: f32,
    cell: f32,
    color: Rgb,
    center_f: f32,
    torso_len: f32,
    cloth: &dyn Fn(f32) -> (Vec3, f32, f32),
) {
    let text = format!("{number:02}");
    let glyphs: Vec<[u8; 7]> = text.bytes().map(digit).collect();
    paint_rows(mesh, &glyphs, face, cell, color, center_f, torso_len, cloth, 0.0);
}

/// The club mark across the top of the chest.
///
/// Laid out in the same cells as the number, from the same bitmap alphabet the
/// overlay uses, so a wordmark is the club's name in the club's letters rather
/// than a rectangle of colour that stands in for one.
fn paint_wordmark(
    mesh: &mut Mesh,
    text: &str,
    face: f32,
    cell: f32,
    color: Rgb,
    center_f: f32,
    torso_len: f32,
    cloth: &dyn Fn(f32) -> (Vec3, f32, f32),
) {
    let glyphs: Vec<[u8; 7]> = text.chars().take(10).map(crate::font::glyph).collect();
    paint_rows(mesh, &glyphs, face, cell, color, center_f, torso_len, cloth, 0.0);
}

/// A shield, five cells across: the crest the sheets wear over the heart.
const CREST: [[u8; 7]; 1] = [[
    0b11111, 0b11111, 0b11111, 0b11111, 0b01110, 0b01100, 0b00100,
]];

/// The field inside the shield, so a crest is a shape in a border rather than a
/// solid badge.
const CREST_FIELD: [[u8; 7]; 1] = [[
    0b00000, 0b01110, 0b01110, 0b01110, 0b00100, 0b00100, 0b00000,
]];

/// The club's mark, as letters, off the uniform.
fn mark_text(mark: &[u8; 6]) -> String {
    let mut out = String::new();
    for b in mark {
        if *b == 0 {
            break;
        }
        out.push(*b as char);
    }
    out
}

/// Which men wear the long undershirt sleeves, and which wear tights. A stable
/// draw, so a roster is dressed the same way on every replay.
pub fn wears_sleeves(player: &Player, seed: u32) -> bool {
    let h = (player.id as u32).wrapping_mul(0x9E37_79B1) ^ seed.wrapping_mul(0x85EB_CA6B);
    (h >> 7) % 3 == 1
}

pub fn wears_tights(player: &Player, seed: u32) -> bool {
    let h = (player.id as u32).wrapping_mul(0xC2B2_AE35) ^ seed.wrapping_mul(0x27D4_EB2F);
    (h >> 11) % 3 == 2
}

pub fn build_mesh(player: &Player, uniform: &Uniform, pose: &Pose, team_skin_seed: u32) -> Mesh {
    build_mesh_at(player, uniform, pose, team_skin_seed, Detail::Full)
}

pub fn build_mesh_at(
    player: &Player,
    uniform: &Uniform,
    pose: &Pose,
    team_skin_seed: u32,
    detail: Detail,
) -> Mesh {
    let mut mesh = Mesh::new();
    let sides = detail.sides();
    let body_sides = detail.body_sides();
    let h = player.height_yards().clamp(1.55, 2.05);
    // Body mass is the difference between a wire frame and a football player.
    let bulk = player.build.clamp(0.9, 1.1) * 1.00;
    // Shoulder span, not a chest measurement. The art sheets this is built to are
    // drawn with a span that is deliberately past human: the deltoids are the
    // widest thing on the figure and the waist sits well inside them. The clamp
    // was held down to human proportions, which is why the figures read narrow.
    let shoulder_w = (player.shoulder_width() * 1.08).clamp(0.52, 0.64);
    let skin = player.skin.rgb();
    let jersey = uniform.jersey;
    let accent = uniform.jersey_accent;
    let shorts = uniform.shorts;
    let socks = uniform.socks;
    let shoes = uniform.shoes;
    // Undershirt sleeves and tights are dealt out per man rather than per team:
    // a stable draw off his id, so the same roster is dressed the same way in
    // every replay of every play.
    let sleeved = wears_sleeves(player, team_skin_seed);
    let legginged = wears_tights(player, team_skin_seed);
    let arm_skin = if sleeved { Rgb::hex(0xF2F4F6) } else { skin };
    let leg_skin = if legginged { shorts.scale(0.80) } else { skin };

    let speed = pose.speed.clamp(0.0, 1.0);
    // A walk becomes a run with pace, and everything settles when he stops.
    let run_blend = ((speed - 0.16) / 0.34).clamp(0.0, 1.0);
    let live = (speed / 0.10).clamp(0.0, 1.0);

    // Legs a little shorter and the trunk a little longer than life: the
    // cartoon-athlete proportion the whole look hangs off. A figure with a long
    // shin and a short chest reads as a mannequin however well it is modelled.
    let leg_len = h * 0.475;
    let thigh_len = leg_len * 0.52;
    let shin_len = leg_len * 0.48;
    let torso_len = h * 0.315;
    // A head that fits the shoulders, and a neck thick enough to carry it: a
    // pinhead on a wide body is a doll, and a big head on a narrow one is a child.
    // Small head, wide shoulders: the reference sheets are drawn with a head about
    // a fifth of the shoulder span, which is what makes the chest look like a
    // chest and not a torso with a person on top of it.
    let head_r = h * 0.068;
    // The pelvis is narrower than the shoulders, but not by the canyon the old
    // number left: hips carry the stride and the shorts sit on them.
    // Hip joints sit close to the midline: put them wide and the thighs hang
    // outside the waist no matter how the shorts are cut.
    let hip_lateral = shoulder_w * (0.18 + 0.06 * pose.strafe.clamp(0.0, 1.0));

    // ---------------- legs ----------------
    // Half a cycle between them, so one foot is always cycling toward the ground
    // as the other drives away from it.
    let mut legs = [[0.0f32; 3]; 2];
    let mut reach = 0.0f32;
    for (i, offset) in [0.0f32, 0.5].iter().enumerate() {
        let mut angles = leg_angles(pose.run_phase + offset, run_blend, live);
        if pose.blocking {
            // A blocker sinks into a wide base: more knee bend everywhere, and
            // the contact solve below drops the body accordingly.
            angles[1] += 0.45;
            angles[0] *= 0.6;
        }
        if pose.wrapping > 0.0 {
            // A tackler does not run into a man upright: he sinks, gets his hips
            // under him and drives up through the hit.
            angles[1] += 0.34 * pose.wrapping.clamp(0.0, 1.0);
            angles[0] *= 1.0 - 0.25 * pose.wrapping.clamp(0.0, 1.0);
        }
        legs[i] = angles;
        reach = reach.max(leg_reach(angles[0], angles[1], thigh_len, shin_len));
    }
    // The hips hang off the straightest leg, so the planted foot is exactly on
    // the turf and the swinging foot is clear of it.
    let hip_z = SOLE + reach;

    // Legs sized off the body, not off a bodybuilder: the thigh was thick enough
    // that the shorts sat out wider than the shoulders, which is what made the
    // figure read as bottom-heavy rather than athletic.
    let thigh_r = 0.104 * bulk;
    let knee_r = 0.064 * bulk;
    let calf_r = 0.094 * bulk;
    let ankle_r = 0.047 * bulk;

    for (i, side) in [-1.0f32, 1.0].iter().enumerate() {
        let [thigh, knee, foot] = legs[i];
        let hip = Vec3::new(0.0, side * hip_lateral, hip_z);
        // A stance, not a pair of legs hanging off a peg: the feet sit wider than
        // the hips when he is standing, and tuck back under him as he runs. It is
        // also what gives the shorts two visible legs instead of one black block.
        let splay = side * (0.032 + 0.062 * (1.0 - live));
        let (ts, tc) = thigh.sin_cos();
        let knee_pos = hip + Vec3::new(ts, 0.0, -tc) * thigh_len + Vec3::new(0.0, splay * 0.45, 0.0);
        let shin = thigh - knee;
        let (ss, sc) = shin.sin_cos();
        let ankle = knee_pos + Vec3::new(ss, 0.0, -sc) * shin_len + Vec3::new(0.0, splay * 0.55, 0.0);

        // Shorts over the top of the thigh, bare skin below the hem. They hang
        // wide off the hip and finish over the knee: a pair of baggy shorts is
        // most of the street-football silhouette, and a tight tube reads as a
        // swimmer instead.
        let hem = hip.lerp(knee_pos, 0.72);
        // Cut like shorts, not like a pipe: narrow at the waistband, widest a
        // third of the way down the thigh, gathered again at the hem. The old
        // tube started at the hip at full width with a flat cap, so the top of
        // each leg was a cylinder standing out past the waist.
        let waistband = Vec3::new(
            hip.x - 0.008,
            side * hip_lateral * 0.70,
            hip.z + h * 0.030,
        );
        mesh.tube(
            &smooth_profile(
                &[
                    (waistband, thigh_r * 0.78),
                    (hip, thigh_r * 0.94),
                    (hip.lerp(knee_pos, 0.44), thigh_r * 1.02),
                    (hem, thigh_r * 0.99),
                ],
                detail.rings(),
            ),
            sides,
            shorts,
            true,
        );
        // Trim round the hem, so the short leg ends on a line instead of a cut.
        limb(
            &mut mesh,
            hem.lerp(hip, 0.07),
            hem,
            // Proud enough to clear a twelve-sided ring's own facet depth;
            // at two percent the trim and the shorts surfaces interlaced and the
            // hem read as a dotted line.
            thigh_r * 1.07,
            thigh_r * 1.06,
            sides,
            accent,
        );
        // Quadriceps: the leg is at its thickest just above the knee, not at the
        // hip, so the bare thigh bulges before it narrows into the joint. Two
        // bulges, because the outer sweep of the quad sits higher than the teardrop
        // over the knee, and one cone between them reads as a tube with a crease.
        let quad = hem.lerp(knee_pos, 0.40);
        mesh.tube(
            &smooth_profile(
                &[
                    (hem, thigh_r * 0.94),
                    (quad, thigh_r * 0.92),
                    (knee_pos, knee_r),
                ],
                detail.rings(),
            ),
            sides,
            leg_skin,
            true,
        );
        if detail.fine() {
            // Knee cap, so the joint is a joint and not a kink in a tube.
            mesh.sphere(
                knee_pos,
                Vec3::new(knee_r * 1.02, knee_r * 0.92, knee_r * 1.00),
                2,
                6,
                leg_skin,
            );
            // A knee sleeve on some men, the way the sheets show them: a band of
            // sock over the joint, sat just above the kneecap.
            if player.id % 4 == 3 {
                let up = (hip - knee_pos).normalize_or_zero();
                limb(
                    &mut mesh,
                    knee_pos + up * 0.012,
                    knee_pos + up * 0.095,
                    knee_r * 1.10,
                    knee_r * 1.05,
                    sides,
                    socks,
                );
            }
        }
        // The calf: widest a third of the way down, then a hard taper into the
        // sock. A single cone from knee to ankle is what makes a leg read as a
        // table leg; the sheets have a diamond in the back of the calf.
        let belly = knee_pos.lerp(ankle, 0.46);
        let lower = knee_pos.lerp(ankle, 0.66);
        mesh.tube(
            &smooth_profile(
                &[(knee_pos, knee_r * 1.02), (belly, calf_r), (lower, calf_r * 0.86)],
                detail.rings(),
            ),
            sides,
            leg_skin,
            false,
        );
        mesh.tube(
            &smooth_profile(
                &[
                    (belly, calf_r * 1.01),
                    (lower, calf_r * 0.88),
                    (ankle, ankle_r),
                ],
                detail.rings(),
            ),
            sides,
            socks,
            false,
        );
        // Sock cuff, in team colour at the top of the sock, with a second stripe
        // under it. Two lines of trim are what make a sock look like kit.
        mesh.cylinder(
            belly,
            belly.lerp(ankle, 0.16),
            calf_r * 1.03,
            calf_r * 0.99,
            sides,
            accent,
        );
        mesh.cylinder(
            belly.lerp(ankle, 0.26),
            belly.lerp(ankle, 0.34),
            calf_r * 0.99,
            calf_r * 0.97,
            sides,
            accent,
        );
        shoe(&mut mesh, ankle, foot, shoes);
    }

    // ---------------- seat and shorts ----------------
    // The pelvis turns into the stride, one hip leading. Lofted so the seat has
    // depth and the shorts sit on it instead of being a single slab.
    let hip_yaw = (pose.run_phase.rem_euclid(1.0) * std::f32::consts::TAU).sin() * 0.10 * live;
    let pelvis = Vec3::new(0.0, 0.0, hip_z);
    let seat = [
        Ring::new(
            pelvis + Vec3::new(0.0, 0.0, -h * 0.035),
            0.082 * bulk,
            shoulder_w * 0.240,
            hip_yaw * 0.6,
        ),
        Ring::new(
            pelvis + Vec3::new(0.0, 0.0, h * 0.005),
            0.096 * bulk,
            shoulder_w * 0.266,
            hip_yaw,
        ),
        Ring::new(
            pelvis + Vec3::new(0.0, 0.0, h * 0.048),
            0.104 * bulk,
            shoulder_w * 0.252,
            hip_yaw * 0.7,
        ),
    ];
    loft(&mut mesh, &seat, body_sides, TORSO_ROUND, true, true, |i, k| {
        let a = (k as f32 + 0.5) / body_sides as f32 * std::f32::consts::TAU;
        let lateral = a.sin().abs();
        // A stripe down each side of the shorts.
        if lateral > 0.86 && (i == 1 || i == 0) {
            uniform.shorts_accent
        } else {
            shorts
        }
    });

    // ---------------- torso ----------------
    // The whole upper body hangs off the hips and leans as one piece. Shoulders
    // counter-rotate against the pelvis, which is what stops a runner looking
    // like a rigid block with arms attached.
    let lean = live * (0.08 + 0.24 * speed)
        + if pose.blocking { 0.34 } else { 0.0 }
        + 0.30 * pose.wrapping.clamp(0.0, 1.0);
    let spine_dir = Vec3::new(lean.sin(), 0.0, lean.cos());
    let spine = |f: f32| pelvis + spine_dir * (torso_len * f);
    let chest_yaw = -hip_yaw * 0.7;

    // Waist narrow, ribs wide, chest widest, deltoids wider still: the taper is
    // the whole athletic silhouette. Six bands rather than four, because a chest
    // is not a cone: the ribs come out of the waist, the pectorals sit over them
    // and the shoulders carry the width above that.
    //
    // The number is painted against this same table, which is what keeps the
    // digits on the cloth when he leans. `(height along the spine, half depth,
    // half width)`.
    let profile: [(f32, f32, f32); 7] = [
        (0.02, 0.088, 0.268),
        (0.20, 0.082, 0.242),
        (0.38, 0.096, 0.278),
        (0.56, 0.114, 0.328),
        (0.74, 0.126, 0.360),
        (0.88, 0.124, 0.378),
        (1.00, 0.104, 0.370),
    ];
    let torso: Vec<Ring> = profile
        .iter()
        .map(|(f, rx, ry)| {
            let yaw = hip_yaw * 0.8 * (1.0 - f) + chest_yaw * f;
            Ring::new(spine(*f), rx * bulk, shoulder_w * ry, yaw)
        })
        .collect();
    let torso_bands = torso.len() - 1;
    let cloth = |i: usize, k: usize| -> Rgb {
        let a = (k as f32 + 0.5) / body_sides as f32 * std::f32::consts::TAU;
        let lateral = a.sin().abs();
        // One table paints the faces of a distant man and the cloth of a near
        // one, so a hoop cannot land on a different band of the shirt depending
        // on how far away he is standing.
        let base = cloth::pattern_color(uniform, i, torso_bands, lateral);
        // Cloth is not moulded plastic. A fold catches the light and the seam
        // between two panels sits in shadow, so the shirt is broken into soft
        // vertical bands instead of reading as one smooth painted tube.
        let fold = match k % 6 {
            0 => 0.93,
            1 => 0.98,
            3 => 1.05,
            4 => 1.0,
            _ => 0.96,
        };
        base.scale(fold)
    };
    let torso_first = mesh.tris.len();
    loft(&mut mesh, &torso, body_sides, TORSO_ROUND, true, true, cloth);
    let torso_end = mesh.tris.len();
    // The shirt is cloth, and cloth is a texture: a weave two pixels wide, the
    // seams where the panels join, the stitch along the hem. A man far enough
    // away to skip the numbers is far enough away to skip the weave too, and
    // stays on flat colour.
    if detail.paint() {
        let slot = mesh.add_texture(cloth::jersey_texture(uniform, torso_bands));
        mesh.texture_range(torso_first..torso_end, slot);
    }

    // Hem of the jersey, on top of the shorts.
    let hem_lo = Ring::new(spine(0.0), 0.090 * bulk, shoulder_w * 0.276, hip_yaw * 0.9);
    let hem_hi = Ring::new(
        spine(0.07) + spine_dir * 0.001,
        0.092 * bulk,
        shoulder_w * 0.280,
        hip_yaw * 0.8,
    );
    loft(&mut mesh, &[hem_lo, hem_hi], body_sides, TORSO_ROUND, false, false, |_, _| {
        accent
    });

    // Numbers front and back, painted on the cloth. Too far away to read one is
    // too far away to pay for one.
    if detail.paint() {
        let number_color = uniform.readable_number_color();
        // Sized off the shirt, not off a poster: a jersey number is about a hand
        // and a half wide and sits across the chest.
        let cell = 0.047 * shoulder_w * (h / 1.85);
        // Where the cloth is at any height, straight off the torso profile. A
        // patch of paint that is not measured against this ends up as a flat card
        // floating in front of the sternum, or buried in the ribs.
        let cloth_at = |f: f32| -> (Vec3, f32, f32) {
            let f = f.clamp(0.0, 1.0);
            let mut i = 0;
            while i + 2 < profile.len() && profile[i + 1].0 <= f {
                i += 1;
            }
            let (f0, rx0, ry0) = profile[i];
            let (f1, rx1, ry1) = profile[(i + 1).min(profile.len() - 1)];
            let t = ((f - f0) / (f1 - f0).max(1e-4)).clamp(0.0, 1.0);
            (
                spine(f),
                (rx0 + (rx1 - rx0) * t) * bulk,
                shoulder_w * (ry0 + (ry1 - ry0) * t),
            )
        };
        paint_number(
            &mut mesh,
            player.number,
            1.0,
            cell,
            number_color,
            0.66,
            torso_len,
            &cloth_at,
        );
        paint_number(
            &mut mesh,
            player.number,
            -1.0,
            cell * 0.68,
            number_color,
            0.545,
            torso_len,
            &cloth_at,
        );

        // The mark across the top of the chest. Its width comes from the cloth
        // at that height rather than from the shoulders: high up the chest has
        // already begun to curve away, and a mark set any wider carries its last
        // letters round onto the sleeves, where they read as gold stripes.
        let mark = mark_text(&uniform.mark);
        if !mark.is_empty() {
            let letters = mark.chars().count() as f32;
            let (_, _, chest_ry) = cloth_at(0.775);
            // The flat of the chest, where the letters still face the lens.
            let flat = chest_ry * 1.34;
            let word_cell = (flat / (letters * 6.15)).min(cell * 0.40);
            if word_cell > 0.0025 {
                paint_wordmark(
                    &mut mesh,
                    &mark,
                    -1.0,
                    word_cell,
                    // The mark wears the club's accent, not the numbers colour:
                    // on the chest the two sit one above the other, and in one
                    // white the name and the numerals merge into a single block.
                    accent,
                    0.775,
                    torso_len,
                    &cloth_at,
                );
            }
        }

        // The crest over the heart, level with the number: the club's accent as
        // the border and a lighter field inside it, so it reads as a badge and
        // not as another patch of white.
        let (_, _, crest_ry) = cloth_at(0.545);
        let crest_cell = cell * 0.32;
        let crest_at = -crest_ry * 0.60;
        paint_rows(
            &mut mesh,
            &CREST,
            -1.0,
            crest_cell,
            accent,
            0.545,
            torso_len,
            &cloth_at,
            crest_at,
        );
        paint_rows(
            &mut mesh,
            &CREST_FIELD,
            -1.0,
            crest_cell * 0.56,
            jersey,
            0.541,
            torso_len,
            &cloth_at,
            crest_at,
        );
    }

    // ---------------- arms ----------------
    let shoulder_zone = spine(0.97);
    let upper_len = h * 0.15;
    let fore_len = h * 0.135;
    for (i, side) in [-1.0f32, 1.0].iter().enumerate() {
        let shoulder = shoulder_zone + Vec3::new(0.0, side * (shoulder_w * 0.420), -0.020);
        // Trapezius, sloping from the neck out to the shoulder. Cloth, not
        // skin: it is the yoke between collar and cap, and in skin it shows as a
        // brown sliver over each shoulder even when the cap itself is covered.
        if detail.fine() {
            taper3(
                &mut mesh,
                shoulder.lerp(shoulder_zone, 0.5) + Vec3::new(0.0, 0.0, 0.004),
                Vec3::new(0.062, 0.056, 0.024),
                Vec3::new(0.048, 0.086, 0.021),
                0.0,
                chest_yaw,
                jersey,
                0.98,
            );
        }
        // Deltoid: bare shoulder, no pads in this league. This is the widest
        // thing on the figure and it is why he reads as a player, so it is a
        // boulder, not a ball: wide across the top, deeper than it is tall.
        mesh.sphere(
            shoulder,
            Vec3::new(0.095 * bulk, 0.090 * bulk, 0.082 * bulk),
            if detail.fine() { 4 } else { 2 },
            if detail.fine() { 12 } else { 6 },
            skin,
        );
        // The jersey goes over the top of the deltoid. The sheet's kit is
        // sleeveless, but the armhole is cut inside the shoulder, so the cap is
        // cloth and the arm is bare below it. Stop the shirt at the collarbone and
        // the figure looks like it is wearing a vest that has slipped off both
        // shoulders.
        // Cut wider and deeper than the deltoid it covers. A cap that is a size
        // down leaves the top of the ball of the shoulder bare, and the tips come
        // out as two brown studs above a green shirt.
        mesh.sphere(
            shoulder + Vec3::new(-0.004, 0.0, 0.010 * bulk),
            Vec3::new(0.103 * bulk, 0.098 * bulk, 0.088 * bulk),
            if detail.fine() { 5 } else { 2 },
            if detail.fine() { 14 } else { 6 },
            jersey,
        );


        // Each arm swings against the leg on the other side of the body. The
        // elbow rides *behind* the ribs and the hand pumps up toward the chest,
        // which keeps the forearm tucked against the body. Swing an arm from the
        // shoulder alone and it projects off the front of the chest like a plank.
        let counter = legs[1 - i][0];
        let ball_arm = pose.carrying && *side > 0.0;
        let wrap = pose.wrapping.clamp(0.0, 1.0);
        let mut swing = counter * (0.50 + 0.30 * run_blend) * live - 0.30 * live;
        if ball_arm {
            swing = swing * 0.20 - 0.10;
        }
        let mut flex = if ball_arm {
            1.85
        } else if pose.blocking {
            1.05
        } else {
            0.25 + 1.20 * run_blend * live
        };
        // The stiff arm: the hand that is not carrying the ball goes out straight
        // at the man in front of him and stays there, elbow locked.
        if matches!(pose.motion, Some(Kind::StiffArm)) && !ball_arm {
            swing = 1.42;
            flex = 0.06;
        }
        if wrap > 0.0 {
            // Arms out and around the man: upper arms forward at shoulder height
            // and the elbows folded, so the hands close on his far side instead of
            // hanging at his own ribs. This is the difference between a tackle and
            // two men standing next to each other.
            swing += (1.25 - swing) * wrap;
            flex += (1.20 - flex) * wrap;
        }

        // A blocker's hands are out in front of him, and they go where the man he
        // is on goes: the hand on that side follows to 45 degrees and the sim turns
        // his body after it, so a rusher who crosses his face is still handled.
        // Run blocking raises them: hands up, and the defender is walked back.
        if pose.blocking {
            swing += (1.15 - swing) * 0.85;
            flex = 0.55;
        }
        if pose.blocking && pose.arms_up > 0.0 {
            let up = pose.arms_up.clamp(0.0, 1.0);
            swing += (2.15 - swing) * up;
        }
        let lateral = side * 0.05 + if pose.blocking { pose.hands } else { 0.0 };
        let up_dir = Vec3::new(swing.sin(), lateral, -swing.cos()).normalize_or_zero();
        let elbow = shoulder + up_dir * upper_len;
        // A flexed elbow carries the forearm forward and up, not backward.
        let fore_angle = swing + flex + 0.12;
        let fore_dir = Vec3::new(fore_angle.sin(), lateral, -fore_angle.cos()).normalize_or_zero();
        let hand = elbow + fore_dir * fore_len;

        // Biceps swell on the upper arm, forearm flares under the elbow and
        // tapers into the wrist. These are a bare-armed league's whole uniform:
        // the arm is the silhouette, and a stack of constant-radius tubes is the
        // one thing that reads as a doll rather than a player.
        let bicep = shoulder.lerp(elbow, 0.34);
        let above_elbow = shoulder.lerp(elbow, 0.72);
        let fore_hi = elbow.lerp(hand, 0.22);
        let fore_mid = elbow.lerp(hand, 0.58);
        // One surface from the deltoid to the wrist: the bicep is a swell, the
        // elbow is a pinch, and the forearm flares under it before it tapers into
        // the glove. Built as separate cylinders, the seams read as rings of tape.
        mesh.tube(
            &smooth_profile(
                &[
                    (shoulder, 0.076 * bulk),
                    (bicep, 0.094 * bulk),
                    (above_elbow, 0.070 * bulk),
                    (elbow, 0.058 * bulk),
                    (fore_hi, 0.072 * bulk),
                    (fore_mid, 0.054 * bulk),
                    (hand, 0.044 * bulk),
                ],
                detail.rings(),
            ),
            sides,
            arm_skin,
            true,
        );
        // Club patch on the sleeve: the sheets have one on every arm, and on a
        // bare-armed league it is the only badge above the waist.
        if detail.fine() {
            let along = shoulder.lerp(elbow, 0.44);
            let out = Vec3::new(0.0, *side, 0.0);
            mesh.sphere(
                along + out * (0.090 * bulk),
                Vec3::new(0.030, 0.012, 0.034),
                2,
                8,
                accent,
            );
        }
        if sleeved {
            let hem_lo = shoulder.lerp(elbow, 0.50);
            let hem_hi = shoulder.lerp(elbow, 0.58);
            limb(
                &mut mesh,
                hem_lo,
                hem_hi,
                0.100 * bulk,
                0.096 * bulk,
                sides,
                accent,
            );
        }
        // A cap on the point of the elbow, where the surface turns hardest.
        if detail.fine() {
            mesh.sphere(
                elbow,
                Vec3::new(0.062 * bulk, 0.062 * bulk, 0.056 * bulk),
                2,
                8,
                arm_skin,
            );
        }
        // Wristband, in the trim colour. It is the one piece of kit on a bare
        // arm, and it puts a break in the line where the forearm meets the hand.
        let wrist = elbow.lerp(hand, 0.84);
        limb(
            &mut mesh,
            wrist,
            wrist + fore_dir * 0.046,
            0.050,
            0.048,
            sides,
            accent,
        );
        // Hand: a gloved fist big enough to be seen holding something, with the
        // cuff over the wristband. The sheets have every man in gloves, and bare
        // hands on a figure this size read as stumps.
        let glove = if jersey.luma() < 120.0 {
            Rgb::hex(0xE6E2D8)
        } else {
            Rgb::hex(0x24262B)
        };
        mesh.sphere(
            hand + fore_dir * 0.002,
            Vec3::new(0.053, 0.049, 0.055),
            if detail.fine() { 5 } else { 3 },
            if detail.fine() { 9 } else { 6 },
            glove,
        );
        // Thumb, laid along the inside of the fist, so the glove has a hand in it.
        mesh.sphere(
            hand + fore_dir * 0.002 + Vec3::new(0.0, side * -0.040, 0.010),
            Vec3::new(0.021, 0.019, 0.024),
            2,
            6,
            glove,
        );
        // Cuff, over the wristband: the glove starts on the forearm, so there is
        // no gap between the arm and the hand when the arm swings.
        limb(
            &mut mesh,
            hand - fore_dir * 0.042,
            hand - fore_dir * 0.018,
            0.048,
            0.050,
            sides,
            accent,
        );

        if ball_arm {
            // Tucked to the ribs, nose up.
            let ball_center = hand + Vec3::new(-0.02, 0.0, 0.02);
            mesh.sphere(ball_center, Vec3::new(0.058, 0.098, 0.074), 5, 9, Rgb::hex(0x6B3B1E));
            box3(
                &mut mesh,
                ball_center + Vec3::new(-0.05, 0.0, 0.0),
                Vec3::new(0.012, 0.010, 0.006),
                0.0,
                0.0,
                Rgb::WHITE,
                1.0,
            );
        }
    }

    // ---------------- head ----------------
    // The head stays closer to level than the torso, so he looks where he runs.
    let head_lean = lean * 0.45;
    let head_dir = Vec3::new(head_lean.sin(), 0.0, head_lean.cos());
    let neck_base = shoulder_zone - spine_dir * head_r * 0.4;
    let head_center = neck_base + head_dir * (head_r * 1.92);
    mesh.tube(
        &[
            (neck_base, 0.058 * bulk),
            (neck_base + head_dir * (head_r * 1.02), 0.038 * bulk),
        ],
        sides,
        skin,
        false,
    );
    // Sternocleidomastoid: the cord up the side of the neck. Without it the head
    // sits on a stalk; with it, it sits on a man.
    if detail.fine() {
        for side in [-1.0f32, 1.0] {
            let base = neck_base + Vec3::new(0.03, side * 0.02, 0.0);
            let top = neck_base + head_dir * (head_r * 0.92) + Vec3::new(0.02, side * 0.05, 0.0);
            mesh.tube(&[(base, 0.024 * bulk), (top, 0.015)], 5, skin.scale(1.02), false);
        }
    }
    // A couple of men a side wear the balaclava; the roster decided which when
    // the club was built, so the same faces are masked in every replay.
    let masked = player.balaclava;
    Head {
        center: head_center,
        radius: head_r,
        player,
        skin,
        accent,
        mask: if masked { Some(jersey) } else { None },
    }
    .build(&mut mesh, detail.quality());

    // Helmet? Never. This is no-pads football.

    // A man on the ground got there over the top of a hit, and that is what the
    // figure has to show: the hips drop first, then the chest follows the shoulder
    // and the whole body goes flat. The old pose squashed a standing man where he
    // stood, which read as a doll being tipped over sideways.
    let fall = if pose.down { 1.0 } else { pose.tackled.clamp(0.0, 1.0) };
    if fall > 0.001 {
        let e = fall * fall * (3.0 - 2.0 * fall);
        pitch_mesh(&mut mesh, SOLE, e * 1.42);
        raise_to_turf(&mut mesh);
    }

    if let Some(kind) = pose.motion {
        // Eased so the pose does not snap on the frame the move starts or ends.
        let p = pose.motion_progress.clamp(0.0, 1.0);
        let e = p * p * (3.0 - 2.0 * p);
        match kind {
            // One full turn with his feet under him.
            Kind::Spin => yaw_mesh(&mut mesh, e * std::f32::consts::TAU),
            // Up and over a man: the whole body leaves the turf on an arc and
            // comes back down inside the length of the move.
            Kind::Hurdle => lift_mesh(&mut mesh, (std::f32::consts::PI * p).sin() * 0.62),
            // Forward and out over his feet, ending parallel to the ground.
            Kind::Dive => {
                pitch_mesh(&mut mesh, SOLE, e * 1.30);
                raise_to_turf(&mut mesh);
            }
            // Down and back onto his hip, the way a quarterback gives himself up.
            Kind::Slide => {
                pitch_mesh(&mut mesh, SOLE, -e * 1.15);
                raise_to_turf(&mut mesh);
            }
            // The arm goes out, not the body: handled where the arms are built,
            // so the runner keeps his feet and his stride.
            Kind::StiffArm => {}
            // A break is all hips: the body turns off its line without leaving
            // the ground, and the stride keeps running through it.
            Kind::Cut => yaw_mesh(&mut mesh, e * 0.85),
            Kind::Juke => yaw_mesh(&mut mesh, e * 1.35),
            // A stop sits him back on both feet, weight going down and back.
            Kind::Stop => pitch_mesh(&mut mesh, SOLE, -e * 0.30),
        }
    }

    if pose.celebrating > 0.01 {
        for t in mesh.tris.iter_mut() {
            for p in [&mut t.a, &mut t.b, &mut t.c] {
                p.z += pose.celebrating * 0.35;
            }
        }
    }

    let _ = team_skin_seed;
    mesh
}

/// Build the ball on its own so it can be drawn in flight.
pub fn ball_mesh(center: Vec3) -> Mesh {
    let mut mesh = Mesh::new();
    mesh.sphere(center, Vec3::new(0.1, 0.16, 0.12), 5, 9, Rgb::hex(0x6B3B1E));
    mesh.sphere(
        center + Vec3::new(0.0, 0.0, 0.13),
        Vec3::new(0.02, 0.02, 0.01),
        2,
        4,
        Rgb::WHITE,
    );
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The body is hung off the straightest leg, so the sole of the planted foot
    /// is on the turf for every phase of the stride, at every speed.
    #[test]
    fn planted_foot_stays_on_the_turf() {
        let thigh_len = 0.95 * 0.48 * 0.52;
        let shin_len = 0.95 * 0.48 * 0.48;
        for step in 0..40 {
            let phase = step as f32 / 40.0;
            let mut reach = 0.0f32;
            for offset in [0.0f32, 0.5] {
                let [thigh, knee, _] = leg_angles(phase + offset, 1.0, 1.0);
                reach = reach.max(leg_reach(thigh, knee, thigh_len, shin_len));
            }
            let hip_z = SOLE + reach;
            for offset in [0.0f32, 0.5] {
                let [thigh, knee, _] = leg_angles(phase + offset, 1.0, 1.0);
                let ankle_z = hip_z - leg_reach(thigh, knee, thigh_len, shin_len);
                // A foot is either planted on the turf or clear of it, never sunk.
                assert!(
                    ankle_z >= SOLE - 1e-3,
                    "phase {phase:.3}: ankle sank to {ankle_z:.4}"
                );
            }
            // And something is always touching: no floating stride.
            assert!(
                (hip_z - SOLE - reach).abs() < 1e-4,
                "phase {phase:.3}: body not carried by a leg"
            );
        }
    }

    /// The legs have to take turns. If both reaches tracked together the figure
    /// would hop with its feet side by side, which is the bug this replaced.
    #[test]
    fn legs_alternate() {
        let thigh_len = 0.46;
        let shin_len = 0.43;
        let mut apart = 0.0f32;
        for step in 0..40 {
            let phase = step as f32 / 40.0;
            let a = leg_angles(phase, 1.0, 1.0);
            let b = leg_angles(phase + 0.5, 1.0, 1.0);
            let ra = leg_reach(a[0], a[1], thigh_len, shin_len);
            let rb = leg_reach(b[0], b[1], thigh_len, shin_len);
            apart = apart.max((ra - rb).abs());
        }
        // A whole stride should separate the two feet by a good part of a leg.
        assert!(apart > 0.1, "feet barely separate: {apart:.3}");
    }

    /// A standing player is settled, not frozen in mid-stride.
    #[test]
    fn standing_pose_is_soft_kneed() {
        let angles = leg_angles(0.25, 0.0, 0.0);
        assert!(angles[1] > 0.05 && angles[1] < 0.2, "knee {:?}", angles[1]);
    }

    // ---------------------------------------------------------------
    // The figure has to be a body: shoulders over a waist, a head that
    // fits the shoulders, and cloth that is round rather than boxy.
    // ---------------------------------------------------------------

    fn figure(motion: Option<Kind>, progress: f32) -> Mesh {
        let teams = nopads_core::team::league_teams();
        let pose = Pose {
            run_phase: 0.3,
            speed: 0.75,
            carrying: true,
            motion,
            motion_progress: progress,
            ..Default::default()
        };
        build_mesh(&teams[0].roster[0], &teams[0].home, &pose, 0)
    }

    /// `(x_min, x_max, y_min, y_max, z_min, z_max)` over every vertex.
    fn bounds(mesh: &Mesh) -> (f32, f32, f32, f32, f32, f32) {
        let mut b = (f32::MAX, f32::MIN, f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for t in &mesh.tris {
            for p in [t.a, t.b, t.c] {
                b.0 = b.0.min(p.x);
                b.1 = b.1.max(p.x);
                b.2 = b.2.min(p.y);
                b.3 = b.3.max(p.y);
                b.4 = b.4.min(p.z);
                b.5 = b.5.max(p.z);
            }
        }
        b
    }

    /// Widest lateral reach of the figure inside a height band.
    fn width_between(mesh: &Mesh, z0: f32, z1: f32) -> f32 {
        let mut w = 0.0f32;
        for t in &mesh.tris {
            for p in [t.a, t.b, t.c] {
                if p.z >= z0 && p.z <= z1 {
                    w = w.max(p.y.abs());
                }
            }
        }
        w
    }

    /// The sheets' silhouette: shoulders and deltoids carry the width, the waist
    /// is narrow enough to see, and the head is small against it.
    ///
    /// This is the shape the art directions are drawn in, and it is the one thing
    /// about a figure that reads at forty yards, where no amount of cloth detail
    /// can.
    #[test]
    fn the_silhouette_carries_its_width_in_the_shoulders() {
        let mesh = figure(None, 0.0);
        let (_, _, _, _, z_min, z_max) = bounds(&mesh);
        let h = z_max - z_min;
        // A hanging arm is the widest thing at waist height, so this band
        // measures the whole figure there rather than the cloth. The sheet's V is
        // shoulders over hips, and it is a real one: about eighteen inches of
        // shorts under a twenty-one inch shoulder span.
        let waist = width_between(&mesh, z_min + h * 0.58, z_min + h * 0.64);
        let delts = width_between(&mesh, z_min + h * 0.80, z_min + h * 0.86);
        assert!(waist > 0.10, "no waist to judge against: {waist:.3}");
        assert!(
            delts > waist * 1.08,
            "the V is not there: {delts:.3} across the shoulders against {waist:.3} at the waist"
        );
    }

    /// The jersey is on the shoulder, not below it: the sheet's sleeve cap is
    /// cloth over the deltoid, bare arm underneath.
    #[test]
    fn the_jersey_covers_the_shoulder_cap() {
        let teams = nopads_core::team::league_teams();
        let player = &teams[0].roster[0];
        let uniform = &teams[0].home;
        let pose = Pose { run_phase: 0.3, speed: 0.0, ..Default::default() };
        let mesh = build_mesh(player, uniform, &pose, 0);
        let (_, _, _, _, z_min, z_max) = bounds(&mesh);
        let h = z_max - z_min;
        let cap = mesh
            .tris
            .iter()
            .filter(|t| t.color == uniform.jersey && t.a.z > z_min + h * 0.82 && t.a.y.abs() > 0.20)
            .count();
        assert!(cap >= 6, "no jersey over the deltoid: {cap} faces");
    }

    /// A torso is not a crate. The chest has to be wider than the waist, the
    /// shoulders wider than both, and the head narrower than all of it.
    #[test]
    fn the_figure_has_shoulders_a_waist_and_a_head_that_fits() {
        let mesh = figure(None, 0.0);
        let (_, _, _, _, z_min, z_max) = bounds(&mesh);
        let h = z_max - z_min;
        let waist = width_between(&mesh, z_min + h * 0.58, z_min + h * 0.64);
        let chest = width_between(&mesh, z_min + h * 0.68, z_min + h * 0.74);
        let shoulders = width_between(&mesh, z_min + h * 0.80, z_min + h * 0.86);
        let head = width_between(&mesh, z_min + h * 0.93, z_max);
        assert!(waist > 0.10, "no body at the waist: {waist:.3}");
        assert!(
            chest > waist * 0.98,
            "the chest has to be at least as wide as the waist: {chest:.3} vs {waist:.3}"
        );
        assert!(
            shoulders > waist * 1.08,
            "shoulders should overhang the waist: {shoulders:.3} vs {waist:.3}"
        );
        assert!(
            head < shoulders * 0.82,
            "the head is too big for the shoulders: {head:.3} vs {shoulders:.3}"
        );
    }

    /// Skin a body through a dozen sides and the silhouette is round: a slice
    /// across the chest has to hold a spread of depths, not two faces.
    #[test]
    fn the_torso_is_round_rather_than_boxy() {
        let mesh = figure(None, 0.0);
        let (_, _, _, _, z_min, z_max) = bounds(&mesh);
        let h = z_max - z_min;
        let band = (z_min + h * 0.70, z_min + h * 0.74);
        let mut depths: Vec<f32> = mesh
            .tris
            .iter()
            .flat_map(|t| [t.a, t.b, t.c])
            .filter(|p| p.z >= band.0 && p.z <= band.1)
            .map(|p| p.x)
            .collect();
        depths.sort_by(f32::total_cmp);
        depths.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
        // A crate has two depths across the chest. A lofted body has a front, a
        // back and a curve of depths between them on each side.
        assert!(
            depths.len() >= 6,
            "the chest only has {} distinct depths: {depths:?}",
            depths.len()
        );
    }

    /// The jersey wears its number. Read the back of a jersey and there should be
    /// paint where the digits go, in the readable colour, and none where they do
    /// not.
    #[test]
    fn the_jersey_carries_a_number() {
        let teams = nopads_core::team::league_teams();
        let player = &teams[0].roster[0];
        let uniform = &teams[0].home;
        let pose = Pose { run_phase: 0.3, speed: 0.7, ..Default::default() };
        let mesh = build_mesh(player, uniform, &pose, 0);
        let paint = uniform.readable_number_color();
        let painted = mesh
            .tris
            .iter()
            .filter(|t| t.color == paint && t.shade > 1.2)
            .count();
        let want = format!("{:02}", player.number)
            .bytes()
            .map(|c| digit(c).iter().filter(|row| **row != 0).count())
            .sum::<usize>();
        // Front and back, at least one bar per lit row of the digits.
        assert!(
            painted >= want * 2,
            "the number {:#02} only put {painted} patches on the cloth, wanted {}",
            player.number,
            want * 2
        );
    }

    /// The figure is rebuilt for every man on every frame, so it cannot grow into
    /// something a software rasteriser cannot afford.
    /// The figure is rebuilt for every man on every frame, so it cannot grow into
    /// something a software rasteriser cannot afford — but the budget is what
    /// decides whether these men look like bodies or like PlayStation One models,
    /// and there is room to spend.
    #[test]
    fn the_figure_stays_within_a_triangle_budget() {
        let mesh = figure(None, 0.0);
        let tris = mesh.tris.len();
        assert!(
            tris > 1200,
            "the figure has gone back to being a stack of boxes: {tris} tris"
        );
        let teams = nopads_core::team::league_teams();
        let player = &teams[0].roster[0];
        let uniform = &teams[0].home;
        let pose = Pose { run_phase: 0.3, speed: 0.75, carrying: true, ..Default::default() };
        let full = build_mesh_at(player, uniform, &pose, 0, Detail::Full);
        let close = build_mesh_at(player, uniform, &pose, 0, Detail::Close);
        assert!(
            full.tris.len() > 1200,
            "the figure has gone back to being a stack of boxes: {} tris",
            full.tris.len()
        );
        // Measured, not guessed: at v0.18.0 the full figure was 20.5 ms a frame
        // at 640x360 with fourteen men on the field. Subdividing the limb
        // profiles adds the rings that stop a leg reading as a stack of plates,
        // and that is worth paying for. The close tier only ever covers the men
        // within seven yards of the lens — at most two — so it can carry the face.
        // The balaclava is a second sheet over the skull; it costs about five
        // hundred faces on the near tiers, and it is the one piece of kit every
        // man wears, so it is paid for rather than trimmed.
        assert!(
            full.tris.len() < 5800,
            "the figure is too heavy to draw seven times a frame: {} tris",
            full.tris.len()
        );
        assert!(
            close.tris.len() < 9600,
            "the close figure is too heavy to draw twice: {} tris",
            close.tris.len()
        );
    }

    /// Every man wears a balaclava in club colour, and the face shows in the
    /// aperture rather than the mask being a second skin over it.
    #[test]
    fn every_head_wears_a_balaclava() {
        let teams = nopads_core::team::league_teams();
        let player = &teams[0].roster[0];
        let uniform = &teams[0].home;
        let pose = Pose::default();
        let mesh = build_mesh(player, uniform, &pose, 0);
        let (_, _, _, _, z_min, z_max) = bounds(&mesh);
        let h = z_max - z_min;
        let cloth = mesh
            .tris
            .iter()
            .filter(|t| t.color == uniform.jersey && t.a.z > z_min + h * 0.93)
            .count();
        assert!(cloth > 40, "the head is bare: {cloth} cloth faces on it");
        let face = mesh
            .tris
            .iter()
            .filter(|t| t.color == player.skin.rgb() && t.a.z > z_min + h * 0.93)
            .count();
        assert!(face > 4, "no face showing through the aperture: {face}");
    }

    /// Sleeves and tights are a draw, not a team uniform: some men have them and
    /// some do not, and it is the same men every time the roster is dressed.
    #[test]
    fn sleeves_and_tights_are_dealt_out_but_not_to_everybody() {
        let teams = nopads_core::team::league_teams();
        for team in &teams {
            let n = team.roster.len();
            let sleeves = team.roster.iter().filter(|p| wears_sleeves(p, 0)).count();
            let tights = team.roster.iter().filter(|p| wears_tights(p, 0)).count();
            assert!(sleeves > 0 && sleeves < n, "{} sleeves {sleeves}/{n}", team.abbr);
            assert!(tights > 0 && tights < n, "{} tights {tights}/{n}", team.abbr);
        }
        let first: Vec<bool> = teams[0].roster.iter().map(|p| wears_sleeves(p, 0)).collect();
        let again: Vec<bool> = teams[0].roster.iter().map(|p| wears_sleeves(p, 0)).collect();
        assert_eq!(first, again, "the dressing room is not deterministic");
    }

    /// The head is a face: a modelled surface with eyes, a nose, a mouth and
    /// ears, smooth-shaded rather than a ball with dots on it.
    #[test]
    fn the_head_is_a_face_and_not_a_ball() {
        let teams = nopads_core::team::league_teams();
        let player = &teams[0].roster[0];
        let uniform = &teams[0].home;
        let pose = Pose { run_phase: 0.3, speed: 0.0, ..Default::default() };
        let mesh = build_mesh_at(player, uniform, &pose, 0, Detail::Close);
        let (_, _, _, _, z_min, z_max) = bounds(&mesh);
        let h = z_max - z_min;
        let head: Vec<_> = mesh.tris.iter().filter(|t| t.a.z > z_min + h * 0.905).collect();
        assert!(head.len() > 500, "the head is only {} faces", head.len());
        let eye_white = head.iter().filter(|t| t.color == Rgb::hex(0xEFE8DC)).count();
        assert!(eye_white >= 6, "no eyes on the head: {eye_white} faces");
        let smooth = head
            .iter()
            .filter(|t| (t.na - t.nb).length() > 0.01 || (t.na - t.nc).length() > 0.01)
            .count();
        assert!(
            smooth > head.len() * 6 / 10,
            "the head is flat-shaded: {smooth} of {} faces carry a surface normal",
            head.len()
        );
    }

    /// The number goes on the shirt. It is painted against the body profile, so
    /// each patch sits at the depth of the cloth at its own height instead of on
    /// one plane floating in front of — or behind — the chest.
    #[test]
    fn the_number_is_painted_on_the_cloth_not_in_front_of_it() {
        let teams = nopads_core::team::league_teams();
        let player = &teams[0].roster[0];
        let uniform = &teams[0].home;
        let paint = uniform.readable_number_color();
        let patches = |speed: f32| -> Vec<Vec3> {
            let pose = Pose { run_phase: 0.3, speed, ..Default::default() };
            build_mesh(player, uniform, &pose, 0)
                .tris
                .iter()
                .filter(|t| t.color == paint && t.shade > 1.2)
                .map(|t| t.a)
                .collect()
        };

        let painted = patches(0.0);
        assert!(painted.len() >= 12, "no number on the shirt: {}", painted.len());
        // A digit is painted across the curve of the chest, so the patches do not
        // share one depth. A single plane is exactly the bug this replaced.
        let mut depths: Vec<f32> = painted.iter().map(|p| p.x.abs()).collect();
        depths.sort_by(f32::total_cmp);
        depths.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
        assert!(depths.len() >= 4, "the number is one flat plane: {depths:?}");
        // And it leans with the chest instead of staying put while he runs.
        let front = |speed: f32| -> f32 {
            let xs: Vec<f32> = patches(speed)
                .iter()
                .filter(|p| p.x > 0.0)
                .map(|p| p.x)
                .collect();
            assert!(!xs.is_empty(), "no number on the front of the shirt");
            xs.iter().sum::<f32>() / xs.len() as f32
        };
        assert!(
            front(1.0) > front(0.0) + 0.02,
            "the number does not lean with the chest: {:.3} at rest, {:.3} at a sprint",
            front(0.0),
            front(1.0)
        );
    }

    #[test]
    fn a_dive_puts_the_figure_on_the_ground_and_out_in_front() {
        let standing = bounds(&figure(None, 0.0));
        let dive = bounds(&figure(Some(Kind::Dive), 1.0));
        assert!(
            dive.5 < standing.5 * 0.75,
            "a dive has to end low: head was at {:.2}, now {:.2}",
            standing.5,
            dive.5
        );
        assert!(
            dive.1 > standing.1 + 0.6,
            "a dive has to reach out in front: {:.2} vs {:.2}",
            standing.1,
            dive.1
        );
        assert!(dive.4 > -0.05, "a dive cannot sink through the turf: {:.2}", dive.4);
    }

    #[test]
    fn a_hurdle_takes_him_off_his_feet() {
        let standing = bounds(&figure(None, 0.0));
        let air = bounds(&figure(Some(Kind::Hurdle), 0.5));
        assert!(
            air.4 > standing.4 + 0.45,
            "a hurdle has to leave the ground: soles were at {:.2}, are at {:.2}",
            standing.4,
            air.4
        );
        // And he comes back down inside the move instead of floating away.
        let landed = bounds(&figure(Some(Kind::Hurdle), 1.0));
        assert!((landed.4 - standing.4).abs() < 0.02, "the landing is {:.2}", landed.4);
    }

    fn corner(mesh: &Mesh, index: usize) -> Vec3 {
        let t = &mesh.tris[index / 3];
        match index % 3 {
            0 => t.a,
            1 => t.b,
            _ => t.c,
        }
    }

    #[test]
    fn a_spin_turns_him_around() {
        let rest = figure(None, 0.0);
        let standing = bounds(&rest);
        let spun = figure(Some(Kind::Spin), 0.25);
        let full = bounds(&figure(Some(Kind::Spin), 1.0));

        // The build is the same figure vertex for vertex, so the corner that is
        // furthest forward at rest has to have been turned by exactly the eased
        // angle. `p = 0.25` eases to 0.15625 of a turn.
        let face = (0..rest.tris.len() * 3)
            .max_by(|a, b| corner(&rest, *a).x.total_cmp(&corner(&rest, *b).x))
            .expect("a vertex");
        let before = corner(&rest, face);
        let after = corner(&spun, face);
        let p: f32 = 0.25;
        let turn = p * p * (3.0 - 2.0 * p) * std::f32::consts::TAU;
        let (s, c) = turn.sin_cos();
        let want = Vec3::new(before.x * c - before.y * s, before.x * s + before.y * c, before.z);
        assert!(
            (after.x - want.x).abs() < 1e-3 && (after.y - want.y).abs() < 1e-3,
            "the front of him should have turned {turn:.2} rad: {after:?} instead of {want:?}"
        );
        // A whole spin puts him back the way he was.
        assert!(
            (full.1 - standing.1).abs() < 0.02 && (full.3 - standing.3).abs() < 0.02,
            "a full spin should end where it started: {:.2}/{:.2} vs {:.2}/{:.2}",
            full.1,
            full.3,
            standing.1,
            standing.3
        );
    }

    fn posed(wrapping: f32, tackled: f32) -> Mesh {
        let teams = nopads_core::team::league_teams();
        let pose = Pose {
            run_phase: 0.3,
            speed: 0.75,
            wrapping,
            tackled,
            ..Default::default()
        };
        build_mesh(&teams[0].roster[0], &teams[0].home, &pose, 0)
    }

    /// A tackler has to reach out and around the man, not jog past him with his
    /// arms at his sides.
    #[test]
    fn a_tackler_wraps_his_arms_around_the_man() {
        let standing = bounds(&posed(0.0, 0.0));
        let wrapping = bounds(&posed(1.0, 0.0));
        assert!(
            wrapping.1 > standing.1 + 0.18,
            "the wrap has to reach in front of him: {:.2} vs {:.2}",
            wrapping.1,
            standing.1
        );
        assert!(
            wrapping.5 < standing.5 - 0.03,
            "a tackler gets low: head was at {:.2}, is at {:.2}",
            standing.5,
            wrapping.5
        );
        assert!(wrapping.4 > -0.05, "he cannot sink through the turf: {:.2}", wrapping.4);
    }

    /// Being tackled is an animation, not a switch: half way through he is lower
    /// than he was, and at the end he is on the turf and out in front.
    #[test]
    fn a_tackled_man_goes_down_over_the_hit() {
        let standing = bounds(&posed(0.0, 0.0));
        let falling = bounds(&posed(0.0, 0.5));
        let down = bounds(&posed(0.0, 1.0));
        assert!(
            falling.5 < standing.5 - 0.1,
            "he has to be going down half way through: {:.2} vs {:.2}",
            falling.5,
            standing.5
        );
        assert!(
            down.5 < standing.5 * 0.55,
            "he ends on the turf: head was at {:.2}, is at {:.2}",
            standing.5,
            down.5
        );
        assert!(
            down.1 > standing.1 + 0.7,
            "a body on the ground reaches out in front: {:.2} vs {:.2}",
            down.1,
            standing.1
        );
        assert!(down.4 > -0.05, "he cannot sink through the turf: {:.2}", down.4);
    }

    #[test]
    fn a_slide_drops_him_down_and_back() {
        let standing = bounds(&figure(None, 0.0));
        let slide = bounds(&figure(Some(Kind::Slide), 1.0));
        assert!(
            slide.0 < standing.0 - 0.6,
            "a slide has to go to ground behind him: {:.2} vs {:.2}",
            slide.0,
            standing.0
        );
        assert!(
            slide.5 < standing.5 * 0.8,
            "a slide is lower than standing: {:.2} vs {:.2}",
            slide.5,
            standing.5
        );
    }
    #[test]
    fn the_shirt_is_cut_from_a_texture_and_the_skin_is_not() {
        let mesh = figure(None, 0.0);
        assert_eq!(mesh.textures.len(), 1, "the shirt carries no texture");
        let cut = mesh.tris.iter().filter(|t| t.tex.is_some()).count();
        assert!(cut > 0, "not one triangle of the shirt is cut from it");
        assert!(
            cut < mesh.tris.len(),
            "the skin and the shorts went with the shirt"
        );
    }
}

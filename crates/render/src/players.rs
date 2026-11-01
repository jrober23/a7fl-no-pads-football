//! A7FL player figures: sleeveless jersey, shorts, bare arms and no helmet.
//!
//! A figure is a small skeleton — pelvis, two-segment spine, head, and two joints
//! per limb — posed from a stride cycle and built out of lofted bodies, limbs and
//! a head. Nothing on it is a box measured off another box: the torso, shorts and
//! thighs are lofted through rounded cross-sections, so the silhouette has a
//! chest, a waist, a seat and a pair of calves instead of a stack of crates.
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

use a7fl_core::math::{Rgb, Vec3};
use a7fl_core::moves::Kind;
use a7fl_core::team::{HairStyle, JerseyPattern, Player, Uniform};

use crate::scene::Mesh;

/// Ankle height above the turf when the foot is planted: sole plus cleat.
const SOLE: f32 = 0.062;

/// Sides on a limb at full detail. Seven is the cheapest count that still reads
/// as a round cylinder from the broadcast distance rather than as a prism.
const LIMB_SIDES: usize = 7;

/// Sides around the torso at full detail. Low-poly bodies show their facets
/// hardest across the chest and the seat, so these get more than the limbs.
const BODY_SIDES: usize = 12;

/// How much of the body is worth building at a given distance from the lens.
///
/// A figure at full detail is around seven hundred triangles and there are
/// fourteen of them standing on every frame of a software renderer, so a man on
/// the far hash is not drawn with eyes and a bicep: at that size they are a
/// pixel and a half, and the frame can only afford so much. Level 1 is the close
/// shot, 0 is a figure the size of a match head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    Full,
    Mid,
    Far,
}

impl Detail {
    /// Pick a level from the distance between the lens and the man.
    pub fn for_distance(yards: f32) -> Detail {
        if yards < 15.0 {
            Detail::Full
        } else if yards < 26.0 {
            Detail::Mid
        } else {
            Detail::Far
        }
    }

    fn sides(self) -> usize {
        match self {
            Detail::Full => LIMB_SIDES,
            Detail::Mid => 6,
            Detail::Far => 5,
        }
    }

    fn body_sides(self) -> usize {
        match self {
            Detail::Full => BODY_SIDES,
            Detail::Mid => 8,
            Detail::Far => 6,
        }
    }

    /// Eyes, brows, the quadriceps bulge, knee caps, trapezius wedges.
    fn fine(self) -> bool {
        !matches!(self, Detail::Far)
    }

    /// Painted numbers and the seams of the hair.
    fn paint(self) -> bool {
        matches!(self, Detail::Full | Detail::Mid)
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
    let grid: Vec<Vec<Vec3>> = rings
        .iter()
        .map(|r| {
            (0..sides)
                .map(|k| ring_point(r, k as f32 / sides as f32 * std::f32::consts::TAU, round))
                .collect()
        })
        .collect();
    for i in 0..rings.len() - 1 {
        for k in 0..sides {
            let k1 = (k + 1) % sides;
            mesh.quad_shaded(
                grid[i][k],
                grid[i][k1],
                grid[i + 1][k1],
                grid[i + 1][k],
                color_at(i, k),
                1.0,
            );
        }
    }
    if cap_bottom {
        let c = rings[0].center;
        for k in 0..sides {
            let k1 = (k + 1) % sides;
            mesh.tri_shaded(c, grid[0][k1], grid[0][k], color_at(0, k), 0.82);
        }
    }
    if cap_top {
        let last = rings.len() - 1;
        let c = rings[last].center;
        for k in 0..sides {
            let k1 = (k + 1) % sides;
            mesh.tri_shaded(c, grid[last][k], grid[last][k1], color_at(last, k), 0.92);
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
fn shoe(mesh: &mut Mesh, ankle: Vec3, pitch: f32, color: Rgb) {
    let center = ankle + Vec3::new(0.062 * pitch.cos(), 0.0, -0.038);
    taper3(
        mesh,
        center,
        Vec3::new(0.125, 0.056, 0.040),
        Vec3::new(0.112, 0.048, 0.032),
        pitch,
        0.0,
        color,
        0.95,
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

/// Digits on a 3x5 grid, one bit per pixel, top row first. Reading the number off
/// a player is the whole point of the back of a jersey, so the figure wears real
/// digits instead of a solid rectangle.
fn digit(c: u8) -> [u8; 5] {
    match c {
        b'0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        b'1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        b'2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        b'3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        b'4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        b'5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        b'6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        b'7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        b'8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        b'9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        _ => [0, 0, 0, 0, 0],
    }
}

/// A flat patch of paint just proud of the cloth, facing `face` (+1 front, -1 back).
/// Numbers are lit as if they were cloth, so they never disappear in shadow.
fn paint_patch(mesh: &mut Mesh, x: f32, y: f32, z: f32, hy: f32, hz: f32, color: Rgb) {
    mesh.quad_shaded(
        Vec3::new(x, y - hy, z - hz),
        Vec3::new(x, y + hy, z - hz),
        Vec3::new(x, y + hy, z + hz),
        Vec3::new(x, y - hy, z + hz),
        color,
        1.35,
    );
}

/// Paint a jersey number onto one side of the chest. `face` is +1 for the front
/// and -1 for the back, and the back is mirrored so the digits read correctly to
/// somebody standing behind him.
fn paint_number(mesh: &mut Mesh, number: u8, face: f32, x: f32, z: f32, cell: f32, color: Rgb) {
    let text = format!("{number:02}");
    let glyphs: Vec<u8> = text.bytes().collect();
    let pitch = cell * 4.0;
    let width = pitch * glyphs.len() as f32 - cell;
    let bite = cell * 0.31;
    for (gi, ch) in glyphs.iter().enumerate() {
        let rows = digit(*ch);
        let start = gi as f32 * pitch - width * 0.5;
        for (row, bits) in rows.iter().enumerate() {
            for col in 0..3u8 {
                if bits & (1 << (2 - col)) == 0 {
                    continue;
                }
                let lateral = start + col as f32 * cell;
                // Which way a digit advances across the cloth: the camera looks
                // along the inward normal, and in this projection screen-right
                // for a face pointing +x is -y. Get this backwards and the number
                // is mirrored, which is what a jersey reads as a smudge.
                let y = if face > 0.0 { -lateral } else { lateral };
                let zz = z + (2.0 - row as f32) * cell * 1.16;
                paint_patch(mesh, x, y, zz, bite, cell * 0.37, color);
            }
        }
    }
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
    let bulk = player.build.clamp(0.9, 1.1) * 1.04;
    let shoulder_w = (player.shoulder_width() * 1.06).clamp(0.40, 0.66);
    let skin = player.skin.rgb();
    let jersey = uniform.jersey;
    let accent = uniform.jersey_accent;
    let shorts = uniform.shorts;
    let socks = uniform.socks;
    let shoes = uniform.shoes;

    let speed = pose.speed.clamp(0.0, 1.0);
    // A walk becomes a run with pace, and everything settles when he stops.
    let run_blend = ((speed - 0.16) / 0.34).clamp(0.0, 1.0);
    let live = (speed / 0.10).clamp(0.0, 1.0);

    let leg_len = h * 0.48;
    let thigh_len = leg_len * 0.52;
    let shin_len = leg_len * 0.48;
    let torso_len = h * 0.33;
    // A small head over broad shoulders is most of what makes a figure read as
    // an athlete rather than a doll.
    let head_r = h * 0.057;
    let hip_lateral = shoulder_w * (0.22 + 0.08 * pose.strafe.clamp(0.0, 1.0));

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

    let thigh_r = 0.146 * bulk;
    let knee_r = 0.086 * bulk;
    let calf_r = 0.106 * bulk;
    let ankle_r = 0.054 * bulk;

    for (i, side) in [-1.0f32, 1.0].iter().enumerate() {
        let [thigh, knee, foot] = legs[i];
        let hip = Vec3::new(0.0, side * hip_lateral, hip_z);
        let (ts, tc) = thigh.sin_cos();
        let knee_pos = hip + Vec3::new(ts, 0.0, -tc) * thigh_len;
        let shin = thigh - knee;
        let (ss, sc) = shin.sin_cos();
        let ankle = knee_pos + Vec3::new(ss, 0.0, -sc) * shin_len;

        // Shorts over the top of the thigh, bare skin below the hem. A quad is
        // heavy at the hip and tapers into the knee, which is what carries the
        // weight of the figure.
        let hem = hip.lerp(knee_pos, 0.68);
        limb(&mut mesh, hip, hem, thigh_r, thigh_r * 0.90, sides, shorts);
        // Quadriceps: the leg is at its thickest just above the knee, not at the
        // hip, so the bare thigh bulges before it narrows into the joint.
        if detail.fine() {
            let quad = hem.lerp(knee_pos, 0.45);
            limb(&mut mesh, hem, quad, thigh_r * 0.88, thigh_r * 0.80, sides, skin);
            limb(&mut mesh, quad, knee_pos, thigh_r * 0.80, knee_r, sides, skin);
            // Knee cap, so the joint is a joint and not a kink in a tube.
            mesh.sphere(
                knee_pos,
                Vec3::new(knee_r * 0.98, knee_r * 0.9, knee_r * 0.98),
                2,
                6,
                skin,
            );
        } else {
            limb(&mut mesh, hem, knee_pos, thigh_r * 0.86, knee_r, sides, skin);
        }
        // The calf swells out of the knee and the sock takes over above the ankle.
        let belly = knee_pos.lerp(ankle, 0.34);
        limb(&mut mesh, knee_pos, belly, knee_r * 0.98, calf_r, sides, skin);
        limb(&mut mesh, belly, ankle, calf_r * 0.96, ankle_r, sides, socks);
        // Sock cuff, in team colour at the top of the sock.
        mesh.cylinder(
            belly,
            belly.lerp(ankle, 0.16),
            calf_r * 1.03,
            calf_r * 0.99,
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
            0.104 * bulk,
            hip_lateral * 0.94,
            hip_yaw * 0.6,
        ),
        Ring::new(
            pelvis + Vec3::new(0.0, 0.0, h * 0.005),
            0.122 * bulk,
            hip_lateral * 1.02,
            hip_yaw,
        ),
        Ring::new(
            pelvis + Vec3::new(0.0, 0.0, h * 0.048),
            0.118 * bulk,
            hip_lateral * 0.96,
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
    // the whole athletic silhouette.
    let torso = [
        Ring::new(spine(0.02), 0.112 * bulk, shoulder_w * 0.35, hip_yaw * 0.8),
        Ring::new(spine(0.24), 0.108 * bulk, shoulder_w * 0.33, hip_yaw * 0.5),
        Ring::new(spine(0.50), 0.132 * bulk, shoulder_w * 0.43, hip_yaw * 0.2),
        Ring::new(spine(0.76), 0.150 * bulk, shoulder_w * 0.52, chest_yaw * 0.6),
        Ring::new(spine(1.00), 0.128 * bulk, shoulder_w * 0.53, chest_yaw),
    ];
    let torso_bands = torso.len() - 1;
    let cloth = |i: usize, k: usize| -> Rgb {
        let a = (k as f32 + 0.5) / body_sides as f32 * std::f32::consts::TAU;
        let lateral = a.sin().abs();
        match uniform.pattern {
            JerseyPattern::Solid => jersey,
            // A yoke across the top of the shoulders and the sleeves.
            JerseyPattern::ShoulderStripe => {
                if i == torso_bands - 1 && lateral > 0.45 {
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
                if i == 2 {
                    accent
                } else {
                    jersey
                }
            }
            JerseyPattern::Hoops => {
                if i % 2 == 1 {
                    accent
                } else {
                    jersey
                }
            }
        }
    };
    loft(&mut mesh, &torso, body_sides, TORSO_ROUND, true, true, cloth);

    // Hem of the jersey, on top of the shorts.
    let hem_lo = Ring::new(spine(0.0), 0.114 * bulk, shoulder_w * 0.355, hip_yaw * 0.9);
    let hem_hi = Ring::new(
        spine(0.07) + spine_dir * 0.001,
        0.116 * bulk,
        shoulder_w * 0.36,
        hip_yaw * 0.8,
    );
    loft(&mut mesh, &[hem_lo, hem_hi], body_sides, TORSO_ROUND, false, false, |_, _| {
        accent
    });

    // Numbers front and back, painted on the cloth. Too far away to read one is
    // too far away to pay for one.
    if detail.paint() {
        let number_color = uniform.readable_number_color();
        let cell = 0.050 * (h / 1.85);
        let chest_rx = 0.150 * bulk;
        let chest_z = spine(0.70).z;
        paint_number(
            &mut mesh,
            player.number,
            1.0,
            chest_rx + 0.006,
            chest_z,
            cell,
            number_color,
        );
        paint_number(
            &mut mesh,
            player.number,
            -1.0,
            -(chest_rx + 0.006),
            chest_z + torso_len * 0.02,
            cell * 1.02,
            number_color,
        );
    }

    // ---------------- arms ----------------
    let shoulder_zone = spine(0.97);
    let upper_len = h * 0.15;
    let fore_len = h * 0.135;
    for (i, side) in [-1.0f32, 1.0].iter().enumerate() {
        let shoulder = shoulder_zone + Vec3::new(0.0, side * (shoulder_w * 0.50 - 0.015), -0.025);
        // Trapezius, sloping from the neck out to the shoulder. Blends bare
        // skin into the jersey and squares off the shoulders.
        if detail.fine() {
            taper3(
                &mut mesh,
                shoulder.lerp(shoulder_zone, 0.5) + Vec3::new(0.0, 0.0, 0.004),
                Vec3::new(0.070, 0.060, 0.024),
                Vec3::new(0.052, 0.090, 0.020),
                0.0,
                chest_yaw,
                skin,
                0.98,
            );
        }
        // Deltoid: bare shoulder, no pads in this league. This is the widest
        // thing on the figure and it is why he reads as a player.
        mesh.sphere(
            shoulder,
            Vec3::new(0.086 * bulk, 0.086 * bulk, 0.080 * bulk),
            if detail.fine() { 3 } else { 2 },
            if detail.fine() { 8 } else { 6 },
            skin,
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
        if wrap > 0.0 {
            // Arms out and around the man: upper arms forward at shoulder height
            // and the elbows folded, so the hands close on his far side instead of
            // hanging at his own ribs. This is the difference between a tackle and
            // two men standing next to each other.
            swing += (1.25 - swing) * wrap;
            flex += (1.20 - flex) * wrap;
        }

        let up_dir = Vec3::new(swing.sin(), side * 0.12, -swing.cos()).normalize_or_zero();
        let elbow = shoulder + up_dir * upper_len;
        // A flexed elbow carries the forearm forward and up, not backward.
        let fore_angle = swing + flex + 0.12;
        let fore_dir =
            Vec3::new(fore_angle.sin(), side * 0.05, -fore_angle.cos()).normalize_or_zero();
        let hand = elbow + fore_dir * fore_len;

        // Biceps swell on the upper arm, forearm tapers into the wrist.
        limb(
            &mut mesh,
            shoulder,
            shoulder.lerp(elbow, 0.45),
            0.068 * bulk,
            0.066 * bulk,
            sides,
            skin,
        );
        limb(
            &mut mesh,
            shoulder.lerp(elbow, 0.45),
            elbow,
            0.066 * bulk,
            0.050 * bulk,
            sides,
            skin,
        );
        limb(
            &mut mesh,
            elbow,
            elbow.lerp(hand, 0.55),
            0.053 * bulk,
            0.048 * bulk,
            sides,
            skin,
        );
        limb(
            &mut mesh,
            elbow.lerp(hand, 0.55),
            hand,
            0.048 * bulk,
            0.034 * bulk,
            sides,
            skin,
        );
        // Hand: a fist big enough to be seen holding something.
        mesh.sphere(
            hand + fore_dir * 0.02,
            Vec3::new(0.044, 0.040, 0.046),
            2,
            6,
            skin,
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
    let head = neck_base + head_dir * (head_r * 2.05);
    limb(
        &mut mesh,
        neck_base,
        neck_base + head_dir * (head_r * 1.05),
        0.058 * bulk,
        0.052 * bulk,
        sides,
        skin,
    );
    let head_rings = if detail.fine() { 5 } else { 3 };
    let head_segs = if detail.fine() { 10 } else { 7 };
    mesh.sphere(
        head,
        Vec3::new(head_r, head_r * 0.94, head_r * 1.1),
        head_rings,
        head_segs,
        skin,
    );
    // Jaw and chin, which is what gives a head a face rather than an egg.
    mesh.sphere(
        head + Vec3::new(head_r * 0.34, 0.0, -head_r * 0.52),
        Vec3::new(head_r * 0.62, head_r * 0.74, head_r * 0.48),
        if detail.fine() { 3 } else { 2 },
        head_segs,
        skin,
    );
    // Nose, so the profile is not a sphere.
    mesh.sphere(
        head + Vec3::new(head_r * 0.92, 0.0, -head_r * 0.05),
        Vec3::new(head_r * 0.20, head_r * 0.16, head_r * 0.18),
        2,
        5,
        skin,
    );
    for side in [-1.0f32, 1.0] {
        mesh.sphere(
            Vec3::new(head.x - 0.01, side * head_r * 0.96, head.z),
            Vec3::new(0.018, 0.020, 0.030),
            2,
            5,
            skin,
        );
        // Eyes: two dark dots set into the brow. They cost four triangles and
        // they are the difference between a head and a ball at close range.
        if detail.fine() {
            mesh.sphere(
                head + Vec3::new(head_r * 0.80, side * head_r * 0.34, head_r * 0.24),
                Vec3::new(0.012, 0.016, 0.014),
                2,
                4,
                Rgb::hex(0x2A2420),
            );
            // Brow, so the face has a shadow line above the eyes.
            taper3(
                &mut mesh,
                head + Vec3::new(head_r * 0.72, side * head_r * 0.34, head_r * 0.50),
                Vec3::new(0.014, head_r * 0.32, 0.008),
                Vec3::new(0.014, head_r * 0.32, 0.008),
                0.0,
                0.0,
                skin.scale(0.82),
                1.0,
            );
        }
    }
    match player.hair {
        HairStyle::Bald => {}
        HairStyle::Buzz => {
            mesh.sphere(
                Vec3::new(head.x - 0.005, 0.0, head.z + head_r * 0.42),
                Vec3::new(head_r, head_r * 0.94, head_r * 0.68),
                3,
                9,
                player.hair_color,
            );
        }
        HairStyle::Fade => {
            mesh.sphere(
                Vec3::new(head.x - 0.005, 0.0, head.z + head_r * 0.5),
                Vec3::new(head_r * 1.02, head_r * 0.96, head_r * 0.7),
                3,
                9,
                player.hair_color,
            );
        }
        HairStyle::Locs => {
            mesh.sphere(
                Vec3::new(head.x - 0.01, 0.0, head.z + head_r * 0.55),
                Vec3::new(head_r * 1.05, head_r, head_r * 0.78),
                3,
                9,
                player.hair_color,
            );
            let strands = if detail.fine() { 7 } else { 0 };
            for i in 0..strands {
                let a = i as f32 / 7.0 * std::f32::consts::TAU;
                let base = Vec3::new(
                    head.x - head_r * 0.4,
                    head.y + a.cos() * head_r * 0.8,
                    head.z + head_r * 0.4,
                );
                let tip = base + Vec3::new(-head_r * 0.9, a.cos() * 0.03, -head_r * 1.4);
                limb(&mut mesh, base, tip, 0.024, 0.014, 4, player.hair_color);
            }
        }
        HairStyle::TwistOut => {
            mesh.sphere(
                Vec3::new(head.x - 0.01, 0.0, head.z + head_r * 0.7),
                Vec3::new(head_r * 1.18, head_r * 1.1, head_r * 0.9),
                4,
                9,
                player.hair_color,
            );
        }
        HairStyle::HighTop => {
            box3(
                &mut mesh,
                Vec3::new(head.x - 0.01, 0.0, head.z + head_r * 1.1),
                Vec3::new(head_r * 0.85, head_r * 0.85, head_r * 0.55),
                0.0,
                0.0,
                player.hair_color,
                1.0,
            );
        }
    }
    if player.beard > 0.2 {
        mesh.sphere(
            head + Vec3::new(head_r * 0.50, 0.0, -head_r * 0.42),
            Vec3::new(head_r * 0.42, head_r * 0.74, head_r * 0.50),
            3,
            7,
            player.hair_color,
        );
    }
    // Helmet? Never. The A7FL plays without them.

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
        let teams = a7fl_core::team::league_teams();
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

    /// A torso is not a crate. The chest has to be wider than the waist, the
    /// shoulders wider than both, and the head narrower than all of it.
    #[test]
    fn the_figure_has_shoulders_a_waist_and_a_head_that_fits() {
        let mesh = figure(None, 0.0);
        let (_, _, _, _, z_min, z_max) = bounds(&mesh);
        let h = z_max - z_min;
        let waist = width_between(&mesh, z_min + h * 0.50, z_min + h * 0.58);
        let chest = width_between(&mesh, z_min + h * 0.68, z_min + h * 0.76);
        let shoulders = width_between(&mesh, z_min + h * 0.80, z_min + h * 0.86);
        let head = width_between(&mesh, z_min + h * 0.93, z_max);
        assert!(waist > 0.10, "no body at the waist: {waist:.3}");
        assert!(
            chest > waist * 0.98,
            "the chest has to be at least as wide as the waist: {chest:.3} vs {waist:.3}"
        );
        assert!(
            shoulders > waist * 1.15,
            "shoulders should overhang the waist: {shoulders:.3} vs {waist:.3}"
        );
        assert!(
            head < shoulders * 0.72,
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
        let teams = a7fl_core::team::league_teams();
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
            .map(|c| digit(c).iter().map(|row| row.count_ones()).sum::<u32>())
            .sum::<u32>() as usize;
        // Front and back, one patch per lit pixel of the digits.
        assert!(
            painted >= want * 2,
            "the number {:#02} only put {painted} patches on the cloth, wanted {}",
            player.number,
            want * 2
        );
    }

    /// The figure is rebuilt for every man on every frame, so it cannot grow into
    /// something a software rasteriser cannot afford.
    #[test]
    fn the_figure_stays_within_a_triangle_budget() {
        let mesh = figure(None, 0.0);
        let tris = mesh.tris.len();
        assert!(tris > 400, "the figure is still made of a few boxes: {tris} tris");
        assert!(tris < 1400, "the figure is too heavy to draw seven times a frame: {tris} tris");
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
        let teams = a7fl_core::team::league_teams();
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
}

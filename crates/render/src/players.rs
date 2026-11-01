//! A7FL player figures: sleeveless jersey, shorts, bare arms and no helmet.
//!
//! A figure is a small skeleton — pelvis, two-segment spine, head, and two joints
//! per limb — posed from a stride cycle and built out of boxes and cylinders.
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
use a7fl_core::team::{HairStyle, JerseyPattern, Player, Uniform};

use crate::scene::Mesh;

/// Ankle height above the turf when the foot is planted: sole plus cleat.
const SOLE: f32 = 0.062;

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
/// at the same time. The old figure could only yaw, so a leaning torso had to be
/// faked by sliding a box forward, which is what made it look like a stack of
/// parts rather than a body.
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
    let center = ankle + Vec3::new(0.052 * pitch.cos(), 0.0, -0.032);
    taper3(
        mesh,
        center,
        Vec3::new(0.100, 0.046, 0.032),
        Vec3::new(0.090, 0.039, 0.027),
        pitch,
        0.0,
        color,
        0.95,
    );
}

pub fn build_mesh(player: &Player, uniform: &Uniform, pose: &Pose, team_skin_seed: u32) -> Mesh {
    let mut mesh = Mesh::new();
    let h = player.height_yards().clamp(1.55, 2.05);
    let shoulder_w = player.shoulder_width().clamp(0.36, 0.62);
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
    let head_r = h * 0.056;
    let hip_lateral = shoulder_w * (0.21 + 0.10 * pose.strafe.clamp(0.0, 1.0));

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
        legs[i] = angles;
        reach = reach.max(leg_reach(
            angles[0],
            angles[1],
            thigh_len,
            shin_len,
        ));
    }
    // The hips hang off the straightest leg, so the planted foot is exactly on
    // the turf and the swinging foot is clear of it.
    let hip_z = SOLE + reach;

    for (i, side) in [-1.0f32, 1.0].iter().enumerate() {
        let [thigh, knee, foot] = legs[i];
        let hip = Vec3::new(0.0, side * hip_lateral, hip_z);
        let (ts, tc) = thigh.sin_cos();
        let knee_pos = hip + Vec3::new(ts, 0.0, -tc) * thigh_len;
        let shin = thigh - knee;
        let (ss, sc) = shin.sin_cos();
        let ankle = knee_pos + Vec3::new(ss, 0.0, -sc) * shin_len;

        // Thigh: shorts over the top, bare skin below. Heavy at the hip and
        // tapering into the knee, which is what carries the weight of the figure.
        let mid = hip.lerp(knee_pos, 0.58);
        limb(&mut mesh, hip, mid, 0.094, 0.080, 5, shorts);
        limb(&mut mesh, mid, knee_pos, 0.078, 0.058, 5, skin);
        // Calf bulges out of the knee, then the sock takes over above the ankle.
        let calf = knee_pos.lerp(ankle, 0.38);
        limb(&mut mesh, knee_pos, calf, 0.060, 0.057, 5, skin);
        limb(&mut mesh, calf, ankle, 0.054, 0.040, 5, socks);
        shoe(&mut mesh, ankle, foot, shoes);
    }

    // ---------------- pelvis ----------------
    // The pelvis turns into the stride, one hip leading.
    let hip_yaw = (pose.run_phase.rem_euclid(1.0) * std::f32::consts::TAU).sin()
        * 0.10
        * live;
    let pelvis = Vec3::new(0.0, 0.0, hip_z);
    taper3(
        &mut mesh,
        pelvis + Vec3::new(0.0, 0.0, h * 0.02),
        Vec3::new(0.125, hip_lateral * 0.95, h * 0.055),
        Vec3::new(0.135, hip_lateral * 0.98, h * 0.055),
        0.0,
        hip_yaw,
        shorts,
        1.0,
    );
    for side in [-1.0f32, 1.0] {
        box3(
            &mut mesh,
            pelvis + Vec3::new(0.0, side * hip_lateral * 0.98, h * 0.02),
            Vec3::new(0.118, 0.010, h * 0.048),
            0.0,
            hip_yaw,
            uniform.shorts_accent,
            1.0,
        );
    }

    // ---------------- spine and chest ----------------
    // The whole upper body hangs off the hips and leans as one piece.
    let lean = live * (0.08 + 0.24 * speed) + if pose.blocking { 0.34 } else { 0.0 };
    let spine_dir = Vec3::new(lean.sin(), 0.0, lean.cos());
    let spine = |f: f32| pelvis + spine_dir * (torso_len * f);
    // The shoulders counter-rotate against the pelvis, which is what stops a
    // runner looking like a rigid block with arms attached.
    let chest_yaw = -hip_yaw * 0.7;

    let chest = spine(0.64);
    let chest_half = Vec3::new(0.142, shoulder_w * 0.54, torso_len * 0.26);
    // Waist first, then the chest flares up into the shoulders on top of it.
    taper3(
        &mut mesh,
        spine(0.28),
        Vec3::new(0.118, shoulder_w * 0.35, torso_len * 0.20),
        Vec3::new(0.128, shoulder_w * 0.41, torso_len * 0.20),
        lean,
        hip_yaw,
        jersey,
        1.0,
    );
    taper3(
        &mut mesh,
        chest,
        Vec3::new(0.140, shoulder_w * 0.43, torso_len * 0.25),
        Vec3::new(0.132, shoulder_w * 0.54, torso_len * 0.23),
        lean,
        chest_yaw,
        jersey,
        1.0,
    );

    // Hem and armhole trim.
    taper3(
        &mut mesh,
        spine(0.28) - spine_dir * torso_len * 0.20 + Vec3::new(0.0, 0.0, 0.004),
        Vec3::new(0.124, shoulder_w * 0.36, 0.012),
        Vec3::new(0.124, shoulder_w * 0.36, 0.012),
        lean,
        hip_yaw,
        accent,
        1.0,
    );

    match uniform.pattern {
        JerseyPattern::ShoulderStripe => {
            for side in [-1.0f32, 1.0] {
                box3(
                    &mut mesh,
                    chest + Vec3::new(0.0, side * (chest_half.y - 0.02), chest_half.z * 0.45),
                    Vec3::new(chest_half.x + 0.006, 0.028, chest_half.z * 0.40),
                    lean,
                    chest_yaw,
                    accent,
                    1.0,
                );
            }
        }
        JerseyPattern::SidePanel => {
            for side in [-1.0f32, 1.0] {
                box3(
                    &mut mesh,
                    chest + Vec3::new(0.0, side * chest_half.y, 0.0),
                    Vec3::new(chest_half.x * 0.9, 0.010, chest_half.z * 0.85),
                    lean,
                    chest_yaw,
                    accent,
                    1.0,
                );
            }
        }
        JerseyPattern::Sash => {
            box3(
                &mut mesh,
                chest - spine_dir * torso_len * 0.06,
                Vec3::new(chest_half.x + 0.008, chest_half.y * 0.85, chest_half.z * 0.18),
                lean,
                chest_yaw,
                accent,
                1.0,
            );
        }
        JerseyPattern::Hoops => {
            for i in -1..=1 {
                box3(
                    &mut mesh,
                    chest + spine_dir * (i as f32 * chest_half.z * 0.6),
                    Vec3::new(chest_half.x + 0.008, chest_half.y + 0.003, chest_half.z * 0.12),
                    lean,
                    chest_yaw,
                    accent,
                    1.0,
                );
            }
        }
        JerseyPattern::Solid => {}
    }

    // Number plate front and back.
    let number_color = uniform.readable_number_color();
    for (side, scale) in [(-1.0f32, 0.5f32), (1.0, 0.42)] {
        box3(
            &mut mesh,
            chest + Vec3::new(side * (chest_half.x + 0.004), 0.0, chest_half.z * 0.1),
            Vec3::new(0.006, chest_half.y * scale, chest_half.z * scale),
            lean,
            chest_yaw,
            number_color,
            1.0,
        );
    }

    // ---------------- arms ----------------
    let shoulder_zone = spine(1.0);
    for (i, side) in [-1.0f32, 1.0].iter().enumerate() {
        let shoulder = shoulder_zone + Vec3::new(0.0, side * (shoulder_w * 0.52 - 0.02), -0.03);
        // Deltoid cap: bare shoulder, no pads in this league.
        mesh.sphere(shoulder, Vec3::new(0.078, 0.076, 0.070), 3, 6, skin);

        // Each arm swings against the leg on the other side of the body. The
        // elbow rides *behind* the ribs and the hand pumps up toward the chest,
        // which keeps the forearm tucked against the body. Swing an arm from the
        // shoulder alone and it projects off the front of the chest like a plank.
        let counter = legs[1 - i][0];
        let ball_arm = pose.carrying && *side > 0.0;
        let mut swing = counter * (0.50 + 0.30 * run_blend) * live - 0.30 * live;
        if ball_arm {
            swing = swing * 0.20 - 0.10;
        }
        let flex = if ball_arm {
            1.85
        } else if pose.blocking {
            1.05
        } else {
            0.25 + 1.20 * run_blend * live
        };

        let upper_len = h * 0.15;
        let fore_len = h * 0.13;
        let up_dir = Vec3::new(swing.sin(), side * 0.12, -swing.cos()).normalize_or_zero();
        let elbow = shoulder + up_dir * upper_len;
        // A flexed elbow carries the forearm forward and up, not backward.
        let fore_angle = swing + flex + 0.12;
        let fore_dir =
            Vec3::new(fore_angle.sin(), side * 0.05, -fore_angle.cos()).normalize_or_zero();
        let hand = elbow + fore_dir * fore_len;

        limb(&mut mesh, shoulder, elbow, 0.052, 0.042, 5, skin);
        limb(&mut mesh, elbow, hand, 0.042, 0.032, 5, skin);
        // Fist, so the arm ends in something instead of an open tube.
        box3(
            &mut mesh,
            hand + fore_dir * 0.02,
            Vec3::new(0.034, 0.030, 0.030),
            0.0,
            0.0,
            skin,
            0.95,
        );

        if ball_arm {
            // Tucked to the ribs, nose up.
            let ball_center = hand + Vec3::new(-0.02, 0.0, 0.02);
            mesh.sphere(ball_center, Vec3::new(0.055, 0.092, 0.070), 4, 8, Rgb::hex(0x6B3B1E));
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
    let head = neck_base + head_dir * (head_r * 2.0);
    limb(
        &mut mesh,
        neck_base,
        neck_base + head_dir * (head_r * 0.9),
        0.052,
        0.048,
        5,
        skin,
    );
    mesh.sphere(head, Vec3::new(head_r, head_r * 0.92, head_r * 1.1), 4, 8, skin);
    for side in [-1.0f32, 1.0] {
        mesh.sphere(
            Vec3::new(head.x - 0.01, side * head_r * 0.95, head.z),
            Vec3::new(0.018, 0.020, 0.030),
            2,
            5,
            skin,
        );
    }
    match player.hair {
        HairStyle::Bald => {}
        HairStyle::Buzz => {
            mesh.sphere(
                Vec3::new(head.x - 0.005, 0.0, head.z + head_r * 0.42),
                Vec3::new(head_r, head_r * 0.94, head_r * 0.68),
                3,
                8,
                player.hair_color,
            );
        }
        HairStyle::Fade => {
            mesh.sphere(
                Vec3::new(head.x - 0.005, 0.0, head.z + head_r * 0.5),
                Vec3::new(head_r * 1.02, head_r * 0.96, head_r * 0.7),
                3,
                8,
                player.hair_color,
            );
        }
        HairStyle::Locs => {
            mesh.sphere(
                Vec3::new(head.x - 0.01, 0.0, head.z + head_r * 0.55),
                Vec3::new(head_r * 1.05, head_r, head_r * 0.78),
                3,
                8,
                player.hair_color,
            );
            for i in 0..7 {
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
                8,
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
        box3(
            &mut mesh,
            Vec3::new(head.x + head_r * 0.62, 0.0, head.z - head_r * 0.45),
            Vec3::new(head_r * 0.28, head_r * 0.72, head_r * 0.42),
            0.0,
            0.0,
            player.hair_color,
            1.0,
        );
    }
    // Helmet? Never. The A7FL plays without them.

    if pose.down {
        // Tipped over: rotate the whole figure onto the turf.
        for t in mesh.tris.iter_mut() {
            for p in [&mut t.a, &mut t.b, &mut t.c] {
                let z = p.z;
                p.z = z * 0.32 + 0.1;
                p.x = p.x * 0.7 - z * 0.55;
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
    mesh.sphere(center, Vec3::new(0.1, 0.16, 0.12), 5, 8, Rgb::hex(0x6B3B1E));
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
}

//! A7FL player figures: sleeveless jersey, shorts, bare arms and no helmet.
//!
//! Players are built from a handful of boxes, cylinders and spheres in local space
//! (facing +x, feet at z=0) and then placed in the world. The silhouette is
//! deliberately slimmer than a padded football player because there are no
//! shoulder pads in the A7FL.

use a7fl_core::math::{Rgb, Vec3};
use a7fl_core::team::{HairStyle, JerseyPattern, Player, Uniform};

use crate::scene::Mesh;

#[derive(Clone, Copy, Debug, Default)]
pub struct Pose {
    /// 0..1 run cycle.
    pub run_phase: f32,
    /// 0..1 how fast they're moving (drives lean and stride).
    pub speed: f32,
    pub carrying: bool,
    pub blocking: bool,
    pub down: bool,
    pub celebrating: f32,
    /// 0 = facing, 1 = lateral shuffle.
    pub strafe: f32,
}

fn limb(mesh: &mut Mesh, from: Vec3, to: Vec3, r0: f32, r1: f32, sides: usize, color: Rgb) {
    mesh.cylinder(from, to, r0, r1, sides, color);
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

    let leg_len = h * 0.47;
    let torso_len = h * 0.32;
    let head_r = h * 0.062;
    let hip_z = leg_len;
    let shoulder_z = hip_z + torso_len;

    let running = pose.speed.clamp(0.0, 1.0);
    let lean = running * 0.22 + if pose.blocking { 0.32 } else { 0.0 };
    let crouch = if pose.blocking { h * 0.09 } else { 0.0 };

    let swing = (pose.run_phase * std::f32::consts::TAU * 2.0).sin() * (0.25 + running * 0.55);
    let counter = (pose.run_phase * std::f32::consts::TAU * 2.0 + std::f32::consts::PI).sin()
        * (0.25 + running * 0.55);

    // ---------------- legs ----------------
    let thigh_len = leg_len * 0.52;
    let shin_len = leg_len * 0.48;
    for (side, phase) in [(-1.0f32, swing), (1.0f32, counter)] {
        let hip = Vec3::new(0.0, side * shoulder_w * 0.22, hip_z - crouch);
        let thigh_angle = phase;
        let knee_bend = 0.35 + running * 0.7 * phase.abs();
        let dir = Vec3::new(thigh_angle.sin(), 0.0, -thigh_angle.cos());
        let knee = hip + dir * thigh_len;
        let shin_dir = Vec3::new(
            (thigh_angle - knee_bend).sin(),
            0.0,
            -(thigh_angle - knee_bend).cos(),
        );
        let ankle = knee + shin_dir * shin_len;
        // Thigh (shorts cover the top half).
        limb(&mut mesh, hip, hip.lerp(knee, 0.55), 0.085, 0.075, 6, shorts);
        limb(&mut mesh, hip.lerp(knee, 0.55), knee, 0.072, 0.062, 6, skin);
        // Calf with a sock.
        let sock_top = knee.lerp(ankle, 0.45);
        limb(&mut mesh, knee, sock_top, 0.062, 0.052, 6, skin);
        limb(&mut mesh, sock_top, ankle, 0.052, 0.044, 6, socks);
        // Shoe.
        let toe = ankle + Vec3::new(0.14, 0.0, -0.02);
        mesh.box_yaw(
            Vec3::new(ankle.x + 0.06, ankle.y, ankle.z - 0.03),
            Vec3::new(0.11, 0.052, 0.035),
            0.0,
            shoes,
        );
        mesh.box_yaw(toe, Vec3::new(0.05, 0.045, 0.02), 0.0, shoes);
    }

    // ---------------- shorts / hips ----------------
    let pelvis = Vec3::new(0.0, 0.0, hip_z - crouch + 0.02);
    mesh.box_yaw(pelvis, Vec3::new(0.155, shoulder_w * 0.5, h * 0.065), 0.0, shorts);
    // Side stripes.
    for side in [-1.0f32, 1.0] {
        mesh.box_yaw(
            Vec3::new(0.0, side * shoulder_w * 0.5, hip_z - crouch + 0.02),
            Vec3::new(0.14, 0.012, h * 0.055),
            0.0,
            uniform.shorts_accent,
        );
    }

    // ---------------- torso ----------------
    let torso_center = Vec3::new(
        lean * 0.12,
        0.0,
        hip_z - crouch + torso_len * 0.5,
    );
    let torso_half = Vec3::new(0.13, shoulder_w * 0.5, torso_len * 0.5);
    mesh.box_yaw(torso_center, torso_half, lean * 0.5, jersey);

    // Jersey trim along the bottom hem and the armholes.
    mesh.box_yaw(
        Vec3::new(torso_center.x, 0.0, torso_center.z - torso_half.z + 0.01),
        Vec3::new(torso_half.x + 0.01, torso_half.y + 0.005, 0.012),
        0.0,
        accent,
    );

    match uniform.pattern {
        JerseyPattern::ShoulderStripe => {
            for side in [-1.0f32, 1.0] {
                mesh.box_yaw(
                    Vec3::new(
                        torso_center.x,
                        side * (torso_half.y - 0.02),
                        torso_center.z + torso_half.z * 0.45,
                    ),
                    Vec3::new(torso_half.x + 0.008, 0.03, torso_half.z * 0.42),
                    0.0,
                    accent,
                );
            }
        }
        JerseyPattern::SidePanel => {
            for side in [-1.0f32, 1.0] {
                mesh.box_yaw(
                    Vec3::new(
                        torso_center.x,
                        side * torso_half.y,
                        torso_center.z,
                    ),
                    Vec3::new(torso_half.x * 0.9, 0.012, torso_half.z * 0.85),
                    0.0,
                    accent,
                );
            }
        }
        JerseyPattern::Sash => {
            mesh.box_yaw(
                Vec3::new(torso_center.x - 0.005, 0.0, torso_center.z - 0.02),
                Vec3::new(torso_half.x + 0.01, torso_half.y * 0.85, torso_half.z * 0.18),
                0.0,
                accent,
            );
        }
        JerseyPattern::Hoops => {
            for i in -1..=1 {
                mesh.box_yaw(
                    Vec3::new(
                        torso_center.x,
                        0.0,
                        torso_center.z + i as f32 * torso_half.z * 0.5,
                    ),
                    Vec3::new(torso_half.x + 0.01, torso_half.y + 0.004, torso_half.z * 0.12),
                    0.0,
                    accent,
                );
            }
        }
        JerseyPattern::Solid => {}
    }

    // Number plate front and back (reads as a number at broadcast distance).
    let number_color = uniform.readable_number_color();
    mesh.box_yaw(
        Vec3::new(torso_center.x + torso_half.x + 0.004, 0.0, torso_center.z + 0.02),
        Vec3::new(0.006, torso_half.y * 0.42, torso_half.z * 0.42),
        0.0,
        number_color,
    );
    mesh.box_yaw(
        Vec3::new(torso_center.x - torso_half.x - 0.004, 0.0, torso_center.z + 0.02),
        Vec3::new(0.006, torso_half.y * 0.5, torso_half.z * 0.5),
        0.0,
        number_color,
    );

    // ---------------- shoulders & arms (bare skin: no pads) ----------------
    let shoulder_z = shoulder_z - crouch;
    for (side, phase) in [(-1.0f32, counter), (1.0f32, swing)] {
        let shoulder = Vec3::new(lean * 0.14, side * shoulder_w * 0.52, shoulder_z - 0.04);
        // Sleeve cap.
        mesh.sphere(shoulder, Vec3::new(0.075, 0.075, 0.07), 3, 6, jersey);
        let arm_angle = phase * 0.9 + if pose.carrying && side > 0.0 { -0.5 } else { 0.0 };
        let upper_len = h * 0.17;
        let fore_len = h * 0.16;
        let elbow = shoulder
            + Vec3::new(
                arm_angle.sin() * 0.35,
                side * 0.06,
                -upper_len,
            );
        let hand = elbow + Vec3::new(arm_angle.sin() * 0.5, side * 0.02, -fore_len);
        limb(&mut mesh, shoulder, elbow, 0.052, 0.043, 5, skin);
        limb(&mut mesh, elbow, hand, 0.043, 0.034, 5, skin);

        // Ball in the carrying hand.
        if pose.carrying && side > 0.0 {
            let ball_center = hand + Vec3::new(0.05, 0.0, 0.07);
            mesh.sphere(ball_center, Vec3::new(0.055, 0.095, 0.075), 4, 8, Rgb::hex(0x6B3B1E));
            mesh.box_yaw(
                ball_center + Vec3::new(0.0, 0.0, 0.09),
                Vec3::new(0.012, 0.012, 0.006),
                0.0,
                Rgb::WHITE,
            );
        }
    }

    // ---------------- head, hair, beard ----------------
    let neck = Vec3::new(lean * 0.18, 0.0, shoulder_z + head_r * 0.6);
    limb(&mut mesh, Vec3::new(lean * 0.16, 0.0, shoulder_z - 0.02), neck, 0.055, 0.05, 5, skin);
    let head = Vec3::new(lean * 0.2, 0.0, shoulder_z + head_r * 1.55);
    mesh.sphere(head, Vec3::new(head_r, head_r * 0.92, head_r * 1.1), 4, 8, skin);
    // Ears.
    for side in [-1.0f32, 1.0] {
        mesh.sphere(
            Vec3::new(head.x - 0.01, side * head_r * 0.95, head.z),
            Vec3::new(0.018, 0.02, 0.03),
            2,
            5,
            skin,
        );
    }
    // Hair.
    match player.hair {
        HairStyle::Bald => {}
        HairStyle::Buzz => {
            mesh.sphere(
                Vec3::new(head.x - 0.005, 0.0, head.z + head_r * 0.42),
                Vec3::new(head_r * 1.0, head_r * 0.94, head_r * 0.68),
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
                Vec3::new(head_r * 1.05, head_r * 1.0, head_r * 0.78),
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
            mesh.box_yaw(
                Vec3::new(head.x - 0.01, 0.0, head.z + head_r * 1.1),
                Vec3::new(head_r * 0.85, head_r * 0.85, head_r * 0.55),
                0.0,
                player.hair_color,
            );
        }
    }
    // Beard.
    if player.beard > 0.2 {
        mesh.box_yaw(
            Vec3::new(head.x + head_r * 0.62, 0.0, head.z - head_r * 0.45),
            Vec3::new(head_r * 0.28, head_r * 0.72, head_r * 0.42),
            0.0,
            player.hair_color,
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

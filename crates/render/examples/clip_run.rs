//! The keyed stride laid beside a captured one.
//!
//! ```text
//! cargo run --release -p nopads_render --features authored --example clip_run -- artifacts/hero.glb
//! ```
//!
//! The imported body is not drawn here at all. What comes out of the file is the
//! *motion*: the limb angles are measured off the rig's joints at four points in
//! a run cycle and handed to the builder that already draws every player on the
//! field. The top row is the hand-keyed cycle, the bottom row is the capture, and
//! the man is identical in both.
//!
//! Writes `artifacts/clip_run.png`.

use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use nopads_core::math::{Rgb, Vec3};
use nopads_core::team::league_teams;
use nopads_render::authored::Hero;
use nopads_render::players::{self, Detail, Pose};
use nopads_render::raster::Frame;
use nopads_render::scene::{Camera, SceneRenderer};
use nopads_render::write_png;

const TILE_W: usize = 300;
const TILE_H: usize = 640;
const PHASES: [f32; 4] = [0.0, 0.25, 0.5, 0.75];

fn main() {
    let path = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("A7FL_HERO_GLB").ok())
        .unwrap_or_else(|| "artifacts/hero.glb".to_string());
    let hero = match Hero::load(&path) {
        Ok(h) => Arc::new(h),
        Err(e) => {
            eprintln!("{path}: {e}");
            std::process::exit(1);
        }
    };
    let clip = match hero
        .clip_index("Running")
        .or_else(|| hero.clip_index("run_fast_10_inplace"))
        .or_else(|| hero.clip_index("Walking"))
    {
        Some(c) => c,
        None => {
            eprintln!("{path}: no run or walk clip in this file");
            std::process::exit(1);
        }
    };
    let duration = hero.clips[clip].duration;
    println!(
        "clip {} ({:.3}s), {} joints",
        hero.clips[clip].name,
        duration,
        hero.skin_joints.len()
    );

    // What the retarget actually measured, so the numbers can be read as well as
    // looked at. A thigh that swings about half a radian either way and a knee
    // that folds to nearly two is a run; anything else is a rig this does not
    // understand.
    for ph in PHASES {
        match hero.limb_angles(clip, ph * duration) {
            Some(a) => println!(
                "  t={:.2}  thigh {:+.2}/{:+.2}  knee {:.2}/{:.2}  foot {:.2}/{:.2}  arm {:+.2}/{:+.2}  elbow {:.2}/{:.2}  lean {:+.2}",
                ph,
                a.legs[0][0], a.legs[1][0],
                a.legs[0][1], a.legs[1][1],
                a.legs[0][2], a.legs[1][2],
                a.arms[0][0], a.arms[1][0],
                a.arms[0][1], a.arms[1][1],
                a.lean
            ),
            None => println!("  t={:.2}  the rig has no mixamo joints to read", ph),
        }
    }

    let teams = league_teams();
    let team = &teams[0];
    let player = &team.roster[0];
    let uniform = &team.home;

    let mut sheet = Frame::new(TILE_W * PHASES.len(), TILE_H * 2);
    sheet.clear(Rgb::hex(0xC8CCD2));
    for (i, ph) in PHASES.iter().enumerate() {
        // Top row: the keyed cycle the game ships with.
        let keyed = Pose {
            run_phase: *ph,
            speed: 1.0,
            ..Default::default()
        };
        draw(&mut sheet, i, 0, player, uniform, &keyed);
        // Bottom row: the same man, driven by the clip.
        let limbs = hero.limb_angles(clip, ph * duration);
        let carried = Pose {
            run_phase: *ph,
            speed: 1.0,
            limbs,
            ..Default::default()
        };
        draw(&mut sheet, i, 1, player, uniform, &carried);
    }

    let _ = std::fs::create_dir_all("artifacts");
    match write_png("artifacts/clip_run.png", &sheet) {
        Ok(()) => println!("wrote artifacts/clip_run.png ({}x{})", sheet.w, sheet.h),
        Err(e) => eprintln!("png failed: {e}"),
    }
}

fn draw(
    sheet: &mut Frame,
    col: usize,
    row: usize,
    player: &nopads_core::team::Player,
    uniform: &nopads_core::team::Uniform,
    pose: &Pose,
) {
    let mut frame = Frame::new(TILE_W, TILE_H);
    frame.clear(Rgb::hex(0xC8CCD2));
    let mut mesh = players::build_mesh_at(player, uniform, pose, player.id as u32, Detail::Close);
    // Side on, so the stride is the thing being looked at.
    mesh.place(Vec3::ZERO, FRAC_PI_2, 1.0);
    let scene = SceneRenderer::new(TILE_W, TILE_H, 34f32.to_radians());
    let cam = Camera::new(Vec3::new(-3.6, 0.0, 0.95), 0.0, -0.02);
    scene.draw_mesh(&mut frame, &mesh, &cam);
    for y in 0..TILE_H {
        for x in 0..TILE_W {
            sheet.color[(row * TILE_H + y) * sheet.w + col * TILE_W + x] =
                frame.color[y * TILE_W + x];
        }
    }
}

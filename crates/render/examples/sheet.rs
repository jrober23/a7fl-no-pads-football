//! A contact sheet of one figure from four sides, the way the art sheets show it.
//!
//! `cargo run --release -p nopads_render --example sheet` writes
//! `artifacts/sheet.png`. It is the only honest way to work on a player model:
//! on the field a man is forty pixels tall, so he can be improved or ruined
//! without anybody being able to see which.

use std::f32::consts::{FRAC_PI_2, PI};

use nopads_core::math::{Rgb, Vec3};
use nopads_core::team::league_teams;
use nopads_render::players::{self, Detail, Pose};
use nopads_render::raster::Frame;
use nopads_render::scene::{Camera, SceneRenderer};
use nopads_render::write_png;

const TILE_W: usize = 300;
const TILE_H: usize = 640;

fn main() {
    let teams = league_teams();
    let team = &teams[0];
    // A sheet is for looking at a face, so take a man who is not masked.
    let player = team
        .roster
        .iter()
        .find(|p| p.number == 2 && !p.balaclava)
        .or_else(|| team.roster.iter().find(|p| !p.balaclava))
        .unwrap_or(&team.roster[0]);
    let uniform = &team.home;
    let pose = Pose {
        run_phase: 0.0,
        speed: 0.0,
        carrying: false,
        blocking: false,
        hands: 0.0,
        arms_up: 0.0,
        down: false,
        celebrating: 0.0,
        strafe: 0.0,
        motion: None,
        motion_progress: 0.0,
        wrapping: 0.0,
        tackled: 0.0,
        limbs: None,
    };

    let views = [("0", 0.0f32), ("90", FRAC_PI_2), ("180", PI), ("270", 3.0 * FRAC_PI_2)];
    let mut sheet = Frame::new(TILE_W * views.len(), TILE_H);
    sheet.clear(Rgb::hex(0xC8CCD2));
    for (i, (_label, heading)) in views.iter().enumerate() {
        let mut frame = Frame::new(TILE_W, TILE_H);
        frame.clear(Rgb::hex(0xC8CCD2));
        let mut mesh =
            players::build_mesh_at(player, uniform, &pose, player.id as u32, Detail::Close);
        mesh.place(Vec3::new(0.0, 0.0, 0.0), *heading, 1.0);
        let scene = SceneRenderer::new(TILE_W, TILE_H, 34f32.to_radians());
        let cam = Camera::new(Vec3::new(-3.6, 0.0, 0.95), 0.0, -0.02);
        scene.draw_mesh(&mut frame, &mesh, &cam);
        for y in 0..TILE_H {
            for x in 0..TILE_W {
                sheet.color[y * sheet.w + i * TILE_W + x] = frame.color[y * TILE_W + x];
            }
        }
    }
    let _ = std::fs::create_dir_all("artifacts");
    match write_png("artifacts/sheet.png", &sheet) {
        Ok(()) => println!("wrote artifacts/sheet.png ({}x{})", sheet.w, sheet.h),
        Err(e) => eprintln!("png failed: {e}"),
    }
}

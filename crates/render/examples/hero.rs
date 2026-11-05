//! An authored character beside the procedural one, at a size you can judge.
//!
//! The broadcast draws a man forty pixels tall, so a model can be wrong in every
//! way and still look fine on the field. This puts it at half the height of the
//! tile, next to the figure we loft ourselves:
//!
//! ```text
//! cargo run --release -p nopads_render --features authored --example hero -- artifacts/hero.glb
//! ```
//!
//! and writes `artifacts/hero_facing.png` (four headings of each figure) and
//! `artifacts/hero_run.png` (four phases of the run).

use std::f32::consts::{FRAC_PI_2, PI};
use std::time::Instant;

use nopads_core::math::{Rgb, Vec3};
use nopads_core::team::league_teams;
use nopads_render::authored::Hero;
use nopads_render::players::{self, Detail, Pose};
use nopads_render::raster::Frame;
use nopads_render::scene::{Camera, Mesh, SceneRenderer};
use nopads_render::write_png;

const TILE_W: usize = 300;
const TILE_H: usize = 640;
const COLS: usize = 4;
/// The authored figure stands 1.70 units tall and the procedural lineman is a
/// shade under 1.9, so this takes the imported one to the same height on the
/// sheet. Nothing in the loader scales anything; it is the caller's job.
const SCALE: f32 = 1.12;
const BACKDROP: u32 = 0xC8CCD2;

fn main() {
    let path = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("A7FL_HERO_GLB").ok())
        .unwrap_or_else(|| "artifacts/hero.glb".to_string());
    let started = Instant::now();
    let hero = match Hero::load(&path) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("{path}: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "loaded {path} in {:?}: {} triangles, {} vertices, {} joints, {} clips, {}x{} atlas, {:.2} units tall",
        started.elapsed(),
        hero.triangle_count(),
        hero.positions.len(),
        hero.skin_joints.len(),
        hero.clips.len(),
        hero.texture.w,
        hero.texture.h,
        hero.height
    );
    for (i, c) in hero.clips.iter().enumerate() {
        println!("  clip {i}: {:<22} {:.3}s", c.name, c.duration);
    }

    let teams = league_teams();
    let team = &teams[0];
    let player = team.roster.iter().find(|p| p.number == 2).unwrap_or(&team.roster[0]);
    let uniform = &team.home;

    let mut sheet = new_sheet(2);
    let mut authored = Vec::with_capacity(COLS);
    let bind = hero.bind_pose();
    let mut built = 0.0f64;
    let mut drew = 0.0f64;
    for col in 0..COLS {
        // A heading of zero points downfield, away from this lens, so the sheet
        // starts half a turn round to look both men in the face.
        let heading = PI + col as f32 * FRAC_PI_2;
        let t0 = Instant::now();
        let mut mesh = hero.skinned_mesh(&bind, SCALE);
        mesh.place(Vec3::ZERO, heading, 1.0);
        built += t0.elapsed().as_secs_f64();
        let t1 = Instant::now();
        authored.push(draw(&mesh));
        drew += t1.elapsed().as_secs_f64();
        let mut ours = players::build_mesh_at(player, uniform, &Pose::default(), player.id as u32, Detail::Close);
        ours.place(Vec3::ZERO, heading, 1.0);
        paste(&mut sheet, &authored[col], col, 0);
        paste(&mut sheet, &draw(&ours), col, 1);
    }
    println!(
        "authored: {:.0} ms to skin a frame, {:.0} ms to draw one",
        built / COLS as f64 * 1000.0,
        drew / COLS as f64 * 1000.0
    );
    write("artifacts/hero_facing.png", &sheet);

    // The run: the same four fractions of the stride for both men. The figures
    // run on different clocks -- the clip is 0.633 s, the procedural cycle is
    // driven by distance -- so the phases line up approximately, not exactly.
    let clip = hero.clip_index("Running").or_else(|| hero.clip_index("RunFast")).unwrap_or(0);
    let duration = hero.clips[clip].duration;
    let mut sheet = new_sheet(2);
    let mut lowest = f32::MAX;
    let mut highest = f32::MIN;
    for step in 0..8 {
        let joints = hero.pose(clip, duration * step as f32 / 8.0);
        let (pos, _) = hero.skin(&joints, SCALE);
        let low = pos.iter().map(|p| p.z).fold(f32::MAX, f32::min);
        lowest = lowest.min(low);
        highest = highest.max(low);
    }
    println!(
        "{} over its cycle: lowest point {:.3} to {:.3} above the turf",
        hero.clips[clip].name, lowest, highest
    );
    for col in 0..COLS {
        let phase = col as f32 / COLS as f32;
        let joints = hero.pose(clip, duration * phase);
        let mut mesh = hero.skinned_mesh(&joints, SCALE);
        mesh.place(Vec3::ZERO, PI, 1.0);
        paste(&mut sheet, &draw(&mesh), col, 0);
        let pose = Pose { run_phase: phase, speed: 1.0, ..Pose::default() };
        let mut ours = players::build_mesh_at(player, uniform, &pose, player.id as u32, Detail::Close);
        ours.place(Vec3::ZERO, PI, 1.0);
        paste(&mut sheet, &draw(&ours), col, 1);
    }
    write("artifacts/hero_run.png", &sheet);
}

fn camera() -> Camera {
    Camera::new(Vec3::new(-3.6, 0.0, 0.95), 0.0, -0.02)
}

fn draw(mesh: &Mesh) -> Frame {
    let mut frame = Frame::new(TILE_W, TILE_H);
    frame.clear(Rgb::hex(BACKDROP));
    let scene = SceneRenderer::new(TILE_W, TILE_H, 34f32.to_radians());
    scene.draw_mesh(&mut frame, mesh, &camera());
    frame
}

fn new_sheet(rows: usize) -> Frame {
    let mut sheet = Frame::new(TILE_W * COLS, TILE_H * rows);
    sheet.clear(Rgb::hex(BACKDROP));
    sheet
}

fn paste(sheet: &mut Frame, tile: &Frame, col: usize, row: usize) {
    for y in 0..TILE_H {
        for x in 0..TILE_W {
            let to = (row * TILE_H + y) * sheet.w + col * TILE_W + x;
            let from = y * TILE_W + x;
            if let (Some(dst), Some(src)) = (sheet.color.get_mut(to), tile.color.get(from)) {
                *dst = *src;
            }
        }
    }
}

fn write(path: &str, sheet: &Frame) {
    let _ = std::fs::create_dir_all("artifacts");
    match write_png(path, sheet) {
        Ok(()) => println!("wrote {path} ({}x{})", sheet.w, sheet.h),
        Err(e) => eprintln!("{path}: {e}"),
    }
}

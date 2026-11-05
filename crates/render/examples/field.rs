//! Every man on the field drawn from the authored character.
//!
//! ```text
//! cargo run --release -p nopads_render --features authored --example field -- artifacts/hero.glb
//! ```
//!
//! The one-man showcase puts the imported figure in place of the nearest running
//! man so a model can be judged in a real frame. This is the other thing: the
//! authored rig as the base for all fourteen men, each wearing his own club's
//! kit, which is repainted from the club's `Uniform` at load time.
//!
//! Writes `artifacts/field_broadcast.png`, `artifacts/field_replay.png`,
//! `artifacts/field_close.png` and `artifacts/field_broadcast_plain.png` (the
//! same frame on the procedural figures, for the comparison), and prints what
//! each frame cost.

use std::f32::consts::PI;
use std::sync::Arc;
use std::time::Instant;

use nopads_core::game::{GameState, PlayRecord, ScrimmageRecord};
use nopads_core::sim::PlaySim;
use nopads_core::team::{league_teams, Team};
use nopads_render::authored::{AuthoredField, Hero};
use nopads_render::broadcast::{Broadcast, CameraMode, HandCamera};
use nopads_render::write_png;

const W: usize = 1280;
const H: usize = 720;
const SEED: u64 = 0xA7F1_5EED;
/// A man on this field stands about 1.9 units tall.
const HEIGHT: f32 = 1.95;

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
    println!(
        "loaded {path}: {} triangles, {} vertices, {} joints, {} clips",
        hero.triangle_count(),
        hero.positions.len(),
        hero.skin_joints.len(),
        hero.clips.len()
    );

    let teams = league_teams();
    let mut game = GameState::new(teams[0].clone(), teams[1].clone(), SEED);
    let mut found: Option<ScrimmageRecord> = None;
    let mut fallback: Option<ScrimmageRecord> = None;
    for _ in 0..40 {
        let Some(rec) = game.simulate_next() else { break };
        if let PlayRecord::Scrimmage(r) = &rec {
            // A play with a runner in it, not a three-yard dive at the pile.
            if r.result.elapsed > 2.5 && !r.result.touchdown {
                if fallback.is_none() {
                    fallback = Some(r.clone());
                }
                if r.result.yards >= 6.0 {
                    found = Some(r.clone());
                    break;
                }
            }
        }
    }
    let Some(record) = found.or(fallback) else {
        eprintln!("no play with a run in it turned up in forty snaps");
        return;
    };
    let (t, peak) = peak_run(&teams, &record);
    println!(
        "{}\n  peak run {:.2}s into a {:.1}s play, at {:.1} yards a second",
        record.result.description, t, record.result.elapsed, peak
    );

    let rec = PlayRecord::Scrimmage(record.clone());
    let mut broadcast = Broadcast::new(W, H, [teams[0].clone(), teams[1].clone()]);
    // `Uniform` is `Copy`, so the kits can be built from the renderer's own copy
    // without borrowing it across the mutation that installs the field.
    let uniforms = broadcast.uniforms;
    let field = AuthoredField::new(Arc::clone(&hero), HEIGHT, [&uniforms[0], &uniforms[1]]);
    println!(
        "kits: {} / {} at {}x{}",
        teams[0].name, teams[1].name, field.kits[0].w, field.kits[0].h
    );
    broadcast.set_authored_field(Some(field));

    // Warm before timing: the first frame finds room for fourteen figures and
    // paints two club atlases, and neither is the cost of a frame.
    let _ = broadcast.frame_at(&game, &rec, t, CameraMode::Broadcast);

    let start = Instant::now();
    let frame = broadcast.frame_at(&game, &rec, t, CameraMode::Broadcast);
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    println!("broadcast frame, fourteen authored men: {ms:.0} ms");
    save("artifacts/field_broadcast.png", &frame);

    let start = Instant::now();
    let replay = broadcast.frame_at(&game, &rec, t, CameraMode::Replay);
    println!("replay rail: {:.0} ms", start.elapsed().as_secs_f64() * 1000.0);
    save("artifacts/field_replay.png", &replay);

    // The close-up. The hand camera belongs to the replay, and the replay draws
    // through `live_frame`; `frame_at` runs on the rail and ignores the hand.
    let sim = step_to(&teams, &record, t);
    let spot = match sim.carrier {
        Some(i) => sim.players[i].pos,
        None => sim.ball_position().0,
    };
    broadcast.set_hand_camera(Some(HandCamera { at: spot, zoom: 0.5, yaw: PI }));
    let start = Instant::now();
    let close = broadcast.live_frame(&sim, CameraMode::Replay);
    println!("close frame: {:.0} ms", start.elapsed().as_secs_f64() * 1000.0);
    save("artifacts/field_close.png", &close);

    // The same frame on the procedural figures, so the change is a picture rather
    // than an opinion.
    broadcast.set_authored_field(None);
    broadcast.set_hand_camera(None);
    let plain = broadcast.frame_at(&game, &rec, t, CameraMode::Broadcast);
    save("artifacts/field_broadcast_plain.png", &plain);
}

/// When the ball carrier was moving fastest, and how fast.
fn peak_run(teams: &[Team], r: &ScrimmageRecord) -> (f32, f32) {
    let off = r.setup.offense_team;
    let def = r.setup.defense_team;
    let mut sim = PlaySim::new(&teams[off], &teams[def], &r.play, &r.defense, &r.setup);
    let dt = 1.0 / 60.0;
    let mut elapsed = 0.0;
    let mut best = (0.0f32, 0.0f32);
    while sim.result.is_none() && elapsed < 15.0 {
        sim.step(dt);
        elapsed += dt;
        if let Some(carrier) = sim.carrier {
            let speed = sim.players[carrier].vel.length();
            if speed > best.1 {
                best = (elapsed, speed);
            }
        }
    }
    best
}

/// The play, stepped to `t`.
fn step_to<'a>(teams: &'a [Team], r: &'a ScrimmageRecord, t: f32) -> PlaySim<'a> {
    let off = r.setup.offense_team;
    let def = r.setup.defense_team;
    let mut sim = PlaySim::new(&teams[off], &teams[def], &r.play, &r.defense, &r.setup);
    let dt = 1.0 / 60.0;
    let mut elapsed = 0.0;
    while elapsed < t && sim.result.is_none() && elapsed < 15.0 {
        sim.step(dt);
        elapsed += dt;
    }
    sim
}

fn save(path: &str, frame: &nopads_render::Frame) {
    match write_png(path, frame) {
        Ok(()) => println!("wrote {path} ({}x{})", frame.w, frame.h),
        Err(e) => eprintln!("{path}: {e}"),
    }
}

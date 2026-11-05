//! A real broadcast frame with the authored figure in it.
//!
//! ```text
//! cargo run --release -p nopads_render --features authored --example hero_shot -- artifacts/hero.glb
//! ```
//!
//! writes two frames: the replay rail, which is the shot the game actually shows,
//! and a hand camera in close, which is the shot 170,000 triangles are for. It
//! renders each of them twice, with the authored figure and without, so the cost
//! of the swap is a number rather than an opinion.

use std::f32::consts::PI;
use std::sync::Arc;
use std::time::Instant;

use nopads_core::game::{GameState, PlayRecord, ScrimmageRecord};
use nopads_core::sim::PlaySim;
use nopads_core::team::{league_teams, Team};
use nopads_render::authored::{Hero, HeroActor};
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
    // The shot is taken where the ball carrier is going fastest: a hero standing
    // at the end of a tackle is a man in a nice uniform doing nothing.
    let (t, peak) = peak_run(&teams, &record);
    println!(
        "{}\n  peak run {:.2}s into a {:.1}s play, at {:.1} yards a second",
        record.result.description, t, record.result.elapsed, peak
    );

    let rec = PlayRecord::Scrimmage(record.clone());
    let mut broadcast = Broadcast::new(W, H, [teams[0].clone(), teams[1].clone()]);

    // Warm both paths before timing either: the first frame of each finds room
    // for fourteen figures, and that is not the cost of the hero.
    broadcast.set_hero(Some(HeroActor::new(Arc::clone(&hero), HEIGHT)));
    let _ = broadcast.frame_at(&game, &rec, t, CameraMode::Replay);
    broadcast.set_hero(None);
    let _ = broadcast.frame_at(&game, &rec, t, CameraMode::Replay);

    broadcast.set_hero(Some(HeroActor::new(Arc::clone(&hero), HEIGHT)));
    let start = Instant::now();
    let with_hero = broadcast.frame_at(&game, &rec, t, CameraMode::Replay);
    let hero_ms = start.elapsed().as_secs_f64() * 1000.0;
    broadcast.set_hero(None);
    let start = Instant::now();
    let without = broadcast.frame_at(&game, &rec, t, CameraMode::Replay);
    let plain_ms = start.elapsed().as_secs_f64() * 1000.0;
    println!("replay frame: {hero_ms:.0} ms with the hero, {plain_ms:.0} ms without");
    save("artifacts/shot_replay.png", &with_hero);
    save("artifacts/shot_replay_plain.png", &without);

    // The close-up. The hand camera is the replay's camera and the replay draws
    // through `live_frame`; `frame_at` runs on the rail and ignores the hand.
    let sim = step_to(&teams, &record, t);
    let spot = match sim.carrier {
        Some(i) => sim.players[i].pos,
        None => sim.ball_position().0,
    };
    broadcast.set_hero(Some(HeroActor::new(Arc::clone(&hero), HEIGHT)));
    // Zoom is the multiplier on the replay's seven and a half yards: half of it
    // puts the lens four yards out, which fills the frame with one man. The yaw
    // turns the shot round in front of him, so the run comes at the camera.
    broadcast.set_hand_camera(Some(HandCamera { at: spot, zoom: 0.5, yaw: PI }));
    let start = Instant::now();
    let close = broadcast.live_frame(&sim, CameraMode::Replay);
    println!("close frame: {:.0} ms", start.elapsed().as_secs_f64() * 1000.0);
    save("artifacts/shot_hand.png", &close);
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
///
/// The close-up has to come round this way: the hand camera belongs to the
/// replay, and the replay draws through `live_frame`. `frame_at` is the rail and
/// ignores the hand entirely.
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

//! Where the frame goes: the middle of the distribution, the tail, and which
//! shot pays for it.
//!
//! ```text
//! cargo run --release -p nopads_render --example perf [venue] [frames-per-shot]
//! ```
//!
//! A steady cost shows up as a steady frame; a stutter shows up as a tail. The
//! game ships the same renderer as this, unoptimised for the harness, so the
//! numbers here are the numbers a player gets from the picture -- on a software
//! rasteriser, without a window present to hide them.

use std::time::Instant;

use nopads_core::game::{GameState, PlayRecord};
use nopads_core::sim::PlaySim;
use nopads_render::broadcast::{Broadcast, CameraMode};
use nopads_render::passing::PassUi;
use nopads_render::venues::Venue;

const W: usize = 1280;
const H: usize = 720;
const DT: f32 = 1.0 / 60.0;

fn ui_none() -> PassUi {
    PassUi {
        flag: None,
        focus: None,
        badges: Vec::new(),
        guide: None,
        power: None,
        short: false,
        needed: None,
    }
}

/// The value at `p` of the way through a sorted copy.
fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i]
}

fn report(name: &str, mut v: Vec<f64>, budget: f64) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mean = v.iter().sum::<f64>() / v.len().max(1) as f64;
    let over = v.iter().filter(|x| **x > budget).count();
    println!(
        "{name:<12} mean {mean:6.1}  p50 {:6.1}  p90 {:6.1}  p99 {:6.1}  max {:6.1}   over 16.6 ms: {over}/{}",
        pct(&v, 0.50),
        pct(&v, 0.90),
        pct(&v, 0.99),
        v.last().copied().unwrap_or(0.0),
        v.len(),
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    let venue_index: usize = args
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let frames: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(240);

    let league = nopads_core::team::league_teams();
    let mut game = GameState::new(league[0].clone(), league[1].clone(), 12);
    let mut record = None;
    while let Some(r) = game.simulate_next() {
        if matches!(r, PlayRecord::Scrimmage(_)) {
            record = Some(r);
            break;
        }
    }
    let Some(PlayRecord::Scrimmage(call)) = record else {
        panic!("no scrimmage snap to play");
    };
    let teams = [game.teams[0].clone(), game.teams[1].clone()];
    let venue = Venue::from_index(venue_index);
    println!(
        "{} at {}x{}, {} frames a shot, {} threads",
        venue.name(),
        W,
        H,
        frames,
        nopads_render::scene::threads()
    );

    let mut live = Broadcast::with_venue(W, H, teams.clone(), venue);
    for mode in [
        CameraMode::Broadcast,
        CameraMode::Classic,
        CameraMode::BehindQb,
        CameraMode::SkyCam,
        CameraMode::Replay,
        CameraMode::Endzone,
    ] {
        let mut sim = PlaySim::new(
            &teams[call.setup.offense_team],
            &teams[call.setup.defense_team],
            &call.play,
            &call.defense,
            &call.setup,
        );
        let mut renders = Vec::with_capacity(frames);
        let mut steps = Vec::with_capacity(frames);
        // A couple of frames are the camera easing in from wherever the last
        // shot left it: those are the join, not the steady state.
        for _ in 0..2 {
            live.live_frame_ui(&sim, mode, &ui_none());
        }
        for _ in 0..frames {
            let t = Instant::now();
            sim.step(DT);
            steps.push(t.elapsed().as_secs_f64() * 1000.0);
            let t = Instant::now();
            let frame = live.live_frame_ui(&sim, mode, &ui_none());
            // The pixels have to be read for the pass to be a pass: a rasteriser
            // that never touches its buffer is not drawing.
            std::hint::black_box(&frame);
            renders.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        report(&format!("{mode:?}"), renders, 16.6);
    }

    // The simulation on its own, so a frame's cost can be put against it.
    let mut sim = PlaySim::new(
        &teams[call.setup.offense_team],
        &teams[call.setup.defense_team],
        &call.play,
        &call.defense,
        &call.setup,
    );
    let mut steps = Vec::with_capacity(1200);
    for _ in 0..1200 {
        let t = Instant::now();
        sim.step(DT);
        steps.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    report("sim step", steps, 16.6);
}

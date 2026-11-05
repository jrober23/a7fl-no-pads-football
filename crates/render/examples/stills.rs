//! Stills of the pictures that were only measured: a defender's lunge, a
//! three-man wrap, and the ground on a yacht.
//!
//! Each is a real simulation state, stepped through the same code the game
//! plays, and then painted by the broadcast renderer. Nothing here cheats a
//! pose into place: the lunge is driven by the dive button, and the pile is a
//! wrap that the sim started itself.

use nopads_core::game::{GameState, PlayRecord};
use nopads_core::math::Vec2;
use nopads_core::sim::{DefControl, PlaySim};
use nopads_render::broadcast::{Broadcast, CameraMode};
use nopads_render::capture::write_png;
use nopads_render::venues::{Kind, Venue};

const W: usize = 1280;
const H: usize = 720;
const DT: f32 = 1.0 / 60.0;

fn main() {
    let league = nopads_core::team::league_teams();
    let mut game = GameState::new(league[0].clone(), league[1].clone(), 12);
    let mut record = None;
    while let Some(r) = game.simulate_next() {
        if matches!(r, PlayRecord::Scrimmage(_)) {
            record = Some(r);
            break;
        }
    }
    let record = record.expect("a scrimmage snap");
    let teams = [game.teams[0].clone(), game.teams[1].clone()];
    let PlayRecord::Scrimmage(call) = &record else {
        return;
    };
    let fresh = || {
        PlaySim::new(
            &teams[call.setup.offense_team],
            &teams[call.setup.defense_team],
            &call.play,
            &call.defense,
            &call.setup,
        )
    };

    // ---------------------------------------------------------------- the lunge
    let mut sim = fresh();
    let mut guard = 0;
    while sim.carrier.is_none() && sim.result.is_none() && guard < 900 {
        sim.step(DT);
        guard += 1;
    }
    let carrier = sim.carrier.expect("the handoff");
    sim.set_human_defender(Some(7));
    // Three and a half yards off him: far enough that the lunge is still in the
    // air in this frame and has not turned into the tackle.
    sim.players[7].pos = sim.players[carrier].pos + Vec2::new(2.6, 2.3);
    sim.players[7].stun = 0.0;
    sim.players[7].down = false;
    sim.players[7].dive_t = 0.0;
    sim.players[7].tackle_cooldown = 0.0;
    for _ in 0..9 {
        sim.set_def_control(Some(DefControl {
            dive: true,
            ..Default::default()
        }));
        sim.step(DT);
    }
    println!(
        "lunge: dive_t {:.2}, {} yards from the carrier",
        sim.players[7].dive_t,
        sim.players[7].pos.dist(sim.players[carrier].pos)
    );
    let mut live = Broadcast::new(W, H, teams.clone());
    let _ = write_png(
        "out/stills/01_defense_lunge_broadcast.png",
        &live.live_frame(&sim, CameraMode::Broadcast),
    );
    let _ = write_png(
        "out/stills/02_defense_lunge_classic.png",
        &live.live_frame(&sim, CameraMode::Classic),
    );
    // Low and tight, where an arm either is or is not out in front of him.
    let _ = write_png(
        "out/stills/02b_defense_lunge_replay.png",
        &live.live_frame(&sim, CameraMode::Replay),
    );

    // ------------------------------------------------------------ the pile-up
    let mut sim = fresh();
    let mut guard = 0;
    while sim.carrier.is_none() && sim.result.is_none() && guard < 900 {
        sim.step(DT);
        guard += 1;
    }
    let carrier = sim.carrier.expect("the handoff");
    guard = 0;
    while sim.wrap.is_none() && sim.result.is_none() && guard < 300 {
        sim.players[7].pos = sim.players[carrier].pos + Vec2::new(0.3, 0.0);
        sim.players[7].stun = 0.0;
        sim.players[7].down = false;
        sim.players[7].tackle_cooldown = 0.0;
        sim.step(DT);
        guard += 1;
    }
    assert!(sim.wrap.is_some(), "no wrap to photograph");
    // Two more arriving from the same side, which is how a pursuit actually
    // gets there: the spread round the body is the sim's doing, not theirs.
    sim.players[8].pos = sim.players[carrier].pos + Vec2::new(0.6, 1.5);
    sim.players[9].pos = sim.players[carrier].pos + Vec2::new(1.4, 1.2);
    for _ in 0..12 {
        if sim.result.is_some() {
            break;
        }
        sim.step(DT);
    }
    let (a, b) = (
        sim.players[8].pos.dist(sim.players[carrier].pos),
        sim.players[9].pos.dist(sim.players[carrier].pos),
    );
    println!("pile: helpers at {a:.2} and {b:.2} yards");
    let _ = write_png(
        "out/stills/03_gang_tackle_broadcast.png",
        &live.live_frame(&sim, CameraMode::Broadcast),
    );
    let _ = write_png(
        "out/stills/04_gang_tackle_classic.png",
        &live.live_frame(&sim, CameraMode::Classic),
    );
    let _ = write_png(
        "out/stills/04b_gang_tackle_replay.png",
        &live.live_frame(&sim, CameraMode::Replay),
    );

    // ----------------------------------------------------------------- the boat
    let venue = (0..nopads_render::venues::COUNT)
        .map(Venue::from_index)
        .find(|v| v.kind() == Kind::Yacht)
        .expect("the yacht is in the list");
    let mut sea = fresh();
    for _ in 0..40 {
        sea.step(DT);
    }
    let mut boat = Broadcast::with_venue(W, H, teams.clone(), venue);
    let _ = write_png(
        "out/stills/05_yacht_sideline.png",
        &boat.live_frame(&sea, CameraMode::Broadcast),
    );
    let _ = write_png(
        "out/stills/06_yacht_endzone.png",
        &boat.live_frame(&sea, CameraMode::Endzone),
    );
    let _ = write_png(
        "out/stills/07_yacht_sky.png",
        &boat.live_frame(&sea, CameraMode::SkyCam),
    );
    println!("wrote out/stills");
}

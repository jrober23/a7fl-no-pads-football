//! The weight of a throw: a tap is a floater, a hold is a rope.
//!
//! The pass meter is supposed to be a range of passes rather than one pass thrown
//! harder, so these check the two halves of the range against each other: the soft
//! end has to be slow and lofted, and the hard end has to be the only way to get
//! the ball downfield.

use a7fl_core::math::Vec2;
use a7fl_core::playbook::{defensive_playbook, playbook};
use a7fl_core::sim::{PlaySim, QbControl, SnapSetup, SNAP_DELAY};
use a7fl_core::team::{league_teams, Team};

fn setup(seed: u64) -> SnapSetup {
    SnapSetup {
        los: 40.0,
        ball_y: 0.0,
        direction: 1,
        down: 1,
        to_go: 10.0,
        seed,
        offense_team: 0,
        defense_team: 1,
        is_try: false,
    }
}

fn sim_for<'a>(off: &'a Team, def: &'a Team, seed: u64) -> PlaySim<'a> {
    let book = playbook();
    let defs = defensive_playbook();
    let play = book
        .iter()
        .find(|p| p.family.is_pass())
        .expect("a pass play")
        .clone();
    PlaySim::new(off, def, &play, &defs[0], &setup(seed))
}

/// The deepest man on a route, so the two ends of the meter are asked different
/// questions rather than both dumping it to the same short receiver.
fn deep_target(sim: &PlaySim<'_>) -> usize {
    let qb = sim.qb_pos();
    sim.eligible_targets()
        .into_iter()
        .map(|(_, i)| i)
        .max_by(|a, b| {
            let da = (sim.players[*a].pos - qb).length();
            let db = (sim.players[*b].pos - qb).length();
            da.total_cmp(&db)
        })
        .expect("a receiver")
}

/// Throw to the deepest man at this meter weight, and report how the ball
/// actually travelled: how fast it left the hand, its apex, and the yards it was
/// aimed at.
fn thrown(seed: u64, power: f32) -> (f32, f32, f32) {
    let teams = league_teams();
    let mut sim = sim_for(&teams[0], &teams[1], seed);
    while sim.t < SNAP_DELAY + 0.25 && sim.result.is_none() {
        sim.step(1.0 / 60.0);
    }
    let target = deep_target(&sim);
    sim.set_control(Some(QbControl {
        target: Some(target),
        throw: true,
        power,
        ..Default::default()
    }));
    sim.step(1.0 / 60.0);
    let flight = sim.ball.flight.clone().expect("the ball is in the air");
    (flight.speed, flight.peak, flight.air_yards)
}

#[test]
fn a_tap_is_a_slower_ball_than_a_hold() {
    for seed in 0..6 {
        let (tap, _, _) = thrown(seed, 0.15);
        let (rope, _, _) = thrown(seed, 1.0);
        assert!(
            tap < rope * 0.7,
            "seed {seed}: a tap has to be a softer ball than a hold: {tap:.1} vs {rope:.1} yd/s"
        );
        // And slow enough to watch: 20 yards a second is a rope, not a floater.
        assert!(tap < 20.0, "seed {seed}: a tap came out at {tap:.1} yd/s");
    }
}

#[test]
fn a_tap_hangs_more_than_it_carries() {
    for seed in 0..6 {
        let (_, tap_peak, tap_air) = thrown(seed, 0.15);
        let (_, rope_peak, rope_air) = thrown(seed, 1.0);
        let tap_arc = tap_peak / tap_air.max(1.0);
        let rope_arc = rope_peak / rope_air.max(1.0);
        assert!(
            tap_arc > rope_arc * 1.5,
            "seed {seed}: a tap should be the lofted ball: {tap_arc:.3} vs {rope_arc:.3} of its length"
        );
    }
}

#[test]
fn the_meter_is_also_the_range() {
    let teams = league_teams();
    let sim = sim_for(&teams[0], &teams[1], 3);
    let target = deep_target(&sim);
    let soft = sim.throw_plan(sim.qb, target, 0.15, Vec2::ZERO);
    let hard = sim.throw_plan(sim.qb, target, 1.0, Vec2::ZERO);
    assert!(
        hard.reach > soft.reach + 10.0,
        "holding has to be how the ball gets downfield: {:.1} vs {:.1} yards",
        soft.reach,
        hard.reach
    );
    // A tap is a short throw: it cannot be a bomb, which is what makes holding
    // the button mean something.
    assert!(soft.reach < 16.0, "a tap carried {:.1} yards", soft.reach);
    assert!(hard.reach > 30.0, "a full hold only carried {:.1} yards", hard.reach);
}

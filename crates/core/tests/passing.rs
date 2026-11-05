//! The shape of a throw: the button sets the arc, the arm sets the range.
//!
//! The pass meter used to be a range of passes thrown harder or softer. It is
//! now the arc the ball travels on, which is what a touch pass actually is: tap
//! it and the ball is a line drive between the receiver's hips and knees, hold it
//! and it comes down over a linebacker's hands. How fast it leaves the hand, and
//! how far it can carry, is the quarterback's arm and nothing else.

use nopads_core::playbook::{defensive_playbook, playbook};
use nopads_core::sim::{PlaySim, SnapSetup, SNAP_DELAY};
use nopads_core::team::{league_teams, Team};

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

/// The deepest man on a route, so the two ends of the button are asked different
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

/// Throw to the deepest man at this arc, and report how the ball actually
/// travelled: how fast it left the hand, its apex, and the yards it was aimed at.
fn thrown(seed: u64, touch: f32) -> (f32, f32, f32) {
    let teams = league_teams();
    let mut sim = sim_for(&teams[0], &teams[1], seed);
    while sim.t < SNAP_DELAY + 0.25 && sim.result.is_none() {
        sim.step(1.0 / 60.0);
    }
    let target = deep_target(&sim);
    // Throw it directly rather than through the pad: the quarterback will have
    // got rid of it on his own by now, and a stale flight would make both ends
    // of the button look like the same pass.
    sim.throw_to_with(sim.qb, Some(target), touch);
    let flight = sim.ball.flight.clone().expect("the ball is in the air");
    (flight.speed, flight.peak, flight.air_yards)
}

#[test]
fn a_hold_lofts_the_ball_over_the_middle() {
    for seed in 0..6 {
        let (_, tap_peak, _) = thrown(seed, 0.18);
        let (_, lob_peak, _) = thrown(seed, 1.0);
        assert!(
            lob_peak > tap_peak * 1.8,
            "seed {seed}: holding it did not loft the ball ({tap_peak:.2} ft -> {lob_peak:.2} ft)"
        );
    }
}

#[test]
fn a_tap_is_a_line_drive_between_the_hips_and_knees() {
    for seed in 0..6 {
        let (tap, tap_peak, tap_air) = thrown(seed, 0.18);
        let (lob, _, _) = thrown(seed, 1.0);
        // Low enough to be a pass under a linebacker's hands rather than over
        // them, on a ball that is still travelling.
        assert!(
            tap_peak < tap_air * 0.16 + 1.2,
            "seed {seed}: a tap flew {tap_peak:.2} ft up over {tap_air:.1} yards"
        );
        // And a low ball is the fast ball: it gives up nothing to gravity.
        assert!(
            tap > lob,
            "seed {seed}: a tap has to be the quicker ball: {tap:.1} vs {lob:.1} yd/s"
        );
        // Both came off the same arm, so the button may only cost the lob a
        // little speed. It must not be the thing deciding the arm.
        assert!(
            tap - lob < 6.0,
            "seed {seed}: the button, not the arm, set the speed ({tap:.1} vs {lob:.1})"
        );
    }
}

#[test]
fn the_arm_is_the_range_not_the_button() {
    let teams = league_teams();
    let sim = sim_for(&teams[0], &teams[1], 3);
    let target = deep_target(&sim);
    let tap = sim.throw_plan(sim.qb, target, 0.0);
    let lob = sim.throw_plan(sim.qb, target, 1.0);
    assert!(
        (tap.reach - lob.reach).abs() < 0.01,
        "the button still changes the range: {:.1} vs {:.1} yards",
        tap.reach,
        lob.reach
    );
    assert!(
        (tap.landing - lob.landing).length() < 0.01,
        "the button still aims the ball somewhere else"
    );
    // The arm has to be worth something: a quarterback who can only throw it
    // twenty-five yards is not playing this game.
    assert!(
        tap.reach > 24.0,
        "the arm only carries {:.1} yards",
        tap.reach
    );
}

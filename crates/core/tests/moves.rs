//! Ball-carrier moves: the spin, the hurdle, the dive and the slide.
//!
//! These tests drive a real run play to the handoff and then work the buttons,
//! because the point of a move is what the defence does about it.

use nopads_core::math::Vec2;
use nopads_core::moves::{Kind, HURDLE_TIME};
use nopads_core::playbook::{defensive_playbook, playbook};
use nopads_core::sim::{PlayOutcome, PlaySim, QbControl, SnapSetup};
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

fn stick(dir: Vec2) -> Option<QbControl> {
    Some(QbControl { move_dir: dir, ..Default::default() })
}

/// A run play, stepped up to the moment the back has the ball.
fn to_the_handoff<'t>(off: &'t Team, def: &'t Team, seed: u64) -> Option<PlaySim<'t>> {
    let book = playbook();
    let defs = defensive_playbook();
    let play = book.iter().find(|p| p.family.is_run())?.clone();
    let mut sim = PlaySim::new(off, def, &play, &defs[0], &setup(seed));
    let mut guard = 0;
    while sim.carrier.is_none() && sim.result.is_none() && guard < 400 {
        sim.set_control(stick(Vec2::new(1.0, 0.0)));
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    Some(sim)
}

#[test]
fn a_dive_gains_the_yard_and_ends_the_play() {
    let teams = league_teams();
    let mut sim = to_the_handoff(&teams[0], &teams[1], 11).expect("a run play");
    let carrier = sim.carrier.expect("the handoff");
    let start = sim.players[carrier].pos.x;

    sim.request_move(Kind::Dive);
    assert_eq!(sim.carrier_move().kind(), Some(Kind::Dive));

    let mut guard = 0;
    while sim.result.is_none() && guard < 120 {
        sim.set_control(stick(Vec2::new(1.0, 0.0)));
        sim.step(1.0 / 60.0);
        guard += 1;
    }

    let result = sim.result.as_ref().expect("the dive finishes the play");
    assert_eq!(result.outcome, PlayOutcome::Run);
    assert!(
        result.description.contains("dives"),
        "the call should say what happened: {}",
        result.description
    );
    assert!(
        sim.players[carrier].pos.x > start,
        "a dive forward has to go forward: {start} -> {}",
        sim.players[carrier].pos.x
    );
}

#[test]
fn a_slide_gives_himself_up() {
    let teams = league_teams();
    let mut sim = to_the_handoff(&teams[0], &teams[1], 12).expect("a run play");
    sim.request_move(Kind::Slide);

    let mut guard = 0;
    while sim.result.is_none() && guard < 120 {
        sim.set_control(stick(Vec2::new(1.0, 0.0)));
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    let result = sim.result.as_ref().expect("the slide finishes the play");
    assert!(
        result.description.contains("slides"),
        "the call should say it was a slide: {}",
        result.description
    );
}

#[test]
fn a_hurdle_goes_over_a_tackle() {
    let teams = league_teams();
    let mut sim = to_the_handoff(&teams[0], &teams[1], 13).expect("a run play");
    let carrier = sim.carrier.expect("the handoff");

    // Put a defender right on him, so a tackle is certain to be attempted.
    sim.players[7].pos = sim.players[carrier].pos + Vec2::new(0.25, 0.0);
    sim.request_move(Kind::Hurdle);
    assert!(sim.carrier_move().is_hurdling());

    for _ in 0..(HURDLE_TIME * 0.4 * 60.0) as usize {
        sim.set_control(stick(Vec2::new(1.0, 0.0)));
        sim.step(1.0 / 60.0);
    }

    assert!(
        sim.result.is_none(),
        "a man in the air cannot be tackled: {:?}",
        sim.result.as_ref().map(|r| &r.description)
    );
    assert!(
        sim.players[7].stun > 0.0,
        "the defender should have gone under him"
    );
}

#[test]
fn a_spin_makes_a_tackle_less_likely() {
    // The same defender, the same geometry, many times: a spin has to get out of
    // more of them than an ordinary run does. Counted over seeds rather than
    // asserted once, because a tackle is a dice roll.
    let teams = league_teams();
    let mut spun = 0;
    let mut plain = 0;
    let trials = 24;
    for seed in 0..trials {
        for (use_spin, count) in [(true, &mut spun), (false, &mut plain)] {
            let Some(mut sim) = to_the_handoff(&teams[0], &teams[1], 100 + seed) else {
                continue;
            };
            let carrier = sim.carrier.unwrap();
            sim.players[7].pos = sim.players[carrier].pos + Vec2::new(0.35, 0.0);
            sim.players[7].tackle_cooldown = 0.0;
            if use_spin {
                sim.request_move(Kind::Spin);
            }
            sim.set_control(stick(Vec2::new(1.0, 0.0)));
            sim.step(1.0 / 60.0);
            // A tackle is not the end of the play any more: the hit starts a wrap
            // that puts him on the ground a beat later. Getting out of it means no
            // arms on him at all, not merely that the card is not up as well.
            if sim.result.is_none() && sim.wrap.is_none() {
                *count += 1;
            }
        }
    }
    assert!(
        spun > plain,
        "a spin has to beat the tackle more often than not spinning: {spun} vs {plain}"
    );
}

#[test]
fn a_move_needs_the_ball_in_his_hands() {
    let teams = league_teams();
    let book = playbook();
    let defs = defensive_playbook();
    let play = book.iter().find(|p| p.family.is_run()).expect("a run play");
    let mut sim = PlaySim::new(&teams[0], &teams[1], play, &defs[0], &setup(21));

    // Before the snap nobody is carrying anything.
    sim.request_move(Kind::Dive);
    assert!(
        sim.carrier_move().kind().is_none(),
        "a move before the snap is a mashed button, not a play"
    );
}

#[test]
fn a_quarterback_past_the_line_owns_the_face_buttons() {
    // The buttons have to go by the ball, not by the sim's carrier field: that
    // field is promoted a frame behind a handoff or a scramble, and in that gap a
    // man ran with the ball while the screen still offered him his receivers.
    let teams = league_teams();
    let book = playbook();
    let defs = defensive_playbook();
    let play = book
        .iter()
        .find(|p| p.family.is_pass())
        .expect("a pass play")
        .clone();
    let mut sim = PlaySim::new(&teams[0], &teams[1], &play, &defs[0], &setup(41));
    for _ in 0..40 {
        sim.step(1.0 / 60.0);
    }
    assert!(sim.ball.holder.is_some(), "he has the snap");
    assert!(
        !sim.face_buttons_are_moves(),
        "behind the line the buttons are the receivers'"
    );

    // Over the line, ball still in his hands.
    let qb = sim.qb;
    sim.players[qb].pos.x = sim.los + 0.6;
    assert!(
        sim.face_buttons_are_moves(),
        "past the line there is no pass to throw, so the buttons are his"
    );
    sim.request_move(Kind::Slide);
    assert_eq!(
        sim.carrier_move().kind(),
        Some(Kind::Slide),
        "the dive button gives the quarterback up, it does not send him head first"
    );
    assert_eq!(sim.carrier, Some(qb), "the man with the ball is the runner");
}

#[test]
fn the_face_buttons_are_passes_in_the_pocket_and_moves_once_he_runs() {
    let teams = league_teams();
    let book = playbook();
    let defs = defensive_playbook();

    // In the pocket the quarterback still has his receivers.
    let pass = book
        .iter()
        .find(|p| p.family.is_pass())
        .expect("a pass play")
        .clone();
    let mut sim = PlaySim::new(&teams[0], &teams[1], &pass, &defs[0], &setup(31));
    while sim.t < 0.9 && sim.result.is_none() {
        sim.step(1.0 / 60.0);
    }
    assert!(
        !sim.face_buttons_are_moves(),
        "the buttons are still the receivers' behind the line"
    );

    // And once the ball is handed off, they belong to the runner.
    let mut run = to_the_handoff(&teams[0], &teams[1], 32).expect("a run play");
    assert!(
        run.face_buttons_are_moves(),
        "the back has the ball and nobody to throw to"
    );
    run.request_move(Kind::Spin);
    assert_eq!(run.carrier_move().kind(), Some(Kind::Spin));
}

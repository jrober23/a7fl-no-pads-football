//! The interactive path: a human steering the quarterback through a snap.

use nopads_core::game::{Action, GamePhase, GameState, PlayRecord};
use nopads_core::throwoff::simulate_throw_off;
use nopads_core::math::Vec2;
use nopads_core::playbook::{defensive_playbook, playbook};
use nopads_core::sim::{PlaySim, QbControl, SnapSetup, SNAP_DELAY};
use nopads_core::team::league_teams;

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

#[test]
fn the_stick_moves_the_quarterback() {
    let teams = league_teams();
    let book = playbook();
    let defs = defensive_playbook();
    let play = book
        .iter()
        .find(|p| p.family.is_pass())
        .expect("a pass play");
    let mut sim = PlaySim::new(&teams[0], &teams[1], play, &defs[0], &setup(5));

    while sim.t < SNAP_DELAY + 0.2 && sim.result.is_none() {
        sim.step(1.0 / 60.0);
    }
    let before = sim.qb_pos();
    assert!(sim.result.is_none(), "play ended before the test began");

    for _ in 0..30 {
        sim.set_control(Some(QbControl {
            move_dir: Vec2::new(0.0, 1.0),
            target: None,
            throw: false,
            ..Default::default()
        }));
        sim.step(1.0 / 60.0);
        if sim.result.is_some() {
            break;
        }
    }

    let after = sim.qb_pos();
    assert!(
        after.y > before.y + 0.3,
        "quarterback did not follow the stick: {before:?} -> {after:?}"
    );
}

#[test]
fn the_throw_button_releases_the_ball() {
    let teams = league_teams();
    let book = playbook();
    let defs = defensive_playbook();
    let play = book
        .iter()
        .find(|p| p.family.is_pass())
        .expect("a pass play");
    let mut sim = PlaySim::new(&teams[0], &teams[1], play, &defs[0], &setup(6));

    while sim.t < SNAP_DELAY + 0.5 && sim.result.is_none() {
        sim.step(1.0 / 60.0);
    }
    let targets = sim.eligible_targets();
    assert!(!targets.is_empty(), "no eligible receivers");
    let target = targets[0].1;

    sim.set_control(Some(QbControl {
        move_dir: Vec2::ZERO,
        target: Some(target),
        throw: true,
        power: 0.8,
        ..Default::default()
    }));
    sim.step(1.0 / 60.0);

    assert!(
        sim.ball.flight.is_some() || sim.result.is_some(),
        "the ball never left the quarterback's hand"
    );
}

#[test]
fn the_stick_runs_the_ball_carrier() {
    let teams = league_teams();
    let book = playbook();
    let defs = defensive_playbook();
    let play = book
        .iter()
        .find(|p| p.family.is_run())
        .expect("a run play");
    let mut sim = PlaySim::new(&teams[0], &teams[1], play, &defs[0], &setup(9));

    let mut guard = 0;
    while sim.carrier.is_none() && sim.result.is_none() && guard < 240 {
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    let carrier = sim.carrier.expect("the handoff should come early in a run play");
    let before = sim.players[carrier].pos;

    // Push across the field: the AI carrier would be heading for the end zone, so
    // sideways movement is the stick's doing.
    let mut after = before;
    for _ in 0..45 {
        if sim.result.is_some() {
            break;
        }
        sim.set_control(Some(QbControl {
            move_dir: Vec2::new(0.0, 1.0),
            target: None,
            throw: false,
            ..Default::default()
        }));
        sim.step(1.0 / 60.0);
        after = sim.players[carrier].pos;
    }

    assert!(
        after.y > before.y + 0.3,
        "the carrier did not answer the stick: {before:?} -> {after:?}"
    );
}

#[test]
fn a_human_snap_is_booked_by_the_game() {
    let teams = league_teams();
    let mut game = GameState::new(teams[0].clone(), teams[1].clone(), 21);
    // Clear the opening throw-off, and the try if it was run all the way back:
    // the test needs a scrimmage down, not a particular first play.
    let mut guard = 0;
    while !matches!(game.action(), nopads_core::game::Action::CallPlay { .. }) && guard < 4 {
        game.simulate_next();
        guard += 1;
    }
    assert!(
        matches!(game.action(), nopads_core::game::Action::CallPlay { .. }),
        "the game never reached a scrimmage down"
    );
    let before = game.total_plays();

    let play = playbook()
        .into_iter()
        .find(|p| p.family.is_pass())
        .expect("a pass play");
    let def = defensive_playbook()[0].clone();

    let mut drove = false;
    let record = game
        .snap_with(play, def, |sim| {
            drove = true;
            while sim.t < SNAP_DELAY + 0.45 && sim.result.is_none() {
                sim.step(1.0 / 60.0);
            }
            let target = sim.eligible_targets().first().map(|(_, index)| *index);
            for _ in 0..40 {
                sim.set_control(Some(QbControl {
                    move_dir: Vec2::ZERO,
                    target,
                    throw: true,
                    power: 0.9,
                    ..Default::default()
                }));
                sim.step(1.0 / 60.0);
                if sim.result.is_some() {
                    break;
                }
            }
        })
        .expect("a scrimmage down should snap");

    assert!(drove, "the driver closure never ran");
    assert_eq!(game.total_plays(), before + 1);
    assert!(record.description().len() > 3);
    // The snap is on the books and the game is still going. What it asks for next
    // depends on what the play did -- a score asks for the conversion -- so the
    // check is that the down was booked and the game moved on, not that the drive
    // happened to be nowhere near a score.
    assert!(!game.over, "one snap ended the game");
}

#[test]
fn the_button_shapes_the_arc_and_the_arm_sets_the_range() {
    let teams = league_teams();
    let book = playbook();
    let defs = defensive_playbook();
    let play = book
        .iter()
        .find(|p| p.family.is_pass())
        .expect("a pass play");
    let mut sim = PlaySim::new(&teams[0], &teams[1], play, &defs[0], &setup(11));
    while sim.t < SNAP_DELAY + 0.6 && sim.result.is_none() {
        sim.step(1.0 / 60.0);
    }

    // The deepest man, so the throw has room to be a real one.
    let targets = sim.eligible_targets();
    let deep = targets
        .iter()
        .map(|(_, i)| *i)
        .max_by(|a, b| {
            sim.players[*a]
                .pos
                .dist(sim.qb_pos())
                .partial_cmp(&sim.players[*b].pos.dist(sim.qb_pos()))
                .unwrap()
        })
        .expect("eligible receivers");

    // How far he can throw is his arm. The button must not change the plan at
    // all any more -- that is the whole revamp: the old meter threw the ball
    // shorter the softer it was pressed, which is not how a pass works.
    let tap = sim.throw_plan(sim.qb, deep, 0.0);
    let rope = sim.throw_plan(sim.qb, deep, 1.0);
    assert!(
        (tap.reach - rope.reach).abs() < 0.01,
        "the button still changes how far he can throw ({:.2} vs {:.2})",
        tap.reach,
        rope.reach
    );
    assert!(
        tap.landing.dist(rope.landing) < 0.01,
        "the button still aims the ball short of the man"
    );

    // And what the button *does* set is the arc: a tap is a line drive, a held
    // button is the lob that comes down over a linebacker.
    let flight = |s: &nopads_core::sim::PlaySim| {
        s.ball
            .flight
            .as_ref()
            .map(|f| (f.peak, f.speed))
            .expect("a pass in the air")
    };
    sim.throw_to_with(sim.qb, Some(deep), 0.18);
    let (flat_peak, flat_speed) = flight(&sim);
    sim.throw_to_with(sim.qb, Some(deep), 1.0);
    let (lob_peak, lob_speed) = flight(&sim);
    assert!(
        lob_peak > flat_peak * 1.6,
        "holding it did not loft the ball ({flat_peak:.2} ft -> {lob_peak:.2} ft)"
    );
    assert!(flat_speed > lob_speed, "the lob was not the slower ball");
    // Both came off the same arm, so the button can only cost the lob a little
    // speed. It must not be the thing deciding the arm.
    assert!(
        (flat_speed - lob_speed).abs() < 6.0,
        "the button, not the arm, set the speed ({flat_speed:.1} vs {lob_speed:.1})"
    );
}

#[test]
fn a_pass_goes_to_the_man_and_not_to_the_stick() {
    let teams = league_teams();
    let book = playbook();
    let defs = defensive_playbook();
    let play = book
        .iter()
        .find(|p| p.family.is_pass())
        .expect("a pass play");
    let mut sim = PlaySim::new(&teams[0], &teams[1], play, &defs[0], &setup(12));
    while sim.t < SNAP_DELAY + 0.5 && sim.result.is_none() {
        sim.step(1.0 / 60.0);
    }
    let receiver = sim.eligible_targets()[0].1;
    let man = sim.players[receiver].pos;

    let plan = sim.throw_plan(sim.qb, receiver, 1.0);
    assert!(
        plan.landing.dist(man) < 3.0,
        "the ball was thrown {:.1} yards off the man it was meant for",
        plan.landing.dist(man)
    );
    // The meter decides how hard it gets there, not which way it goes: a soft
    // throw of the same ball is the same line, stopping shorter.
    let soft = sim.throw_plan(sim.qb, receiver, 0.1);
    let line = (soft.landing - sim.qb_pos()).normalize_or_zero();
    let hard_line = (plan.landing - sim.qb_pos()).normalize_or_zero();
    assert!(
        line.dot(hard_line) > 0.9,
        "a soft throw left the line to the man"
    );
}

#[test]
fn a_conversion_is_a_snap_you_can_call_and_play() {
    let teams = league_teams();
    let mut game = GameState::new(teams[0].clone(), teams[1].clone(), 33);
    let before = game.total_plays();

    game.set_conversion(true);
    assert_eq!(game.total_plays(), before, "picking the try ran a play by itself");
    assert!(
        matches!(game.action(), Action::CallPlay { fourth_down: false }),
        "the try was not left waiting for a play call"
    );

    let play = playbook()
        .into_iter()
        .find(|p| p.family.is_pass())
        .expect("a pass play");
    let def = defensive_playbook()[0].clone();
    let record = game
        .snap_with(play, def, |sim| {
            while sim.t < SNAP_DELAY + 0.45 && sim.result.is_none() {
                sim.step(1.0 / 60.0);
            }
            let target = sim.eligible_targets().first().map(|(_, index)| *index);
            for _ in 0..40 {
                sim.set_control(Some(QbControl {
                    move_dir: Vec2::ZERO,
                    target,
                    throw: true,
                    power: 0.9,
                    ..Default::default()
                }));
                sim.step(1.0 / 60.0);
                if sim.result.is_some() {
                    break;
                }
            }
        })
        .expect("a try should snap");

    match record {
        PlayRecord::Scrimmage(r) => assert!(r.is_try, "the snap was not booked as a try"),
        _ => panic!("a try should book a scrimmage record"),
    }
}

#[test]
fn a_throw_off_set_up_by_hand_is_booked_when_it_is_played() {
    let teams = league_teams();
    let mut game = GameState::new(teams[0].clone(), teams[1].clone(), 44);
    let (receiving, from_own) = match &game.phase {
        GamePhase::ThrowOff { receiving, from_own } => (*receiving, *from_own),
        other => panic!("the game should open with a throw-off, not {other:?}"),
    };

    // Set it up, play it out separately, then hand the result back.
    let setup = game.throw_off_setup(receiving, from_own, false);
    let before = game.total_plays();
    let result = simulate_throw_off(
        &setup,
        &game.teams[setup.throwing_team],
        &game.teams[setup.receiving_team],
    );
    let rec = game.finish_throw_off(setup, result);

    assert_eq!(game.total_plays(), before + 1, "the throw-off was not booked");
    match rec {
        PlayRecord::ThrowOff(r) => {
            assert!(!r.result.description.is_empty());
            assert!(r.result.start_spot >= 0.0 && r.result.start_spot <= 100.0);
        }
        other => panic!("a throw-off should book a throw-off record, got {other:?}"),
    }
    assert!(
        !matches!(game.phase, GamePhase::ThrowOff { .. }) || game.clock >= 0.0,
        "the game should have moved on from the throw-off"
    );
}

#[test]
fn a_pump_fake_does_not_throw_the_ball() {
    let teams = league_teams();
    let book = playbook();
    let defs = defensive_playbook();
    let play = book
        .iter()
        .find(|p| p.family.is_pass())
        .expect("a pass play");
    let mut sim = PlaySim::new(&teams[0], &teams[1], play, &defs[0], &setup(12));
    while sim.t < SNAP_DELAY + 0.7 && sim.result.is_none() {
        sim.step(1.0 / 60.0);
    }
    assert!(sim.ball.holder == Some(sim.qb), "the ball was not in his hand");

    sim.pump_fake();
    assert!(sim.ball.flight.is_none(), "the pump fake was a throw");
    assert!(sim.result.is_none(), "the pump fake ended the play");
    assert!(sim.carrier.is_none(), "the pump fake made him a runner");
}

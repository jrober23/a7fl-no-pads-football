//! The interactive path: a human steering the quarterback through a snap.

use a7fl_core::game::{Action, GameState, PlayRecord};
use a7fl_core::math::Vec2;
use a7fl_core::playbook::{defensive_playbook, playbook};
use a7fl_core::sim::{PlaySim, QbControl, SnapSetup, SNAP_DELAY};
use a7fl_core::team::league_teams;

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
    // Clear the opening throw-off.
    game.simulate_next();
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
    assert!(matches!(game.action(), a7fl_core::game::Action::CallPlay { .. }));
}

#[test]
fn a_soft_throw_comes_down_short_of_the_man() {
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

    // The deepest man, so there is room to test an underthrow.
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

    let soft = sim.throw_plan(sim.qb, deep, 0.05, Vec2::ZERO);
    let hard = sim.throw_plan(sim.qb, deep, 1.0, Vec2::ZERO);

    assert!(soft.reach < hard.reach, "a tap threw further than a rope");
    assert!(
        soft.distance > soft.reach + 1.0,
        "no receiver was deep enough to underthrow: {:?}",
        soft
    );
    assert!(
        soft.landing.dist(sim.qb_pos()) < hard.landing.dist(sim.qb_pos()) - 2.0,
        "the soft throw did not come down short"
    );
    // A full-weight throw gets there, which is what the meter is for.
    assert!(hard.distance <= hard.reach + 0.01, "a full arm should reach him");
}

#[test]
fn the_aim_stick_bends_the_pass_off_the_receiver() {
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

    let straight = sim.throw_plan(sim.qb, receiver, 1.0, Vec2::ZERO);
    let led = sim.throw_plan(sim.qb, receiver, 1.0, Vec2::new(0.0, 6.0));
    assert!(
        (led.landing.y - straight.landing.y - 6.0).abs() < 0.75,
        "aiming six yards across moved the ball {:.1}",
        led.landing.y - straight.landing.y
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

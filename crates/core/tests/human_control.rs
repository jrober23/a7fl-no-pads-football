//! The interactive path: a human steering the quarterback through a snap.

use a7fl_core::game::GameState;
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

    sim.set_control(Some(QbControl { move_dir: Vec2::ZERO, target: Some(target), throw: true }));
    sim.step(1.0 / 60.0);

    assert!(
        sim.ball.flight.is_some() || sim.result.is_some(),
        "the ball never left the quarterback's hand"
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

//! The read option: on a run the face buttons are the pitch, and a man who is
//! ahead of the ball is not a target at all -- the man carrying it keeps it.

use nopads_core::math::Vec2;
use nopads_core::playbook::{defensive_playbook, Assign, Formation, OffRole, Play, PlayFamily};
use nopads_core::sim::{PlaySim, QbControl, SnapSetup};
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

/// The shape the designer makes when the carrier pick is on the quarterback and
/// the back and the tight end have paths drawn: a keep with two men coming round
/// behind the ball.
fn option_play() -> Play {
    Play {
        name: "READ OPTION",
        concept: "Option",
        family: PlayFamily::QbRun,
        formation: Formation::Pistol,
        drop: 0.0,
        run_side: 1,
        flip: false,
        assignments: vec![
            (
                OffRole::Qb,
                Assign::QbCarry {
                    points: vec![
                        Vec2::new(-1.0, 0.0),
                        Vec2::new(2.0, 2.0),
                        Vec2::new(9.0, 5.0),
                    ],
                },
            ),
            (OffRole::Lg, Assign::RunBlock { aim: Vec2::new(2.0, -1.0), seal: false }),
            (OffRole::Rg, Assign::RunBlock { aim: Vec2::new(2.0, 1.0), seal: false }),
            // The sweep: he starts behind the ball and comes round the back.
            (
                OffRole::Back,
                Assign::Route {
                    points: vec![
                        Vec2::new(-7.0, 0.0),
                        Vec2::new(-4.0, 4.0),
                        Vec2::new(1.0, 7.0),
                    ],
                    depth: 0,
                },
            ),
            // This one runs upfield, so a pitch to him would be a forward pass.
            (
                OffRole::Slot,
                Assign::Route {
                    points: vec![Vec2::new(2.0, -2.0), Vec2::new(5.0, -4.0)],
                    depth: 0,
                },
            ),
            (OffRole::WideL, Assign::RunBlock { aim: Vec2::new(1.0, -6.0), seal: true }),
            (OffRole::WideR, Assign::RunBlock { aim: Vec2::new(1.0, 8.0), seal: true }),
        ],
    }
}

/// A snapped play with the quarterback holding it.
fn to_the_ball(seed: u64) -> PlaySim<'static> {
    let teams = Box::leak(Box::new(league_teams()));
    let defs = Box::leak(Box::new(defensive_playbook()));
    let play = Box::leak(Box::new(option_play()));
    let mut sim = PlaySim::new(&teams[0], &teams[1], play, &defs[0], &setup(seed));
    // The ball is in the quarterback's hands before the snap too, so the wait is
    // for the snap and not for a holder.
    let mut guard = 0;
    while !sim.is_snapped() && sim.result.is_none() && guard < 240 {
        sim.set_control(Some(QbControl {
            move_dir: Vec2::new(1.0, 0.0),
            ..Default::default()
        }));
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    sim
}

fn man(sim: &PlaySim<'_>, role: OffRole) -> usize {
    (0..7)
        .find(|&i| OffRole::ALL[sim.players[i].role_index] == role)
        .expect("the role is on the field")
}

#[test]
fn the_quarterback_keeps_it_unless_he_pitches() {
    let sim = to_the_ball(21);
    let qb = man(&sim, OffRole::Qb);
    assert_eq!(sim.ball.holder, Some(qb), "the quarterback has the ball");
    assert_eq!(sim.carrier, Some(qb), "and he is the runner");
    let targets = sim.pitch_targets();
    assert!(
        targets.iter().any(|(role, _)| *role == OffRole::Back),
        "the man coming round the back is on a button: {targets:?}"
    );
    assert!(
        !targets.iter().any(|(role, _)| *role == OffRole::Qb),
        "the man with the ball is not a pitch target"
    );
}

#[test]
fn a_pitch_back_hands_him_the_ball() {
    let mut sim = to_the_ball(22);
    let qb = man(&sim, OffRole::Qb);
    let back = man(&sim, OffRole::Back);
    // The pitch has to be a backward pass, so wait for the sweep to be behind
    // the ball. He starts one and starts running that way.
    let mut guard = 0;
    while sim.players[back].pos.x > sim.players[qb].pos.x && guard < 60 {
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    assert!(
        sim.players[back].pos.x <= sim.players[qb].pos.x,
        "the sweep is behind the ball"
    );
    assert!(sim.pitch(back), "he takes the pitch");
    assert_eq!(sim.carrier, Some(back), "he is the runner now");
    assert_eq!(sim.ball.holder, Some(back), "and he has the ball");
}

#[test]
fn a_forward_pitch_is_refused_and_the_quarterback_keeps_it() {
    let mut sim = to_the_ball(23);
    let qb = man(&sim, OffRole::Qb);
    let slot = man(&sim, OffRole::Slot);
    let ahead = sim.players[slot].pos.x - sim.players[qb].pos.x;
    assert!(ahead > 0.35, "the man is ahead of the ball: {ahead}");
    assert!(!sim.pitch(slot), "a forward pitch is not a pitch");
    assert_eq!(sim.ball.holder, Some(qb), "so the quarterback still has it");
}

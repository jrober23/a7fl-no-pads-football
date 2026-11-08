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

/// The same keep with a third man trailing deeper than the back: the shape a
/// triple option is drawn in.
fn triple_option_play() -> Play {
    let mut p = option_play();
    p.name = "TRIPLE OPTION";
    for (role, a) in p.assignments.iter_mut() {
        // Both men stay behind the line, where the option is: the back sweeps
        // across and the third man comes round behind him, deeper all the way.
        match role {
            OffRole::Back => {
                *a = Assign::Route {
                    points: vec![
                        Vec2::new(-8.0, 4.0),
                        Vec2::new(-6.0, 8.0),
                        Vec2::new(-4.0, 11.0),
                    ],
                    depth: 0,
                }
            }
            OffRole::Slot => {
                *a = Assign::Route {
                    points: vec![
                        Vec2::new(-11.0, -2.0),
                        Vec2::new(-10.0, 2.0),
                        Vec2::new(-8.0, 6.0),
                    ],
                    depth: 0,
                }
            }
            _ => {}
        }
    }
    p
}

/// A snapped play with the quarterback holding it.
fn to_the_ball(seed: u64) -> PlaySim<'static> {
    to_the_ball_with(option_play(), seed)
}

fn to_the_ball_with(play: Play, seed: u64) -> PlaySim<'static> {
    let teams = Box::leak(Box::new(league_teams()));
    let defs = Box::leak(Box::new(defensive_playbook()));
    let play = Box::leak(Box::new(play));
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
fn a_triple_option_pitches_twice_to_the_man_trailing() {
    let mut sim = to_the_ball_with(triple_option_play(), 24);
    let qb = man(&sim, OffRole::Qb);
    let back = man(&sim, OffRole::Back);
    let slot = man(&sim, OffRole::Slot);
    // Nobody on the stick: the men run the paths they were drawn, which is how
    // the pitch gets a trailer to go to.
    sim.set_control(None);
    let mut guard = 0;
    while sim.players[back].pos.x > sim.players[qb].pos.x && guard < 60 {
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    assert!(sim.pitch(back), "the first pitch goes to the back");
    assert_eq!(sim.ball.holder, Some(back), "he has it");
    // Wait for the third man to come round behind him: the option is still alive
    // because the ball is still behind the line, so the buttons are the pitch
    // rather than the runner's moves.
    let mut guard = 0;
    while sim.players[slot].pos.x >= sim.players[back].pos.x && guard < 200 {
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    assert!(sim.behind_the_line(), "the ball is still behind the line");
    assert!(
        sim.players[slot].pos.x < sim.players[back].pos.x,
        "the third man trails the second"
    );
    assert!(
        sim.pitch_targets().iter().any(|(_, i)| *i == slot),
        "and he is on a button: {:?}",
        sim.pitch_targets()
    );
    assert!(sim.pitch(slot), "the second pitch goes to him");
    assert_eq!(sim.ball.holder, Some(slot), "he has it now");
    assert_eq!(sim.carrier, Some(slot), "and he is the runner");
}

#[test]
fn the_option_is_gone_once_the_ball_is_past_the_line() {
    let mut sim = to_the_ball(25);
    assert!(
        !sim.pitch_targets().is_empty(),
        "the option is live behind the line"
    );
    // A yard past the line: the pitch is gone and the buttons are the runner's
    // moves again.
    sim.ball.pos.x = sim.setup.los + 1.0;
    assert!(!sim.behind_the_line());
    assert!(sim.pitch_targets().is_empty());
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

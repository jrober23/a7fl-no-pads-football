//! The read option: on a run the face buttons are the pitch, and a man who is
//! ahead of the ball is not a target at all -- the man carrying it keeps it.

use nopads_core::math::Vec2;
use nopads_core::playbook::{
    defensive_playbook, Assign, Formation, Handoff, OffRole, Play, PlayFamily,
};
use nopads_core::sim::{OptionChoice, PlaySim, QbControl, SnapSetup, SNAP_DELAY};
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
        handoff: Handoff::Plain,
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

/// The same option with the quarterback sweeping behind the line, so a keeper can
/// still give the ball on instead of running out of room to do it.
fn keep_and_pitch_play() -> Play {
    let mut p = triple_option_play();
    p.name = "KEEP AND PITCH";
    for (role, a) in p.assignments.iter_mut() {
        if *role == OffRole::Qb {
            *a = Assign::QbCarry {
                points: vec![
                    Vec2::new(-1.0, 0.0),
                    Vec2::new(-1.0, 4.0),
                    Vec2::new(0.0, 8.0),
                ],
            };
        }
    }
    p
}

/// A snapped play with nobody on the pad, so the computer is the one running it.
fn to_the_ball_ai(play: Play, seed: u64) -> PlaySim<'static> {
    let teams = Box::leak(Box::new(league_teams()));
    let defs = Box::leak(Box::new(defensive_playbook()));
    let play = Box::leak(Box::new(play));
    let mut sim = PlaySim::new(&teams[0], &teams[1], play, &defs[0], &setup(seed));
    let mut guard = 0;
    while !sim.is_snapped() && guard < 240 {
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    sim
}

/// Stand every man the computer has looked at out on the edge, where an end who
/// is playing the dive does not go.
fn hold_wide(sim: &mut PlaySim, held: &mut Vec<usize>) {
    if let Some(r) = sim.option_read() {
        if !held.contains(&r) {
            held.push(r);
        }
    }
    for &d in held.iter() {
        let side = if sim.players[d].pos.y < sim.setup.ball_y { -1.0 } else { 1.0 };
        sim.players[d].pos.y = sim.setup.ball_y + side * 9.0;
        sim.players[d].vel = Vec2::ZERO;
    }
}

/// The opposite: every man it has looked at has come down into the dive lane.
fn hold_inside(sim: &mut PlaySim, held: &mut Vec<usize>) {
    if let Some(r) = sim.option_read() {
        if !held.contains(&r) {
            held.push(r);
        }
    }
    let base = sim.setup.ball_y;
    for &d in held.iter() {
        sim.players[d].pos.y = base;
        sim.players[d].vel = Vec2::ZERO;
    }
}

#[test]
fn the_computer_gives_it_on_when_the_end_stays_wide() {
    let mut sim = to_the_ball_ai(option_play(), 41);
    let mut held = Vec::new();
    let mut guard = 0;
    while sim.option_choice.is_none() && guard < 240 {
        hold_wide(&mut sim, &mut held);
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    assert!(!held.is_empty(), "it had an end to read");
    assert!(sim.option_key.is_some(), "and it read him");
    assert_eq!(
        sim.option_choice,
        Some(OptionChoice::Handoff),
        "the end stayed wide, so the ball goes back to the dive"
    );
    assert_ne!(sim.ball.holder, Some(sim.qb), "it is out of his hands");
}

#[test]
fn the_computer_keeps_it_when_the_end_crashes_down() {
    let mut sim = to_the_ball_ai(option_play(), 42);
    let mut held = Vec::new();
    let mut guard = 0;
    while sim.option_choice.is_none() && guard < 240 {
        hold_inside(&mut sim, &mut held);
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    assert_eq!(
        sim.option_choice,
        Some(OptionChoice::Keep),
        "the end came down for the dive, so the quarterback pulls it"
    );
    assert_eq!(sim.ball.holder, Some(sim.qb), "and he still has it");
}

#[test]
fn the_computer_gives_it_on_again_when_they_come_for_the_keeper() {
    let mut sim = to_the_ball_ai(keep_and_pitch_play(), 43);
    let back = man(&sim, OffRole::Back);
    let slot = man(&sim, OffRole::Slot);
    let mut held = Vec::new();
    // Stage what the second read is about: a man on the keeper, the ends crashing
    // down, and the two men behind the ball with room to take it.
    fn stage(sim: &mut PlaySim, held: &[usize], back: usize, slot: usize) {
        let hunter = held[0];
        sim.players[hunter].pos = sim.players[sim.qb].pos + Vec2::new(1.0, 0.0);
        sim.players[hunter].vel = Vec2::ZERO;
        sim.players[back].pos = Vec2::new(sim.los - 7.0, sim.setup.ball_y + 7.0);
        sim.players[slot].pos = Vec2::new(sim.los - 9.0, sim.setup.ball_y - 7.0);
        sim.players[back].vel = Vec2::ZERO;
        sim.players[slot].vel = Vec2::ZERO;
    }
    let mut guard = 0;
    while sim.option_choice.is_none() && guard < 240 {
        hold_inside(&mut sim, &mut held);
        if !held.is_empty() {
            stage(&mut sim, &held, back, slot);
        }
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    assert_eq!(sim.option_choice, Some(OptionChoice::Keep), "he kept it at the mesh");
    let mut guard = 0;
    while sim.option_choice == Some(OptionChoice::Keep) && guard < 120 {
        hold_inside(&mut sim, &mut held);
        if !held.is_empty() {
            stage(&mut sim, &held, back, slot);
        }
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    assert_eq!(
        sim.option_choice,
        Some(OptionChoice::Pitch),
        "he gave it on rather than take the hit"
    );
    assert_ne!(sim.ball.holder, Some(sim.qb));
}

/// A jet sweep: the wide man is the one who carries, and his path brings him
/// behind the quarterback rather than starting where he stands.
#[test]
fn a_jet_sweep_takes_the_ball_only_when_he_gets_behind_the_ball() {
    let mut play = option_play();
    play.name = "JET SWEEP";
    play.family = PlayFamily::RunOutside;
    let block = || Assign::RunBlock {
        aim: Vec2::new(1.0, 0.0),
        seal: false,
    };
    // Work the sweep off where he actually lines up, so it comes across the
    // quarterback's face whichever side of the formation he is on.
    let wing = play.alignment(OffRole::WideL);
    play.assignments = vec![
        (
            OffRole::Qb,
            Assign::PassBlock {
                slide: 0.0,
                max_depth: 1.0,
            },
        ),
        (OffRole::Lg, block()),
        (OffRole::Rg, block()),
        (OffRole::Back, block()),
        (OffRole::Slot, block()),
        (OffRole::WideL, Assign::Carry {
            points: vec![
                Vec2::new(-3.0, -wing.y * 0.95),
                Vec2::new(-2.0, -wing.y * 0.5),
                Vec2::new(2.0, -wing.y * 0.1),
            ],
        }),
        (OffRole::WideR, block()),
    ];
    let mut sim = to_the_ball_ai(play, 51);
    let qb = man(&sim, OffRole::Qb);
    let wide = man(&sim, OffRole::WideL);
    // The beat the handoff used to be handed over on has gone by and he is still
    // standing out on the line: the ball stays with the quarterback.
    let mut guard = 0;
    while sim.t < SNAP_DELAY + 0.7 && guard < 240 {
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    assert_eq!(
        sim.ball.holder,
        Some(qb),
        "the ball is not beamed out to a man still on the line"
    );
    let qb_spot = sim.players[qb].pos;
    // He comes round, and the ball changes hands when he is behind the line and
    // has actually got to the quarterback.
    let mut guard = 0;
    while sim.ball.holder == Some(qb) && guard < 480 {
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    assert_eq!(
        sim.ball.holder,
        Some(wide),
        "the sweep takes the handoff: t={:.2} wide={:?} qb={:?} behind={}",
        sim.t,
        sim.players[wide].pos,
        sim.players[qb].pos,
        sim.behind_the_line()
    );
    assert!(sim.behind_the_line(), "and he took it behind the line");
    assert!(
        sim.players[qb].pos.dist(qb_spot) < 2.5,
        "the quarterback held his ground instead of chasing him across the field"
    );
}

/// The handoff is a meeting now, not a button: the quarterback runs the start
/// of the play, the man the ball is drawn for comes to him, and the ball changes
/// hands when they get there. Pressing nothing is not a way to keep it, and no
/// face button gives it away -- the buttons are the pitch.
#[test]
fn the_handoff_is_a_meeting_and_the_buttons_are_the_pitch() {
    let mut play = triple_option_play();
    play.name = "TE HANDOFF";
    play.family = PlayFamily::RunInside;
    // Both men are sent to the same spot: the quarterback's path is his start,
    // and where it crosses the runner's path is where the ball is given.
    let qb_spot = play.alignment(OffRole::Qb);
    let te_spot = play.alignment(OffRole::Slot);
    let meet = Vec2::new(
        (qb_spot.x + te_spot.x) * 0.5,
        (qb_spot.y + te_spot.y) * 0.5,
    );
    for (role, a) in play.assignments.iter_mut() {
        match role {
            OffRole::Qb => {
                *a = Assign::Route {
                    points: vec![Vec2::new(meet.x - qb_spot.x, meet.y - qb_spot.y)],
                    depth: 0,
                }
            }
            OffRole::Slot => {
                *a = Assign::Carry {
                    points: vec![
                        Vec2::new((meet.x + te_spot.x) * 0.5, 0.0),
                        Vec2::new(meet.x, meet.y - te_spot.y),
                    ],
                }
            }
            OffRole::WideL => {
                *a = Assign::Route {
                    points: vec![Vec2::new(-7.0, 11.0), Vec2::new(-6.5, 12.0)],
                    depth: 0,
                }
            }
            _ => {}
        }
    }
    let mut sim = to_the_ball_ai(play, 61);
    let qb = man(&sim, OffRole::Qb);
    let te = man(&sim, OffRole::Slot);
    let back = man(&sim, OffRole::WideL);
    // Nobody presses anything: the two men meet and the ball is given.
    let mut guard = 0;
    while sim.ball.holder == Some(qb) && sim.result.is_none() && guard < 600 {
        sim.step(1.0 / 60.0);
        guard += 1;
    }
    assert_eq!(
        sim.ball.holder,
        Some(te),
        "the handoff is made on the meeting: t={:.2} te={:?} qb={:?} events={:?}",
        sim.t,
        sim.players[te].pos,
        sim.players[qb].pos,
        sim.events.iter().rev().take(3).map(|e| e.text.clone()).collect::<Vec<_>>()
    );
    // And the man who already has it is not a button any more: the buttons are
    // the men behind the ball.
    let targets = sim.pitch_targets();
    assert!(
        !targets.iter().any(|(_, i)| *i == te),
        "the drawn carrier is not on a button: {targets:?}"
    );
    assert!(
        targets.iter().any(|(_, i)| *i == back),
        "the man behind him is on one: {targets:?}"
    );
}

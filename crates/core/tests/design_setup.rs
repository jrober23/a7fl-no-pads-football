//! What the designer sets about the quarterback's part of a play: the drop he
//! takes on a pass, and what kind of give he makes on a run.

use nopads_core::design::{self, DesignBook, Drop};
use nopads_core::math::Vec2;
use nopads_core::playbook::{Assign, Book, Handoff, OffRole, PlayFamily};

/// The designed book is one global, so the tests that draw into it take turns.
fn lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[test]
fn the_drop_and_the_handoff_are_written_into_the_play() {
    let _g = lock();
    design::clear();
    let mut book = DesignBook::from_built_in(Book::WestCoast);
    book.plays[0][0].family = PlayFamily::DropbackPass;
    book.plays[0][0].drop = Drop::Seven;
    book.plays[0][1].family = PlayFamily::RunInside;
    book.plays[0][1].handoff = Handoff::Lateral;
    let slot = design::add(book);
    let plays = design::offense_plays_at(slot);
    assert_eq!(plays[0].drop, 7.0, "the seven-step drop is the one drawn");
    assert_eq!(
        plays[1].handoff,
        Handoff::Lateral,
        "a run set to a toss says so on the field"
    );
    // A lateral can be made with the runner level with the ball; a plain
    // handoff cannot, and stays behind the line.
    assert!(plays[1].handoff.gives_it());
    // The league's own plays are the plain exchange, so nothing in the book the
    // game ships with changed meaning.
    assert_eq!(plays[0].handoff, Handoff::Inside);
    let _ = OffRole::Qb;
    design::clear();
}

/// The read option as a designer draws it: the quarterback keeps it on his own
/// path and the man the play was drawn for runs his as the pitch.
#[test]
fn a_designed_option_is_the_quarterbacks_play() {
    let _g = lock();
    design::clear();
    let mut book = DesignBook::blank();
    let form = book.offense[0].clone();
    let qb = form
        .roles
        .iter()
        .position(|r| *r == OffRole::Qb)
        .expect("the formation has a quarterback");
    let back = form
        .roles
        .iter()
        .position(|r| *r == OffRole::Back)
        .expect("and a back");
    book.plays[0][0].family = PlayFamily::RunInside;
    book.plays[0][0].handoff = Handoff::Option;
    book.plays[0][0].carrier = back;
    book.plays[0][0].routes[qb] = vec![Vec2::new(-1.0, 0.0), Vec2::new(1.0, 3.0)];
    book.plays[0][0].routes[back] = vec![Vec2::new(-8.0, 2.0), Vec2::new(-5.0, 5.0)];
    assert!(
        !book
            .problems()
            .iter()
            .any(|m| m.contains("AN OPTION NEEDS THE QB'S PATH")),
        "both paths are drawn, so the option is a play"
    );

    let slot = design::add(book.clone());
    let plays = design::offense_plays_at(slot);
    let p = &plays[0];
    assert_eq!(p.handoff, Handoff::Option, "the give is the read");
    assert!(!p.handoff.gives_it(), "an option is kept at the mesh");
    assert_eq!(
        p.carrier_role(),
        Some(OffRole::Qb),
        "the quarterback is the runner, not the man drawn for"
    );
    assert!(
        matches!(p.assignment(OffRole::Qb), Some(Assign::QbCarry { .. })),
        "he keeps on his own path"
    );
    assert!(
        matches!(p.assignment(OffRole::Back), Some(Assign::Route { .. })),
        "and the drawn man runs his, so he is there to pitch to"
    );

    // The keep is half the play: without the quarterback's path there is no
    // option to read, and the book says so rather than running a scramble.
    book.plays[0][0].routes[qb].clear();
    assert!(
        book.problems()
            .iter()
            .any(|m| m.contains("AN OPTION NEEDS THE QB'S PATH")),
        "an option with no keep is refused"
    );
    design::clear();
}

/// A designed option is the player's play: nobody gives it away at the mesh
/// until he pitches it. The quarterback keeps it for as long as he is on the pad.
#[test]
fn a_designed_option_is_not_given_away_at_the_mesh() {
    let _g = lock();
    design::clear();
    let mut book = DesignBook::blank();
    let form = book.offense[0].clone();
    let qb = form
        .roles
        .iter()
        .position(|r| *r == OffRole::Qb)
        .expect("a quarterback");
    let back = form
        .roles
        .iter()
        .position(|r| *r == OffRole::Back)
        .expect("a back");
    book.plays[0][0].family = PlayFamily::RunInside;
    book.plays[0][0].handoff = Handoff::Option;
    book.plays[0][0].carrier = back;
    book.plays[0][0].routes[qb] = vec![Vec2::new(-1.0, 0.0), Vec2::new(1.0, 3.0)];
    book.plays[0][0].routes[back] = vec![Vec2::new(-8.0, 2.0), Vec2::new(-5.0, 5.0)];
    let slot = design::add(book);
    let plays = design::offense_plays_at(slot);
    assert_eq!(plays[0].carrier_role(), Some(OffRole::Qb));

    let teams = Box::leak(Box::new(nopads_core::team::league_teams()));
    let defs = Box::leak(Box::new(nopads_core::playbook::defensive_playbook()));
    let play = Box::leak(Box::new(plays[0].clone()));
    let setup = nopads_core::sim::SnapSetup {
        los: 40.0,
        ball_y: 0.0,
        direction: 1,
        down: 1,
        to_go: 10.0,
        seed: 9,
        offense_team: 0,
        defense_team: 1,
        is_try: false,
    };
    let mut sim = nopads_core::sim::PlaySim::new(&teams[0], &teams[1], play, &defs[0], &setup);
    let keeper = (0..7)
        .find(|&i| OffRole::ALL[sim.players[i].role_index] == OffRole::Qb)
        .expect("the quarterback is on the field");
    sim.request_snap();
    // A player on the pad running the keep: the ball stays with him.
    for _ in 0..180 {
        sim.set_control(Some(nopads_core::sim::QbControl {
            move_dir: Vec2::new(1.0, 0.0),
            ..Default::default()
        }));
        sim.step(1.0 / 60.0);
        if sim.result.is_some() {
            break;
        }
        assert_eq!(
            sim.ball.holder,
            Some(keeper),
            "an option is kept until the player gives it"
        );
    }
    design::clear();
}

/// There is no plain handoff to choose any more: the inside handoff is the
/// plain exchange, and the editor cycles the four others from it.
#[test]
fn the_handoff_cycle_is_five_kinds_and_starts_inside() {
    let _g = lock();
    assert_eq!(Handoff::default(), Handoff::Inside);
    let mut seen = vec![Handoff::default()];
    let mut h = Handoff::default();
    while seen.len() < 5 {
        h = h.next();
        assert!(!seen.contains(&h), "the cycle repeats itself");
        seen.push(h);
    }
    assert_eq!(h.next(), Handoff::default(), "the cycle closes");
    for kind in [
        Handoff::Forward,
        Handoff::Inside,
        Handoff::Lateral,
        Handoff::Fake,
        Handoff::Option,
    ] {
        assert!(seen.contains(&kind), "{kind:?} is off the cycle");
    }
}

/// A sweep is given the ball the runner's way: the quarterback takes his inside
/// and hands it off, however the two paths are drawn.
#[test]
fn a_sweep_still_gets_the_ball_from_the_runners_inside() {
    let _g = lock();
    design::clear();
    let mut book = DesignBook::blank();
    let form = book.offense[0].clone();
    let qb = form.roles.iter().position(|r| *r == OffRole::Qb).unwrap();
    let back = form.roles.iter().position(|r| *r == OffRole::Back).unwrap();
    let qb_spot = form.spots[qb];
    let back_spot = form.spots[back];
    book.plays[0][0].family = PlayFamily::RunOutside;
    book.plays[0][0].handoff = Handoff::Inside;
    book.plays[0][0].carrier = back;
    // Both men sweep the same way, the quarterback a little behind and inside.
    book.plays[0][0].routes[qb] = vec![qb_spot + Vec2::new(-1.0, 4.0)];
    book.plays[0][0].routes[back] = vec![back_spot + Vec2::new(-1.0, 7.0)];
    let slot = design::add(book);
    let plays = design::offense_plays_at(slot);
    let teams = Box::leak(Box::new(nopads_core::team::league_teams()));
    let defs = Box::leak(Box::new(nopads_core::playbook::defensive_playbook()));
    let play = Box::leak(Box::new(plays[0].clone()));
    let setup = nopads_core::sim::SnapSetup {
        los: 40.0,
        ball_y: 0.0,
        direction: 1,
        down: 1,
        to_go: 10.0,
        seed: 3,
        offense_team: 0,
        defense_team: 1,
        is_try: false,
    };
    let mut sim = nopads_core::sim::PlaySim::new(&teams[0], &teams[1], play, &defs[0], &setup);
    sim.request_snap();
    let find = |sim: &nopads_core::sim::PlaySim, role: OffRole| {
        (0..7)
            .find(|&i| OffRole::ALL[sim.players[i].role_index] == role)
            .map(|i| sim.players[i].pos.y)
            .expect("the man is on the field")
    };
    let mut took = None;
    // The sweep goes to +y, so the give is made from the runner's inside: his
    // side of the field, with the quarterback on the other side of him.
    let mut gap = None;
    // And the give is a thing offered before it is a thing taken: how much of
    // the ball is shown, and how far round he has turned to show it.
    let mut offer = 0.0f32;
    let mut facing: Option<f32> = None;
    for _ in 0..600 {
        if sim.handoff.is_some() {
            let men = |role: OffRole| {
                (0..7).find(|&i| OffRole::ALL[sim.players[i].role_index] == role)
            };
            if let (Some(q), Some(b)) = (men(OffRole::Qb), men(OffRole::Back)) {
                if sim.ball.holder == Some(q) {
                    let give = sim.players[q].give;
                    offer = offer.max(give);
                    if give > 0.3 {
                        assert!(
                            sim.ball.pos.y > sim.players[q].pos.y,
                            "the ball was not held out the runner's way: {:.2} against {:.2}",
                            sim.ball.pos.y,
                            sim.players[q].pos.y
                        );
                    }
                    let to = sim.players[b].pos - sim.players[q].pos;
                    let want = to.y.atan2(to.x);
                    let off = (want - sim.players[q].heading + std::f32::consts::PI)
                        .rem_euclid(std::f32::consts::TAU)
                        - std::f32::consts::PI;
                    facing = Some(facing.map_or(off.abs(), |f: f32| f.min(off.abs())));
                }
            }
        }
        let (qb_y, back_y) = (find(&sim, OffRole::Qb), find(&sim, OffRole::Back));
        sim.step(1.0 / 60.0);
        if let Some(e) = sim
            .events
            .iter()
            .find(|e| e.text.contains("takes the inside handoff"))
        {
            took = Some(e.text.clone());
            gap = Some(qb_y - back_y);
            break;
        }
        if sim.result.is_some() {
            break;
        }
    }
    let gap = gap.expect("the handoff happened");
    assert!(
        gap <= 1.1,
        "the ball was handed off from the runner's far side: {gap} yards"
    );
    let took = took.unwrap_or_else(|| {
        panic!(
            "the runner never got it: {:?}",
            sim.events.iter().map(|e| &e.text).collect::<Vec<_>>()
        )
    });
    assert!(took.contains("takes the inside handoff"), "{took}");
    assert!(
        offer > 0.4,
        "he never held the ball out to the runner: {offer:.2}"
    );
    assert!(
        facing.map(|f| f < 0.7).unwrap_or(false),
        "his shoulders never turned to the runner: {facing:?} rad"
    );
    design::clear();
}

/// A pass can be drawn as a play action: the family says so on the field, and
/// nothing changes hands.
#[test]
fn a_pass_can_be_drawn_as_a_play_action() {
    let _g = lock();
    design::clear();
    let mut book = DesignBook::from_built_in(Book::WestCoast);
    let form = book.offense[0].clone();
    let wr = form
        .roles
        .iter()
        .position(|r| *r == OffRole::WideL)
        .expect("a wide receiver");
    book.plays[0][0].family = PlayFamily::PlayAction;
    book.plays[0][0].routes[wr] = vec![Vec2::new(14.0, -2.0)];
    let slot = design::add(book);
    let plays = design::offense_plays_at(slot);
    assert_eq!(plays[0].family, PlayFamily::PlayAction);
    assert!(plays[0].family.is_pass(), "a play action is a pass");
    assert_eq!(
        plays[0].carrier_role(),
        None,
        "nothing changes hands on a play action"
    );
    design::clear();
}

/// A designed run goes where the player drew it. A back lines up behind the line
/// and runs to a point drawn from the line, so an outside run drawn to the left
/// starts going left instead of forward-left.
#[test]
fn a_designed_run_starts_the_way_it_was_drawn() {
    let _g = lock();
    design::clear();
    let mut book = DesignBook::blank();
    let form = book.offense[0].clone();
    let back = form
        .roles
        .iter()
        .position(|r| *r == OffRole::Back)
        .expect("a back");
    let spot = form.spots[back];
    book.plays[0][0].family = PlayFamily::RunOutside;
    book.plays[0][0].carrier = back;
    let drawn = [spot + Vec2::new(-1.0, -6.0), spot + Vec2::new(2.0, -9.0)];
    book.plays[0][0].routes[back] = drawn.to_vec();
    let slot = design::add(book);
    let plays = design::offense_plays_at(slot);

    let teams = Box::leak(Box::new(nopads_core::team::league_teams()));
    let defs = Box::leak(Box::new(nopads_core::playbook::defensive_playbook()));
    let play = Box::leak(Box::new(plays[0].clone()));
    let los = 40.0;
    let setup = nopads_core::sim::SnapSetup {
        los,
        ball_y: 0.0,
        direction: 1,
        down: 1,
        to_go: 10.0,
        seed: 5,
        offense_team: 0,
        defense_team: 1,
        is_try: false,
    };
    let mut sim = nopads_core::sim::PlaySim::new(&teams[0], &teams[1], play, &defs[0], &setup);
    let runner = (0..7)
        .find(|&i| OffRole::ALL[sim.players[i].role_index] == OffRole::Back)
        .expect("the back is on the field");

    // The point on the field is the point on the card: the run is laid from the
    // line, not from however deep the runner happens to line up.
    let first = sim.players[runner].route[0];
    assert!(
        (first.x - los - drawn[0].x).abs() < 0.05 && (first.y - drawn[0].y).abs() < 0.05,
        "the drawn point moved: {first:?} against {:?}",
        drawn[0]
    );
    assert!(
        (sim.players[runner].pos.x - los - spot.x).abs() < 0.05,
        "the back is not standing where the formation puts him"
    );

    sim.request_snap();
    let start = {
        while sim.ball.holder.is_none() && !sim.result.is_some() {
            sim.step(1.0 / 60.0);
        }
        for _ in 0..4 {
            sim.step(1.0 / 60.0);
        }
        sim.players[runner].pos
    };
    let mut seen = 0;
    for _ in 0..20 {
        if sim.result.is_some() {
            break;
        }
        let before = sim.players[runner].pos;
        sim.step(1.0 / 60.0);
        seen += 1;
        let _ = before;
    }
    assert!(seen > 0, "the play ended before he moved");
    let now = sim.players[runner].pos;
    assert!(
        start.y - now.y > 0.6,
        "he did not start to the side he was drawn: {:.2} yards across",
        start.y - now.y
    );
    assert!(
        now.x - start.x < 0.6,
        "he started forward instead of to the side: {:.2} yards downfield",
        now.x - start.x
    );
    design::clear();
}

/// The runner the play was drawn for does not stop at the mesh: with the stick
/// at rest he keeps running his own path, and the stick still owns him the
/// moment it moves.
#[test]
fn the_runner_keeps_running_the_play_with_the_stick_at_rest() {
    let _g = lock();
    design::clear();
    let mut book = DesignBook::blank();
    let form = book.offense[0].clone();
    let back = form
        .roles
        .iter()
        .position(|r| *r == OffRole::Back)
        .expect("a back");
    let spot = form.spots[back];
    book.plays[0][0].family = PlayFamily::RunOutside;
    book.plays[0][0].carrier = back;
    let drawn = [spot + Vec2::new(-1.0, -3.0), spot + Vec2::new(3.0, -8.0)];
    book.plays[0][0].routes[back] = drawn.to_vec();
    let slot = design::add(book);
    let plays = design::offense_plays_at(slot);

    let teams = Box::leak(Box::new(nopads_core::team::league_teams()));
    let defs = Box::leak(Box::new(nopads_core::playbook::defensive_playbook()));
    let play = Box::leak(Box::new(plays[0].clone()));
    let los = 40.0;
    let setup = nopads_core::sim::SnapSetup {
        los,
        ball_y: 0.0,
        direction: 1,
        down: 1,
        to_go: 10.0,
        seed: 7,
        offense_team: 0,
        defense_team: 1,
        is_try: false,
    };
    let mut sim = nopads_core::sim::PlaySim::new(&teams[0], &teams[1], play, &defs[0], &setup);
    let runner = (0..7)
        .find(|&i| OffRole::ALL[sim.players[i].role_index] == OffRole::Back)
        .expect("the back is on the field");
    sim.request_snap();
    while sim.ball.holder.is_none() && !sim.result.is_some() {
        sim.step(1.0 / 60.0);
    }
    // Put him on his own path with nobody near him, so what he does next is the
    // only thing moving him: the handoff is behind him and the ball is his.
    let first = sim.players[runner].route[0];
    sim.players[runner].pos = first;
    sim.players[runner].vel = Vec2::ZERO;
    sim.players[runner].waypoint = 0;
    sim.carrier = Some(runner);
    sim.ball.holder = Some(runner);
    sim.handoff = None;
    for d in 7..14 {
        sim.players[d].pos = Vec2::new(los - 14.0, 20.0);
        sim.players[d].vel = Vec2::ZERO;
    }
    sim.set_control(Some(nopads_core::sim::QbControl {
        move_dir: Vec2::ZERO,
        ..Default::default()
    }));
    let start = sim.players[runner].pos;
    for _ in 0..50 {
        if sim.result.is_some() {
            break;
        }
        sim.step(1.0 / 60.0);
    }
    let coast = sim.players[runner].pos;
    assert!(
        start.y - coast.y > 1.5,
        "he stopped at the mesh instead of running the play: {:.2} yards across",
        start.y - coast.y
    );

    // And the stick takes him straight back.
    sim.set_control(Some(nopads_core::sim::QbControl {
        move_dir: Vec2::new(1.0, 0.0),
        ..Default::default()
    }));
    let x0 = sim.players[runner].pos.x;
    for _ in 0..25 {
        if sim.result.is_some() {
            break;
        }
        sim.step(1.0 / 60.0);
    }
    assert!(
        sim.players[runner].pos.x - x0 > 0.8,
        "the stick did not take him: {:.2} yards downfield",
        sim.players[runner].pos.x - x0
    );
    design::clear();
}

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
    for _ in 0..600 {
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
    design::clear();
}

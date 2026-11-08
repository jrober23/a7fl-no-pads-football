//! What the designer sets about the quarterback's part of a play: the drop he
//! takes on a pass, and what kind of give he makes on a run.

use nopads_core::design::{self, DesignBook, Drop};
use nopads_core::math::Vec2;
use nopads_core::playbook::{Assign, Book, Handoff, OffRole, PlayFamily};

#[test]
fn the_drop_and_the_handoff_are_written_into_the_play() {
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
    assert_eq!(plays[0].handoff, Handoff::Plain);
    let _ = OffRole::Qb;
    design::clear();
}

/// The read option as a designer draws it: the quarterback keeps it on his own
/// path and the man the play was drawn for runs his as the pitch.
#[test]
fn a_designed_option_is_the_quarterbacks_play() {
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

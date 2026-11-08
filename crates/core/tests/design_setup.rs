//! What the designer sets about the quarterback's part of a play: the drop he
//! takes on a pass, and what kind of give he makes on a run.

use nopads_core::design::{self, DesignBook, Drop};
use nopads_core::playbook::{Book, Handoff, OffRole, PlayFamily};

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

//! The layered call sheet: the formations down one side, the plays of the
//! highlighted formation on the cards.
//!
//! A flat list of two dozen plays is a wall of cards nobody can read in ten
//! seconds at the line. A coach's file is not a list: it is formations, and inside
//! each formation the handful of things that formation runs. This draws that, from
//! the same `Play` data the simulation runs, so the card and the snap cannot
//! disagree.

use a7fl_core::playbook::{Formation, Play};

use crate::playsheet::{self, Dir};
use crate::raster::Frame;

/// Width of the formation column.
const SIDE: i32 = 132;
const MARGIN: i32 = 11;
const GAP: i32 = 6;
/// Cards across in the play pane.
pub const COLS: usize = 3;

/// Move a cursor inside one formation's plays, wrapping round the grid.
pub fn step_in(len: usize, cursor: usize, dir: Dir) -> usize {
    if len == 0 {
        return 0;
    }
    let cursor = cursor.min(len - 1);
    let rows = (len + COLS - 1) / COLS;
    let col = cursor % COLS;
    let row = cursor / COLS;
    let next = match dir {
        Dir::Right => row * COLS + (col + 1) % COLS,
        Dir::Left => row * COLS + (col + COLS - 1) % COLS,
        Dir::Down => ((row + 1) % rows) * COLS + col,
        Dir::Up => ((row + rows - 1) % rows) * COLS + col,
    };
    next.min(len - 1)
}

/// Draw the sheet. `form_pick` is the formation on the left, `play_pick` the play
/// inside it, and `in_plays` says which column the stick is driving.
pub fn draw_layered(
    frame: &mut Frame,
    book: &[Play],
    formations: &[Formation],
    form_pick: usize,
    play_pick: usize,
    in_plays: bool,
    flipped: bool,
) {
    playsheet::draw_panel(frame, formations, form_pick, in_plays, flipped);
    if formations.is_empty() {
        return;
    }
    let list: Vec<usize> = a7fl_core::playbook::plays_in(book, formations[form_pick.min(formations.len() - 1)]);
    let x0 = MARGIN + SIDE + GAP;
    let w = frame.w as i32 - x0 - MARGIN;
    let top = 26;
    let bottom = frame.h as i32 - 64;
    let card_w = (w - (COLS as i32 - 1) * GAP) / COLS as i32;
    let rows = ((list.len() + COLS - 1) / COLS).max(1) as i32;
    let card_h = (bottom - top - (rows - 1) * GAP) / rows;
    if list.is_empty() {
        return;
    }
    for (slot, &index) in list.iter().enumerate() {
        let col = (slot % COLS) as i32;
        let row = (slot / COLS) as i32;
        let x = x0 + col * (card_w + GAP);
        let y = top + row * (card_h + GAP);
        playsheet::draw_play_card(
            frame,
            &book[index],
            x,
            y,
            card_w,
            card_h,
            slot == play_pick.min(list.len().saturating_sub(1)),
            flipped,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_cursor_stays_inside_the_formation() {
        for len in [0usize, 1, 2, 5, 6, 8, 9] {
            for cursor in 0..len.max(1) {
                for dir in [Dir::Left, Dir::Right, Dir::Up, Dir::Down] {
                    let next = step_in(len, cursor, dir);
                    if len > 0 {
                        assert!(next < len, "step_in({len}, {cursor}, {dir:?}) -> {next}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_formation_with_no_plays_draws_nothing_and_panics_at_nothing() {
        let book = a7fl_core::playbook::playbook();
        let mut frame = Frame::new(640, 360);
        draw_layered(&mut frame, &book, &[Formation::Empty], 0, 0, true, false);
    }

    #[test]
    fn the_layered_sheet_keeps_the_text_bar_clear() {
        let book = a7fl_core::playbook::playbook();
        let forms = a7fl_core::playbook::formations_in(&book);
        for (i, _) in forms.iter().enumerate() {
            let mut frame = Frame::new(640, 360);
            draw_layered(&mut frame, &book, &forms, i, 0, true, false);
            for y in 296..360 {
                for x in 0..640 {
                    assert_eq!(
                        frame.color[y * 640 + x],
                        a7fl_core::math::Rgb::hex(0x0B0F16),
                        "card into the bar at {x},{y} on formation {i}"
                    );
                }
            }
        }
    }
}

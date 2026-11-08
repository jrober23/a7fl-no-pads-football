//! The call sheet: the playbook as a grid of diagrams, one set at a time.
//!
//! Each card is a small chalkboard of the play it calls — the formation, every
//! route, the run path and the blocks — drawn from the same `Assign` data the
//! simulation runs, so a card can never advertise a play the game does not play.
//! The client highlights one with the d-pad and snaps it with the A button.

use nopads_core::math::{Rgb, Vec2};
use nopads_core::playbook::{personnel_label, Assign, Formation, OffRole, Play};

use crate::raster::Frame;

/// Cards across.
pub const COLS: usize = 4;
/// Cards down.
pub const ROWS: usize = 2;
/// Plays on screen at once: one set of the call sheet.
pub const PER_PAGE: usize = COLS * ROWS;

pub(crate) const BACKDROP: Rgb = Rgb::hex(0x0B0F16);
pub(crate) const HEADER: Rgb = Rgb::hex(0x121A26);
pub(crate) const CARD: Rgb = Rgb::hex(0x16202C);
pub(crate) const CARD_ON: Rgb = Rgb::hex(0x22303F);
pub(crate) const EDGE: Rgb = Rgb::hex(0x2C3540);
pub(crate) const TURF: Rgb = Rgb::hex(0x1B3B24);
pub(crate) const TURF_LINE: Rgb = Rgb::hex(0x2E5A36);
pub(crate) const LOS: Rgb = Rgb::hex(0xBFC8D4);
pub(crate) const INK: Rgb = Rgb::hex(0xF4F6F8);
pub(crate) const CARRY: Rgb = Rgb::hex(0xF2C14E);
pub(crate) const BLOCK: Rgb = Rgb::hex(0x74838F);
pub(crate) const MUTED: Rgb = Rgb::hex(0xA9B3BF);
pub(crate) const ACCENT: Rgb = Rgb::hex(0xFFC440);

pub(crate) const MARGIN: i32 = 11;
pub(crate) const GAP: i32 = 6;
const TOP: i32 = 26;
/// Left clear for the client's text bar, which is laid over the bottom of the
/// frame after this draws.
const TEXT_BAR: i32 = 64;

/// A direction the player pushed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// Which set of plays the cursor is sitting in.
pub fn page_of(cursor: usize) -> usize {
    cursor / PER_PAGE
}

/// Where the cursor lands when the player pushes `dir`.
///
/// Sideways stays inside the set on screen and wraps round it; up and down move
/// between sets, dropping off the bottom row into the top of the next one, which
/// is how a call sheet is meant to scroll.
pub fn step(len: usize, cursor: usize, dir: Dir) -> usize {
    if len == 0 {
        return 0;
    }
    let cursor = cursor.min(len - 1);
    let page = page_of(cursor);
    let off = cursor % PER_PAGE;
    let col = off % COLS;
    let row = off / COLS;
    let next = match dir {
        Dir::Right => page * PER_PAGE + (off + 1) % PER_PAGE,
        Dir::Left => page * PER_PAGE + (off + PER_PAGE - 1) % PER_PAGE,
        Dir::Up => {
            if row > 0 {
                cursor - COLS
            } else if page > 0 {
                (page - 1) * PER_PAGE + (ROWS - 1) * COLS + col
            } else {
                cursor
            }
        }
        Dir::Down => {
            if row + 1 < ROWS {
                cursor + COLS
            } else if (page + 1) * PER_PAGE < len {
                (page + 1) * PER_PAGE + col
            } else {
                cursor
            }
        }
    };
    next.min(len - 1)
}

/// Draw the whole call sheet, with `cursor` highlighted.
pub fn draw(frame: &mut Frame, book: &[Play], cursor: usize) {
    let w = frame.w as i32;
    let h = frame.h as i32;
    frame.clear(BACKDROP);
    frame.fill_rect(0, 0, w, 22, HEADER);
    frame.fill_rect(0, 22, w, 1, EDGE);
    frame.text(MARGIN, 4, "CALL SHEET", 2, INK);
    if book.is_empty() {
        return;
    }

    let cursor = cursor.min(book.len() - 1);
    let pages = (book.len() + PER_PAGE - 1) / PER_PAGE;
    let label = format!("SET {} / {}", page_of(cursor) + 1, pages);
    let lw = (label.chars().count() * 6) as i32;
    frame.text(w - MARGIN - lw, 8, &label, 1, MUTED);

    let card_w = (w - 2 * MARGIN - (COLS as i32 - 1) * GAP) / COLS as i32;
    let card_h = (h - TEXT_BAR - TOP - GAP - 2) / ROWS as i32;
    for slot in 0..PER_PAGE {
        let index = page_of(cursor) * PER_PAGE + slot;
        if index >= book.len() {
            break;
        }
        let x = MARGIN + (slot % COLS) as i32 * (card_w + GAP);
        let y = TOP + (slot / COLS) as i32 * (card_h + GAP);
        draw_card(frame, &book[index], x, y, card_w, card_h, index == cursor);
    }
}

fn draw_card(frame: &mut Frame, play: &Play, x: i32, y: i32, w: i32, h: i32, selected: bool) {
    let bg = if selected { CARD_ON } else { CARD };
    frame.rounded_rect(x, y, w, h, 4, if selected { ACCENT } else { EDGE });
    frame.rounded_rect(x + 1, y + 1, w - 2, h - 2, 3, bg);

    // The name, the diagram, then what kind of play it is.
    let fits = ((w - 10) / 6) as usize;
    let name: String = play.name.chars().take(fits).collect();
    frame.text(x + 5, y + 5, &name, 1, if selected { ACCENT } else { INK });

    let foot = format!("{} / {}", play.family.label(), play.formation.label());
    let foot: String = foot.chars().take(fits).collect();
    frame.text(x + 5, y + h - 11, &foot, 1, MUTED);

    let (fx, fy) = (x + 5, y + 15);
    let (fw, fh) = (w - 10, h - 15 - 14);
    frame.fill_rect(fx, fy, fw, fh, TURF);
    let board = Board::new(&diagram_points(play), fx, fy, fw, fh);

    // Yard lines, measured from the line of scrimmage so every card in the sheet
    // lines up with its neighbours.
    for d in [-10.0f32, -5.0, 5.0, 10.0, 15.0, 20.0] {
        let ly = (board.los - d * board.sx).round() as i32;
        if ly > fy + 1 && ly < fy + fh - 2 {
            frame.line(fx + 1, ly, fx + fw - 2, ly, TURF_LINE, 1);
        }
    }
    // The line of scrimmage, so the routes read as going somewhere.
    let los_y = board.los.round() as i32;
    frame.line(fx, los_y, fx + fw - 1, los_y, LOS, 1);

    for (role, assign) in &play.assignments {
        let base = play.alignment(*role);
        match assign {
            Assign::Route { points, .. } => trail(frame, &board, base, points, INK, 1),
            Assign::Fake { points, .. } => trail(frame, &board, base, points, BLOCK, 1),
            // A run path is laid from the line of scrimmage, so the card draws
            // the ground the field runs rather than that path moved by the depth
            // he lines up at.
            Assign::Carry { points } | Assign::QbCarry { points } => {
                let run: Vec<Vec2> = points.iter().map(|p| Vec2::new(p.x - base.x, p.y)).collect();
                trail(frame, &board, base, &run, CARRY, 2)
            }
            Assign::RunBlock { aim, .. } | Assign::ScreenBlock { aim } => {
                let (sx, sy) = board.at(base);
                let (tx, ty) = board.at(base + *aim);
                frame.line(sx, sy, tx, ty, BLOCK, 1);
            }
            Assign::PassBlock { .. } => {}
        }
    }

    // The men themselves: skill players as dots, the line as ticks.
    for (role, _) in &play.assignments {
        let (px, py) = board.at(play.alignment(*role));
        match role {
            OffRole::Qb => frame.fill_rect(px - 1, py - 1, 3, 3, ACCENT),
            OffRole::Lg | OffRole::Rg => frame.fill_rect(px - 1, py, 3, 1, BLOCK),
            _ => frame.fill_rect(px - 1, py - 1, 3, 3, INK),
        }
    }
}

/// A route or a run path, from the man's alignment through every waypoint, with
/// an arrowhead on the last leg so it reads as going somewhere.
fn trail(frame: &mut Frame, board: &Board, base: Vec2, points: &[Vec2], color: Rgb, thickness: i32) {
    if points.is_empty() {
        return;
    }
    let mut pen = board.at(base);
    let mut before = pen;
    let mut at = base;
    for point in points {
        at = base + *point;
        let next = board.at(at);
        frame.line(pen.0, pen.1, next.0, next.1, color, thickness);
        before = pen;
        pen = next;
    }

    let (dx, dy) = ((pen.0 - before.0) as f32, (pen.1 - before.1) as f32);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    for side in [-1.0f32, 1.0] {
        let bx = pen.0 - (ux * 3.4 - side * uy * 2.2).round() as i32;
        let by = pen.1 - (uy * 3.4 + side * ux * 2.2).round() as i32;
        frame.line(pen.0, pen.1, bx, by, color, thickness);
    }
}

/// Every point a diagram draws, in field yards: `x` downfield and `y` across,
/// relative to the line of scrimmage.
fn diagram_points(play: &Play) -> Vec<Vec2> {
    let mut out = Vec::new();
    for (role, assign) in &play.assignments {
        let base = play.alignment(*role);
        out.push(base);
        match assign {
            Assign::Route { points, .. } | Assign::Fake { points, .. } => {
                out.extend(points.iter().map(|p| base + *p))
            }
            Assign::Carry { points } | Assign::QbCarry { points } => {
                out.extend(points.iter().map(|p| Vec2::new(p.x, base.y + p.y)))
            }
            Assign::RunBlock { aim, .. } | Assign::ScreenBlock { aim } => out.push(base + *aim),
            Assign::PassBlock { .. } => {}
        }
    }
    out
}

/// Maps field yards onto one card's chalkboard.
///
/// Downfield and across are scaled separately, the way a printed play card is:
/// the width of the card is the width of the field, and the height is however
/// deep that play goes. A dive and a twenty-five yard shot then both fill their
/// card instead of one being a dot in the middle. The line of scrimmage sits a
/// fixed distance off the bottom, so the cards read as a set.
struct Board {
    x: f32,
    w: f32,
    top: f32,
    bottom: f32,
    /// Pixels per yard, downfield and across.
    sx: f32,
    sy: f32,
    /// Screen row of the line of scrimmage.
    los: f32,
}

impl Board {
    fn new(points: &[Vec2], x: i32, y: i32, w: i32, h: i32) -> Board {
        // Behind the line there is only the backfield; ahead of it there may be
        // twenty-five yards of route.
        let behind = 4.5f32;
        let ahead = points.iter().fold(7.0f32, |deepest, p| deepest.max(p.x)) + 1.5;
        let sx = h as f32 / (behind + ahead);
        // Thirty yards across fills the card: the widest alignment in the book is
        // twenty-eight.
        let sy = w as f32 / 30.0;
        Board {
            x: x as f32,
            w: w as f32,
            top: y as f32,
            bottom: (y + h) as f32,
            sx,
            sy,
            los: (y + h) as f32 - behind * sx,
        }
    }

    /// Field yards to pixels, held inside the card so a diagram can never spill
    /// out over its neighbours.
    fn at(&self, p: Vec2) -> (i32, i32) {
        let px = self.x + self.w * 0.5 + p.y * self.sy;
        let py = self.los - p.x * self.sx;
        (
            (px.round() as i32).clamp(self.x as i32 + 2, (self.x + self.w) as i32 - 3),
            (py.round() as i32).clamp(self.top as i32 + 2, self.bottom as i32 - 3),
        )
    }
}

/// The formation column of the layered sheet: the first thing a player picks.
///
/// Drawn here rather than in the client so the whole sheet is one picture on one
/// canvas, and so the column and the cards share a palette.
pub fn draw_panel(
    frame: &mut Frame,
    formations: &[Formation],
    form_pick: usize,
    in_plays: bool,
    flipped: bool,
) {
    let w = frame.w as i32;
    frame.clear(BACKDROP);
    frame.fill_rect(0, 0, w, 22, HEADER);
    frame.fill_rect(0, 22, w, 1, EDGE);
    frame.text(MARGIN, 4, "CALL SHEET", 2, INK);
    if flipped {
        let tw = ("FLIPPED".chars().count() * 6) as i32;
        frame.text(w - MARGIN - tw, 8, "FLIPPED", 1, ACCENT);
    }

    let x = MARGIN;
    let box_w = 118;
    let top = 30;
    let row_h = 30;
    let rows = formations.len() as i32;
    frame.rounded_rect(x, top - 8, box_w, rows * row_h + 14, 4, EDGE);
    for (i, f) in formations.iter().enumerate() {
        let y = top + i as i32 * row_h;
        let on = i == form_pick;
        if on {
            frame.rounded_rect(x + 3, y - 3, box_w - 6, row_h - 4, 3, CARD_ON);
        }
        let mark = if on && in_plays { ">" } else if on { "*" } else { " " };
        let color = if on { ACCENT } else { MUTED };
        frame.text(x + 7, y + 1, &format!("{mark} {}", f.label()), 1, color);
        frame.text(x + 16, y + 12, personnel_label(*f), 1, MUTED);
    }
}

/// One card of the layered sheet. The flip is drawn, not implied: a card that
/// runs right has to show the routes going right.
pub fn draw_play_card(
    frame: &mut Frame,
    play: &Play,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    selected: bool,
    flipped: bool,
) {
    let card = if flipped { play.flipped() } else { play.clone() };
    draw_card(frame, &card, x, y, w, h, selected);
    if flipped {
        frame.text(x + w - 26, y + 5, "FLIP", 1, ACCENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nopads_core::playbook::playbook;

    #[test]
    fn sideways_stays_in_the_set_and_wraps() {
        assert_eq!(step(22, 0, Dir::Right), 1);
        assert_eq!(step(22, 7, Dir::Right), 0, "right off the last card wraps the set");
        assert_eq!(step(22, 0, Dir::Left), 7, "left off the first card wraps back");
        assert_eq!(step(22, 3, Dir::Left), 2);
    }

    #[test]
    fn up_and_down_move_between_sets() {
        // Down from the top row drops into the bottom row of the same set.
        assert_eq!(step(22, 1, Dir::Down), 5);
        // Down from the bottom row is the next set, in the same column.
        assert_eq!(step(22, 5, Dir::Down), 9);
        // Up from the top row is the previous set's bottom row.
        assert_eq!(step(22, 9, Dir::Up), 5);
        assert_eq!(step(22, 1, Dir::Up), 1, "there is nothing above the first set");
        assert_eq!(step(22, 21, Dir::Down), 21, "and nothing below the last");
    }

    #[test]
    fn the_cursor_never_leaves_the_book() {
        for len in [0usize, 1, 5, 8, 22] {
            for cursor in 0..len.max(1) {
                for dir in [Dir::Left, Dir::Right, Dir::Up, Dir::Down] {
                    let next = step(len, cursor, dir);
                    if len > 0 {
                        assert!(next < len, "step({len}, {cursor}, {dir:?}) -> {next}");
                    }
                }
            }
        }
    }

    #[test]
    fn every_play_draws_a_diagram() {
        let book = playbook();
        for (i, play) in book.iter().enumerate() {
            let mut frame = Frame::new(640, 360);
            draw(&mut frame, &book, i);
            // The routes and the men are drawn in these colours; a card with none
            // of them is an empty green box.
            let marks = frame
                .color
                .iter()
                .filter(|c| **c == INK || **c == CARRY || **c == ACCENT)
                .count();
            assert!(marks > 40, "{} ({i}) drew almost nothing: {marks} px", play.name);
        }
    }

    #[test]
    fn the_text_bar_stays_clear() {
        let book = playbook();
        let mut frame = Frame::new(640, 360);
        draw(&mut frame, &book, book.len() - 1);
        let bottom = 360 - TEXT_BAR as usize;
        for y in bottom..360 {
            for x in 0..640 {
                assert_eq!(frame.color[y * 640 + x], BACKDROP, "card spilt into the bar at {x},{y}");
            }
        }
    }

    #[test]
    fn different_plays_look_different() {
        let book = playbook();
        let mut a = Frame::new(640, 360);
        let mut b = Frame::new(640, 360);
        draw(&mut a, &book, 0);
        draw(&mut b, &book, 1);
        assert!(a.color != b.color, "two different plays drew the same picture");
    }
}

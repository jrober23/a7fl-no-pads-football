//! The defensive call sheet: the fronts down one side, the calls of the
//! highlighted front on the cards.
//!
//! The offense has been layered since the call sheet was rebuilt — formation, then
//! play — while defense was still one flat list of nine names with a coverage
//! label. A defense is not called that way. The front is chosen first, the coverage
//! comes with it, and the line technique belongs to the front. This draws the same
//! kind of picture the offensive sheet does, from the same `DefCall` data the snap
//! is played with, so a card can never advertise a defence the game does not run.

use nopads_core::math::{Rgb, Vec2};
use nopads_core::playbook::{def_calls_in, defender_positions, DefCall, DefFront, DefRole};

use crate::playsheet::{
    ACCENT, BACKDROP, BLOCK, CARD, CARD_ON, CARRY, EDGE, GAP, HEADER, INK, LOS, MARGIN, MUTED, TURF,
    TURF_LINE,
};
use crate::raster::Frame;

/// Cards across in the call pane.
pub const COLS: usize = 3;
/// Width of the front column.
const SIDE: i32 = 132;

/// Draw the sheet. `front_pick` is the front on the left, `call_pick` the call
/// inside it, and `in_calls` says which column the stick is driving.
pub fn draw_layered(
    frame: &mut Frame,
    book: &[DefCall],
    fronts: &[DefFront],
    front_pick: usize,
    call_pick: usize,
    in_calls: bool,
) {
    draw_panel(frame, fronts, front_pick, in_calls);
    if fronts.is_empty() {
        return;
    }
    let list = def_calls_in(book, fronts[front_pick.min(fronts.len() - 1)]);
    if list.is_empty() {
        return;
    }
    let x0 = MARGIN + SIDE + GAP;
    let w = frame.w as i32 - x0 - MARGIN;
    let top = 26;
    let bottom = frame.h as i32 - 64;
    let card_w = (w - (COLS as i32 - 1) * GAP) / COLS as i32;
    let rows = ((list.len() + COLS - 1) / COLS).max(1) as i32;
    let card_h = (bottom - top - (rows - 1) * GAP) / rows;
    let selected = call_pick.min(list.len() - 1);
    for (slot, &index) in list.iter().enumerate() {
        let x = x0 + (slot % COLS) as i32 * (card_w + GAP);
        let y = top + (slot / COLS) as i32 * (card_h + GAP);
        draw_call_card(frame, &book[index], x, y, card_w, card_h, slot == selected);
    }
}

/// The front column: the front, its two techniques and what the line is being
/// asked to do with the run.
fn draw_panel(frame: &mut Frame, fronts: &[DefFront], pick: usize, in_calls: bool) {
    let w = frame.w as i32;
    frame.clear(BACKDROP);
    frame.fill_rect(0, 0, w, 22, HEADER);
    frame.fill_rect(0, 22, w, 1, EDGE);
    frame.text(MARGIN, 4, "DEFENSIVE CALL SHEET", 2, INK);

    let x = MARGIN;
    let box_w = 118;
    let top = 30;
    let row_h = 34;
    frame.rounded_rect(x, top - 8, box_w, fronts.len() as i32 * row_h + 14, 4, EDGE);
    for (i, f) in fronts.iter().enumerate() {
        let y = top + i as i32 * row_h;
        let on = i == pick;
        if on {
            frame.rounded_rect(x + 3, y - 3, box_w - 6, row_h - 4, 3, CARD_ON);
        }
        let mark = if on && in_calls {
            ">"
        } else if on {
            "*"
        } else {
            " "
        };
        let color = if on { ACCENT } else { MUTED };
        frame.text(x + 7, y + 1, &format!("{mark} {}", f.label()), 1, color);
        frame.text(x + 16, y + 12, &format!("{} TECH", f.techs()), 1, MUTED);
        frame.text(x + 16, y + 22, f.line_call(), 1, MUTED);
    }
}

fn draw_call_card(frame: &mut Frame, call: &DefCall, x: i32, y: i32, w: i32, h: i32, selected: bool) {
    frame.rounded_rect(x, y, w, h, 4, if selected { ACCENT } else { EDGE });
    frame.rounded_rect(x + 1, y + 1, w - 2, h - 2, 3, if selected { CARD_ON } else { CARD });

    let fits = ((w - 10) / 6) as usize;
    let name: String = call.name.chars().take(fits).collect();
    frame.text(x + 5, y + 5, &name, 1, if selected { ACCENT } else { INK });
    let foot = format!("{} - {} RUSH", call.coverage.label(), call.rushers());
    let foot: String = foot.chars().take(fits).collect();
    frame.text(x + 5, y + h - 11, &foot, 1, MUTED);

    let (fx, fy) = (x + 5, y + 15);
    let (fw, fh) = (w - 10, h - 15 - 14);
    frame.fill_rect(fx, fy, fw, fh, TURF);
    let board = Board::new(fx, fy, fw, fh);

    // Yard lines, measured from the line of scrimmage so every card in the sheet
    // lines up with its neighbours.
    for d in [-5.0f32, 5.0, 10.0, 15.0, 20.0] {
        let ly = (board.los - d * board.sx).round() as i32;
        if ly > fy + 1 && ly < fy + fh - 2 {
            frame.line(fx + 1, ly, fx + fw - 2, ly, TURF_LINE, 1);
        }
    }
    let los_y = board.los.round() as i32;
    frame.line(fx, los_y, fx + fw - 1, los_y, LOS, 1);

    // The quarterback they are coming for, in the backfield.
    let qb = board.at(Vec2::new(-3.4, 0.0));
    frame.fill_rect(qb.0 - 1, qb.1 - 1, 3, 3, ACCENT);

    let defenders = defender_positions(call, 0.0, 1);
    // Where an eligible would line up, for the man-coverage lines.
    let targets: Vec<(i32, i32)> = [-12.0f32, -5.5, 5.5, 12.0]
        .iter()
        .map(|&y| board.at(Vec2::new(5.5, y)))
        .collect();

    // The men in man coverage are covering somebody, so draw who: a line to a
    // receiver marker reads as a matchup instead of stopping in the grass.
    if call.coverage.is_man() {
        for &(tx, ty) in &targets {
            frame.fill_rect(tx - 1, ty - 1, 3, 3, BLOCK);
        }
    }

    for (role, pos) in &defenders {
        let here = board.at(*pos);
        let rushing = role.is_lineman() || call.blitzers.contains(role);
        if rushing {
            // A rusher goes where a rusher goes: at the quarterback.
            arrow(frame, here, qb, CARRY, 2);
        } else if call.coverage.is_man() {
            let nearest = targets
                .iter()
                .min_by_key(|t| (t.0 - here.0).abs() + (t.1 - here.1).abs())
                .copied()
                .unwrap_or(qb);
            frame.line(here.0, here.1, nearest.0, nearest.1, BLOCK, 1);
        } else {
            // Zone: everybody backpedals to a spot and owns the ground around it.
            let deep = matches!(role, DefRole::CbL | DefRole::CbR | DefRole::Safety);
            let drop = board.at(Vec2::new(
                pos.x + if deep { 6.0 } else { 3.0 },
                pos.y * 0.6,
            ));
            frame.line(here.0, here.1, drop.0, drop.1, BLOCK, 1);
            zone_box(frame, drop.0, drop.1, 10, 7, TURF_LINE);
        }
    }

    for (role, pos) in &defenders {
        let (px, py) = board.at(*pos);
        let color = if role.is_lineman() {
            CARRY
        } else if call.blitzers.contains(role) {
            ACCENT
        } else {
            INK
        };
        frame.fill_rect(px - 1, py - 1, 3, 3, color);
        // The technique number on the edges, the job letter inside.
        let tag: String = match role {
            DefRole::EdgeL | DefRole::EdgeR => call.front.technique(*role).to_string(),
            DefRole::Mike => "M".to_string(),
            DefRole::Will => "W".to_string(),
            DefRole::CbL | DefRole::CbR => "C".to_string(),
            DefRole::Safety => "S".to_string(),
        };
        frame.text(px + 2, py - 3, &tag, 1, MUTED);
    }
}

/// A rush arrow: the line, and a head at the far end.
fn arrow(frame: &mut Frame, from: (i32, i32), to: (i32, i32), color: Rgb, thickness: i32) {
    let (dx, dy) = ((to.0 - from.0) as f32, (to.1 - from.1) as f32);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 3.0 {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    let end = (
        (from.0 as f32 + ux * (len - 2.0)).round() as i32,
        (from.1 as f32 + uy * (len - 2.0)).round() as i32,
    );
    frame.line(from.0, from.1, end.0, end.1, color, thickness);
    for side in [-1.0f32, 1.0] {
        let bx = end.0 - (ux * 3.4 - side * uy * 2.2).round() as i32;
        let by = end.1 - (uy * 3.4 + side * ux * 2.2).round() as i32;
        frame.line(end.0, end.1, bx, by, color, thickness);
    }
}

/// The ground a zone defender owns, drawn as an outline so the turf still reads.
fn zone_box(frame: &mut Frame, cx: i32, cy: i32, w: i32, h: i32, color: Rgb) {
    let (x, y) = (cx - w / 2, cy - h / 2);
    frame.line(x, y, x + w, y, color, 1);
    frame.line(x, y + h, x + w, y + h, color, 1);
    frame.line(x, y, x, y + h, color, 1);
    frame.line(x + w, y, x + w, y + h, color, 1);
}

/// Maps defensive yardage onto one card's chalkboard: `+x` is upfield, so it goes
/// up the card, and the line of scrimmage sits low enough to leave the backfield
/// visible for the rush.
struct Board {
    x: f32,
    w: f32,
    top: f32,
    bottom: f32,
    sx: f32,
    sy: f32,
    los: f32,
}

impl Board {
    fn new(x: i32, y: i32, w: i32, h: i32) -> Board {
        let behind = 8.0f32;
        let ahead = 22.0f32;
        let sx = h as f32 / (behind + ahead);
        let sy = w as f32 / 34.0;
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

    fn at(&self, p: Vec2) -> (i32, i32) {
        let px = self.x + self.w * 0.5 + p.y * self.sy;
        let py = self.los - p.x * self.sx;
        (
            (px.round() as i32).clamp(self.x as i32 + 2, (self.x + self.w) as i32 - 3),
            (py.round() as i32).clamp(self.top as i32 + 2, self.bottom as i32 - 3),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nopads_core::playbook::{defensive_playbook, fronts_in};

    #[test]
    fn every_call_draws_a_diagram() {
        let book = defensive_playbook();
        let fronts = fronts_in(&book);
        for (i, call) in book.iter().enumerate() {
            let mut frame = Frame::new(640, 360);
            let front = fronts.iter().position(|f| *f == call.front).unwrap_or(0);
            let slot = def_calls_in(&book, call.front)
                .iter()
                .position(|&j| j == i)
                .unwrap_or(0);
            draw_layered(&mut frame, &book, &fronts, front, slot, true);
            // Defenders, rush arrows and zone boxes all leave marks; a card with
            // none of them is an empty green box.
            let marks = frame
                .color
                .iter()
                .filter(|c| **c == INK || **c == CARRY || **c == ACCENT)
                .count();
            assert!(marks > 40, "{} ({i}) drew almost nothing: {marks} px", call.name);
        }
    }

    #[test]
    fn the_layered_defensive_sheet_keeps_the_text_bar_clear() {
        let book = defensive_playbook();
        let fronts = fronts_in(&book);
        for (i, _) in fronts.iter().enumerate() {
            let mut frame = Frame::new(640, 360);
            draw_layered(&mut frame, &book, &fronts, i, 0, false);
            for y in 296..360 {
                for x in 0..640 {
                    assert_eq!(
                        frame.color[y * 640 + x],
                        nopads_core::math::Rgb::hex(0x0B0F16),
                        "card into the bar at {x},{y} on front {i}"
                    );
                }
            }
        }
    }

    #[test]
    fn different_calls_look_different() {
        let book = defensive_playbook();
        let fronts = fronts_in(&book);
        let a = book.iter().position(|c| c.coverage.is_man()).unwrap();
        let b = book.iter().position(|c| c.coverage == nopads_core::playbook::Coverage::Cover3).unwrap();
        let mut fa = Frame::new(640, 360);
        let mut fb = Frame::new(640, 360);
        let draw_one = |frame: &mut Frame, index: usize| {
            let front = fronts.iter().position(|f| *f == book[index].front).unwrap();
            let slot = def_calls_in(&book, book[index].front)
                .iter()
                .position(|&j| j == index)
                .unwrap();
            draw_layered(frame, &book, &fronts, front, slot, true);
        };
        draw_one(&mut fa, a);
        draw_one(&mut fb, b);
        assert!(fa.color != fb.color, "man and zone drew the same picture");
    }
}

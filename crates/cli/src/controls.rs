//! The controls page: every button, in one place.
//!
//! The live picture used to carry the buttons along its bottom edge, in a bar of
//! hints that sat over the ball and could only say what was true that second.
//! The same information is a page now: the settings opens it, and so does every
//! pause menu, so a player reads it when he wants it and the field stays clear.
//!
//! It is four pages rather than one long list because a page of controls is read
//! at a glance or not at all: offense, running and the throw-off, defense, and
//! the tape. The page draws itself on the interface's 640-wide grid, which is
//! what lets the settings show it on its own and the pause menu lift it over a
//! frozen picture without either of them owning a layout.

use nopads_core::math::Rgb;
use nopads_render::raster::Frame;
use nopads_render::{font, ui};

use crate::view::{UI_H, UI_W};

/// The paper the actions are set in. The kit keeps its muted and accent inks in
/// `ui`; this is the one the page needs that no kit row uses, because a row here
/// is an action on the left and the button on the right rather than a setting
/// and its value.
const PAPER: Rgb = Rgb::hex(0xE6EBF1);

/// One line of a page: what the button does, and the button.
pub struct Row {
    pub what: &'static str,
    pub keys: &'static str,
}

pub struct Group {
    pub title: &'static str,
    pub rows: &'static [Row],
}

pub struct Page {
    /// The tab under the heading.
    pub tab: &'static str,
    pub groups: &'static [Group],
}

const fn row(what: &'static str, keys: &'static str) -> Row {
    Row { what, keys }
}

const AT_THE_LINE: [Row; 5] = [
    row("SNAP THE BALL", "[A]"),
    row("AUDIBLE - CHANGE THE PLAY", "[X]"),
    row("FLIP THE PLAY", "[Y]"),
    row("PREVIEW THE CALL - HOLD", "[LT]"),
    row("HARD COUNT - TRY TO DRAW THEM OFFSIDE", "[LB]"),
];

const IN_THE_POCKET: [Row; 3] = [
    row("MOVE THE QUARTERBACK", "[STICK]"),
    row("THROW - HOLD HIS BUTTON", "[A] [B] [X] [Y]"),
    row("A TAP IS A LOW BALL, A LONG HOLD LOFTS IT", ""),
];

const RUNNING: [Row; 8] = [
    row("DIVE", "[X]"),
    row("SPIN", "[B]"),
    row("HURDLE - OR SLIDE, AS THE QUARTERBACK", "[Y]"),
    row("CUT LEFT / RIGHT", "[LB] / [RB]"),
    row("STIFF ARM", "[RS] UP"),
    row("JUKE", "[RS] SIDE"),
    row("STOP DEAD", "[RS] DOWN"),
    row("SAME MOVES ON A PICK OR KICK RETURN", ""),
];

const THROW_OFF: [Row; 4] = [
    row("AIM THE KICK", "[STICK]"),
    row("LOAD THE ARM - HOLD, LET GO TO THROW", "[A]"),
    row("FAIR CATCH", "[Y]"),
    row("WRAP UP / DIVE AT THE RETURNER", "[A] / [X]"),
];

const DEFENSE_BEFORE: [Row; 4] = [
    row("WALK UP TO THE LINE", "[STICK]"),
    row("PREVIEW YOUR CALL - HOLD", "[LT]"),
    row("CHECK TO ANOTHER CALL", "[RB]"),
    row("TIMEOUT", "[SELECT]"),
];

const DEFENSE_LIVE: [Row; 6] = [
    row("SWITCH DEFENDER", "[B]"),
    row("TACKLE", "[A]"),
    row("DIVE TACKLE / BAT THE BALL DOWN", "[X]"),
    row("INTERCEPT", "[Y]"),
    row("HIGH HIT - GOES FOR THE BALL", "[RS] UP"),
    row("LOW HIT", "[RS] DOWN"),
];

const WATCHING: [Row; 7] = [
    row("PLAY / PAUSE", "[A]"),
    row("FAST FORWARD", "[RB]"),
    row("REWIND", "[LB]"),
    row("STEP A FRAME", "[Y] FWD / [X] BACK"),
    row("PAN THE SHOT", "[LS]"),
    row("ZOOM / ORBIT", "[RS]"),
    row("BACK TO THE GAME", "[B]"),
];

const EVERY_SCREEN: [Row; 5] = [
    row("MOVE / CHOOSE", "[STICK]"),
    row("SELECT / BACK", "[A] / [B]"),
    row("PAUSE", "[START]"),
    row("TIMEOUT", "[SELECT]"),
    row("CHANGE CAMERA - NOT AT THE LINE", "[LB]"),
];

pub const PAGES: [Page; 4] = [
    Page {
        tab: "OFFENSE",
        groups: &[
            Group {
                title: "AT THE LINE",
                rows: &AT_THE_LINE,
            },
            Group {
                title: "IN THE POCKET",
                rows: &IN_THE_POCKET,
            },
        ],
    },
    Page {
        tab: "RUNNING",
        groups: &[
            Group {
                title: "WITH THE BALL IN YOUR HANDS",
                rows: &RUNNING,
            },
            Group {
                title: "THE THROW-OFF",
                rows: &THROW_OFF,
            },
        ],
    },
    Page {
        tab: "DEFENSE",
        groups: &[
            Group {
                title: "BEFORE THE SNAP",
                rows: &DEFENSE_BEFORE,
            },
            Group {
                title: "WHEN THE BALL IS LIVE",
                rows: &DEFENSE_LIVE,
            },
        ],
    },
    Page {
        tab: "REPLAY",
        groups: &[
            Group {
                title: "WATCHING A REPLAY",
                rows: &WATCHING,
            },
            Group {
                title: "ON EVERY SCREEN",
                rows: &EVERY_SCREEN,
            },
        ],
    },
];

/// The card the page is drawn on, on the interface grid. Its height follows the
/// page -- a page of six lines should not sit in a card built for fourteen -- and
/// it always stops above the footer band the settings hangs under it.
const CARD_W: i32 = UI_W as i32 - 80;
/// How tall one line is, and where the first group starts inside the card.
const ROW_H: i32 = 18;
const FIRST_GROUP: i32 = 60;
/// What a group's caption takes before its first row, the rule and the air before
/// the next group, and the margin under the last row.
const CAPTION_H: i32 = 12;
const GROUP_GAP: i32 = 14;
const BOTTOM_PAD: i32 = 8;
/// The footer band the settings draws along the bottom of the screen.
const FOOTER_GAP: i32 = 30;

/// How tall a page's card has to be. The drawing walks the same steps, so the
/// two cannot disagree about where the last row lands.
fn height(page: &Page) -> i32 {
    let mut tall = FIRST_GROUP;
    for (i, group) in page.groups.iter().enumerate() {
        if i > 0 {
            tall += GROUP_GAP;
        }
        tall += CAPTION_H + group.rows.len() as i32 * ROW_H;
    }
    tall + BOTTOM_PAD
}

/// Walk the pages by one, whichever way round that goes.
pub fn walk(page: usize, turn: isize) -> usize {
    let n = PAGES.len() as isize;
    ((page as isize + turn).rem_euclid(n)) as usize
}

/// A held stick or bumper pages once, not once a frame. The pause menu and the
/// settings both page, so the edge is worked out in one place.
#[derive(Default)]
pub struct Pager {
    /// Whether a control has already turned the page without being let go.
    held: bool,
}

impl Pager {
    /// Which way to turn this frame, if a control asks for a page at all. Left
    /// and right both mean a page; a stick held across pages once and waits to
    /// be let go, so a thumb resting on it cannot flick through the list.
    pub fn turn(&mut self, left: bool, right: bool) -> Option<isize> {
        if !left && !right {
            self.held = false;
            return None;
        }
        if self.held {
            None
        } else {
            self.held = true;
            Some(if left { -1 } else { 1 })
        }
    }
}

/// Draw one page on a 640-wide grid. Returns the card it took up, so a caller
/// lifting it over a picture knows which pixels changed and which are still the
/// game.
pub fn draw(frame: &mut Frame, page: usize) -> (i32, i32, i32, i32) {
    let index = page.min(PAGES.len() - 1);
    let page = &PAGES[index];
    let w = CARD_W;
    let h = height(page);
    let x = (UI_W as i32 - w) / 2;
    // Centred, but never over the footer the settings writes under it.
    let y = ((UI_H as i32 - h) / 2).clamp(6, (UI_H as i32 - FOOTER_GAP - h).max(6));
    ui::glass(frame, x, y, w, h, ui::ACCENT, 0.94);
    ui::heading(frame, x + w / 2, y + 14, "CONTROLS", ui::T_BODY, ui::ACCENT);
    draw_tabs(frame, index, x, y + 38, w);
    ui::divider(frame, x + ui::S5, y + 52, w - ui::S5 * 2);
    let mut gy = y + FIRST_GROUP;
    for (i, group) in page.groups.iter().enumerate() {
        if i > 0 {
            // A rule between groups: the caption of the second group used to sit
            // right under the last row of the first and read as part of it.
            gy += GROUP_GAP;
            ui::divider(frame, x + ui::S5, gy - 5, w - ui::S5 * 3);
        }
        ui::caption(
            frame,
            x + ui::S5,
            gy,
            group.title,
            ui::ACCENT.lerp(ui::MUTED, 0.25),
        );
        gy += CAPTION_H;
        for r in group.rows {
            draw_row(frame, x + ui::S5, gy, w - ui::S5 * 2, r);
            gy += ROW_H;
        }
    }
    (x, y, w, h)
}

/// The four page names, the open one lit and ruled under its own name.
fn draw_tabs(frame: &mut Frame, index: usize, x: i32, y: i32, w: i32) {
    let gap = ui::S4;
    let widths: Vec<i32> = PAGES
        .iter()
        .map(|p| ui::tracked_width(p.tab, ui::T_CAPTION, 1))
        .collect();
    let total: i32 = widths.iter().sum::<i32>() + gap * (PAGES.len() as i32 - 1);
    let mut pen = x + (w - total) / 2;
    for (i, page) in PAGES.iter().enumerate() {
        let lit = i == index;
        ui::caption(
            frame,
            pen,
            y,
            page.tab,
            if lit { ui::ACCENT } else { ui::MUTED },
        );
        if lit {
            frame.fill_rect(pen, y + 10, widths[i] - 1, 2, ui::ACCENT);
        }
        pen += widths[i] + gap;
    }
}

/// One line: the action on the left in ink, the button on the right in the
/// accent, so the eye can run down either column.
fn draw_row(frame: &mut Frame, x: i32, y: i32, w: i32, r: &Row) {
    let ty = y + (ROW_H - font::text_height(ui::T_LABEL) as i32) / 2;
    font::draw_styled(frame, x, ty, r.what, ui::T_LABEL, PAPER, 1, false);
    if r.keys.is_empty() {
        return;
    }
    let kw = font::text_width(r.keys, ui::T_LABEL) as i32;
    font::draw_styled(frame, x + w - kw, ty, r.keys, ui::T_LABEL, ui::ACCENT, 1, false);
}

/// The same page as text, for the terminal, where a picture is not what reads.
pub fn lines(page: usize) -> Vec<String> {
    let index = page.min(PAGES.len() - 1);
    let mut out = vec!["".to_string(), "  CONTROLS".to_string(), "".to_string()];
    for group in PAGES[index].groups {
        out.push(format!("  {}", group.title));
        for r in group.rows {
            if r.keys.is_empty() {
                out.push(format!("      {}", r.what));
            } else {
                out.push(format!("      {:<46}{}", r.what, r.keys));
            }
        }
        out.push(String::new());
    }
    out.push(format!(
        "  PAGE {} OF {}   [LEFT/RIGHT] PAGE   [A] DONE",
        index + 1,
        PAGES.len()
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_fits_inside_the_card() {
        // The actions run from the left and the buttons from the right, so the
        // pair has to leave a gap between them as well as fit the card.
        let inner = CARD_W - ui::S5 * 2;
        for page in &PAGES {
            for group in page.groups {
                for r in group.rows {
                    let keys = if r.keys.is_empty() {
                        0
                    } else {
                        ui::S4 + font::text_width(r.keys, ui::T_LABEL) as i32
                    };
                    let need = font::text_width(r.what, ui::T_LABEL) as i32 + keys;
                    assert!(need <= inner, "{} / {} needs {need}", r.what, r.keys);
                }
            }
        }
    }

    #[test]
    fn every_page_fits_on_the_screen() {
        for (i, page) in PAGES.iter().enumerate() {
            let tall = height(page);
            assert!(
                tall <= UI_H as i32 - FOOTER_GAP,
                "page {i} ({}) stands {tall} tall",
                page.tab
            );
        }
    }

    #[test]
    fn the_pages_are_the_ones_the_tabs_name() {
        assert_eq!(PAGES.len(), 4);
        for page in &PAGES {
            assert!(!page.groups.is_empty(), "{} has nothing on it", page.tab);
            for group in page.groups {
                assert!(!group.rows.is_empty(), "{} has an empty group", page.tab);
            }
        }
    }

    #[test]
    fn the_pager_pages_once_per_push() {
        let mut pager = Pager::default();
        let mut page = 0usize;
        // Held left: the first frame pages, the ones after it do not.
        assert_eq!(pager.turn(true, false), Some(-1));
        assert_eq!(pager.turn(true, false), None);
        page = walk(page, -1);
        assert_eq!(page, PAGES.len() - 1);
        // Released, then pushed again, it pages again.
        assert_eq!(pager.turn(false, false), None);
        assert_eq!(pager.turn(false, true), Some(1));
        assert_eq!(walk(0, 1), 1);
    }
}

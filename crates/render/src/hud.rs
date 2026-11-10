//! Broadcast overlay: score bug, down & distance, play clock, ticker and the
//! big-moment banners. Laid out for a 1280x720 frame.
//!
//! The overlay draws from [`HudState`] rather than the live game so a rendered
//! frame of an old play shows the score, clock and down that existed *then*.

use nopads_core::game::{GamePhase, GameState};
use nopads_core::rules::PLAY_CLOCK_SECONDS;
use nopads_core::math::Rgb;

use crate::font;
use crate::raster::Frame;

pub const INK: Rgb = Rgb::hex(0x0C0F14);
pub const PANEL: Rgb = Rgb::hex(0x11161D);
pub const PANEL_EDGE: Rgb = Rgb::hex(0x2C3540);
pub const WHITE: Rgb = Rgb::hex(0xF4F6F8);
pub const MUTED: Rgb = Rgb::hex(0xA9B3BF);
pub const RED: Rgb = Rgb::hex(0xE4572E);
pub const YELLOW: Rgb = Rgb::hex(0xF2C14E);

pub struct Banner {
    pub title: String,
    pub subtitle: String,
    pub color: Rgb,
}

/// A point-in-time snapshot of everything the overlays need.
#[derive(Clone)]
pub struct HudState<'a> {
    pub game: &'a GameState,
    pub quarter: u8,
    pub clock: f32,
    pub down: u8,
    pub to_go: f32,
    pub yards_to_goal: f32,
    pub possession: usize,
    pub score: [u32; 2],
    pub phase: GamePhase,
    pub play_clock: f32,
    pub is_overtime: bool,
    pub under_two: bool,
}

impl<'a> HudState<'a> {
    pub fn from_game(game: &'a GameState) -> Self {
        HudState {
            game,
            quarter: game.quarter,
            clock: game.clock,
            down: game.down,
            to_go: game.to_go,
            yards_to_goal: game.yards_to_goal(),
            possession: game.possession,
            score: game.score,
            phase: game.phase,
            play_clock: game.play_clock,
            is_overtime: game.is_overtime(),
            under_two: game.is_under_two(),
        }
    }

    pub fn live(game: &'a GameState) -> Self {
        HudState::from_game(game)
    }

    fn quarter_text(&self) -> String {
        match self.quarter {
            1 => "1ST".to_string(),
            2 => "2ND".to_string(),
            3 => "3RD".to_string(),
            4 => "4TH".to_string(),
            q => format!("OT{}", q - 4),
        }
    }

    fn clock_text(&self) -> String {
        let c = self.clock.max(0.0);
        let m = (c / 60.0).floor() as u32;
        let s = (c - m as f32 * 60.0).floor() as u32;
        format!("{m}:{s:02}")
    }

    fn down_and_distance(&self) -> String {
        let ord = match self.down {
            1 => "1ST",
            2 => "2ND",
            3 => "3RD",
            _ => "4TH",
        };
        if self.yards_to_goal <= self.to_go {
            format!("{ord} & GOAL")
        } else if (self.to_go - 10.0).abs() < 0.6 {
            format!("{ord} & 10")
        } else {
            format!("{ord} & {}", self.to_go.round() as i32)
        }
    }
}

fn team_row(
    frame: &mut Frame,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    team: &nopads_core::team::Team,
    score: u32,
    possession: bool,
    timeouts: u8,
) {
    let bg = if possession { team.primary.scale(0.55) } else { PANEL };
    frame.fill_rect(x, y, w, h, bg);
    frame.fill_rect(x, y, 10, h, team.primary);
    frame.fill_rect(x + 10, y, 3, h, team.accent);
    let text_color = if possession && team.primary.luma() > 150.0 {
        INK
    } else {
        WHITE
    };
    font::draw_styled(frame, x + 24, y + (h - 7 * 4) / 2, &team.abbr.to_uppercase(), 4, text_color, 1, false);
    // Timeout pips.
    for i in 0..3 {
        let tx = x + 118 + i * 14;
        let ty = y + h / 2 - 4;
        let filled = (i as u8) < timeouts;
        frame.fill_rect(tx, ty, 8, 8, if filled { team.accent } else { PANEL_EDGE });
    }
    let score_w = 82;
    frame.fill_rect(x + w - score_w, y, score_w, h, INK);
    font::draw_right(frame, x + w - 12, y + (h - 7 * 5) / 2, &format!("{score}"), 5, WHITE, 1);
}

/// The longest run of `text` that fits in `room` pixels at `scale`, cut at a
/// space where there is one and given an ellipsis when it had to be shortened.
///
/// Everything the overlay says comes from a play description, and those are as
/// long as the play was: a card is a fixed width and the text is not.
fn fit_text(text: &str, scale: usize, room: i32) -> String {
    let width = |t: &str| font::text_width(t, scale) as i32;
    if width(text) <= room {
        return text.to_string();
    }
    let ell = width("...");
    let mut used = 0;
    let mut cut = 0;
    let mut buf = [0u8; 4];
    for (i, ch) in text.char_indices() {
        let w = width(ch.encode_utf8(&mut buf));
        if used + w + ell > room {
            break;
        }
        used += w;
        cut = i + ch.len_utf8();
    }
    let head = text[..cut].trim_end();
    match head.rfind(' ') {
        Some(at) if at > 0 => format!("{}...", head[..at].trim_end()),
        _ => format!("{head}..."),
    }
}

pub fn draw(frame: &mut Frame, state: &HudState, banner: Option<&Banner>, caption: Option<&str>) {
    let w = frame.w as i32;
    let h = frame.h as i32;
    let game = state.game;

    // ------------------------- top-left game state -------------------------
    frame.fill_rect_alpha(0, 0, w, 44, INK, 0.40);
    font::draw_styled(frame, 28, 10, &state.quarter_text(), 3, MUTED, 1, false);
    font::draw_styled(frame, 84, 6, &state.clock_text(), 4, WHITE, 1, false);
    font::draw_styled(frame, 210, 10, &state.down_and_distance(), 3, YELLOW, 1, false);

    // ------------------------------ score bug ------------------------------
    let bug_w = 430;
    let row_h = 70;
    let bx = 28;
    let by = h - row_h * 2 - 56;
    frame.panel(
        bx - 6,
        by - 6,
        bug_w + 12,
        row_h * 2 + 12,
        9,
        Rgb::hex(0x1B2432),
        Rgb::hex(0x0A0E15),
        PANEL_EDGE,
        0.6,
    );
    team_row(frame, bx, by, bug_w, row_h, &game.teams[0], state.score[0], state.possession == 0, game.timeouts[0]);
    team_row(frame, bx, by + row_h, bug_w, row_h, &game.teams[1], state.score[1], state.possession == 1, game.timeouts[1]);
    frame.fill_rect(bx, by + row_h - 2, bug_w, 2, INK);
    frame.fill_rect(bx, by, bug_w, 2, PANEL_EDGE);

    // Down & distance + ball on, to the right of the bug.
    let dx = bx + bug_w + 16;
    let dcard_w = 300;
    let dcard_h = row_h * 2;
    frame.panel(
        dx,
        by,
        dcard_w,
        dcard_h,
        9,
        Rgb::hex(0x18202C),
        Rgb::hex(0x0A0E15),
        PANEL_EDGE,
        0.55,
    );
    frame.fill_rect(dx, by, 4, dcard_h, game.teams[state.possession].primary);
    font::draw_styled(frame, dx + 18, by + 12, &state.down_and_distance(), 4, WHITE, 1, false);
    let side = if state.yards_to_goal > 50.0 { "OWN" } else { "OPP" };
    let spot = nopads_core::field::ball_on_label(state.yards_to_goal);
    font::draw_styled(
        frame,
        dx + 18,
        by + 52,
        &format!("BALL ON {side} {spot}"),
        3,
        MUTED,
        1,
        false,
    );
    // Play clock.
    let pc = (state.play_clock.max(0.0) / PLAY_CLOCK_SECONDS).clamp(0.0, 1.0);
    let bar_w = ((dcard_w - 36) as f32 * pc) as i32;
    frame.fill_rect(dx + 18, by + dcard_h - 24, dcard_w - 36, 8, PANEL_EDGE);
    frame.fill_rect(dx + 18, by + dcard_h - 24, bar_w, 8, if pc < 0.2 { RED } else { YELLOW });
    font::draw_styled(frame, dx + 18, by + dcard_h - 44, "PLAY CLOCK", 2, MUTED, 1, false);

    // ---------------------------- league badge -----------------------------
    let lw = 178;
    let lx = w - lw - 28;
    frame.rounded_rect(lx, 24, lw, 46, 8, RED);
    font::draw_styled(frame, lx + 16, 32, "7-ON-7", 3, WHITE, 2, true);
    font::draw_styled(frame, lx + 16, 58, "FOOTBALL", 2, WHITE, 1, false);

    // ------------------------------- ticker --------------------------------
    let ty = h - 44;
    frame.gradient_rect(0, ty, w, 44, Rgb::hex(0x141B26), Rgb::hex(0x080B10));
    frame.fill_rect(0, ty, w, 2, RED);
    frame.fill_rect(0, ty + 2, w, 1, Rgb::hex(0x3A4A5E));
    let ticker = caption
        .map(|s| s.to_string())
        .or_else(|| game.log.last().map(|r| r.description().to_string()))
        .unwrap_or_default()
        .to_uppercase();
    let trimmed = fit_text(&ticker, 2, w - 56);
    font::draw_styled(frame, 28, ty + 13, &trimmed, 2, WHITE, 1, false);

    // ----------------------- situational banner ----------------------------
    if let Some(b) = banner {
        let bw = 780;
        let bh = 128;
        let bx = (w - bw) / 2;
        let by = 156;
        // A painted card: rounded, lifted off the picture by its own shadow, the
        // club colour lit at the top and shading down, with a thin rule under it.
        frame.panel(
            bx,
            by,
            bw,
            bh,
            12,
            b.color.lerp(Rgb::hex(0xFFFFFF), 0.14),
            b.color.scale(0.38),
            b.color.scale(0.8),
            0.75,
        );
        frame.fill_rect(bx + 14, by + 6, bw - 28, 3, WHITE);
        font::draw_centered(frame, bx + bw / 2, by + 26, &b.title.to_uppercase(), 6, WHITE, 2);
        if !b.subtitle.is_empty() {
            let sub = b.subtitle.to_uppercase();
            // Fit to the card rather than to a character count. A count cut the
            // last word through the middle at whatever letter the tally landed
            // on -- "...BROUGHT DOWN BY C PEMBERTON AFTE" -- on a card with
            // nothing behind that word.
            let sub = fit_text(&sub, 2, bw - 64);
            // The gradient darkens toward the bottom, so pale text needs a dark
            // colour only over bright team colours.
            let ink = if b.color.luma() > 140.0 { Rgb::hex(0x101418) } else { WHITE };
            font::draw_centered(frame, bx + bw / 2, by + 82, &sub, 2, ink, 1);
        }
    }

    // --------------------------- phase caption ------------------------------
    match state.phase {
        GamePhase::ThrowOff { receiving, from_own } => {
            font::draw_centered(
                frame,
                w / 2,
                60,
                &format!(
                    // A hyphen, not a middle dot: the 5x7 face has no glyph for
                    // `·` and drew the fallback box, which read as an O.
                    "THROW-OFF FROM THE {} - {} RETURNS",
                    from_own as i32, game.teams[receiving].abbr
                ),
                2,
                WHITE,
                1,
            );
        }
        GamePhase::ConversionChoice { offense } => {
            font::draw_centered(
                frame,
                w / 2,
                60,
                &format!("{}: 1 FROM THE 5 OR 2 FROM THE 10", game.teams[offense].abbr),
                2,
                YELLOW,
                1,
            );
        }
        GamePhase::Conversion { points, .. } => {
            font::draw_centered(frame, w / 2, 60, &format!("{points}-POINT CONVERSION"), 2, YELLOW, 1);
        }
        _ => {}
    }

    if state.is_overtime {
        font::draw_centered(frame, w / 2, 96, "OVERTIME", 3, RED, 2);
    } else if state.under_two {
        font::draw_centered(frame, w / 2, 96, "UNDER TWO MINUTES", 2, YELLOW, 1);
    }
}

/// Everything the live scoreboard needs, owned rather than borrowed.
///
/// The interactive client takes one of these before the snap instead of holding
/// on to the game, because the game is lent to the simulation for the whole of
/// the play and the scoreboard has to stay drawable from inside that loan.
#[derive(Clone)]
pub struct Scoreboard {
    pub abbr: [String; 2],
    pub primary: [Rgb; 2],
    pub accent: [Rgb; 2],
    pub score: [u32; 2],
    pub timeouts: [u8; 2],
    pub possession: usize,
    pub quarter: u8,
    pub clock: f32,
    pub down: u8,
    pub to_go: f32,
    pub yards_to_goal: f32,
}

impl Scoreboard {
    pub fn from_game(game: &GameState) -> Self {
        Scoreboard {
            abbr: [
                game.teams[0].abbr.to_uppercase(),
                game.teams[1].abbr.to_uppercase(),
            ],
            primary: [game.teams[0].primary, game.teams[1].primary],
            accent: [game.teams[0].accent, game.teams[1].accent],
            score: game.score,
            timeouts: game.timeouts,
            possession: game.possession,
            quarter: game.quarter,
            clock: game.clock,
            down: game.down,
            to_go: game.to_go,
            yards_to_goal: game.yards_to_goal(),
        }
    }

    fn quarter_text(&self) -> String {
        match self.quarter {
            1 => "1ST".to_string(),
            2 => "2ND".to_string(),
            3 => "3RD".to_string(),
            4 => "4TH".to_string(),
            q => format!("OT{}", q - 4),
        }
    }

    fn clock_text(&self) -> String {
        let c = self.clock.max(0.0);
        let m = (c / 60.0).floor() as u32;
        let s = (c - m as f32 * 60.0).floor() as u32;
        format!("{m}:{s:02}")
    }

    fn down_text(&self) -> String {
        let ord = match self.down {
            1 => "1ST",
            2 => "2ND",
            3 => "3RD",
            _ => "4TH",
        };
        if self.yards_to_goal <= self.to_go {
            format!("{ord} & GOAL")
        } else if (self.to_go - 10.0).abs() < 0.6 {
            format!("{ord} & 10")
        } else {
            format!("{ord} & {}", self.to_go.round() as i32)
        }
    }

    fn ball_on(&self) -> String {
        let side = if self.yards_to_goal > 50.0 { "OWN" } else { "OPP" };
        let spot = nopads_core::field::ball_on_label(self.yards_to_goal);
        format!("BALL ON {side} {spot}")
    }
}

/// The corner scoreboard the interactive client draws over the live picture.
///
/// Laid out for the client's 640x360 canvas: two club rows with the possession
/// highlight, timeouts and score, a clock block beside them and the down and
/// distance underneath. The client only draws it before the snap and again once
/// the play is dead and reset, which is where a broadcast's score bug lives too.
pub fn draw_scoreboard(frame: &mut Frame, s: &Scoreboard) {
    let x = 14;
    let y = 12;
    let row_w = 180;
    // The score is set at scale 3, which is 21 pixels: a 22 pixel row clipped a
    // pixel off the bottom of every digit.
    let row_h = 24;
    let clock_w = 84;
    let total_w = row_w + clock_w + 8;
    let total_h = row_h * 2 + 17;
    // The whole bug is one card rather than three rectangles stacked: a shadow,
    // a hairline and a graded face, the way the rest of the overlay is built.
    frame.panel(
        x - 8,
        y - 8,
        total_w + 16,
        total_h + 16,
        10,
        Rgb::hex(0x161F2B),
        Rgb::hex(0x0A0E15),
        PANEL_EDGE,
        0.4,
    );

    for t in 0..2 {
        let ty = y + t as i32 * row_h;
        let has_it = s.possession == t;
        let bg = if has_it { s.primary[t].scale(0.62) } else { Rgb::hex(0x11161D) };
        frame.rounded_rect(x - 4, ty, row_w + 8, row_h - 2, 4, bg);
        // The club's colour on the leading edge, with its accent as a hairline
        // beside it when it has the ball.
        frame.rounded_rect(x - 4, ty, 4, row_h - 2, 2, s.primary[t]);
        if has_it {
            frame.fill_rect(x - 1, ty + 4, 1, row_h - 10, s.accent[t]);
        }
        let ink = if has_it && s.primary[t].luma() > 150.0 { INK } else { WHITE };
        font::draw_styled(frame, x + 8, ty + 4, &s.abbr[t], 2, ink, 1, false);
        if has_it {
            // Possession: a small lit mark beside the name.
            let nw = font::text_width(&s.abbr[t], 2) as i32;
            frame.fill_rect(x + 13 + nw, ty + 9, 3, 3, ink);
        }
        for i in 0..3 {
            let filled = (i as u8) < s.timeouts[t];
            let px = x + row_w - 78 + i * 9;
            frame.rounded_rect(
                px,
                ty + row_h / 2 - 5,
                6,
                6,
                3,
                if filled { s.accent[t] } else { PANEL_EDGE },
            );
        }
        font::draw_right(frame, x + row_w - 8, ty, &format!("{}", s.score[t]), 3, WHITE, 1);
    }
    frame.fill_rect(x - 4, y + row_h - 2, row_w + 8, 1, Rgb::hex(0x0A0E15));

    // The clock block: quarter, then the clock, in its own well.
    let cx = x + row_w + 8;
    frame.rounded_rect(cx, y, clock_w, total_h - 4, 5, INK);
    font::draw_styled(frame, cx + 7, y + 3, &s.quarter_text(), 1, MUTED, 1, false);
    font::draw_styled(frame, cx + 7, y + 12, &s.clock_text(), 2, WHITE, 1, false);

    // Down and distance along the bottom of the card, on their own strip.
    let dy = y + row_h * 2;
    frame.rounded_rect(x - 4, dy, total_w + 8, 13, 4, INK);
    font::draw_styled(frame, x + 4, dy + 3, &s.down_text(), 1, YELLOW, 1, false);
    let ball = s.ball_on();
    font::draw_right(frame, x + total_w + 4, dy + 3, &ball, 1, MUTED, 1);
}

/// Full-screen statistical panel, shown at halftime and after the game.
pub fn draw_stats_panel(frame: &mut Frame, game: &GameState, title: &str) {
    let w = frame.w as i32;
    let h = frame.h as i32;
    frame.fill_rect_alpha(0, 0, w, h, INK, 0.85);
    let panel_w = 900;
    let panel_h = 520;
    let x = (w - panel_w) / 2;
    let y = (h - panel_h) / 2;
    frame.panel(
        x,
        y,
        panel_w,
        panel_h,
        14,
        Rgb::hex(0x1C2533),
        Rgb::hex(0x0B1017),
        PANEL_EDGE,
        0.8,
    );
    frame.fill_rect(x + 14, y + 6, panel_w - 28, 4, RED);
    font::draw_centered(frame, x + panel_w / 2, y + 26, title, 4, WHITE, 2);

    let hdr_y = y + 78;
    font::draw_styled(
        frame,
        x + 40,
        hdr_y,
        &game.teams[0].abbr.to_uppercase(),
        3,
        game.teams[0].primary.lerp(WHITE, 0.55),
        1,
        false,
    );
    font::draw_right(
        frame,
        x + panel_w - 40,
        hdr_y,
        &game.teams[1].abbr.to_uppercase(),
        3,
        game.teams[1].primary.lerp(WHITE, 0.55),
        1,
    );
    frame.fill_rect(x + 40, hdr_y + 34, panel_w - 80, 2, PANEL_EDGE);

    let b = &game.box_score;
    let rows: [(&str, String, String); 8] = [
        ("SCORE", format!("{}", game.score[0]), format!("{}", game.score[1])),
        ("TOTAL YARDS", format!("{:.0}", b.teams[0].total_yards), format!("{:.0}", b.teams[1].total_yards)),
        (
            "PASSING",
            format!("{}/{} {:.0}", b.teams[0].pass_comp, b.teams[0].pass_att, b.teams[0].pass_yards),
            format!("{}/{} {:.0}", b.teams[1].pass_comp, b.teams[1].pass_att, b.teams[1].pass_yards),
        ),
        (
            "RUSHING",
            format!("{} {:.0}", b.teams[0].rush_att, b.teams[0].rush_yards),
            format!("{} {:.0}", b.teams[1].rush_att, b.teams[1].rush_yards),
        ),
        ("TURNOVERS", format!("{}", b.teams[0].turnovers), format!("{}", b.teams[1].turnovers)),
        ("FIRST DOWNS", format!("{}", b.teams[0].first_downs), format!("{}", b.teams[1].first_downs)),
        (
            "PENALTIES",
            format!("{} / {:.0}", b.teams[0].penalties, b.teams[0].penalty_yards),
            format!("{} / {:.0}", b.teams[1].penalties, b.teams[1].penalty_yards),
        ),
        (
            "POSSESSION",
            format_time(b.teams[0].time_of_possession),
            format_time(b.teams[1].time_of_possession),
        ),
    ];

    let mut ry = hdr_y + 58;
    for (label, a, bb) in rows {
        font::draw_centered(frame, x + panel_w / 2, ry, label, 2, MUTED, 1);
        font::draw_styled(frame, x + 150, ry - 4, &a, 3, WHITE, 1, false);
        font::draw_right(frame, x + panel_w - 150, ry - 4, &bb, 3, WHITE, 1);
        frame.fill_rect(x + 150, ry + 30, panel_w - 300, 1, PANEL_EDGE.scale(0.6));
        ry += 50;
    }
}

fn format_time(seconds: f32) -> String {
    let total = seconds.max(0.0) as u32;
    format!("{}:{:02}", total / 60, total % 60)
}


/// An arrow hanging over a point on the field, pointing down at it.
///
/// Drawn in pixels over the picture rather than in the world: a cone in the scene
/// is a thing the camera can lose behind a helmet, and this must always be the
/// clearest mark on the frame.
pub fn draw_down_arrow(frame: &mut Frame, x: i32, y: i32, color: Rgb, floor: i32) {
    // A dark edge under it, so the arrow reads over a white jersey as well as a
    // green field.
    let edge = Rgb::hex(0x0B1017);
    // The shaft, then a head that comes to a point at the foot of it.
    for (w, shade) in [(5, edge), (3, color)] {
        for row in 0..14 {
            frame.fill_rect(x - w / 2, y + row, w, 1, shade);
        }
    }
    for row in 0..9 {
        let half = 9 - row;
        for (grow, shade) in [(1, edge), (0, color)] {
            frame.fill_rect(x - half - grow, y + 14 + row, half * 2 + 1 + grow * 2, 1, shade);
        }
    }
    frame.fill_rect(x, y + 23, 1, 2, color);
    // A stub of shadow on the ground under the point, so it sits on him.
    let foot = (y + 27).min(floor - 4);
    frame.fill_rect_alpha(x - 10, foot, 20, 4, edge, 0.35);
}

/// A full-screen presentation card: a title, a subtitle and two columns.
///
/// The same panel as the box score, used for the matchup before the game and for
/// each play in the halftime reel, so the broadcast graphics look like one set.
pub fn draw_card(
    frame: &mut Frame,
    title: &str,
    subtitle: &str,
    left: &[String],
    right: &[String],
    accent: Rgb,
) {
    let w = frame.w as i32;
    let h = frame.h as i32;
    // Light enough to keep the picture behind it: the card is over a live shot.
    frame.fill_rect_alpha(0, 0, w, h, INK, 0.45);
    let panel_w = 900;
    let panel_h = 480;
    let x = (w - panel_w) / 2;
    let y = (h - panel_h) / 2;
    frame.panel(
        x,
        y,
        panel_w,
        panel_h,
        14,
        Rgb::hex(0x1C2533),
        Rgb::hex(0x0B1017),
        PANEL_EDGE,
        0.8,
    );
    frame.fill_rect(x + 14, y + 6, panel_w - 28, 5, accent);
    font::draw_centered(frame, x + panel_w / 2, y + 26, title, 5, WHITE, 3);
    if !subtitle.is_empty() {
        font::draw_centered(frame, x + panel_w / 2, y + 74, subtitle, 2, accent, 1);
    }
    frame.fill_rect(x + 40, y + 106, panel_w - 80, 2, PANEL_EDGE);
    for (col, lines) in [left, right].iter().enumerate() {
        let cx = if col == 0 { x + 48 } else { x + panel_w / 2 + 24 };
        for (row, line) in lines.iter().enumerate() {
            let ry = y + 128 + row as i32 * 36;
            if ry > y + panel_h - 40 {
                break;
            }
            font::draw_styled(frame, cx, ry, line, 2, WHITE, 1, false);
        }
    }
}


/// A compact card laid over the picture: a title, a line under it, and the rows
/// that go with them.
///
/// The read-out after a play and the questions the game asks are the same card.
/// They used to be full-screen text pages: the picture went away, and a page that
/// reported one sentence filled the screen to do it. This is a pane of glass in
/// the middle of the shot instead -- smaller than a page, see-through, and the
/// same furniture the introduction's cards are built from.
pub fn draw_notice(frame: &mut Frame, title: &str, subtitle: &str, lines: &[String], accent: Rgb) {
    let w = frame.w as i32;
    let h = frame.h as i32;
    // The picture stays behind it: dimmed, not replaced, so the card sits on the
    // play it is talking about.
    frame.fill_rect_alpha(0, 0, w, h, INK, 0.32);

    // Wrapped first, because the card is sized by what it has to say.
    let text_w: usize = 560;
    let mut rows: Vec<String> = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            rows.push(String::new());
            continue;
        }
        for piece in wrap_to(line.trim(), 2, text_w) {
            rows.push(piece);
        }
    }
    let pad = 24;
    let head = if subtitle.is_empty() { 54 } else { 82 };
    let panel_w = text_w as i32 + pad * 2;
    let panel_h = (head + row_height(&rows, 24)).min(h - 40);
    let x = (w - panel_w) / 2;
    let y = (h - panel_h) / 2;

    frame.rounded_rect_alpha(x - 2, y - 1, panel_w + 4, panel_h + 5, 16, Rgb::BLACK, 0.22);
    frame.rounded_rect_alpha(x - 1, y - 1, panel_w + 2, panel_h + 2, 15, PANEL_EDGE, 0.55);
    frame.rounded_rect_alpha(x, y, panel_w, panel_h, 14, Rgb::hex(0x1C2533), 0.62);
    frame.fill_rect(x + 14, y + 6, panel_w - 28, 4, accent);
    font::draw_centered(frame, w / 2, y + 20, title, 3, WHITE, 2);
    if !subtitle.is_empty() {
        font::draw_centered(frame, w / 2, y + 52, subtitle, 1, Rgb::hex(0xDCE3EC), 1);
    }
    let mut ry = y + head;
    for row in &rows {
        if ry + 20 > y + panel_h - 8 {
            break;
        }
        if !row.is_empty() {
            // The choice the pad is on reads the way it reads everywhere else.
            let colour = if row.contains(">>") { YELLOW } else { WHITE };
            font::draw_styled(frame, x + pad, ry, row, 2, colour, 1, false);
        }
        ry += 24;
    }
}

/// How tall a run of wrapped rows is, blanks included.
fn row_height(rows: &[String], line: i32) -> i32 {
    12 + rows.len() as i32 * line
}

/// The bar along the bottom of a highlight: what it was, and the call written
/// across it.
///
/// A film package puts the words on the lower third of the picture instead of in
/// a card over the middle of it, so the play is watched and read at once.
pub fn draw_lower_third(frame: &mut Frame, label: &str, text: &str, accent: Rgb) {
    let w = frame.w as i32;
    let h = frame.h as i32;
    let x = 56;
    let panel_w = w - x * 2;
    // Sized by the call it carries: a bar that reserves room for three lines to
    // say one leaves a slab of ink across the bottom of the picture.
    let rows: Vec<String> = wrap_to(text, 2, (panel_w - 48) as usize)
        .into_iter()
        .take(2)
        .collect();
    let bar_h = 34 + rows.len() as i32 * 26 + 12;
    let y = h - bar_h - 26;
    frame.rounded_rect_alpha(x, y, panel_w, bar_h, 10, Rgb::hex(0x0B1017), 0.74);
    frame.fill_rect(x, y, 5, bar_h, accent);
    font::draw_styled(frame, x + 22, y + 14, label, 1, accent, 1, false);
    let mut ry = y + 34;
    for piece in &rows {
        font::draw_styled(frame, x + 22, ry, piece, 2, WHITE, 1, false);
        ry += 26;
    }
}

/// Two cards that each take half the frame, one per club.
///
/// The introduction is a set of these rather than one panel: each club gets a
/// half of the screen, and the panel is see-through the way the pause menu is,
/// so the warm-up behind it is still the picture. One set carries the teams and
/// their unit matchups, the next carries the men to watch.
/// The pause menu's own veil, so the introduction and the pause screen are the
/// same piece of glass: `crates/cli/src/menu.rs` washes the frame in this colour
/// at this weight when the game is paused over a live picture.
const PAUSE_INK: Rgb = Rgb::new(4, 6, 10);
const PAUSE_VEIL: f32 = 0.50;

/// Break `text` into lines that fit `max_w` at `scale`, measuring with the font's
/// own advance. A single word wider than the card is split on characters, because
/// a club with a long name is the case this exists for.
fn wrap_to(text: &str, scale: usize, max_w: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let joined = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if font::text_width(&joined, scale) <= max_w {
            line = joined;
            continue;
        }
        if !line.is_empty() {
            out.push(std::mem::take(&mut line));
        }
        if font::text_width(word, scale) <= max_w {
            line = word.to_string();
            continue;
        }
        for ch in word.chars() {
            let mut candidate = line.clone();
            candidate.push(ch);
            if font::text_width(&candidate, scale) > max_w && !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            line.push(ch);
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// One see-through card: a soft shadow, a thin border, a wash of ink, and the
/// club's colour across the top.
///
/// It is drawn by hand rather than with [`Frame::panel`] because `panel` takes a
/// shadow strength, not an alpha, and fills flat: passing it 0.5 gives a solid
/// card with a lighter shadow, which is why the cards stayed opaque through two
/// rounds of opacity changes.
fn glass_card(frame: &mut Frame, x: i32, y: i32, w: i32, h: i32, accent: Rgb) {
    let r = 12;
    frame.rounded_rect_alpha(x - 2, y - 1, w + 4, h + 5, r + 2, Rgb::BLACK, PAUSE_VEIL * 0.30);
    // Blended, not solid: `rounded_rect` fills its whole shape, so an "outline"
    // drawn with it is a slab of edge colour behind every card pixel -- the
    // second opaque surface, under a translucent fill, holding the field out.
    frame.rounded_rect_alpha(x - 1, y - 1, w + 2, h + 2, r + 1, PANEL_EDGE, 0.55);
    frame.rounded_rect_alpha(x, y, w, h, r, Rgb::hex(0x1C2533), PAUSE_VEIL);
    frame.fill_rect(x + 12, y + 5, w - 24, 4, accent);
}

pub fn draw_half_cards(
    frame: &mut Frame,
    title: &str,
    subtitle: &str,
    left: &[String],
    right: &[String],
    accents: [Rgb; 2],
    reveal: f32,
) {
    let w = frame.w as i32;
    let h = frame.h as i32;
    let margin = 32;
    let gap = 32;
    let card_w = (w - margin * 2 - gap) / 2;
    let card_h = 320;
    // Sat in the middle of the frame, not on its lip: two half-width cards
    // hanging off the bottom read as a footer under the picture.
    let y = (h - card_h) / 2;
    let x = [margin, margin + card_w + gap];

    // A slim banner at the top: this is a set of cards, not a page. It gets the
    // screen to itself, leaves, and only then do the cards arrive -- the two are
    // never up together, so the introduction is two beats rather than one busy
    // frame.
    const BANNER_OUT: f32 = 2.20;
    const CARDS_IN: f32 = 2.60;
    let banner_w = 520;
    let bx = (w - banner_w) / 2;
    if reveal < BANNER_OUT {
        frame.fill_rect_alpha(bx, 26, banner_w, 76, PAUSE_INK, PAUSE_VEIL);
        frame.fill_rect(bx, 26, banner_w, 3, accents[0]);
        frame.fill_rect(bx + banner_w - 140, 26, 140, 3, accents[1]);
        font::draw_centered(frame, w / 2, 36, title, 3, WHITE, 2);
        font::draw_centered(frame, w / 2, 74, subtitle, 2, Rgb::hex(0xDCE3EC), 1);
    }

    if reveal < CARDS_IN {
        return;
    }
    for (i, lines) in [left, right].iter().enumerate() {
        let cx = x[i];
        // Glass, not slate: the same weight as the pause menu, so the field reads
        // through it.
        glass_card(frame, cx, y, card_w, card_h, accents[i]);
        let inset = 20;
        let max_w = (card_w - inset * 2).max(40) as usize;
        let mut ry = y + 22;
        for (row, line) in lines.iter().enumerate() {
            // The club's name runs a size larger than the rest, in its own colour.
            let scale = if row == 0 { 3 } else { 2 };
            let colour = if row == 0 {
                accents[i].lerp(WHITE, 0.55)
            } else {
                Rgb::hex(0xE9EEF5)
            };
            for piece in wrap_to(line, scale, max_w) {
                let tall = (crate::font::GLYPH_H * scale) as i32;
                if ry + tall > y + card_h - 18 {
                    break;
                }
                font::draw_styled(frame, cx + inset, ry, &piece, scale, colour, 1, false);
                ry += if scale > 2 { 30 } else { 28 };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nopads_core::team::league_teams;





    #[test]
    fn the_notice_is_a_card_over_the_picture_not_a_page_instead_of_it() {
        let base = Rgb::new(40, 80, 60);
        let draw = |backdrop: Rgb| {
            let mut f = Frame::new(1280, 720);
            f.color.fill(backdrop);
            draw_notice(
                &mut f,
                "THE PLAY",
                "BOS 14 - CHI 7",
                &["PASS COMPLETE FOR TWELVE YARDS".to_string()],
                Rgb::hex(0xE2572B),
            );
            f
        };
        let dark = draw(base);
        let light = draw(Rgb::new(80, 130, 100));

        // The picture is still there, and still comes through the card: the same
        // card over two different fields is not the same card.
        let (mut inside, mut differs) = (0, 0);
        for y in 300..430 {
            for x in 400..880 {
                inside += 1;
                if dark.color[y * 1280 + x] != light.color[y * 1280 + x] {
                    differs += 1;
                }
            }
        }
        assert!(inside > 50_000, "the sample is off the card: {inside} px");
        assert!(
            differs * 100 / inside > 30,
            "the card is opaque: only {differs} of {inside} px saw the field"
        );

        // And it is a card, not a page: the edges of the frame carry the dimming
        // and nothing else.
        let mut veil = Frame::new(1280, 720);
        veil.color.fill(base);
        veil.fill_rect_alpha(0, 0, 1280, 720, INK, 0.32);
        assert_eq!(
            dark.color[360 * 1280 + 8],
            veil.color[360 * 1280 + 8],
            "the card reaches the edge of the picture"
        );
        assert_ne!(
            dark.color[360 * 1280 + 640],
            veil.color[360 * 1280 + 640],
            "there is no card in the middle of the picture"
        );
    }

    #[test]
    fn the_lower_third_sits_at_the_bottom_and_leaves_the_play_alone() {
        let base = Rgb::new(30, 60, 40);
        let mut f = Frame::new(1280, 720);
        f.color.fill(base);
        draw_lower_third(&mut f, "Q2   3:12   HIGHLIGHT 1 OF 3", "PASS COMPLETE FOR 24 YARDS", Rgb::hex(0xE2572B));
        let mut above = 0;
        for y in 0..560 {
            for x in 0..1280 {
                if f.color[y * 1280 + x] != base {
                    above += 1;
                }
            }
        }
        assert_eq!(above, 0, "the caption reached into the play: {above} px");
        let painted = (600..694)
            .flat_map(|y| (56..1224).map(move |x| (y, x)))
            .filter(|(y, x)| f.color[y * 1280 + x] != base)
            .count();
        assert!(painted > 40_000, "the lower third is a speck: {painted} px");
    }

    #[test]
    fn the_cards_are_glass_and_not_slate() {
        // The property that matters is that the picture behind shows through: the
        // same cards over two different backdrops have to come out different. The
        // first version of this test compared one pixel against a hand-worked
        // blend and was wrong about what the panel draws, not about the panel.
        let draw = |base: Rgb| {
            let mut f = Frame::new(1280, 720);
            f.color.fill(base);
            draw_half_cards(
                &mut f,
                "MATCHUP",
                "BOS AT CHI",
                &["BOSTON".to_string()],
                &["CHICAGO".to_string()],
                [Rgb::hex(0xE2572B), Rgb::hex(0x2F6FBF)],
                3.0,
            );
            f
        };
        let dark = draw(Rgb::new(10, 20, 30));
        let light = draw(Rgb::new(70, 100, 130));
        // A strip of plain card, below the type.
        let (mut strip, mut differs) = (0, 0);
        for y in 200 + 286..200 + 308 {
            for x in 32 + 24..32 + 552 {
                strip += 1;
                if dark.color[y * 1280 + x] != light.color[y * 1280 + x] {
                    differs += 1;
                }
            }
        }
        assert!(strip > 5_000, "the sample strip is off the card: {strip} px");
        assert!(
            differs * 100 / strip > 80,
            "the card is opaque: only {differs} of {strip} px saw the backdrop"
        );
    }

    #[test]
    fn the_banner_comes_up_before_the_cards() {
        let base = Rgb::new(10, 20, 30);
        let mut f = Frame::new(1280, 720);
        f.color.fill(base);
        draw_half_cards(
            &mut f,
            "MATCHUP",
            "BOS AT CHI",
            &["BOSTON".to_string()],
            &["CHICAGO".to_string()],
            [Rgb::hex(0xE2572B), Rgb::hex(0x2F6FBF)],
            0.0,
        );
        let mut cards = 0;
        for y in 200..520 {
            for x in 32..1248 {
                if f.color[y * 1280 + x] != base {
                    cards += 1;
                }
            }
        }
        assert_eq!(cards, 0, "the cards came up with the banner: {cards} px");
        // And the banner itself is up.
        let banner = (26..102)
            .flat_map(|y| (380..900).map(move |x| (y, x)))
            .filter(|(y, x)| f.color[y * 1280 + x] != base)
            .count();
        assert!(banner > 2_000, "the banner is missing: {banner} px");
    }



    #[test]
    fn the_banner_leaves_before_the_cards_arrive() {
        // Between the two beats the screen is the picture and nothing else.
        let base = Rgb::new(10, 20, 30);
        let mut f = Frame::new(1280, 720);
        f.color.fill(base);
        draw_half_cards(
            &mut f,
            "MATCHUP",
            "BOS AT CHI",
            &["BOSTON".to_string()],
            &["CHICAGO".to_string()],
            [Rgb::hex(0xE2572B), Rgb::hex(0x2F6FBF)],
            2.4,
        );
        let painted = f.color.iter().filter(|px| **px != base).count();
        assert_eq!(painted, 0, "the two beats overlap: {painted} px painted");
    }

    #[test]
    fn the_banner_leaves_once_the_cards_are_up() {
        let base = Rgb::new(10, 20, 30);
        let mut f = Frame::new(1280, 720);
        f.color.fill(base);
        draw_half_cards(
            &mut f,
            "MATCHUP",
            "BOS AT CHI",
            &["BOSTON".to_string()],
            &["CHICAGO".to_string()],
            [Rgb::hex(0xE2572B), Rgb::hex(0x2F6FBF)],
            3.0,
        );
        let mut banner = 0;
        for y in 26..102 {
            for x in 380..900 {
                if f.color[y * 1280 + x] != base {
                    banner += 1;
                }
            }
        }
        assert_eq!(banner, 0, "the banner is still up over the cards: {banner} px");
        let mut cards = 0;
        for y in 200..520 {
            for x in 32..1248 {
                if f.color[y * 1280 + x] != base {
                    cards += 1;
                }
            }
        }
        assert!(cards > 20_000, "the cards are missing: {cards} px");
    }

    #[test]
    fn a_long_club_name_wraps_inside_its_card() {
        // A control render with short names: the only thing that changes is the
        // text, so anything that differs outside the card rectangles is text that
        // escaped. Measuring against the backdrop instead counts the panel's own
        // edge as spill, which is what the first version of this test did.
        let base = Rgb::new(10, 20, 30);
        let draw = |left: &[String], right: &[String]| {
            let mut f = Frame::new(1280, 720);
            f.color.fill(base);
            draw_half_cards(
                &mut f,
                "MATCHUP",
                "PHI AT CHI",
                left,
                right,
                [Rgb::hex(0xE2572B), Rgb::hex(0x2F6FBF)],
                3.0,
            );
            f
        };
        let short = draw(
            &["PHI".to_string(), "OVERALL 71".to_string()],
            &["CHI".to_string(), "OVERALL 68".to_string()],
        );
        let long = draw(
            &[
                "THE PHILADELPHIA FOUNDRY ATHLETIC CLUB".to_string(),
                "OVERALL 71".to_string(),
            ],
            &["CHICAGO FOUNDRY".to_string(), "OVERALL 68".to_string()],
        );
        // The card rectangles, from the draw above: 32..624 and 656..1248, at
        // 200..520 on a 720 high frame.
        let in_card = |x: usize, y: usize| {
            (200..520).contains(&y)
                && ((32..624).contains(&x) || (656..1248).contains(&x))
        };
        let mut spilled = 0;
        for y in 0..720 {
            for x in 0..1280 {
                if !in_card(x, y) && long.color[y * 1280 + x] != short.color[y * 1280 + x] {
                    spilled += 1;
                }
            }
        }
        assert_eq!(spilled, 0, "the name spilled out of its card: {spilled} px");
    }

    #[test]
    fn the_half_cards_are_half_the_frame_each_and_leave_the_sky_alone() {
        let base = Rgb::new(10, 20, 30);
        let mut f = Frame::new(1280, 720);
        f.color.fill(base);
        let left = vec!["BOSTON".to_string(), "OVERALL 71".to_string()];
        let right = vec!["CHICAGO".to_string(), "OVERALL 68".to_string()];
        draw_half_cards(
            &mut f,
            "MATCHUP",
            "BOS AT CHI",
            &left,
            &right,
            [Rgb::hex(0xE2572B), Rgb::hex(0x2F6FBF)],
            3.0,
        );
        // The picture above the cards is the warm-up, and it has to survive. The
        // cards start at 200; the border is drawn a pixel outside them.
        let mut touched = 0;
        for y in 130..199 {
            for x in 0..1280 {
                if f.color[y * 1280 + x] != base {
                    touched += 1;
                }
            }
        }
        assert_eq!(touched, 0, "the cards reach into the picture: {touched} px");
        // Each card is about half the width: 592 of 1280.
        for x0 in [32usize, 656] {
            let painted = (0..720)
                .flat_map(|y| (x0..x0 + 592).map(move |x| (y, x)))
                .filter(|(y, x)| f.color[y * 1280 + x] != base)
                .count();
            assert!(painted > 40_000, "the card at {x0} is missing: {painted} px");
        }
    }

    #[test]
    fn the_flag_arrow_points_down_and_is_not_a_speck() {
        let mut f = Frame::new(120, 120);
        draw_down_arrow(&mut f, 60, 10, Rgb::hex(0xF2C14E), 120);
        let ink = |c: &Rgb| *c != Rgb::new(0, 0, 0);
        let painted = f.color.iter().filter(|c| ink(c)).count();
        assert!(painted > 120, "the arrow is a speck: {painted} px");
        let row = |y: usize| f.color[y * 120..y * 120 + 120].iter().filter(|c| ink(c)).count();
        // x is 60 and y is 10: the shaft runs down to 24, the head 24 to 33, and
        // the two-pixel point closes it at 34.
        let (shaft, head, tip) = (row(12), row(26), row(34));
        assert!(head > shaft, "the head is not wider than the shaft: {head} vs {shaft}");
        assert!(tip < head, "the point never narrows: {tip} vs {head}");
    }

    #[test]
    fn a_card_draws_both_columns() {
        let mut f = Frame::new(640, 360);
        let left = vec!["LEFT ONE".to_string(), "LEFT TWO".to_string()];
        let right = vec!["RIGHT ONE".to_string()];
        draw_card(&mut f, "MATCHUP", "BOS AT CHI", &left, &right, Rgb::hex(0xE2572B));
        let ink = |c: &Rgb| *c != Rgb::new(0, 0, 0);
        let half = |x0: usize, x1: usize| {
            (0..360)
                .flat_map(|y| (x0..x1).map(move |x| (y, x)))
                .filter(|(y, x)| ink(&f.color[y * 640 + x]))
                .count()
        };
        assert!(half(0, 320) > 2_000, "the left column is empty");
        assert!(half(320, 640) > 2_000, "the right column is empty");
    }

    /// The score bug over a field, rendered so it can be looked at rather than
    /// only measured. Only runs when asked.
    #[test]
    fn the_scoreboard_can_be_rendered_to_a_picture() {
        if std::env::var_os("NOPADS_DUMP_UI").is_none() {
            return;
        }
        let teams = league_teams();
        let mut game = GameState::new(teams[0].clone(), teams[1].clone(), 5);
        game.score = [14, 7];
        game.timeouts = [3, 2];
        game.down = 3;
        game.to_go = 7.0;
        let board = Scoreboard::from_game(&game);
        let mut frame = Frame::new(640, 360);
        frame.clear(Rgb::hex(0x2C4A2E));
        for y in 0..360 {
            let c = Rgb::hex(0x2C4A2E).lerp(Rgb::hex(0x16261A), y as f32 / 360.0);
            frame.fill_rect(0, y, 640, 1, c);
        }
        draw_scoreboard(&mut frame, &board);
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../out/ui");
        crate::write_png(format!("{dir}/scoreboard.png"), &frame).expect("write png");
    }

    #[test]
    fn the_scoreboard_draws_over_the_picture_inside_the_canvas() {
        let teams = league_teams();
        let game = GameState::new(teams[0].clone(), teams[1].clone(), 5);
        let board = Scoreboard::from_game(&game);
        let mut frame = Frame::new(640, 360);
        draw_scoreboard(&mut frame, &board);
        let painted = frame
            .color
            .iter()
            .filter(|c| **c != Rgb::new(0, 0, 0))
            .count();
        assert!(painted > 4_000, "the scoreboard is a speck: {painted} px");
        assert_eq!(frame.w, 640, "the scoreboard changed the canvas");
    }
}

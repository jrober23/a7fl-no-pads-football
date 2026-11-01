//! `a7fl arcade` — play the game live, in a window or in the terminal.
//!
//! The field is drawn by the same software renderer the broadcast package uses:
//! a real window when the machine has a display, ANSI colour blocks when it does
//! not. The quarterbacks are yours: pick a play from the call sheet, then move the
//! quarterback with the stick and throw to whichever receiver you have locked on.
//! Everything else — defense, clock, downs, the rulebook — runs as it does in
//! `sim`.

use std::io::{self, Write};
use std::time::{Duration, Instant};

use a7fl_core::ai;
use a7fl_core::game::{Action, GamePhase, GameState};
use a7fl_core::math::{Rng, Vec2, Vec3};
use a7fl_core::moves::Kind;
use a7fl_core::playbook::{defensive_playbook, playbook, DefCall, Play};
use a7fl_core::rules::THROW_OFF_MIN_AIR_YARDS;
use a7fl_core::sim::{PlaySim, QbControl};
use a7fl_core::throwoff::{power_for_air, ThrowOffSetup, ThrowOffSim, ThrowPhase, ThrowRole};
use a7fl_render::broadcast::{Broadcast, CameraMode};
use a7fl_render::passing::{Badge, Guide, PassUi, Tone};
use a7fl_render::playsheet::{self, Dir};
use a7fl_render::terminal;

use crate::pad::{self, Input, Pad};
use crate::view::{View, CANVAS_H, CANVAS_W};

/// Picture refresh for live play. The field is drawn at the full canvas size, so
/// what you see is the renderer's real detail rather than a scaled-up quarter
/// image; when a frame costs more than this the loop renders as fast as it can
/// instead of sleeping on the clock.
const FRAME: Duration = Duration::from_millis(16);
const STEP: f32 = 1.0 / 60.0;
/// How long a result stays on screen before it moves on by itself.
const RESULT_HOLD: Duration = Duration::from_millis(2600);
/// How long the pass meter takes to fill from nothing to a full-weight throw.
const CHARGE_TIME: f32 = 1.05;
/// Even a tap throws the ball; it just does not throw it far.
const MIN_POWER: f32 = 0.18;
/// How far the aim stick can bend a pass off the man it is aimed at.
const MAX_LEAD_YARDS: f32 = 7.5;
/// The four passing buttons, in the order the badges are drawn.
const PASS_LABELS: [&str; 4] = ["A", "B", "X", "Y"];
/// The same four buttons once the ball is in a runner's hands.
const MOVES: [Kind; 4] = [Kind::Spin, Kind::Hurdle, Kind::Dive, Kind::Slide];
/// How far across the field a throw-off can be aimed: the width of the landing
/// zone the returner has to work in.
const THROW_OFF_AIM_YARDS: f32 = 13.0;

/// The views the player can switch between while the ball is live.
const CAMS: [CameraMode; 3] = [
    CameraMode::Broadcast,
    CameraMode::Classic,
    CameraMode::SkyCam,
];

fn cam_name(mode: CameraMode) -> &'static str {
    match mode {
        CameraMode::Classic => "CLASSIC",
        CameraMode::SkyCam => "SKY",
        _ => "BROADCAST",
    }
}

/// How long the direction has to be held before the call sheet starts scrolling.
const REPEAT_DELAY: f32 = 0.34;
/// How fast it then scrolls.
const REPEAT_EVERY: f32 = 0.09;

/// Menu navigation off the stick: one step for the push, then repeats while it
/// is held, the way a call sheet is meant to feel.
#[derive(Default)]
struct Nav {
    x: i8,
    y: i8,
    held: f32,
}

impl Nav {
    fn step(&mut self, stick: Vec2, dt: f32) -> Option<Dir> {
        let x = axis(stick.x);
        let y = axis(stick.y);
        // The stick is round, so a diagonal has to pick one axis: vertical wins,
        // because moving between sets is the bigger jump.
        let (dx, dy) = if y != 0 { (0, y) } else { (x, 0) };
        if (dx, dy) != (self.x, self.y) {
            self.x = dx;
            self.y = dy;
            self.held = 0.0;
            // A fresh push is a fresh step, including when the stick was let go.
            return dir_of(dx, dy);
        }
        if (dx, dy) == (0, 0) {
            return None;
        }
        self.held += dt;
        if self.held >= REPEAT_DELAY {
            self.held -= REPEAT_EVERY;
            return dir_of(dx, dy);
        }
        None
    }
}

fn dir_of(dx: i8, dy: i8) -> Option<Dir> {
    match (dx, dy) {
        (0, d) if d > 0 => Some(Dir::Up),
        (0, d) if d < 0 => Some(Dir::Down),
        (d, _) if d > 0 => Some(Dir::Right),
        (d, _) if d < 0 => Some(Dir::Left),
        _ => None,
    }
}

fn axis(v: f32) -> i8 {
    if v > 0.5 {
        1
    } else if v < -0.5 {
        -1
    } else {
        0
    }
}

pub struct Arcade {
    game: GameState,
    pad: Option<Pad>,
    view: View,
    broadcast: Broadcast,
    book: Vec<Play>,
    defbook: Vec<DefCall>,
    pick: usize,
    /// The team the player is running. He is always the offense, so this is
    /// whichever side of a throw-off he is on.
    my_team: Option<usize>,
    /// Which of `CAMS` the picture is coming from.
    cam: usize,
    quit: bool,
    /// Penalty insurance: some pads report the stick's Y the other way round.
    flip_y: bool,
}

impl Arcade {
    pub fn new(game: GameState, pad: Option<Pad>, view: View) -> Self {
        let book = playbook();
        let defbook = defensive_playbook();
        let teams = [game.teams[0].clone(), game.teams[1].clone()];
        let broadcast = Broadcast::new(CANVAS_W, CANVAS_H, teams);
        Arcade {
            game,
            pad,
            view,
            broadcast,
            book,
            defbook,
            pick: 0,
            my_team: None,
            cam: 0,
            quit: false,
            flip_y: false,
        }
    }

    /// Reverse the stick's vertical axis, for pads that report it inverted.
    pub fn with_flip_y(mut self, flip: bool) -> Self {
        self.flip_y = flip;
        self
    }

    pub fn run(&mut self) {
        let terminal_mode = !self.view.is_window();
        if terminal_mode {
            print!("{}", terminal::enter());
            io::stdout().flush().ok();
        }
        self.play();
        if terminal_mode {
            print!("{}", terminal::leave());
            io::stdout().flush().ok();
        }
    }

    fn play(&mut self) {
        while !self.game.is_over() && !self.quit {
            match self.game.action() {
                Action::GameOver => break,
                Action::ThrowOff => {
                    if !self.play_throw_off(false) {
                        break;
                    }
                }
                Action::ChooseConversion => {
                    // Pick the try, then go back round: a conversion is a snap
                    // like any other, so it gets called and played, not rolled.
                    let two = self.choose_points();
                    self.game.set_conversion(two);
                }
                Action::CallPlay { fourth_down } => {
                    if fourth_down && self.ask_throw_off() {
                        if !self.play_throw_off(true) {
                            break;
                        }
                        continue;
                    }
                    if self.call_sheet() {
                        self.run_snap();
                    }
                }
            }
        }
        if !self.quit {
            self.show_final();
        }
    }

    // ------------------------------------------------------------------
    // Play calling
    // ------------------------------------------------------------------

    /// The call sheet: one set of plays as diagrams, highlighted with the d-pad.
    /// Returns true when a play has been chosen.
    fn call_sheet(&mut self) -> bool {
        let mut nav = Nav::default();
        loop {
            let mut frame = a7fl_render::Frame::new(CANVAS_W, CANVAS_H);
            playsheet::draw(&mut frame, &self.book, self.pick);
            let lines = self.call_sheet_lines();
            if !self.view.show(&mut frame, &lines) {
                self.quit = true;
                return false;
            }
            let input = self.wait(Duration::from_millis(16));
            if input.pressed.start {
                // The call sheet is where a player spends most of the game, so this
                // is the door to the camera setting and to quitting, not an instant
                // exit the way it used to be.
                if !pause_menu(&mut self.view, self.pad.as_mut(), &mut self.cam) {
                    self.quit = true;
                    return false;
                }
                continue;
            }
            if input.pressed.y {
                // "Ask the coach": let the AI call it.
                let mut rng = Rng::new(self.game.seed ^ 0x51CE);
                self.pick = rng.index(self.book.len());
            }
            if let Some(dir) = nav.step(input.stick, 0.016) {
                self.pick = playsheet::step(self.book.len(), self.pick, dir);
            }
            if input.pressed.a {
                return true;
            }
        }
    }

    fn call_sheet_lines(&self) -> Vec<String> {
        let play = &self.book[self.pick];
        let pages = (self.book.len() + playsheet::PER_PAGE - 1) / playsheet::PER_PAGE;
        let mut lines = vec![
            self.header(),
            format!(
                "  {:<16} {} / {}        PLAY {} OF {}   SET {} OF {}",
                play.name,
                play.family.label(),
                play.formation.label(),
                self.pick + 1,
                self.book.len(),
                playsheet::page_of(self.pick) + 1,
                pages
            ),
            "  DPAD / STICK: HIGHLIGHT   [A] SNAP   [Y] ASK COACH   [START] PAUSE".to_string(),
        ];
        if let Some(last) = self.game.recent_plays(1).pop() {
            lines.push(format!("  Last: {last}"));
        }
        lines
    }

    fn choose_points(&mut self) -> bool {
        // The second overtime round forces two-point tries.
        let mut two = self.game.conversion_forced_two();
        loop {
            let lines = vec![
                self.header(),
                String::new(),
                "  CONVERSION".to_string(),
                String::new(),
                format!("    {} 1 point from the 5", if two { "  " } else { ">>" }),
                format!("    {} 2 points from the 10", if two { ">>" } else { "  " }),
                String::new(),
                "  [B] TOGGLE   [A] CONFIRM".to_string(),
            ];
            if !self.view.page(&lines) {
                self.quit = true;
                return two;
            }
            let input = self.wait(Duration::from_millis(16));
            if input.pressed.start {
                if !pause_menu(&mut self.view, self.pad.as_mut(), &mut self.cam) {
                    self.quit = true;
                    return two;
                }
                continue;
            }
            if input.pressed.b {
                two = !two;
            }
            if input.pressed.a {
                return two;
            }
        }
    }

    /// Fourth down: go for it, or use the A7FL throw-off in place of a punt.
    fn ask_throw_off(&mut self) -> bool {
        let mut throw_off = false;
        loop {
            let lines = vec![
                self.header(),
                String::new(),
                "  FOURTH DOWN - no punting in this league".to_string(),
                String::new(),
                format!("    {} GO FOR IT", if throw_off { "  " } else { ">>" }),
                format!("    {} THROW IT OFF", if throw_off { ">>" } else { "  " }),
                String::new(),
                "  [B] TOGGLE   [A] CONFIRM".to_string(),
            ];
            if !self.view.page(&lines) {
                self.quit = true;
                return false;
            }
            let input = self.wait(Duration::from_millis(16));
            if input.pressed.start {
                if !pause_menu(&mut self.view, self.pad.as_mut(), &mut self.cam) {
                    self.quit = true;
                    return false;
                }
                continue;
            }
            if input.pressed.b {
                throw_off = !throw_off;
            }
            if input.pressed.a {
                return throw_off;
            }
        }
    }

    // ------------------------------------------------------------------
    // Throw-offs
    // ------------------------------------------------------------------

    /// Play a throw-off: throw it off if it is yours to throw, or get under it
    /// and bring it back if it is not. Returns false when the player quit.
    fn play_throw_off(&mut self, free_throw: bool) -> bool {
        let Some(setup) = self.throw_off_to_play(free_throw) else {
            return true;
        };
        // The player runs the offense, so a throw-off is his to throw exactly
        // when his team is the one throwing it: after he scores, he throws it
        // off; after the other lot score, he receives it.
        let my_team = self.my_team.unwrap_or(setup.receiving_team);
        self.my_team = Some(my_team);
        let header = self.header();

        // The sim borrows the two teams, so hand it its own copies and leave the
        // rest of the struct free to draw the picture.
        let throwing = self.game.teams[setup.throwing_team].clone();
        let receiving = self.game.teams[setup.receiving_team].clone();
        let mut sim = ThrowOffSim::new(&setup, &throwing, &receiving);
        sim.human_thrower = setup.throwing_team == my_team;

        let finished = self.drive_throw_off(&mut sim, my_team, &header);
        if !finished {
            return false;
        }
        if let Some(result) = sim.result.clone() {
            let rec = self.game.finish_throw_off(setup, result);
            let text = rec.description().to_string();
            self.show_result(&text, None);
        }
        true
    }

    /// The throw-off the game is waiting on, set up but not run.
    fn throw_off_to_play(&mut self, free_throw: bool) -> Option<ThrowOffSetup> {
        if free_throw {
            return self.game.fourth_down_throw_off_setup();
        }
        match &self.game.phase {
            GamePhase::ThrowOff { receiving, from_own } => {
                Some(self.game.throw_off_setup(*receiving, *from_own, false))
            }
            _ => None,
        }
    }

    /// The live throw-off. Returns false when the player has quit.
    fn drive_throw_off(&mut self, sim: &mut ThrowOffSim, my_team: usize, header: &str) -> bool {
        let mut acc = 0.0f64;
        let mut last = Instant::now();
        let mut drew = Instant::now() - FRAME;
        // The meter, filled while the throw button is held.
        let mut hold = 0.0f32;
        let mut charging = false;
        // Where across the field the throw is aimed, in yards.
        let mut aim_y = 0.0f32;

        loop {
            let now = Instant::now();
            let frame_dt = now.duration_since(last).as_secs_f64();
            acc += frame_dt;
            last = now;

            let input = pad::mix(self.pad.as_mut(), self.view.poll());
            if input.pressed.start {
                // START pauses: the camera setting and quitting both live behind it.
                if !pause_menu(&mut self.view, self.pad.as_mut(), &mut self.cam) {
                    self.quit = true;
                    return false;
                }
                last = Instant::now();
                acc = 0.0;
                continue;
            }
            // X, not the trigger: on a throw-off the trigger is the throw itself.
            if input.pressed.x {
                self.cam = (self.cam + 1) % CAMS.len();
            }
            // The same screen mapping as a scrimmage snap: screen-up is world -y.
            let vertical = if self.flip_y { input.stick.y } else { -input.stick.y };
            let stick = Vec2::new(input.stick.x, vertical);

            if sim.phase == ThrowPhase::Presnap && sim.human_thrower {
                // Hold to wind the arm up, let go to throw it off. The aim can be
                // picked before the charge starts, which is what the guide is for.
                let holding = input.held.a || input.held.throw;
                if holding {
                    charging = true;
                    hold = (hold + frame_dt as f32).min(CHARGE_TIME);
                } else if charging {
                    let power = (hold / CHARGE_TIME).clamp(MIN_POWER, 1.0);
                    sim.launch_with(power, aim_y);
                    charging = false;
                    hold = 0.0;
                }
                if stick.y.abs() > 0.30 {
                    aim_y = stick.y.clamp(-1.0, 1.0) * THROW_OFF_AIM_YARDS;
                }
            }

            // Whoever is yours: the returner while you are receiving, otherwise
            // the man on your team nearest the ball once it is in the air.
            let mine = human_man(sim, my_team);
            sim.set_human(mine, stick);

            while acc >= STEP as f64 && sim.result.is_none() {
                sim.step(STEP);
                acc -= STEP as f64;
            }

            if now.duration_since(drew) >= FRAME || sim.result.is_some() {
                drew = now;
                let mut guide = None;
                let mut short = false;
                let mut needed = None;
                let mut power = None;
                if sim.phase == ThrowPhase::Presnap && sim.human_thrower {
                    let p = (hold / CHARGE_TIME).clamp(0.0, 1.0);
                    let plan = sim.throw_off_plan(p, aim_y);
                    short = plan.too_short;
                    // The notch is the 40-yard rule: under it is a Run Back.
                    needed = Some(power_for_air(THROW_OFF_MIN_AIR_YARDS));
                    power = Some(p);
                    let hand = sim.players[0].pos;
                    guide = Some(Guide {
                        from: Vec3::new(hand.x, hand.y, 1.8),
                        receiver: Vec3::new(plan.landing.x, plan.landing.y, 0.12),
                        landing: Vec3::new(plan.landing.x, plan.landing.y, 0.12),
                    });
                }
                let ui = PassUi {
                    badges: Vec::new(),
                    guide,
                    power,
                    short,
                    needed,
                };
                let mode = CAMS[self.cam % CAMS.len()];
                let mut frame = self.broadcast.throw_off_frame_live(sim, mode, &ui);
                let lines = vec![
                    header.to_string(),
                    throw_off_line(self, sim, my_team),
                    throw_off_controls(self, sim, my_team),
                ];
                if !self.view.show(&mut frame, &lines) {
                    self.quit = true;
                    return false;
                }
            }

            if sim.result.is_some() {
                // Let the last picture sit so the catch or the return reads.
                std::thread::sleep(Duration::from_millis(260));
                return true;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    // ------------------------------------------------------------------
    // The snap itself
    // ------------------------------------------------------------------

    fn run_snap(&mut self) {
        // Playing an offensive snap is what tells us whose team this is.
        self.my_team = Some(self.game.possession);
        let play = self.book[self.pick].clone();
        let def = self.cpu_defense();
        let header = self.header();
        let play_line = format!(
            "{} - {} - {} shows {}",
            play.name,
            play.formation.label(),
            self.game.teams[self.game.defense()].abbr,
            def.name
        );
        let off_abbr = self.game.teams[self.game.offense()].abbr.clone();

        // Borrow the fields separately: the game owns the snap, the rest of the
        // struct drives the picture and reads the controls.
        let game = &mut self.game;
        let broadcast = &mut self.broadcast;
        let view = &mut self.view;
        let pad = &mut self.pad;
        let quit = &mut self.quit;
        let cam = &mut self.cam;
        let flip_y = self.flip_y;

        let record = game.snap_with(play, def, |sim| {
            let finished = drive(
                sim,
                broadcast,
                view,
                pad.as_mut(),
                flip_y,
                cam,
                &header,
                &play_line,
                &off_abbr,
            );
            if !finished {
                *quit = true;
            }
        });

        if let Some(record) = record {
            let text = record.description().to_string();
            self.show_result(&text, None);
        }
    }

    fn cpu_defense(&self) -> DefCall {
        let book = &self.defbook;
        let situation = ai::Situation::from_game(&self.game);
        if situation.yards_to_goal <= 5.0 {
            if let Some(call) = book.iter().find(|d| d.name == "GOAL LINE BEAR") {
                return call.clone();
            }
        }
        if situation.down >= 3 && situation.to_go <= 3.0 {
            if let Some(call) = book.iter().find(|d| d.name == "MIKE BLITZ") {
                return call.clone();
            }
        }
        if situation.down >= 3 && situation.to_go >= 8.0 {
            if let Some(call) = book.iter().find(|d| d.name == "COVER 3 ZONE") {
                return call.clone();
            }
        }
        let mut rng = Rng::new(self.game.seed ^ 0xD37E_C0DE);
        book[rng.index(book.len())].clone()
    }

    // ------------------------------------------------------------------
    // Screens
    // ------------------------------------------------------------------

    fn header(&self) -> String {
        format!(
            "{} {} - {} {}   {} {}   {}   BALL ON {}",
            self.game.teams[0].abbr,
            self.game.score[0],
            self.game.teams[1].abbr,
            self.game.score[1],
            self.game.quarter_text(),
            self.game.clock_text(),
            self.game.down_and_distance(),
            a7fl_core::field::ball_on_label(self.game.yards_to_goal())
        )
    }

    fn show_result(&mut self, text: &str, points: Option<u32>) {
        let mut lines = vec![self.header(), String::new()];
        if let Some(points) = points {
            let label = if points == 2 { "POINTS" } else { "POINT" };
            lines.push(format!("  CONVERSION - {points} {label}"));
            lines.push(String::new());
        }
        lines.push(format!("  {}", text.to_uppercase()));
        lines.push(String::new());
        for line in self.game.recent_plays(3) {
            lines.push(format!("  · {line}"));
        }
        lines.push(String::new());
        lines.push("  [A] CONTINUE".to_string());
        if !self.view.page(&lines) {
            self.quit = true;
            return;
        }

        let start = Instant::now();
        loop {
            let input = self.wait(Duration::from_millis(24));
            if input.pressed.a || input.pressed.start {
                break;
            }
            if start.elapsed() >= RESULT_HOLD {
                break;
            }
        }
    }

    fn show_final(&mut self) {
        let winner = self.game.winner.unwrap_or(0);
        let mut lines = vec![
            String::new(),
            format!("  FINAL   {}", self.game.teams[winner].display_upper()),
            String::new(),
            format!(
                "  {} {}  -  {} {}",
                self.game.teams[0].abbr,
                self.game.score[0],
                self.game.teams[1].abbr,
                self.game.score[1]
            ),
            String::new(),
        ];
        let book = &self.game.box_score;
        for side in 0..2 {
            let t = &book.teams[side];
            lines.push(format!(
                "  {:<4} {:.0} yds   {}/{} pass {:.0}   {} rush {:.0}   {} TO   {} 1st",
                self.game.teams[side].abbr,
                t.total_yards,
                t.pass_comp,
                t.pass_att,
                t.pass_yards,
                t.rush_att,
                t.rush_yards,
                t.turnovers,
                t.first_downs
            ));
        }
        lines.push(String::new());
        for line in self.game.recent_plays(5) {
            lines.push(format!("  · {line}"));
        }
        lines.push(String::new());
        lines.push("  [A] EXIT".to_string());
        if !self.view.page(&lines) {
            return;
        }
        loop {
            let input = self.wait(Duration::from_millis(30));
            if input.pressed.a || input.pressed.start {
                break;
            }
        }
    }

    fn input(&mut self) -> Input {
        pad::mix(self.pad.as_mut(), self.view.poll())
    }

    fn wait(&mut self, gap: Duration) -> Input {
        std::thread::sleep(gap);
        self.input()
    }
}

/// Run one snap live: simulate in fixed steps, render at a steady rate and let the
/// player steer the quarterback until the play resolves. Returns false when the
/// player has quit.
#[allow(clippy::too_many_arguments)]
/// The pause screen. Returns false when the player chose to quit the game.
///
/// START used to close the game on the spot, which put the camera setting — the
/// one thing a player wants to change in the middle of a drive — out of reach
/// once the ball was snapped.
fn pause_menu(
    view: &mut View,
    mut pad: Option<&mut Pad>,
    cam: &mut usize,
) -> bool {
    const ITEMS: usize = 3;
    let mut nav = Nav::default();
    let mut pick = 0usize;
    let mut last = Instant::now();
    // A button still held from the game must not select the first item.
    view.reset();
    let keep = loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.start || input.pressed.b {
            break true;
        }
        match nav.step(input.stick, dt) {
            Some(Dir::Up) => pick = (pick + ITEMS - 1) % ITEMS,
            Some(Dir::Down) => pick = (pick + 1) % ITEMS,
            _ => {}
        }
        let mark = |on: bool| if on { ">>" } else { "  " };
        let lines = vec![
            String::new(),
            String::new(),
            "  PAUSED".to_string(),
            String::new(),
            format!("    {} RESUME", mark(pick == 0)),
            format!(
                "    {} CAMERA    {:<9}   [A] CHANGE",
                mark(pick == 1),
                cam_name(CAMS[*cam % CAMS.len()])
            ),
            format!("    {} QUIT GAME", mark(pick == 2)),
            String::new(),
            "  [STICK] CHOOSE   [A] SELECT   [START] RESUME".to_string(),
        ];
        if !view.page(&lines) {
            break false;
        }
        if input.pressed.a {
            match pick {
                0 => break true,
                1 => *cam = (*cam + 1) % CAMS.len(),
                _ => break false,
            }
        }
        std::thread::sleep(Duration::from_millis(16));
    };
    // Whatever was pressed in here must not carry into the game.
    view.reset();
    keep
}

fn drive(
    sim: &mut PlaySim,
    broadcast: &mut Broadcast,
    view: &mut View,
    mut pad: Option<&mut Pad>,
    flip_y: bool,
    cam: &mut usize,
    header: &str,
    play_line: &str,
    off_abbr: &str,
) -> bool {
    let mut acc = 0.0f64;
    let mut last = Instant::now();
    let mut drew = Instant::now() - FRAME;
    // The button being held for a pass: which receiver, and for how long. The
    // meter fills while it is down and the ball leaves when it comes up.
    let mut charge: Option<(usize, f32)> = None;
    // Where the aim stick has bent the throw, in yards off the receiver.
    let mut aim = Vec2::ZERO;

    loop {
        let now = Instant::now();
        let frame_dt = now.duration_since(last).as_secs_f64();
        acc += frame_dt;
        last = now;

        let input = pad::mix(pad.as_deref_mut(), view.poll());
        if input.pressed.start {
            if !pause_menu(view, pad.as_deref_mut(), cam) {
                return false;
            }
            // The whole pause was spent inside the wall clock: do not run the play
            // forward by it.
            last = Instant::now();
            acc = 0.0;
            continue;
        }
        // The trigger is free during a snap now that the face buttons pass, so it
        // cycles the camera.
        if input.pressed.throw {
            *cam = (*cam + 1) % CAMS.len();
        }
        let view_mode = CAMS[*cam % CAMS.len()];

        // Once the ball is in a runner's hands and there is no pass to throw, the
        // face buttons are his move set rather than the receivers'.
        let running = sim.face_buttons_are_moves();
        let targets = if running {
            Vec::new()
        } else {
            sim.eligible_targets()
        };
        // The arcade camera looks across the field, so screen-right is world +x
        // and screen-up is world -y. Feed the stick through that mapping: handed
        // in raw, up and down ran backwards.
        let vertical = if flip_y { input.stick.y } else { -input.stick.y };
        let stick = Vec2::new(input.stick.x, vertical);
        let held = [input.held.a, input.held.b, input.held.x, input.held.y];
        let down = [input.pressed.a, input.pressed.b, input.pressed.x, input.pressed.y];
        if running {
            // A pass wound up before he crossed the line is not a throw any more.
            charge = None;
            for (slot, kind) in MOVES.iter().enumerate() {
                if down[slot] {
                    sim.request_move(*kind);
                }
            }
        }

        // Holding a receiver's button winds the pass up. The stick bends it off
        // him while the meter fills, which is how you lead a man into grass or
        // put it on his back shoulder.
        if let Some((slot, held_for)) = charge {
            if slot >= targets.len() {
                charge = None;
            } else {
                if stick.length() > 0.30 {
                    aim = Vec2::new(stick.x, stick.y).normalize_or_zero()
                        * (MAX_LEAD_YARDS * stick.length().min(1.0));
                }
                charge = Some((slot, held_for + frame_dt as f32));
            }
        }
        if charge.is_none() && !running {
            if let Some(slot) = (0..targets.len().min(PASS_LABELS.len())).find(|&s| down[s]) {
                charge = Some((slot, 0.0));
                aim = Vec2::ZERO;
            }
        }

        // Letting go throws it at whatever weight the meter got to.
        let mut release: Option<(usize, f32)> = None;
        if let Some((slot, held_for)) = charge {
            if !held[slot] {
                let power = (held_for / CHARGE_TIME).clamp(MIN_POWER, 1.0);
                release = Some((targets[slot].1, power));
                charge = None;
            }
        }

        let aimed = charge
            .map(|(slot, _)| targets[slot].1)
            .or_else(|| release.map(|(index, _)| index));
        let power = release.map(|(_, p)| p).unwrap_or_else(|| {
            charge
                .map(|(_, t)| (t / CHARGE_TIME).clamp(0.0, 1.0))
                .unwrap_or(0.0)
        });
        sim.set_control(Some(QbControl {
            move_dir: stick,
            target: aimed,
            throw: release.is_some(),
            power,
            aim,
        }));
        if release.is_some() {
            aim = Vec2::ZERO;
        }

        while acc >= STEP as f64 && sim.result.is_none() {
            sim.step(STEP);
            acc -= STEP as f64;
        }

        if now.duration_since(drew) >= FRAME || sim.result.is_some() {
            drew = now;
            // A button over every eligible man, and while a pass is wound up,
            // the guide showing where it will come down and the meter showing
            // whether the arm can get it there.
            let mut badges = Vec::new();
            for (slot, (_, index)) in targets.iter().enumerate().take(PASS_LABELS.len()) {
                let separation = sim.separation(*index);
                let tone = if separation > 3.0 {
                    Tone::Open
                } else if separation > 1.6 {
                    Tone::Tight
                } else {
                    Tone::Covered
                };
                let man = sim.players[*index].pos;
                badges.push(Badge {
                    at: Vec3::new(man.x, man.y, 2.15),
                    label: PASS_LABELS[slot],
                    tone,
                    aimed: charge.map(|(s, _)| s) == Some(slot),
                });
            }

            let mut guide = None;
            let mut short = false;
            let mut needed = None;
            if let Some((slot, _)) = charge {
                if let Some((_, index)) = targets.get(slot) {
                    let plan = sim.throw_plan(sim.qb, *index, power, aim);
                    short = plan.distance > plan.reach + 0.4;
                    needed = Some(plan.power_needed);
                    let hand = sim.qb_pos();
                    let man = sim.players[*index].pos;
                    guide = Some(Guide {
                        from: Vec3::new(hand.x, hand.y, 1.8),
                        receiver: Vec3::new(man.x, man.y, 1.0),
                        landing: Vec3::new(plan.landing.x, plan.landing.y, 0.12),
                    });
                }
            }
            let ui = PassUi {
                badges,
                guide,
                power: charge.map(|(_, t)| (t / CHARGE_TIME).clamp(0.0, 1.0)),
                short,
                needed,
            };

            let mut frame = broadcast.live_frame_ui(sim, view_mode, &ui);
            let mut lines = vec![header.to_string(), play_line.to_string()];

            if running {
                // The moves, and which one is in flight right now.
                let mut row = String::from("  [A] SPIN   [B] HURDLE   [X] DIVE   [Y] SLIDE");
                if let Some(kind) = sim.carrier_move().kind() {
                    row.push_str(&format!("      {}", kind.label()));
                }
                lines.push(row);
            } else {
                // Who is open, on the buttons the badges are showing.
                let mut row = String::new();
                for (slot, (_, index)) in targets.iter().enumerate().take(PASS_LABELS.len()) {
                    let name = broadcast.teams[sim.setup.offense_team].roster[*index].last_name();
                    if !row.is_empty() {
                        row.push_str("   ");
                    }
                    row.push_str(&format!(
                        "{} {:<9} {:>4.1}",
                        PASS_LABELS[slot],
                        name,
                        sim.separation(*index)
                    ));
                }
                lines.push(row);
            }
            lines.push(format!(
                "{off_abbr} ball   MOVE   HOLD A/B/X/Y TO PASS   AIM WITH STICK   [RT] {}   [START] PAUSE",
                cam_name(view_mode)
            ));

            if !view.show(&mut frame, &lines) {
                return false;
            }
        }

        if sim.result.is_some() {
            // Let the last picture sit for a moment so the catch or the hit reads.
            std::thread::sleep(Duration::from_millis(260));
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Which man on your team you are driving during a throw-off: the returner while
/// you are receiving, and otherwise the man nearest the ball once it is in the
/// air — which is how you get downfield to make the tackle.
fn human_man(sim: &ThrowOffSim, my_team: usize) -> Option<usize> {
    let returner = sim.players.iter().position(|p| p.role == ThrowRole::Returner)?;
    if sim.players[returner].team == my_team {
        return Some(returner);
    }
    if sim.phase == ThrowPhase::Presnap {
        return None;
    }
    sim.players
        .iter()
        .enumerate()
        .filter(|(_, p)| p.team == my_team && !p.down)
        .min_by(|a, b| {
            a.1.pos
                .dist(sim.ball)
                .partial_cmp(&b.1.pos.dist(sim.ball))
                .unwrap()
        })
        .map(|(i, _)| i)
}

fn throw_off_line(arcade: &Arcade, sim: &ThrowOffSim, my_team: usize) -> String {
    let abbr = |t: usize| arcade.game.teams[t].abbr.clone();
    match sim.phase {
        ThrowPhase::Presnap => {
            if sim.human_thrower {
                format!(
                    "  {} THROW-OFF FROM THE {} - HOLD A TO WIND UP, LET GO TO THROW",
                    abbr(sim.setup.throwing_team),
                    sim.setup.from_own_yards as i32
                )
            } else {
                format!(
                    "  {} THROW-OFF FROM THE {} - GET UNDER IT",
                    abbr(sim.setup.throwing_team),
                    sim.setup.from_own_yards as i32
                )
            }
        }
        ThrowPhase::Flight => "  THE BALL IS IN THE AIR".to_string(),
        ThrowPhase::Return => format!(
            "  RETURN - {} BALL IN HAND",
            abbr(sim.setup.receiving_team)
        ),
        ThrowPhase::Dead => format!("  THROW-OFF OVER - {} BALL", abbr(my_team)),
    }
}

fn throw_off_controls(arcade: &Arcade, sim: &ThrowOffSim, my_team: usize) -> String {
    let job = if sim.setup.throwing_team == my_team {
        "COVER IT"
    } else {
        "RETURN IT"
    };
    format!(
        "{:<9} {} BALL   MOVE WITH STICK   [X] {}   [START] PAUSE",
        job,
        arcade.game.teams[sim.setup.receiving_team].abbr,
        cam_name(CAMS[arcade.cam % CAMS.len()])
    )
}

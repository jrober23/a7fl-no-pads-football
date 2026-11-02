//! `a7fl` — command line client for No Pads Football.
//!
//! ```text
//! a7fl sim   [--seed N] [--home IDX] [--away IDX] [--verbose]
//! a7fl play  [--seed N] [--home IDX] [--away IDX]
//! a7fl film  [--seed N] [--out DIR]
//! a7fl teams
//! a7fl rules
//! ```

use std::io::{self, BufRead, Write};

use a7fl_core::ai;
use a7fl_core::game::{GamePhase, GameState, PlayRecord};
use a7fl_core::math::{Vec2, Vec3};
use a7fl_core::playbook::{defensive_playbook, playbook, DefCall, Play};
use a7fl_core::sim::PlaySim;
use a7fl_core::team::{league_teams, Team};
use a7fl_render::broadcast::{Broadcast, CameraMode};
use a7fl_render::capture::{write_gif, write_png};
use a7fl_render::hud;
use a7fl_render::passing::{Badge, Guide, PassUi, Tone};
use a7fl_render::raster::Frame;

mod arcade;
mod pad;
mod view;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Bare `a7fl` starts the game; anything else is a named command.
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("arcade");
    let rest = if args.len() > 1 { &args[1..] } else { &args[0..0] };
    match cmd {
        "sim" => cmd_sim(rest),
        "play" => cmd_play(rest),
        "film" => cmd_film(rest),
        "arcade" => cmd_arcade(rest),
        "teams" => cmd_teams(),
        "rules" => cmd_rules(),
        _ => print_help(),
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn num(args: &[String], name: &str, default: u64) -> u64 {
    flag(args, name)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn print_help() {
    println!("A7FL: No Pads Football — 7-on-7 simulation and broadcast renderer");
    println!();
    println!("USAGE");
    println!("  a7fl sim   [--seed N] [--home IDX] [--away IDX] [--verbose]");
    println!("  a7fl play  [--seed N] [--home IDX] [--away IDX]");
    println!("  a7fl film  [--seed N] [--out DIR]");
    println!("  a7fl arcade [--seed N] [--home IDX] [--away IDX] [--terminal]");
    println!("  a7fl teams");
    println!("  a7fl rules");
    println!();
    println!("  sim    simulate a full game and print the box score");
    println!("  play   call plays yourself from the terminal");
    println!("  film   render broadcast stills and a highlight GIF");
    println!("  arcade play a full game live on a gamepad (this is the default command)");
    println!("  teams  list the league and its rosters");
    println!("  rules  print the A7FL rules this game enforces");
}

fn pick_teams(args: &[String]) -> (Team, Team) {
    let teams = league_teams();
    let home = num(args, "--home", 0) as usize;
    let away = num(args, "--away", 1) as usize;
    let home = home.min(teams.len() - 1);
    let mut away = away.min(teams.len() - 1);
    if away == home {
        away = (home + 1) % teams.len();
    }
    (teams[home].clone(), teams[away].clone())
}

// ---------------------------------------------------------------------------
// sim
// ---------------------------------------------------------------------------

fn cmd_sim(args: &[String]) {
    let seed = num(args, "--seed", 7);
    let verbose = has(args, "--verbose");
    let (home, away) = pick_teams(args);
    let mut game = GameState::new(home, away, seed);
    game.simulate_game();
    print_box_score(&game);
    if verbose {
        println!("\nPLAY-BY-PLAY");
        for r in &game.log {
            println!(
                "  Q{} {}  {}",
                r.quarter(),
                clock_text(r.clock()),
                r.description()
            );
        }
    }
    println!("\nCOMMENTARY");
    for line in game.commentary.iter().rev().take(10).rev() {
        println!("  {line}");
    }
}

fn clock_text(seconds: f32) -> String {
    let s = seconds.max(0.0);
    let m = (s / 60.0).floor() as u32;
    let sec = (s - m as f32 * 60.0).floor() as u32;
    format!("{m}:{sec:02}")
}

fn print_box_score(game: &GameState) {
    let t = &game.teams;
    let b = &game.box_score;
    println!();
    println!(
        "{:<24} {:>3} {:>3} {:>3} {:>3} {:>6}",
        "", "Q1", "Q2", "Q3", "Q4", "FINAL"
    );
    for side in 0..2 {
        let mut ot = 0;
        for q in 4..5 {
            ot += b.quarter_scores[side][q];
        }
        println!(
            "{:<24} {:>3} {:>3} {:>3} {:>3} {:>6}",
            t[side].display_upper(),
            b.quarter_scores[side][0],
            b.quarter_scores[side][1],
            b.quarter_scores[side][2],
            b.quarter_scores[side][3],
            game.score[side]
        );
        let _ = ot;
    }
    if game.is_overtime() {
        println!("(overtime: {} - {})", b.quarter_scores[0][4], b.quarter_scores[1][4]);
    }
    println!();
    println!(
        "{:<22} {:>14} {:>14}",
        "TEAM STATS", t[0].abbr, t[1].abbr
    );
    let row = |label: &str, a: String, b: String| {
        println!("{label:<22} {a:>14} {b:>14}");
    };
    row(
        "Total yards",
        format!("{:.0}", b.teams[0].total_yards),
        format!("{:.0}", b.teams[1].total_yards),
    );
    row(
        "Passing",
        format!("{}/{} {:.0}", b.teams[0].pass_comp, b.teams[0].pass_att, b.teams[0].pass_yards),
        format!("{}/{} {:.0}", b.teams[1].pass_comp, b.teams[1].pass_att, b.teams[1].pass_yards),
    );
    row(
        "Rushing",
        format!("{} {:.0}", b.teams[0].rush_att, b.teams[0].rush_yards),
        format!("{} {:.0}", b.teams[1].rush_att, b.teams[1].rush_yards),
    );
    row(
        "Sacks allowed",
        format!("{}", b.teams[0].sacks_allowed),
        format!("{}", b.teams[1].sacks_allowed),
    );
    row(
        "Turnovers",
        format!("{}", b.teams[0].turnovers),
        format!("{}", b.teams[1].turnovers),
    );
    row(
        "First downs",
        format!("{}", b.teams[0].first_downs),
        format!("{}", b.teams[1].first_downs),
    );
    row(
        "Penalties / yards",
        format!("{} / {:.0}", b.teams[0].penalties, b.teams[0].penalty_yards),
        format!("{} / {:.0}", b.teams[1].penalties, b.teams[1].penalty_yards),
    );
    row(
        "Conversions",
        format!("{}/{}", b.teams[0].conversions_made, b.teams[0].conversions_att),
        format!("{}/{}", b.teams[1].conversions_made, b.teams[1].conversions_att),
    );
    row(
        "Possession",
        clock_text(b.teams[0].time_of_possession),
        clock_text(b.teams[1].time_of_possession),
    );

    println!("\nTOP PERFORMERS");
    for side in 0..2 {
        let team = &t[side];
        let mut best_pass: Option<(String, u32, u32, f32)> = None;
        let mut best_rush: Option<(String, u32, f32)> = None;
        let mut best_rec: Option<(String, u32, f32)> = None;
        let mut best_def: Option<(String, u32)> = None;
        for p in &b.players[side] {
            let name = team
                .roster
                .iter()
                .find(|pl| pl.id as usize == p.roster_index)
                .map(|pl| pl.full_name())
                .unwrap_or_else(|| "?".to_string());
            if p.pass_att > 0 && best_pass.as_ref().map(|(_, a, _, _)| p.pass_att > *a).unwrap_or(true) {
                best_pass = Some((name.clone(), p.pass_att, p.pass_comp, p.pass_yards));
            }
            if p.rush_att > 0 && best_rush.as_ref().map(|(_, a, _)| p.rush_att > *a).unwrap_or(true) {
                best_rush = Some((name.clone(), p.rush_att, p.rush_yards));
            }
            if p.receptions > 0 && best_rec.as_ref().map(|(_, _, a)| p.rec_yards > *a).unwrap_or(true) {
                best_rec = Some((name.clone(), p.receptions, p.rec_yards));
            }
            if p.tackles > 0 && best_def.as_ref().map(|(_, a)| p.tackles > *a).unwrap_or(true) {
                best_def = Some((name, p.tackles));
            }
        }
        println!("  {}", team.display_upper());
        if let Some((n, att, comp, yds)) = best_pass {
            println!("    PASS  {n:<24} {comp}/{att}, {yds:.0} yds");
        }
        if let Some((n, att, yds)) = best_rush {
            println!("    RUSH  {n:<24} {att} car, {yds:.0} yds");
        }
        if let Some((n, rec, yds)) = best_rec {
            println!("    REC   {n:<24} {rec} rec, {yds:.0} yds");
        }
        if let Some((n, tkl)) = best_def {
            println!("    TACK  {n:<24} {tkl} tackles");
        }
    }

    println!("\nDRIVE CHART");
    for d in &game.box_score.drives {
        println!(
            "  {:<5} {:>3} plays  {:>5.0} yds  {:>4.0} yd line  {:<18} {:>2} pts  {}",
            t[d.offense].abbr,
            d.plays,
            d.yards,
            d.start_yards_to_goal,
            d.result,
            d.points,
            clock_text(d.time)
        );
    }
    println!("\n{}", game.summary().trim_end());
}

// ---------------------------------------------------------------------------
// teams
// ---------------------------------------------------------------------------

fn cmd_teams() {
    for t in league_teams() {
        println!();
        println!("{} ({})", t.display_upper(), t.abbr);
        println!("  jersey #{:02X}{:02X}{:02X}  accent #{:02X}{:02X}{:02X}", t.primary.r, t.primary.g, t.primary.b, t.accent.r, t.accent.g, t.accent.b);
        let off = t.offense_personnel();
        let def = t.defense_personnel();
        println!("  starters (offense)");
        for (i, &idx) in off.players.iter().enumerate() {
            let p = t.player(idx);
            println!(
                "    {:<3} #{:<2} {:<22} {:<3} {}",
                ["QB", "LG", "RG", "RB", "TE", "WR", "WR"][i],
                p.number,
                p.full_name(),
                p.pos.abbr(),
                p.overall()
            );
        }
        println!("  starters (defense)");
        for (i, &idx) in def.players.iter().enumerate() {
            let p = t.player(idx);
            println!(
                "    {:<3} #{:<2} {:<22} {:<3} {}",
                ["EDGE", "EDGE", "MIKE", "WILL", "CB", "CB", "SAF"][i],
                p.number,
                p.full_name(),
                p.pos.abbr(),
                p.overall()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// rules
// ---------------------------------------------------------------------------

fn cmd_rules() {
    println!("A7FL RULESET IMPLEMENTED");
    println!("  field        120 x 37.5 yards, 100 yard field of play, 10 yard end zones, no goal posts");
    println!("  players      7 on 7: QB + 2 down linemen + 4 eligibles vs 2 DL + 2 LB + 3 DB");
    println!("  clock        4 x 15 minutes, 30 second play clock, running clock");
    println!("               stops only on penalties, change of possession and conversions");
    println!("               (plus incomplete passes and out of bounds inside two minutes)");
    println!("  downs        4 downs to gain 10 yards");
    println!("  scoring      touchdown 6, safety 2, no kicking of any kind");
    println!("  conversions  1 point from the 5 or 2 points from the 10, offense's choice, locked in");
    println!("  throw-offs   thrown from the team's own 35, 3 gunners vs 1 returner,");
    println!("               minimum 40 air yards, fair catch legal, touchback to the 20,");
    println!("               returner downed in his own end zone is a safety");
    println!("  fourth down  throw-off from the line of scrimmage replaces the punt");
    println!("  overtime     possessions from the 25, two first downs allowed, one timeout;");
    println!("               two-point conversions forced from the second overtime on");
    println!("  mercy        seven touchdown lead at the start of the fourth ends the game");
    println!();
    println!("PENALTIES ENFORCED");
    for kind in [
        a7fl_core::rules::PenaltyKind::FalseStart,
        a7fl_core::rules::PenaltyKind::OffsideDefense,
        a7fl_core::rules::PenaltyKind::DelayOfGame,
        a7fl_core::rules::PenaltyKind::Holding,
        a7fl_core::rules::PenaltyKind::IllegalBlockInTheBack,
        a7fl_core::rules::PenaltyKind::Clipping,
        a7fl_core::rules::PenaltyKind::BlockingBelowWaist,
        a7fl_core::rules::PenaltyKind::ChopBlock,
        a7fl_core::rules::PenaltyKind::IntentionalGrounding,
        a7fl_core::rules::PenaltyKind::OffensivePassInterference,
        a7fl_core::rules::PenaltyKind::DefensivePassInterference,
        a7fl_core::rules::PenaltyKind::DefensiveHolding,
        a7fl_core::rules::PenaltyKind::IllegalContact,
        a7fl_core::rules::PenaltyKind::RoughingThePasser,
        a7fl_core::rules::PenaltyKind::LateHit,
        a7fl_core::rules::PenaltyKind::Unsportsmanlike,
        a7fl_core::rules::PenaltyKind::Targeting,
        a7fl_core::rules::PenaltyKind::HorseCollar,
        a7fl_core::rules::PenaltyKind::BlindsideBlock,
    ] {
        use a7fl_core::rules::FoulTeam;
        let team = match kind.against() {
            FoulTeam::Offense => "offense",
            FoulTeam::Defense => "defense",
        };
        println!(
            "  {:<28} {:>3.0} yds  {:<8} {}{}{}",
            kind.label(),
            kind.yards(),
            team,
            if kind.dead_ball() { "dead ball " } else { "live ball " },
            if kind.auto_first_down() { "auto 1st " } else { "" },
            if kind.loss_of_down() { "loss of down" } else { "" }
        );
    }
}

// ---------------------------------------------------------------------------
// play
// ---------------------------------------------------------------------------

fn cmd_play(args: &[String]) {
    let seed = num(args, "--seed", 12);
    let (home, away) = pick_teams(args);
    let mut game = GameState::new(home, away, seed);
    println!("A7FL: NO PADS FOOTBALL — {}", game.teams[0].full_name());
    println!("vs {}", game.teams[1].full_name());
    println!("Type a play number to call it, 'a' to let the coach call it, 'q' to quit.");

    let book = playbook();
    let mut guard = 0;
    while !game.is_over() && guard < 600 {
        guard += 1;
        print_situation(&game);

        match game.action() {
            a7fl_core::game::Action::GameOver => break,
            a7fl_core::game::Action::ThrowOff => {
                let rec = game.simulate_next();
                report(rec.as_ref());
                continue;
            }
            a7fl_core::game::Action::ChooseConversion => {
                println!("  Conversion: [1] from the 5  [2] from the 10");
                let two = read_choice(&["1", "2"]).map(|c| c == "2");
                let rec = match two {
                    Some(two) => game.choose_conversion(two),
                    None => game.simulate_next(),
                };
                report(rec.as_ref());
                continue;
            }
            a7fl_core::game::Action::CallPlay { fourth_down } => {
                if fourth_down {
                    println!("  Fourth down: [g] go for it  [t] throw-off");
                    if let Some(choice) = read_choice(&["g", "t"]) {
                        if choice == "t" {
                            let rec = game.fourth_down_throw_off();
                            report(rec.as_ref());
                            continue;
                        }
                    }
                }
            }
        }

        for (i, p) in book.iter().enumerate() {
            println!(
                "  {:>2}. {:<14} {:<9} {}",
                i + 1,
                p.name,
                p.family.label(),
                p.formation.label()
            );
        }
        let choice = read_choice(&[]);
        let record = match choice {
            Some(c) if c == "q" => break,
            Some(c) if c == "a" || c.is_empty() => game.simulate_next(),
            Some(c) => {
                let idx = c.parse::<usize>().unwrap_or(1).saturating_sub(1).min(book.len() - 1);
                let play: Play = book[idx].clone();
                let seed = 999;
                let def: DefCall = choose_defense_for(&game, seed);
                let d = game.defense();
                println!(
                    "  Defense shows: {} ({})",
                    def.name,
                    game.teams[d].abbr
                );
                game.call_play(play, def)
            }
            None => game.simulate_next(),
        };
        report(record.as_ref());
    }

    println!();
    print_box_score(&game);
}

fn choose_defense_for(game: &GameState, seed: u64) -> DefCall {
    let _ = seed;
    let book = defensive_playbook();
    let s = ai::Situation::from_game(game);
    let mut rng = a7fl_core::math::Rng::new(game.seed ^ 0xBEEF);
    if s.yards_to_goal <= 5.0 {
        return book.iter().find(|d| d.name == "GOAL LINE BEAR").cloned().unwrap_or_else(|| book[0].clone());
    }
    if s.down >= 3 && s.to_go <= 3.0 {
        return book.iter().find(|d| d.name == "MIKE BLITZ").cloned().unwrap_or_else(|| book[0].clone());
    }
    if s.down >= 3 && s.to_go >= 8.0 {
        return book.iter().find(|d| d.name == "COVER 3 ZONE").cloned().unwrap_or_else(|| book[0].clone());
    }
    let idx = rng.index(book.len());
    book[idx].clone()
}

fn print_situation(game: &GameState) {
    println!();
    println!(
        "  Q{} {}  {} {} — {} {}  |  {}  BALL ON {}",
        game.quarter,
        game.clock_text(),
        game.teams[0].abbr,
        game.score[0],
        game.teams[1].abbr,
        game.score[1],
        game.down_and_distance(),
        a7fl_core::field::ball_on_label(game.yards_to_goal())
    );
    if matches!(game.phase, GamePhase::Conversion { .. }) {
        println!("  (conversion attempt)");
    }
}

fn report(record: Option<&PlayRecord>) {
    if let Some(r) = record {
        println!("  >> {}", r.description());
    }
    io::stdout().flush().ok();
}

fn read_choice(valid: &[&str]) -> Option<String> {
    print!("  > ");
    io::stdout().flush().ok();
    let stdin = io::stdin();
    let mut line = String::new();
    match stdin.lock().read_line(&mut line) {
        Ok(0) => Some("a".to_string()),
        Ok(_) => {
            let t = line.trim().to_lowercase();
            if valid.is_empty() || valid.contains(&t.as_str()) || t == "a" || t == "q" {
                Some(t)
            } else {
                Some(t)
            }
        }
        Err(_) => Some("a".to_string()),
    }
}

// ---------------------------------------------------------------------------
// film
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// arcade
// ---------------------------------------------------------------------------

fn cmd_arcade(args: &[String]) {
    let seed = num(args, "--seed", 12);
    let (home, away) = pick_teams(args);
    let game = GameState::new(home, away, seed);

    // Frame-time check for the canvas the arcade client draws on.
    if has(args, "--bench") {
        let mut probe = GameState::new(game.teams[0].clone(), game.teams[1].clone(), seed);
        let mut record = None;
        while let Some(r) = probe.simulate_next() {
            if matches!(r, PlayRecord::Scrimmage(_)) {
                record = Some(r);
                break;
            }
        }
        let Some(record) = record else {
            println!("no scrimmage snap to benchmark");
            return;
        };
        let teams = [probe.teams[0].clone(), probe.teams[1].clone()];
        let frames = 20;

        // Stills path: re-simulate the play up to each moment, which is what
        // `film` does.
        let mut broadcast = Broadcast::new(view::CANVAS_W, view::CANVAS_H, teams.clone());
        let start = std::time::Instant::now();
        for i in 0..frames {
            let t = 0.2 + i as f32 * 0.12;
            let _ = broadcast.frame_at_clean(&probe, &record, t, CameraMode::Broadcast);
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / frames as f64;
        println!(
            "replay still {}x{}: {ms:.1} ms ({:.1} fps ceiling)",
            view::CANVAS_W,
            view::CANVAS_H,
            1000.0 / ms
        );

        // Live path: drawn from the sim the client is already stepping, mid-play,
        // so this is the honest per-frame cost of the interactive picture.
        let mut sim = match &record {
            PlayRecord::Scrimmage(r) => PlaySim::new(
                &teams[r.setup.offense_team],
                &teams[r.setup.defense_team],
                &r.play,
                &r.defense,
                &r.setup,
            ),
            _ => {
                println!("no scrimmage snap to benchmark");
                return;
            }
        };
        let dt = 1.0 / 60.0;
        let mut elapsed = 0.0;
        while elapsed < 1.5 && sim.result.is_none() {
            sim.step(dt);
            elapsed += dt;
        }
        let mut live = Broadcast::new(view::CANVAS_W, view::CANVAS_H, teams.clone());
        let start = std::time::Instant::now();
        for _ in 0..frames {
            let _ = live.live_frame(&sim, CameraMode::Broadcast);
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / frames as f64;
        println!(
            "live frame {}x{}: {ms:.1} ms ({:.1} fps ceiling)",
            view::CANVAS_W,
            view::CANVAS_H,
            1000.0 / ms
        );

        // A pass in the air, four stills a fraction of a second apart: the camera
        // has to go where the ball goes, and this is how that gets looked at rather
        // than argued about.
        if let PlayRecord::Scrimmage(r) = &record {
            let mut air = PlaySim::new(
                &teams[r.setup.offense_team],
                &teams[r.setup.defense_team],
                &r.play,
                &r.defense,
                &r.setup,
            );
            while air.t < 1.2 && air.result.is_none() {
                air.step(dt);
            }
            let deep = air
                .eligible_targets()
                .into_iter()
                .map(|(_, i)| i)
                .max_by(|a, b| {
                    let qb = air.qb_pos();
                    air.players[*a].pos.dist(qb).total_cmp(&air.players[*b].pos.dist(qb))
                });
            if let Some(index) = deep {
                air.set_control(Some(a7fl_core::sim::QbControl {
                    move_dir: Vec2::ZERO,
                    target: Some(index),
                    throw: true,
                    power: 1.0,
                    aim: Vec2::ZERO,
                }));
                air.step(dt);
                for (n, gap) in [(0usize, 0.18f32), (1, 0.30), (2, 0.34), (3, 0.36)] {
                    let mut left = gap;
                    while left > 0.0 && air.result.is_none() {
                        air.step(dt);
                        left -= dt;
                    }
                    let (ball, _) = air.ball_position();
                    println!("pass still {n}: ball at {:.1}, {:.1}", ball.x, ball.y);
                    let framed = view::with_hud(
                        live.live_frame(&air, CameraMode::Classic),
                        &["PASS IN THE AIR".to_string()],
                    );
                    let path = format!("out/15_pass_{n}.png");
                    let _ = write_png(&path, &framed);
                }
            }
        }

        let last = broadcast.frame_at_clean(&probe, &record, 1.4, CameraMode::Broadcast);
        let path = "out/08_arcade_view.png";
        if write_png(path, &last).is_ok() {
            println!("wrote {path}");
        }
        // What the interactive client actually puts on screen, HUD and all.
        let lines = vec![
            "NEW 14 - 7  PAT   2ND 6:41   1ST & 10   BALL ON OWN 34".to_string(),
            "TARGET WR  Frankie Kelsey     4.2 yds  OPEN".to_string(),
            "NEW ball   MOVE  TARGET  THROW  [START] PAUSE".to_string(),
        ];
        let framed = view::with_hud(live.live_frame(&sim, CameraMode::Broadcast), &lines);
        let path = "out/09_live_frame.png";
        if write_png(path, &framed).is_ok() {
            println!("wrote {path}");
        }

        // What live play puts on screen with a pass being wound up: a button over
        // every eligible man and the aim guide out on the grass.
        let targets = sim.eligible_targets();
        let mut badges = Vec::new();
        for (slot, (_, index)) in targets.iter().enumerate().take(4) {
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
                label: ["A", "B", "X", "Y"][slot],
                tone,
                aimed: slot == 0,
            });
        }
        let mut guide = None;
        let mut short = false;
        let mut needed = None;
        if let Some((_, index)) = targets.first() {
            let plan = sim.throw_plan(sim.qb, *index, 0.62, Vec2::new(0.0, 3.0));
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
        let ui = PassUi {
            focus: None,
            badges,
            guide,
            power: Some(0.62),
            short,
            needed,
        };
        let lines = vec![
            "NEW 14 - 7  PAT   2ND 6:41   1ST & 10   BALL ON OWN 34".to_string(),
            "SLANT RIGHT - DROPBACK PASS - DAL shows COVER 2".to_string(),
            "A KELSEY 4.2   B ROSS 1.4   X HALE 3.1   Y DIAZ 0.9".to_string(),
            "NEW ball   MOVE   HOLD A/B/X/Y TO PASS   AIM WITH STICK   [START] PAUSE".to_string(),
        ];
        let framed = view::with_hud(
            live.live_frame_ui(&sim, CameraMode::Broadcast, &ui),
            &lines,
        );
        let path = "out/10_passing.png";
        if write_png(path, &framed).is_ok() {
            println!("wrote {path}");
        }

        // A throw-off being thrown off: meter, and the marker that says where it
        // comes down.
        let throw_home = live.teams[0].clone();
        let throw_away = live.teams[1].clone();
        let setup = a7fl_core::throwoff::ThrowOffSetup {
            seed: 7,
            ..a7fl_core::throwoff::ThrowOffSetup::normal()
        };
        let off = a7fl_core::throwoff::ThrowOffSim::new(&setup, &throw_home, &throw_away);
        let plan = off.throw_off_plan(0.62, 5.0);
        let hand = off.players[0].pos;
        let ui = PassUi {
            focus: None,
            badges: Vec::new(),
            guide: Some(Guide {
                from: Vec3::new(hand.x, hand.y, 1.8),
                receiver: Vec3::new(plan.landing.x, plan.landing.y, 0.12),
                landing: Vec3::new(plan.landing.x, plan.landing.y, 0.12),
            }),
            power: Some(0.62),
            short: plan.too_short,
            needed: Some(a7fl_core::throwoff::power_for_air(
                a7fl_core::rules::THROW_OFF_MIN_AIR_YARDS,
            )),
        };
        let lines = vec![
            "NEW 14 - 7  PAT   2ND 6:41   1ST & 10   BALL ON OWN 34".to_string(),
            "NEW THROW-OFF FROM THE 35 - HOLD A TO WIND UP, LET GO TO THROW".to_string(),
            "COVER IT   NEW BALL   MOVE WITH STICK   [START] PAUSE".to_string(),
        ];
        let framed = view::with_hud(live.throw_off_frame_live(&off, CameraMode::Broadcast, &ui), &lines);
        let path = "out/11_throw_off.png";
        if write_png(path, &framed).is_ok() {
            println!("wrote {path}");
        }

        // The classic game view: behind the offense, facing the defense.
        let lines = vec![
            "NEW 14 - 7  PAT   2ND 6:41   1ST & 10   BALL ON OWN 34".to_string(),
            "SLANT RIGHT - DROPBACK PASS - DAL shows COVER 2".to_string(),
            "A KELSEY 4.2   B ROSS 1.4   X HALE 3.1   Y DIAZ 0.9".to_string(),
            "NEW ball   MOVE   HOLD A/B/X/Y TO PASS   AIM WITH STICK   [RT] CLASSIC   [START] PAUSE"
                .to_string(),
        ];
        let framed = view::with_hud(live.live_frame(&sim, CameraMode::Classic), &lines);
        let path = "out/12_classic.png";
        if write_png(path, &framed).is_ok() {
            println!("wrote {path}");
        }

        // The call sheet, the way it is drawn when the player is picking a play.
        let mut sheet = a7fl_render::Frame::new(640, 360);
        a7fl_render::playsheet::draw(&mut sheet, &playbook(), 1);
        let lines = vec![
            "NEW 14 - 7  PAT   2ND 6:41   1ST & 10   BALL ON OWN 34".to_string(),
            "  POWER O          RUN / BONE          PLAY 2 OF 22   SET 1 OF 3".to_string(),
            "  DPAD / STICK: HIGHLIGHT   [A] SNAP   [Y] ASK COACH   [START] PAUSE".to_string(),
        ];
        let framed = view::with_hud(sheet, &lines);
        let path = "out/13_call_sheet.png";
        if write_png(path, &framed).is_ok() {
            println!("wrote {path}");
        }

        // Running with it: the face buttons are the carrier's moves.
        let lines = vec![
            "NEW 14 - 7  PAT   2ND 6:41   1ST & 10   BALL ON OWN 34".to_string(),
            "ZONE READ - RUN - PISTOL   KAISER 6 for 9".to_string(),
            "  [A] SPIN   [B] HURDLE   [X] DIVE   [Y] SLIDE      SPIN".to_string(),
            "NEW ball   MOVE   [A/B/X/Y] MOVES   [RT] CLASSIC   [START] PAUSE".to_string(),
        ];
        let framed = view::with_hud(live.live_frame(&sim, CameraMode::Classic), &lines);
        let path = "out/14_running.png";
        if write_png(path, &framed).is_ok() {
            println!("wrote {path}");
        }
        return;
    }

    let view = view::View::open(has(args, "--terminal"));
    let pad = pad::open();
    if pad.is_none() && !view.is_window() {
        eprintln!("No controller and no display.");
        #[cfg(feature = "gamepad")]
        eprintln!("Connect your controller (an 8BitDo: turn it on in XInput mode), then run this again.");
        #[cfg(not(feature = "gamepad"))]
        eprintln!("This build has no controller support: rebuild with the `gamepad` feature.");
        return;
    }
    match &pad {
        Some(pad) => println!("Controller: {}", pad.name()),
        None => println!("No controller detected - playing on the keyboard."),
    }
    println!("Display: {}", view.describe());
    println!("  move: stick / arrows+WASD");
    println!("  snap, call, confirm: A / Space");
    println!("  pass: hold A, B, X or Y (Space / X / Z / C) and release; the stick aims it");
    println!("  ask the coach: Y / C      quit: START / Esc");
    println!("Starting in a moment.");
    std::thread::sleep(std::time::Duration::from_millis(900));

    let flip_y = has(args, "--flip-y");
    arcade::Arcade::new(game, pad, view).with_flip_y(flip_y).run();
}

fn cmd_film(args: &[String]) {
    let seed = num(args, "--seed", 4);
    let out_dir = flag(args, "--out").unwrap_or_else(|| "out".to_string());
    std::fs::create_dir_all(&out_dir).ok();

    let (home, away) = pick_teams(args);
    let mut game = GameState::new(home, away, seed);
    let mut pass_play: Option<PlayRecord> = None;
    let mut score_play: Option<PlayRecord> = None;
    let mut throw_off: Option<PlayRecord> = None;
    let mut interception: Option<PlayRecord> = None;

    let mut guard = 0;
    while !game.is_over() && guard < 600 {
        let Some(rec) = game.simulate_next() else { break };
        guard += 1;
        match &rec {
            PlayRecord::ThrowOff(_) => {
                if throw_off.is_none() {
                    throw_off = Some(rec.clone());
                }
            }
            PlayRecord::Scrimmage(r) => {
                if pass_play.is_none()
                    && r.result.outcome.is_pass_attempt()
                    && r.result.elapsed > 2.0
                {
                    pass_play = Some(rec.clone());
                }
                if score_play.is_none() && r.result.touchdown {
                    score_play = Some(rec.clone());
                }
                if interception.is_none() && r.result.outcome == a7fl_core::sim::PlayOutcome::Interception {
                    interception = Some(rec.clone());
                }
            }
        }
    }

    let teams = [game.teams[0].clone(), game.teams[1].clone()];
    let mut broadcast = Broadcast::new(1280, 720, teams);

    println!("Rendering broadcast package from seed {seed} ...");
    let mut written: Vec<String> = Vec::new();

    if let Some(rec) = &throw_off {
        let f = broadcast.frame_at(&game, rec, 2.2, CameraMode::Broadcast);
        let p = format!("{out_dir}/01_throw_off.png");
        if write_png(&p, &f).is_ok() {
            written.push(p);
        }
    }
    if let Some(rec) = &pass_play {
        let f = broadcast.frame_at(&game, rec, 0.35, CameraMode::BehindQb);
        let p = format!("{out_dir}/02_presnap.png");
        if write_png(&p, &f).is_ok() {
            written.push(p);
        }
        let mid = duration_of(rec) * 0.55;
        let f = broadcast.frame_at(&game, rec, mid, CameraMode::Broadcast);
        let p = format!("{out_dir}/03_pass_broadcast.png");
        if write_png(&p, &f).is_ok() {
            written.push(p);
        }
    }
    if let Some(rec) = &score_play {
        let f = broadcast.frame_at(&game, rec, duration_of(rec) * 0.95, CameraMode::Replay);
        let p = format!("{out_dir}/04_score.png");
        if write_png(&p, &f).is_ok() {
            written.push(p);
        }
    }
    if let Some(rec) = &interception {
        let f = broadcast.frame_at(&game, rec, duration_of(rec) * 0.9, CameraMode::SkyCam);
        let p = format!("{out_dir}/05_turnover.png");
        if write_png(&p, &f).is_ok() {
            written.push(p);
        }
    }

    // Highlight GIF from the best play we found.
    let highlight = score_play.clone().or_else(|| pass_play.clone());
    if let Some(rec) = highlight {
        let frames = 44;
        let duration = duration_of(&rec).min(7.0);
        let mut gif_frames = Vec::new();
        for i in 0..frames {
            let t = duration * (i as f32 / (frames - 1) as f32);
            let mode = if i < frames / 3 {
                CameraMode::BehindQb
            } else if i < frames * 2 / 3 {
                CameraMode::Broadcast
            } else {
                CameraMode::Broadcast
            };
            let f = broadcast.frame_at(&game, &rec, t, mode);
            gif_frames.push(f.downsample(2));
        }
        let p = format!("{out_dir}/06_highlight.gif");
        match write_gif(&p, &gif_frames, 90) {
            Ok(()) => written.push(p),
            Err(e) => eprintln!("gif failed: {e}"),
        }
    }

    // Post-game stats panel.
    let mut panel = Frame::new(1280, 720);
    hud::draw_stats_panel(&mut panel, &game, "FINAL");
    let p = format!("{out_dir}/07_final_stats.png");
    if write_png(&p, &panel).is_ok() {
        written.push(p);
    }

    println!();
    println!("{} {} — {} {}", game.teams[0].abbr, game.score[0], game.teams[1].abbr, game.score[1]);
    for w in &written {
        println!("  wrote {w}");
    }
}

fn duration_of(rec: &PlayRecord) -> f32 {
    match rec {
        PlayRecord::Scrimmage(r) => (r.result.elapsed + 1.6).clamp(2.0, 11.0),
        PlayRecord::ThrowOff(r) => (r.result.elapsed + 1.6).clamp(2.0, 12.0),
    }
}

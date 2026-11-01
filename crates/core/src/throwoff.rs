//! The A7FL throw-off: the league's replacement for the kickoff.
//!
//! Rules implemented here, straight from the rulebook:
//!
//! * thrown from the throwing team's own 35 (own 25 after a safety) by a thrower
//!   who may take a running start but must release before the 35,
//! * three defenders (gunners) against one returner,
//! * the throw must travel at least 40 yards in the air and land in bounds,
//! * a throw that sails through the back of the end zone is a touchback at the 20,
//! * fair catch is legal; a muffed ball is live and can be recovered by the
//!   throwing team but never advanced,
//! * a returner downed in his own end zone is a safety,
//! * on fourth down the throw-off replaces the punt: no minimum distance, legal to
//!   throw out of bounds.
//!
//! Rule violations (out of bounds, short throw, offside) put the returner on his own
//! 25 with the ball in his hand for a "Run Back" — the 3-on-1 live play.

use crate::field::{self, FIELD_HALF_WIDTH, FIELD_LENGTH};
use crate::math::{approach_angle, clamp, lerp, Rng, Vec2};
use crate::rules::{
    SAFETY_THROW_OFF_YARD_LINE, THROW_OFF_MAX_AIR_YARDS, THROW_OFF_MIN_AIR_YARDS,
    THROW_OFF_SPOT_YARD_LINE, THROW_OFF_TOUCHBACK_TO,
};
use crate::team::Team;

#[derive(Clone, Debug)]
pub struct ThrowOffSetup {
    pub throwing_team: usize,
    pub receiving_team: usize,
    /// Yards from the throwing team's own goal line (35 normally, 25 after a safety).
    pub from_own_yards: f32,
    /// Direction the receiving team attacks.
    pub receive_direction: i8,
    pub seed: u64,
    /// Fourth-down throw-off in place of a punt.
    pub free_throw: bool,
}

impl ThrowOffSetup {
    pub fn spot(&self) -> f32 {
        let throw_dir = -self.receive_direction;
        field::spot_from_yards_to_goal(FIELD_LENGTH - self.from_own_yards, throw_dir)
    }

    pub fn throwing_direction(&self) -> i8 {
        -self.receive_direction
    }

    pub fn normal() -> Self {
        ThrowOffSetup {
            throwing_team: 0,
            receiving_team: 1,
            from_own_yards: THROW_OFF_SPOT_YARD_LINE,
            receive_direction: 1,
            seed: 0,
            free_throw: false,
        }
    }

    pub fn after_safety() -> Self {
        ThrowOffSetup { from_own_yards: SAFETY_THROW_OFF_YARD_LINE, ..ThrowOffSetup::normal() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThrowOffOutcome {
    Touchback,
    FairCatch,
    Returned,
    Downed,
    RunBack,
    Safety,
    Touchdown,
    RecoveredByK,
    OutOfBounds,
}

#[derive(Clone, Debug)]
pub struct ThrowOffResult {
    pub outcome: ThrowOffOutcome,
    /// Team that has the ball next.
    pub next_offense: usize,
    /// Canonical spot for the next snap.
    pub start_spot: f32,
    pub points: u32,
    pub scoring_team: Option<usize>,
    pub return_yards: f32,
    pub fair_catch: bool,
    pub touchback: bool,
    pub safety: bool,
    pub touchdown: bool,
    /// Seconds of game clock the throw-off itself eats.
    pub elapsed: f32,
    pub description: String,
    pub highlight: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThrowPhase {
    Presnap,
    Flight,
    /// The ball is on the ground with nobody on it: a live ball, played out until
    /// somebody falls on it.
    Loose,
    Return,
    Dead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThrowRole {
    Thrower,
    Gunner,
    Returner,
}

#[derive(Clone, Debug)]
pub struct ThrowPlayer {
    pub team: usize,
    pub roster: usize,
    pub role: ThrowRole,
    pub pos: Vec2,
    pub vel: Vec2,
    pub down: bool,
    pub run_phase: f32,
    pub heading: f32,
    /// 0..1 through the hit: the gunner driving it home, the returner going down.
    pub wrap: f32,
    pub tackled: f32,
}

/// Where a throw-off at a given weight and aim comes down, worked out before it
/// is thrown so that the marker on the field is drawn from the same arithmetic
/// that flies the ball.
#[derive(Clone, Copy, Debug)]
pub struct ThrowOffPlan {
    /// Yards travelled in the air.
    pub air: f32,
    pub landing: Vec2,
    /// Clears the back of the receiving team's end zone: touchback.
    pub through_endzone: bool,
    /// Short of the 40 yards the rulebook asks for.
    pub too_short: bool,
    /// Lands off the sideline (only legal on a fourth-down throw).
    pub out_of_bounds: bool,
}

/// The lightest throw the meter can put up: six yards short of the 40 the rule
/// asks for, so a lazy throw is a Run Back rather than a free 30-yarder.
const THROW_OFF_LIGHT_AIR: f32 = THROW_OFF_MIN_AIR_YARDS - 6.0;

/// How long a loose ball lies there while everybody converges on it, before the
/// nearest man is on it and the play is dead.
const LOOSE_SECONDS: f32 = 1.35;

/// How long the tackle that ends a return takes to put him on the turf. The ball
/// is dead at the moment of contact; this is the picture of it, so the return does
/// not vanish into a card while both men are still standing.
const TACKLE_SECONDS: f32 = 0.40;

/// A tackle in progress: who has him, who is hitting him, and how long ago the
/// arms went on.
#[derive(Clone, Copy, Debug)]
struct TackleBeat {
    returner: usize,
    gunner: usize,
    t: f32,
    spot: f32,
    safety: bool,
    yards: f32,
}

/// Air yards a throw-off carries at a given weight on the meter. It tops out on
/// the receiving team's 20 — 45 yards from the 35 — instead of sailing into the
/// end zone, which used to make full power a guaranteed touchback and a picture
/// of nothing but a ball in the air.
pub fn throw_off_air(power: f32) -> f32 {
    THROW_OFF_LIGHT_AIR
        + power.clamp(0.0, 1.0) * (THROW_OFF_MAX_AIR_YARDS - THROW_OFF_LIGHT_AIR)
}

/// The meter reading that would just carry a throw `air` yards.
pub fn power_for_air(air: f32) -> f32 {
    ((air - THROW_OFF_LIGHT_AIR) / (THROW_OFF_MAX_AIR_YARDS - THROW_OFF_LIGHT_AIR)).clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug)]
struct Flight {
    from: Vec2,
    to: Vec2,
    t: f32,
    duration: f32,
    peak: f32,
}

pub struct ThrowOffSim<'a> {
    pub setup: ThrowOffSetup,
    pub throwing: &'a Team,
    pub receiving: &'a Team,
    pub players: Vec<ThrowPlayer>,
    pub ball: Vec2,
    pub ball_z: f32,
    pub phase: ThrowPhase,
    pub t: f32,
    pub rng: Rng,
    pub result: Option<ThrowOffResult>,
    pub events: Vec<String>,
    flight: Option<Flight>,
    /// How long the thrower waits before he lets it go. A client with a human
    /// returner on the other end raises it so the ball is not in the air before
    /// he has looked up.
    pub snap_delay: f32,
    returner_has_ball: bool,
    safety_risk: bool,
    /// The man the human is steering, and which way he is pushing him.
    pub human: Option<usize>,
    pub human_dir: Vec2,
    /// A human is throwing this one, so the AI must not launch it for him.
    pub human_thrower: bool,
    /// Sim time the returner took the ball in, so a client can turn its camera
    /// upfield the way a broadcast does.
    catch_t: Option<f32>,
    /// A rule the throw has already broken, kept back until the ball is dead: the
    /// throw still flies, so the player sees it leave the hand.
    violation: Option<(ThrowOffOutcome, String)>,
    /// Seconds the loose ball has been on the ground.
    loose_t: f32,
    /// A tackle that is still ending: the returner is going down and the card has
    /// not been called for yet.
    tackle: Option<TackleBeat>,
}

impl<'a> ThrowOffSim<'a> {
    pub fn new(setup: &ThrowOffSetup, throwing: &'a Team, receiving: &'a Team) -> Self {
        let spot = setup.spot();
        let mut rng = Rng::new(setup.seed ^ 0x7A_11_0F);
        let thrower = throwing.qb();
        let returner = receiving
            .roster
            .iter()
            .position(|p| p.pos == crate::team::Position::Wr)
            .unwrap_or(0);
        let gunners: Vec<usize> = throwing
            .roster
            .iter()
            .enumerate()
            .filter(|(_, p)| p.pos == crate::team::Position::Db || p.pos == crate::team::Position::Lb)
            .take(3)
            .map(|(i, _)| i)
            .collect();
        let mut players = vec![ThrowPlayer {
            team: setup.throwing_team,
            roster: thrower,
            role: ThrowRole::Thrower,
            pos: Vec2::new(spot, 0.0),
            vel: Vec2::ZERO,
            down: false,
            run_phase: 0.0,
            heading: 0.0,
            wrap: 0.0,
            tackled: 0.0,
        }];
        for (n, &g) in gunners.iter().enumerate() {
            let y = (n as f32 - 1.0) * 3.2;
            players.push(ThrowPlayer {
                team: setup.throwing_team,
                roster: g,
                role: ThrowRole::Gunner,
                pos: Vec2::new(spot, y),
                vel: Vec2::ZERO,
                down: false,
                run_phase: 0.0,
                heading: 0.0,
                wrap: 0.0,
                tackled: 0.0,
            });
        }
        // Returner sits where a 45-yard throw would land.
        let catch_x = spot - setup.receive_direction as f32 * 45.0;
        players.push(ThrowPlayer {
            team: setup.receiving_team,
            roster: returner,
            role: ThrowRole::Returner,
            pos: Vec2::new(clamp(catch_x, 0.0, FIELD_LENGTH), 0.0),
            vel: Vec2::ZERO,
            down: false,
            run_phase: 0.0,
            heading: 0.0,
            wrap: 0.0,
            tackled: 0.0,
        });
        let _ = &mut rng;
        ThrowOffSim {
            setup: setup.clone(),
            throwing,
            receiving,
            players,
            ball: Vec2::new(spot, 0.0),
            ball_z: 1.2,
            phase: ThrowPhase::Presnap,
            t: 0.0,
            rng,
            result: None,
            events: Vec::new(),
            flight: None,
            snap_delay: 0.9,
            returner_has_ball: false,
            safety_risk: false,
            human: None,
            human_dir: Vec2::ZERO,
            human_thrower: false,
            catch_t: None,
            violation: None,
            loose_t: 0.0,
            tackle: None,
        }
    }

    /// Hand a man to the human to steer, or take him back.
    pub fn set_human(&mut self, index: Option<usize>, dir: Vec2) {
        self.human = index;
        self.human_dir = dir.clamp_len(1.0);
    }

    fn returner(&self) -> usize {
        4
    }

    fn attrs(&self, i: usize) -> crate::team::Attributes {
        let p = &self.players[i];
        if p.team == self.setup.throwing_team {
            self.throwing.roster[p.roster].attrs
        } else {
            self.receiving.roster[p.roster].attrs
        }
    }

    fn speed(&self, i: usize) -> f32 {
        self.attrs(i).top_speed()
    }

    fn name(&self, i: usize) -> String {
        let p = &self.players[i];
        if p.team == self.setup.throwing_team {
            self.throwing.roster[p.roster].short_name()
        } else {
            self.receiving.roster[p.roster].short_name()
        }
    }

    fn steer(&mut self, i: usize, target: Vec2, speed: f32, dt: f32) {
        let accel = self.attrs(i).acceleration();
        let p = &mut self.players[i];
        let to = target - p.pos;
        let dist = to.length();
        let cap = if dist < 0.4 { speed * (dist / 0.4) } else { speed };
        let desired = to.normalize_or_zero() * cap;
        let dv = desired - p.vel;
        p.vel += dv.clamp_len(accel * dt);
        p.pos += p.vel * dt;
        if p.vel.length() > 0.3 {
            p.heading = approach_angle(p.heading, p.vel.angle(), 9.0 * dt);
        }
        p.run_phase =
            (p.run_phase + p.vel.length() * dt / crate::sim::STRIDE_YARDS).rem_euclid(1.0);
    }

    pub fn run_to_completion(&mut self) -> ThrowOffResult {
        let mut guard = 0;
        while self.result.is_none() && guard < 3000 {
            self.step(1.0 / 60.0);
            guard += 1;
        }
        self.result.clone().unwrap_or_else(|| ThrowOffResult {
            outcome: ThrowOffOutcome::Touchback,
            next_offense: self.setup.receiving_team,
            start_spot: self.own_20(),
            points: 0,
            scoring_team: None,
            return_yards: 0.0,
            fair_catch: false,
            touchback: true,
            safety: false,
            touchdown: false,
            elapsed: 5.0,
            description: "Throw-off comes up short of everything.".to_string(),
            highlight: false,
        })
    }

    fn own_20(&self) -> f32 {
        let dir = self.setup.receive_direction;
        field::spot_from_yards_to_goal(FIELD_LENGTH - THROW_OFF_TOUCHBACK_TO, dir)
    }

    pub fn step(&mut self, dt: f32) -> Option<&ThrowOffResult> {
        if self.result.is_some() {
            return self.result.as_ref();
        }
        self.t += dt;
        match self.phase {
            ThrowPhase::Presnap => {
                if !self.human_thrower && self.t >= self.snap_delay {
                    self.launch();
                }
            }
            ThrowPhase::Flight => self.update_flight(dt),
            ThrowPhase::Loose => self.update_loose(dt),
            ThrowPhase::Return => self.update_return(dt),
            ThrowPhase::Dead => {}
        }
        self.result.as_ref()
    }

    /// Where a throw-off at this weight and lateral aim would come down.
    pub fn throw_off_plan(&self, power: f32, aim_y: f32) -> ThrowOffPlan {
        let dir = self.setup.receive_direction;
        let throw_dir = -dir;
        let spot = self.setup.spot();
        let air = throw_off_air(power);
        // A throw-off that is not a fourth-down throw has to stay in bounds, so
        // it gets pulled back inside the sideline rather than sailing out.
        let out_of_bounds = self.setup.free_throw && aim_y.abs() > FIELD_HALF_WIDTH - 1.2;
        let landing_y = if out_of_bounds { aim_y } else { clamp(aim_y, -13.0, 13.0) };
        let landing_x = spot + throw_dir as f32 * air;
        // The receiving team's own end zone is the one behind it, opposite to its
        // return direction. A throw that clears the back of it is a touchback.
        let through_endzone = if dir > 0 {
            landing_x < -field::ENDZONE_DEPTH
        } else {
            landing_x > FIELD_LENGTH + field::ENDZONE_DEPTH
        };
        ThrowOffPlan {
            air,
            landing: Vec2::new(landing_x, landing_y),
            through_endzone,
            too_short: !self.setup.free_throw && air < THROW_OFF_MIN_AIR_YARDS,
            out_of_bounds,
        }
    }

    /// Throw it off at a weight and aim picked by a human.
    pub fn launch_with(&mut self, power: f32, aim_y: f32) {
        if self.phase != ThrowPhase::Presnap {
            return;
        }
        let plan = self.throw_off_plan(power, aim_y);
        self.release(plan);
    }

    fn launch(&mut self) {
        // No throw can reach the end zone any more, so the AI's "bomb" is the top
        // of the meter — fielded on the 20 — rather than a ball into the seats.
        let air = if self.setup.free_throw {
            self.rng.range(30.0, THROW_OFF_MAX_AIR_YARDS)
        } else if self.rng.chance(0.25) {
            self.rng.range(43.5, THROW_OFF_MAX_AIR_YARDS)
        } else {
            self.rng.range(THROW_OFF_MIN_AIR_YARDS + 0.5, 44.0)
        };
        let aim = self.rng.range(-13.0, 13.0);
        let plan = self.throw_off_plan(power_for_air(air), aim);
        self.release(plan);
    }

    /// Send the ball on its way. Everything the throw can do to the game — a
    /// touchback, a short throw, a ball off the sideline — is settled here from
    /// the plan, so the marker the client draws and the flight cannot disagree.
    fn release(&mut self, plan: ThrowOffPlan) {
        self.phase = ThrowPhase::Flight;
        // A throw that breaks a rule is still thrown. Calling it dead on the release
        // cut the picture away before the ball had left the hand, so the player never
        // saw the throw he had just made: the walk-off is kept back and read once the
        // ball is dead.
        if plan.through_endzone {
            self.violation = Some((
                ThrowOffOutcome::Touchback,
                "The throw-off sails through the back of the end zone. Touchback.".to_string(),
            ));
        } else if plan.out_of_bounds {
            self.violation = Some((
                ThrowOffOutcome::OutOfBounds,
                "The throw-off lands out of bounds. Run Back.".to_string(),
            ));
        } else if plan.too_short {
            self.violation = Some((
                ThrowOffOutcome::RunBack,
                format!(
                    "The throw-off travels {:.0} yards in the air, short of the forty the rule asks for. Run Back.",
                    plan.air
                ),
            ));
        }

        let landing = plan.landing;
        // A 45-yard throw is a high lob: roughly three and a half seconds of hang
        // time, which is what lets the three gunners get downfield. The returner
        // is not teleported under it any more; he has to get there.
        let duration = (plan.air / 13.0).clamp(1.8, 4.4);
        self.flight = Some(Flight {
            from: Vec2::new(self.players[0].pos.x, 1.2),
            to: landing,
            t: 0.0,
            duration,
            peak: 3.0 + plan.air * 0.22,
        });
        self.events.push(format!("Throw-off from the {}", self.setup.from_own_yards as i32));
        self.ball = self.players[0].pos;
        self.ball_z = 1.2;
    }

    fn run_back_spot(&self) -> f32 {
        field::spot_from_yards_to_goal(FIELD_LENGTH - 25.0, self.setup.receive_direction)
    }

    fn update_flight(&mut self, dt: f32) {
        // Gunners can only start running once the ball is thrown.
        let landing = self.flight.map(|f| f.to).unwrap_or(self.ball);
        let human = self.human;
        let push = self.human_dir;
        for i in 1..4 {
            let speed = self.speed(i);
            let target = if human == Some(i) {
                self.players[i].pos + push * 4.0
            } else {
                landing
            };
            self.steer(i, target, speed, dt);
        }
        let returner = self.returner();
        let speed = self.speed(returner);
        let target = if human == Some(returner) {
            self.players[returner].pos + push * 4.0
        } else {
            landing
        };
        self.steer(returner, target, speed * if human == Some(returner) { 1.0 } else { 0.9 }, dt);

        let Some(mut flight) = self.flight else { return };
        flight.t += dt;
        let progress = (flight.t / flight.duration).clamp(0.0, 1.0);
        self.ball = flight.from.lerp(flight.to, progress);
        self.ball_z = lerp(1.2, 1.0, progress)
            + flight.peak * (std::f32::consts::PI * progress).sin();
        if progress >= 1.0 {
            self.flight = None;
            self.resolve_catch(flight.to);
        } else {
            self.flight = Some(flight);
        }
    }

    fn resolve_catch(&mut self, landing: Vec2) {
        // A throw that broke a rule is dead where it comes down, which is the first
        // moment the play is over.
        if let Some((outcome, desc)) = self.violation.take() {
            let spot = match outcome {
                ThrowOffOutcome::Touchback => self.own_20(),
                ThrowOffOutcome::RunBack | ThrowOffOutcome::OutOfBounds => self.run_back_spot(),
                _ => landing.x,
            };
            self.ball = landing;
            self.ball_z = 0.3;
            self.finish(outcome, self.setup.receiving_team, spot, 0, None, desc);
            return;
        }

        let dir = self.setup.receive_direction;
        let returner = self.returner();
        let nearest_gunner = (1..4)
            .map(|g| self.players[g].pos.dist(self.players[returner].pos))
            .fold(99.0f32, f32::min);

        // Caught in its own end zone and downed: touchback.
        let in_endzone = if dir > 0 {
            landing.x <= 0.0
        } else {
            landing.x >= FIELD_LENGTH
        };
        if in_endzone {
            self.finish(
                ThrowOffOutcome::Touchback,
                self.setup.receiving_team,
                self.own_20(),
                0,
                None,
                "Caught in the end zone, downed for a touchback.".to_string(),
            );
            return;
        }

        // Nobody got under it: it is a live ball on the ground, and it stays live
        // until somebody falls on it.
        let gap = self.players[returner].pos.dist(landing);
        if gap > 2.4 {
            self.loose(landing);
            return;
        }

        // Fair catch when the gunners are closing. A human returner calls his own
        // by simply not being where the ball comes down.
        let awareness = self.attrs(returner).awareness_factor();
        if self.human != Some(returner)
            && nearest_gunner < 5.0
            && self.rng.chance(0.35 + awareness * 0.4)
        {
            let spot = landing.x;
            self.finish(
                ThrowOffOutcome::FairCatch,
                self.setup.receiving_team,
                spot,
                0,
                None,
                format!("{} signals for a fair catch.", self.name(returner)),
            );
            return;
        }

        // Catch, muff or strip.
        let catch_p = 0.80 + self.attrs(returner).catch_factor() * 0.18;
        if !self.rng.chance(catch_p) {
            // A muff is a live ball: the throwing team may recover it but cannot
            // advance. It is played out on the ground like any other loose ball.
            self.loose(landing);
            return;
        }

        self.returner_has_ball = true;
        self.phase = ThrowPhase::Return;
        self.catch_t = Some(self.t);
        self.ball = self.players[returner].pos;
        self.ball_z = 1.0;
        self.events.push(format!("{} brings it out", self.name(returner)));
    }

    /// The ball is down and nobody has it. It is a live ball: the returner and the
    /// three gunners converge on it, and it is dead when the nearest man gets there.
    fn loose(&mut self, landing: Vec2) {
        self.phase = ThrowPhase::Loose;
        self.ball = landing;
        self.ball_z = 0.3;
        self.loose_t = 0.0;
        self.events.push("Loose ball on the ground!".to_string());
    }

    fn update_loose(&mut self, dt: f32) {
        let ball = self.ball;
        let human = self.human;
        let push = self.human_dir;
        for i in 1..self.players.len() {
            let speed = self.speed(i);
            let target = if human == Some(i) {
                self.players[i].pos + push * 4.0
            } else {
                ball
            };
            self.steer(i, target, speed, dt);
        }
        self.loose_t += dt;
        if self.loose_t < LOOSE_SECONDS {
            return;
        }

        // Whoever is closest when the whistle goes is the man on it. A ball nobody
        // got to is simply down where it lies.
        let returner = self.returner();
        let (winner, gap) = (1..self.players.len())
            .map(|i| (i, self.players[i].pos.dist(ball)))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((returner, 99.0));
        let on_it = gap < 2.0;
        let throwing_team_has_it = on_it && self.players[winner].team == self.setup.throwing_team;
        let (outcome, next, desc) = if throwing_team_has_it {
            (
                ThrowOffOutcome::RecoveredByK,
                self.setup.throwing_team,
                format!("Muffed, and {} falls on it for the throwing team.", self.name(winner)),
            )
        } else if on_it {
            (
                ThrowOffOutcome::Downed,
                self.setup.receiving_team,
                format!("Muffed, and {} falls on it.", self.name(winner)),
            )
        } else {
            (
                ThrowOffOutcome::Downed,
                self.setup.receiving_team,
                "The ball is down where it lies.".to_string(),
            )
        };
        self.finish(outcome, next, ball.x, 0, None, desc);
    }

    fn update_return(&mut self, dt: f32) {
        let dir = self.setup.receive_direction;
        let returner = self.returner();

        // A tackle that is being made: he has him, and the two of them are going to
        // the turf. The play is over when they get there, not when the hands close.
        if let Some(mut b) = self.tackle {
            b.t += dt;
            let p = (b.t / TACKLE_SECONDS).clamp(0.0, 1.0);
            self.players[b.returner].tackled = p;
            self.players[b.returner].vel = Vec2::ZERO;
            self.players[b.gunner].wrap = p;
            let spot = self.players[b.returner].pos;
            self.players[b.gunner].pos = self.players[b.gunner].pos.lerp(spot, (dt * 7.0).min(1.0));
            self.ball = spot;
            self.ball_z = 1.0;
            if p < 1.0 {
                self.tackle = Some(b);
                return;
            }
            self.tackle = None;
            self.players[b.returner].down = true;
            self.players[b.gunner].wrap = 0.0;
            if b.safety {
                let desc = format!(
                    "{} is dragged down in the end zone. Safety!",
                    self.name(b.returner)
                );
                self.finish(
                    ThrowOffOutcome::Safety,
                    self.setup.receiving_team,
                    0.0,
                    2,
                    Some(self.setup.throwing_team),
                    desc,
                );
            } else {
                let desc = format!(
                    "{} returns it {} yards before {} gets him.",
                    self.name(b.returner),
                    b.yards.round() as i32,
                    self.name(b.gunner)
                );
                self.finish(
                    ThrowOffOutcome::Returned,
                    self.setup.receiving_team,
                    b.spot,
                    0,
                    None,
                    desc,
                );
                if let Some(r) = self.result.as_mut() {
                    r.return_yards = b.yards;
                }
            }
            return;
        }

        let goal = field::opponent_goal_side(dir);
        let target = Vec2::new(goal, self.players[returner].pos.y * 0.6);
        let human = self.human;
        let push = self.human_dir;
        let speed = self.speed(returner);
        let target = if human == Some(returner) {
            self.players[returner].pos + push * 4.0
        } else {
            target
        };
        self.steer(returner, target, speed, dt);
        let pos = self.players[returner].pos;
        self.ball = pos;
        self.ball_z = 1.0;

        // Gunners chase, unless one of them is the man the human is driving.
        for g in 1..4 {
            let speed = self.speed(g);
            let target = if human == Some(g) { self.players[g].pos + push * 4.0 } else { pos };
            self.steer(g, target, speed, dt);
        }

        // Touchdown.
        let scored = if dir > 0 { pos.x >= goal } else { pos.x <= goal };
        if scored {
            let team = self.setup.receiving_team;
            let desc = format!("{} takes it all the way back!", self.name(returner));
            self.finish(ThrowOffOutcome::Touchdown, team, goal, 6, Some(team), desc);
            return;
        }

        // Sideline.
        if pos.y.abs() > FIELD_HALF_WIDTH {
            self.finish(
                ThrowOffOutcome::Returned,
                self.setup.receiving_team,
                pos.x,
                0,
                None,
                format!("{} is forced out of bounds.", self.name(returner)),
            );
            return;
        }

        // Tackles.
        for g in 1..4 {
            if self.players[g].down {
                continue;
            }
            let dist = self.players[g].pos.dist(pos);
            if dist < 1.0 {
                let defender = self.attrs(g).tackle_factor();
                let returner_skill = self.attrs(returner).break_tackle_factor();
                let p = clamp(0.36 + (defender - returner_skill) * 0.5, 0.1, 0.9);
                if self.rng.chance(p) {
                    // The hit lands here. What it is worth is settled when he is
                    // on the ground.
                    let in_own_endzone = if dir > 0 {
                        pos.x <= 0.0
                    } else {
                        pos.x >= FIELD_LENGTH
                    };
                    let yards = (pos.x - self.setup.spot()).abs();
                    self.tackle = Some(TackleBeat {
                        returner,
                        gunner: g,
                        t: 0.0,
                        spot: pos.x,
                        safety: in_own_endzone,
                        yards,
                    });
                    return;
                } else {
                    self.players[g].down = true;
                    self.events.push(format!("{} breaks free!", self.name(returner)));
                }
            }
        }

        if self.t > 30.0 {
            let desc = "The return dies at the bottom of the pile.".to_string();
            self.finish(
                ThrowOffOutcome::Downed,
                self.setup.receiving_team,
                pos.x,
                0,
                None,
                desc,
            );
        }
    }

    fn finish(
        &mut self,
        outcome: ThrowOffOutcome,
        next_offense: usize,
        start_spot: f32,
        points: u32,
        scoring_team: Option<usize>,
        description: String,
    ) {
        let safety = outcome == ThrowOffOutcome::Safety;
        let touchdown = outcome == ThrowOffOutcome::Touchdown;
        let touchback = outcome == ThrowOffOutcome::Touchback;
        let fair_catch = outcome == ThrowOffOutcome::FairCatch;
        let result = ThrowOffResult {
            outcome,
            next_offense,
            start_spot: clamp(start_spot, 0.0, FIELD_LENGTH),
            points,
            scoring_team,
            return_yards: if touchdown { 100.0 } else { 0.0 },
            fair_catch,
            touchback,
            safety,
            touchdown,
            elapsed: (self.t - self.snap_delay).max(1.0),
            description,
            highlight: touchdown || safety,
        };
        self.phase = ThrowPhase::Dead;
        self.result = Some(result);
    }

    pub fn is_snapped(&self) -> bool {
        self.t >= self.snap_delay
    }

    /// A tackle is being made right now: the picture has a hit to show and the
    /// card should wait for it.
    pub fn tackle_in_progress(&self) -> bool {
        self.tackle.is_some()
    }

    /// Seconds left before a computer thrower lets it go.
    pub fn presnap_left(&self) -> f32 {
        (self.snap_delay - self.t).max(0.0)
    }

    pub fn ball_position(&self) -> (Vec2, f32) {
        (self.ball, self.ball_z)
    }

    /// Where the returner is standing, so a client can frame both ends of the
    /// field before the ball is thrown.
    pub fn returner_position(&self) -> Option<Vec2> {
        self.players
            .iter()
            .find(|p| p.role == ThrowRole::Returner)
            .map(|p| p.pos)
    }

    /// Where the ball in the air is going to come down.
    pub fn landing_spot(&self) -> Option<Vec2> {
        self.flight.map(|f| f.to)
    }

    /// When the returner took the ball in, if he has.
    pub fn catch_time(&self) -> Option<f32> {
        self.catch_t
    }
}

pub fn simulate_throw_off(
    setup: &ThrowOffSetup,
    throwing: &Team,
    receiving: &Team,
) -> ThrowOffResult {
    let mut sim = ThrowOffSim::new(setup, throwing, receiving);
    sim.run_to_completion()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::team::league_teams;

    fn teams() -> (Team, Team) {
        let t = league_teams();
        (t[0].clone(), t[1].clone())
    }

    #[test]
    fn throw_off_spots_are_on_the_35() {
        let mut s = ThrowOffSetup::normal();
        s.receive_direction = 1;
        // Receiving team attacks +x, so the thrower stands on the 35 yard line of
        // the other end: x = 100 - 35 = 65.
        assert!((s.spot() - 65.0).abs() < 1e-3);
        s.receive_direction = -1;
        assert!((s.spot() - 35.0).abs() < 1e-3);
    }

    #[test]
    fn throw_offs_produce_legal_outcomes() {
        let (a, b) = teams();
        let mut seen = std::collections::HashSet::new();
        for seed in 0..120u64 {
            let setup = ThrowOffSetup {
                throwing_team: 0,
                receiving_team: 1,
                from_own_yards: 35.0,
                receive_direction: 1,
                seed,
                free_throw: false,
            };
            let r = simulate_throw_off(&setup, &a, &b);
            assert!(r.start_spot >= 0.0 && r.start_spot <= 100.0);
            assert!(r.next_offense < 2);
            assert!(!r.description.is_empty());
            seen.insert(format!("{:?}", r.outcome));
        }
        assert!(seen.len() >= 2, "throw-offs should be varied: {seen:?}");
    }

    /// A throw-off can no longer be thrown through the back of the end zone: full
    /// power comes down on the 20 with eighty yards of field in front of it.
    #[test]
    fn full_power_lands_twenty_yards_short_of_the_end_zone() {
        let (a, b) = teams();
        let setup = ThrowOffSetup { receive_direction: 1, ..ThrowOffSetup::normal() };
        let sim = ThrowOffSim::new(&setup, &a, &b);
        let plan = sim.throw_off_plan(1.0, 0.0);
        assert!(!plan.through_endzone, "full power still clears the end zone: {plan:?}");
        assert!(!plan.too_short, "full power should be a legal throw: {plan:?}");
        // Thrown from the 35 toward the far end zone, so it comes down on the 20.
        let yards_to_goal = if setup.receive_direction > 0 {
            plan.landing.x
        } else {
            FIELD_LENGTH - plan.landing.x
        };
        assert!(
            (yards_to_goal - 20.0).abs() < 0.5,
            "full power landed {yards_to_goal:.1} yards from the end zone"
        );
        // And the lightest throw is still short of the rulebook's forty.
        assert!(sim.throw_off_plan(0.0, 0.0).too_short);
    }

    /// Touchbacks survive as a rule, but only a fourth-down free throw from the
    /// opponent's side of the field can reach an end zone any more: a throw-off
    /// from the 35 tops out on the 20.
    #[test]
    fn a_throw_off_caught_in_the_end_zone_is_a_touchback() {
        let (a, b) = teams();
        let setup = ThrowOffSetup {
            from_own_yards: 56.0,
            receive_direction: -1,
            free_throw: true,
            ..ThrowOffSetup::normal()
        };
        let mut sim = ThrowOffSim::new(&setup, &a, &b);
        sim.launch_with(1.0, 0.0);
        let r = sim.run_to_completion();
        assert!(r.touchback, "expected a touchback, got {:?}", r.outcome);
        // Receiving team attacks -x, so its own 20 is x = 80.
        assert!((r.start_spot - 80.0).abs() < 1e-2, "spot {}", r.start_spot);
    }

    #[test]
    fn the_plan_and_the_flight_agree() {
        let (a, b) = teams();
        let setup = ThrowOffSetup { receive_direction: 1, ..ThrowOffSetup::normal() };
        let mut sim = ThrowOffSim::new(&setup, &a, &b);
        let plan = sim.throw_off_plan(0.7, 6.0);
        sim.launch_with(0.7, 6.0);
        let flight = sim.flight.expect("a legal throw should fly");
        assert!((flight.to.x - plan.landing.x).abs() < 1e-3);
        assert!((flight.to.y - plan.landing.y).abs() < 1e-3);
        assert_eq!(sim.phase, ThrowPhase::Flight);
    }

    #[test]
    fn a_weak_throw_off_does_not_make_forty_yards() {
        let (a, b) = teams();
        let setup = ThrowOffSetup::normal();
        let mut sim = ThrowOffSim::new(&setup, &a, &b);
        let plan = sim.throw_off_plan(0.1, 0.0);
        assert!(plan.too_short, "a 0.1 throw should be short: {:?}", plan);
        sim.launch_with(0.1, 0.0);
        // The throw itself is not the end of the play any more: it flies, and the
        // Run Back is read once the ball is down.
        assert!(sim.result.is_none(), "the short throw was called on the release");
        assert_eq!(sim.phase, ThrowPhase::Flight);
        let r = sim.run_to_completion();
        assert_eq!(r.outcome, ThrowOffOutcome::RunBack);
        assert!(
            (r.start_spot - 25.0).abs() < 0.5,
            "Run Back spot should be the 25, got {}",
            r.start_spot
        );
    }

    /// A short throw still leaves the hand, and the ball is in the air for a good
    /// couple of seconds before anybody calls it.
    #[test]
    fn a_short_throw_off_flies_before_it_is_called() {
        let (a, b) = teams();
        let setup = ThrowOffSetup { receive_direction: 1, ..ThrowOffSetup::normal() };
        let mut sim = ThrowOffSim::new(&setup, &a, &b);
        let plan = sim.throw_off_plan(0.1, 0.0);
        sim.launch_with(0.1, 0.0);
        let flight = sim.flight.expect("a short throw still goes up");
        assert!((flight.to.x - plan.landing.x).abs() < 1e-3);
        assert!(flight.duration > 1.5, "hang time {}", flight.duration);
        for _ in 0..30 {
            sim.step(1.0 / 60.0);
        }
        assert!(sim.result.is_none(), "the play was called mid-flight");
        assert_eq!(sim.phase, ThrowPhase::Flight);
    }

    /// A ball nobody catches is live on the ground before it is dead, not called the
    /// instant it lands.
    #[test]
    fn a_ball_nobody_catches_is_live_before_it_is_dead() {
        let (a, b) = teams();
        let setup = ThrowOffSetup { receive_direction: 1, ..ThrowOffSetup::normal() };
        let mut sim = ThrowOffSim::new(&setup, &a, &b);
        // Stand the returner well away from where the ball comes down and hold him
        // there, so nobody can get under it.
        sim.players[4].pos = Vec2::new(60.0, 0.0);
        sim.human = Some(4);
        sim.human_dir = Vec2::ZERO;
        sim.launch_with(0.8, 0.0);
        for _ in 0..600 {
            if sim.phase == ThrowPhase::Loose || sim.result.is_some() {
                break;
            }
            sim.step(1.0 / 60.0);
        }
        assert_eq!(sim.phase, ThrowPhase::Loose, "the ball was never live");
        assert!(sim.result.is_none(), "the loose ball was dead on arrival");
        // It lies there for a beat while everybody runs to it.
        for _ in 0..30 {
            sim.step(1.0 / 60.0);
        }
        assert!(sim.result.is_none(), "the loose ball died in half a second");
        let r = sim.run_to_completion();
        assert!(
            matches!(r.outcome, ThrowOffOutcome::Downed | ThrowOffOutcome::RecoveredByK),
            "a loose ball ends up downed, not {:?}",
            r.outcome
        );
    }

    #[test]
    fn a_human_thrower_can_choose_where_it_comes_down() {
        let (a, b) = teams();
        let setup = ThrowOffSetup::normal();
        let mut sim = ThrowOffSim::new(&setup, &a, &b);
        sim.human_thrower = true;
        // With a human on the lever the AI must not launch it for him.
        for _ in 0..120 {
            sim.step(1.0 / 60.0);
        }
        assert_eq!(sim.phase, ThrowPhase::Presnap, "the AI threw it off for the player");
        sim.launch_with(0.8, -9.0);
        let flight = sim.flight.expect("the throw should be in the air");
        assert!((flight.to.y + 9.0).abs() < 1e-3, "landed at y={}", flight.to.y);
    }

    /// A return that ends in a tackle is not called the instant the hands close:
    /// he has to go to the ground first, the way a tackle anywhere else does.
    #[test]
    fn a_return_tackle_puts_him_down_before_the_play_is_called() {
        let (a, b) = teams();
        let setup = ThrowOffSetup { receive_direction: 1, ..ThrowOffSetup::normal() };
        let mut sim = ThrowOffSim::new(&setup, &a, &b);
        sim.launch_with(0.7, 0.0);
        // Keep the returner under the ball so it is caught, then let a gunner get
        // to him.
        let mut guard = 0;
        while sim.phase != ThrowPhase::Return && sim.result.is_none() && guard < 900 {
            if let Some(spot) = sim.landing_spot() {
                sim.players[4].pos = spot;
            }
            sim.step(1.0 / 60.0);
            guard += 1;
        }
        assert_eq!(
            sim.phase,
            ThrowPhase::Return,
            "nobody caught it: {:?}",
            sim.result.as_ref().map(|r| &r.outcome)
        );

        let mut guard = 0;
        while !sim.tackle_in_progress() && sim.result.is_none() && guard < 900 {
            sim.players[1].pos = sim.players[4].pos;
            sim.players[1].down = false;
            sim.step(1.0 / 60.0);
            guard += 1;
        }
        assert!(sim.tackle_in_progress(), "no tackle was ever made on the return");
        assert!(
            sim.result.is_none(),
            "the return was called on the hit, before he went down"
        );
        assert!(!sim.players[4].down, "he is down before the wrap has run");

        let mut t = 0.0;
        while sim.result.is_none() && t < 2.0 {
            sim.step(1.0 / 60.0);
            t += 1.0 / 60.0;
        }
        let r = sim.result.clone().expect("the tackle has to end the return");
        assert!(t > 0.25, "the hit resolved in {t:.2}s");
        assert!(sim.players[4].down, "the returner never went down");
        assert_eq!(r.outcome, ThrowOffOutcome::Returned, "{:?}", r.outcome);
    }
}

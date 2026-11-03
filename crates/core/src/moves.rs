//! Ball-carrier moves: the spin, the hurdle, the dive and the quarterback slide.
//!
//! These are the things a runner does to a defender who has the angle on him, and
//! the one thing a quarterback does instead of taking the hit. Each is a short
//! timed state on the carrier rather than an instant effect, so the simulation,
//! the animation and the tackle mathematics all agree on what is happening and
//! when it ends — and so a move can be punished. A dive gains the yard and ends
//! the play; a hurdle loses speed and cannot be steered; a spin costs nothing but
//! its length.

use crate::math::Vec2;

/// How long each move runs, in seconds.
pub const SPIN_TIME: f32 = 0.42;
pub const HURDLE_TIME: f32 = 0.36;
pub const DIVE_TIME: f32 = 0.34;
pub const SLIDE_TIME: f32 = 0.40;
pub const STIFF_ARM_TIME: f32 = 0.45;
/// A cut is a stride's worth of plant; a juke is the same step taken harder, and
/// a stop costs about as long as it takes to sit down on both feet.
pub const CUT_TIME: f32 = 0.22;
pub const JUKE_TIME: f32 = 0.28;
pub const STOP_TIME: f32 = 0.26;
/// Breathing room after a move before the next one can start, so a mash of the
/// button cannot chain moves into a permanent immunity.
pub const COOLDOWN: f32 = 0.18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Feet stay under him but the angle changes: harder to wrap up.
    Spin,
    /// He leaves his feet over a defender: the wrap goes under him, but he is
    /// committed to his line for as long as he is in the air.
    Hurdle,
    /// Forward and down: the extra yard, and the play ends where he lands.
    Dive,
    /// The quarterback gives himself up rather than take the hit.
    Slide,
    /// The free hand goes out at the man in front of him and shoves.
    StiffArm,
    /// A plant and a break: he leaves his line flat-footed and goes sideways
    /// without losing his feet.
    Cut,
    /// The sharper break, all hips: the jump step that takes the tackle off him.
    Juke,
    /// Both feet and a dead stop, so the man chasing him runs past.
    Stop,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Spin => "SPIN",
            Kind::Hurdle => "HURDLE",
            Kind::Dive => "DIVE",
            Kind::Slide => "SLIDE",
            Kind::StiffArm => "STIFF ARM",
            Kind::Cut => "CUT",
            Kind::Juke => "JUKE",
            Kind::Stop => "STOP",
        }
    }

    /// How long this move lasts. Spins and hurdles are a beat; a dive is quick;
    /// a slide is the runner slowing to a stop.
    pub fn time(self) -> f32 {
        match self {
            Kind::Spin => SPIN_TIME,
            Kind::Hurdle => HURDLE_TIME,
            Kind::Dive => DIVE_TIME,
            Kind::Slide => SLIDE_TIME,
            Kind::StiffArm => STIFF_ARM_TIME,
            Kind::Cut => CUT_TIME,
            Kind::Juke => JUKE_TIME,
            Kind::Stop => STOP_TIME,
        }
    }

    /// Whether the stick still steers the runner while the move runs.
    pub fn steerable(self) -> bool {
        matches!(self, Kind::Spin | Kind::StiffArm)
    }

    /// Whether the move ends the play when it finishes.
    pub fn commits(self) -> bool {
        matches!(self, Kind::Dive | Kind::Slide)
    }
}

/// The move a carrier is in the middle of, if any.
#[derive(Clone, Copy, Debug, Default)]
pub struct CarrierMove {
    active: Option<(Kind, f32)>,
    cooldown: f32,
    /// The line the move was started on, so a dive goes where it was aimed and
    /// not where the stick happens to be a frame later.
    dir: Vec2,
}

impl CarrierMove {
    /// Start a move. Ignored when one is already running or the runner is still
    /// gathering himself from the last one.
    pub fn start(&mut self, kind: Kind, facing: Vec2) -> bool {
        if self.active.is_some() || self.cooldown > 0.0 {
            return false;
        }
        self.dir = if facing.length() > 0.1 {
            facing.normalize_or_zero()
        } else {
            Vec2::new(1.0, 0.0)
        };
        self.active = Some((kind, 0.0));
        true
    }

    pub fn kind(&self) -> Option<Kind> {
        self.active.map(|(kind, _)| kind)
    }

    /// The line the move is being run on.
    pub fn dir(&self) -> Vec2 {
        self.dir
    }

    /// How far through the move, 0..1.
    pub fn progress(&self) -> f32 {
        match self.active {
            Some((kind, t)) => (t / kind.time()).clamp(0.0, 1.0),
            None => 0.0,
        }
    }

    pub fn is_hurdling(&self) -> bool {
        self.kind() == Some(Kind::Hurdle)
    }

    /// Whether the play ends when this move does.
    pub fn commits(&self) -> bool {
        self.kind().map(|k| k.commits()).unwrap_or(false)
    }

    /// Speed multiplier while the move runs: a dive is a lunge, a hurdle costs
    /// the stride you spend in the air.
    pub fn speed_scale(&self) -> f32 {
        match self.kind() {
            Some(Kind::Spin) => 1.04,
            Some(Kind::Hurdle) => 0.86,
            Some(Kind::Dive) => 1.45,
            Some(Kind::Slide) => 1.10,
            Some(Kind::StiffArm) => 1.02,
            // He does not slow down to break: the break is what keeps the angle.
            Some(Kind::Cut) => 1.12,
            Some(Kind::Juke) => 1.08,
            // A dead stop is a dead stop.
            Some(Kind::Stop) => 0.08,
            None => 1.0,
        }
    }

    /// What a wrap-up attempt has to beat. A man in the air cannot be wrapped up
    /// at all — that is handled by the tackle code — and a man who has given
    /// himself up does not need to be.
    pub fn tackle_scale(&self) -> f32 {
        match self.kind() {
            Some(Kind::Spin) => 0.55,
            Some(Kind::Hurdle) => 0.0,
            Some(Kind::Dive) => 0.75,
            Some(Kind::Slide) => 0.0,
            // The hand is on the man, so the wrap is late, but he is still a
            // runner on his feet with his whole body to hit.
            Some(Kind::StiffArm) => 0.45,
            // Breaking off his line makes him a worse target for as long as the
            // break lasts, which is the whole point of running it.
            Some(Kind::Cut) => 0.85,
            Some(Kind::Juke) => 0.70,
            Some(Kind::Stop) => 1.0,
            None => 1.0,
        }
    }

    /// Advance the move by one timestep, returning the kind that just finished.
    pub fn step(&mut self, dt: f32) -> Option<Kind> {
        if self.cooldown > 0.0 {
            self.cooldown = (self.cooldown - dt).max(0.0);
        }
        let (kind, t) = self.active?;
        let t = t + dt;
        if t >= kind.time() {
            self.active = None;
            self.cooldown = COOLDOWN;
            return Some(kind);
        }
        self.active = Some((kind, t));
        None
    }

    /// Drop everything, for a new snap.
    pub fn reset(&mut self) {
        *self = CarrierMove::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_move_has_to_finish_before_the_next_one() {
        let mut m = CarrierMove::default();
        assert!(m.start(Kind::Spin, Vec2::new(1.0, 0.0)));
        assert!(!m.start(Kind::Dive, Vec2::new(1.0, 0.0)), "spun twice in one breath");
        // Run the spin out.
        let mut done = None;
        for _ in 0..60 {
            if let Some(k) = m.step(1.0 / 60.0) {
                done = Some(k);
                break;
            }
        }
        assert_eq!(done, Some(Kind::Spin));
        assert!(m.kind().is_none());
        // And the cooldown still has to pass.
        assert!(!m.start(Kind::Dive, Vec2::new(1.0, 0.0)));
        for _ in 0..30 {
            m.step(1.0 / 60.0);
        }
        assert!(m.start(Kind::Dive, Vec2::new(1.0, 0.0)));
    }

    #[test]
    fn a_stiff_arm_is_not_a_commitment() {
        assert!(!Kind::StiffArm.commits(), "a stiff arm ended the play");
        assert!(Kind::StiffArm.steerable(), "the runner could not steer in it");
        let mut m = CarrierMove::default();
        assert!(m.start(Kind::StiffArm, Vec2::new(1.0, 0.0)));
        assert!(m.tackle_scale() < 1.0, "the arm did nothing");
        assert!(m.speed_scale() >= 1.0, "the arm cost him his stride");
    }

    #[test]
    fn the_spin_is_the_only_move_that_keeps_the_steering() {
        assert!(Kind::Spin.steerable());
        assert!(Kind::StiffArm.steerable());
        assert!(!Kind::Hurdle.steerable());
        assert!(!Kind::Dive.steerable());
        assert!(!Kind::Slide.steerable());
    }

    #[test]
    fn diving_and_sliding_end_the_play_and_hurdling_does_not() {
        assert!(Kind::Dive.commits());
        assert!(Kind::Slide.commits());
        assert!(!Kind::Hurdle.commits());
        assert!(!Kind::Spin.commits());
    }

    #[test]
    fn a_move_keeps_the_line_it_was_started_on() {
        let mut m = CarrierMove::default();
        m.start(Kind::Dive, Vec2::new(0.0, -3.0));
        assert!((m.dir().y + 1.0).abs() < 1e-5, "the line is normalised, {:?}", m.dir());
    }

    #[test]
    fn progress_runs_out_with_the_clock() {
        let mut m = CarrierMove::default();
        m.start(Kind::Hurdle, Vec2::new(1.0, 0.0));
        m.step(HURDLE_TIME * 0.5);
        assert!((m.progress() - 0.5).abs() < 0.02, "{}", m.progress());
        m.step(HURDLE_TIME);
        assert!(m.kind().is_none());
    }
}

//! How hard the other sideline plays.
//!
//! Easy is the game the league shipped with: the coordinator calls a sensible mix
//! and plays what is in front of him. Normal leans on moving the ball downfield
//! and finishing drives. Hard is Normal with a defence that hunts the ball: it
//! presses, it jumps routes, and it goes for the strip on the tackle.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}

impl Default for Difficulty {
    fn default() -> Self {
        Difficulty::Easy
    }
}

impl Difficulty {
    pub const ALL: [Difficulty; 3] = [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard];

    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Easy => "EASY",
            Difficulty::Normal => "NORMAL",
            Difficulty::Hard => "HARD",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Difficulty::Easy => 0,
            Difficulty::Normal => 1,
            Difficulty::Hard => 2,
        }
    }

    pub fn from_index(i: usize) -> Self {
        Difficulty::ALL[i % Difficulty::ALL.len()]
    }

    /// Extra chance of the ball coming loose on a tackle. Hard hands go for it.
    pub fn fumble_bonus(self) -> f32 {
        match self {
            Difficulty::Easy => 0.0,
            Difficulty::Normal => 0.004,
            Difficulty::Hard => 0.014,
        }
    }

    /// Extra chance a defender in position takes the ball away instead of only
    /// knocking it down.
    pub fn pick_bonus(self) -> f32 {
        match self {
            Difficulty::Easy => 0.0,
            Difficulty::Normal => 0.03,
            Difficulty::Hard => 0.11,
        }
    }

    /// Scale on the quarterback's read: a harder side throws the ball further
    /// downfield rather than settling for the check-down.
    pub fn aggression(self) -> f32 {
        match self {
            Difficulty::Easy => 0.0,
            Difficulty::Normal => 0.5,
            Difficulty::Hard => 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_levels_round_trip_and_hard_plays_the_ball() {
        for (i, d) in Difficulty::ALL.iter().enumerate() {
            assert_eq!(d.index(), i);
            assert_eq!(Difficulty::from_index(i), *d);
            assert!(!d.label().is_empty());
        }
        assert!(Difficulty::Hard.pick_bonus() > Difficulty::Normal.pick_bonus());
        assert!(Difficulty::Hard.fumble_bonus() > Difficulty::Easy.fumble_bonus());
        assert_eq!(Difficulty::Easy.pick_bonus(), 0.0);
        assert_eq!(Difficulty::default(), Difficulty::Easy);
    }
}

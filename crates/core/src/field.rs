//! A7FL field geometry.
//!
//! Official A7FL field: 120 yards long overall, 100 yards of field of play between
//! the goal lines, 10-yard end zones and 37.5 yards wide. No goal posts are used.
//!
//! Canonical simulation coordinates:
//!   * `x` runs along the length of the field: 0.0 at the left goal line, 100.0 at the
//!     right goal line, so end zones live in `-10..0` and `100..110`.
//!   * `y` runs across the field: `-FIELD_HALF_WIDTH..FIELD_HALF_WIDTH`.
//!   * `z` is height above the turf.
//!
//! Each team has an attack direction of `+1` (toward x=100) or `-1` (toward x=0).
//! All rules code converts to "yards to the opponent's goal line" before doing math.

use crate::math::clamp;

/// Distance between the two goal lines.
pub const FIELD_LENGTH: f32 = 100.0;
/// Depth of one end zone.
pub const ENDZONE_DEPTH: f32 = 10.0;
/// Full width of the playing field.
pub const FIELD_WIDTH: f32 = 37.5;
pub const FIELD_HALF_WIDTH: f32 = FIELD_WIDTH * 0.5;
/// `x` of the far goal line.
pub const FAR_GOAL_LINE: f32 = FIELD_LENGTH;
/// Left edge of the left end zone.
pub const BACK_OF_LEFT_ENDZONE: f32 = -ENDZONE_DEPTH;
/// Right edge of the right end zone.
pub const BACK_OF_RIGHT_ENDZONE: f32 = FIELD_LENGTH + ENDZONE_DEPTH;
/// The A7FL keeps the hashes narrow thanks to the 37.5 yard field.
pub const HASH_Y: f32 = 9.0;

/// Attack direction for the team that owns the ball, given a canonical spot.
pub fn opponent_goal_side(direction: i8) -> f32 {
    if direction > 0 {
        FAR_GOAL_LINE
    } else {
        0.0
    }
}

/// Yards from the spot to the opponent's goal line.
pub fn yards_to_goal(spot: f32, direction: i8) -> f32 {
    if direction > 0 {
        FAR_GOAL_LINE - spot
    } else {
        spot
    }
}

/// Convert "yards to goal" back into a canonical spot.
pub fn spot_from_yards_to_goal(yards: f32, direction: i8) -> f32 {
    if direction > 0 {
        FAR_GOAL_LINE - yards
    } else {
        yards
    }
}

/// The number painted on the field at a given spot (10..50..10). Returns
/// `(label, is_left_half)` where `is_left_half` means the value is on the
/// 50-to-left-goal side (numbers are mirrored there).
pub fn painted_yard_number(spot: f32) -> (u32, bool) {
    let x = clamp(spot, 0.0, FIELD_LENGTH);
    let steps = ((50.0 - x).abs() / 10.0).round() as i32;
    let n = if steps == 0 { 50 } else { (steps * 10) as u32 };
    (n, x <= 50.0)
}

/// Broadcast style "BALL ON" label, e.g. `35` or `MIDFIELD`.
pub fn ball_on_label(yards_to_goal: f32) -> String {
    let ytg = clamp(yards_to_goal, 0.0, FIELD_LENGTH);
    if (ytg - 50.0).abs() < 0.5 {
        "MIDFIELD".to_string()
    } else if ytg < 50.0 {
        // On the opponent's side of the field the painted number is the yard line
        // itself, e.g. "35" on the opponent's 35.
        format!("{:02}", ytg.round() as u32)
    } else {
        // Own side: show the painted number, i.e. 100 - yards to go.
        format!("{:02}", (FIELD_LENGTH - ytg).round().max(0.0) as u32)
    }
}

/// True when the ball is on the offense's own side of midfield.
pub fn is_own_side(yards_to_goal: f32) -> bool {
    yards_to_goal > 50.0
}

/// Side of the field (`L`, `M`, `R`) for a spot, used by the broadcast HUD.
pub fn side_of_field(y: f32) -> &'static str {
    if y < -HASH_Y {
        "L"
    } else if y > HASH_Y {
        "R"
    } else {
        "M"
    }
}

/// How far a spot is from the nearest sideline.
pub fn distance_to_sideline(y: f32) -> f32 {
    FIELD_HALF_WIDTH - y.abs()
}

pub fn is_in_endzone(spot: f32, direction: i8) -> bool {
    let goal = opponent_goal_side(direction);
    if direction > 0 {
        spot >= goal
    } else {
        spot <= goal
    }
}

/// Is the spot inside the end zone the offense is defending (i.e. own end zone)?
pub fn is_in_own_endzone(spot: f32, direction: i8) -> bool {
    if direction > 0 {
        spot <= 0.0
    } else {
        spot >= FAR_GOAL_LINE
    }
}

/// Clamp a spot so the ball never sits inside an end zone during a scrimmage play.
pub fn clamp_scrimmage_spot(spot: f32) -> f32 {
    clamp(spot, 0.0, FIELD_LENGTH)
}

/// Clamp a spot to somewhere on the playing surface incl. end zones.
pub fn clamp_playable(spot: f32) -> f32 {
    clamp(spot, BACK_OF_LEFT_ENDZONE, BACK_OF_RIGHT_ENDZONE)
}

/// Convert to a 0..1 normalised position across the field width for the renderer.
pub fn normalise_y(y: f32) -> f32 {
    (y + FIELD_HALF_WIDTH) / FIELD_WIDTH
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yard_labels_read_like_a_broadcast() {
        assert_eq!(ball_on_label(100.0), "00"); // own goal line
        assert_eq!(ball_on_label(75.0), "25"); // own 25
        assert_eq!(ball_on_label(35.0), "35"); // opponent 35
        assert_eq!(ball_on_label(15.0), "15"); // opponent 15
        assert_eq!(ball_on_label(50.0), "MIDFIELD");
        assert!(is_own_side(75.0));
        assert!(!is_own_side(25.0));
    }

    #[test]
    fn painted_numbers_mirror_at_midfield() {
        assert_eq!(painted_yard_number(10.0), (40, true));
        assert_eq!(painted_yard_number(50.0), (50, true));
        assert_eq!(painted_yard_number(90.0), (40, false));
        assert_eq!(painted_yard_number(30.0), (20, true));
    }

    #[test]
    fn spots_round_trip() {
        for spot in [0.0, 12.5, 47.0, 99.0] {
            for dir in [1i8, -1] {
                let ytg = yards_to_goal(spot, dir);
                assert!((spot_from_yards_to_goal(ytg, dir) - spot).abs() < 1e-4);
            }
        }
        assert_eq!(yards_to_goal(20.0, 1), 80.0);
        assert_eq!(yards_to_goal(20.0, -1), 20.0);
    }

    #[test]
    fn endzone_detection() {
        assert!(is_in_endzone(101.0, 1));
        assert!(!is_in_endzone(99.0, 1));
        assert!(is_in_endzone(-1.0, -1));
        assert!(is_in_own_endzone(0.0, 1));
        assert!(!is_in_own_endzone(1.0, 1));
    }
}

//! 7 on 7 No Pads Football — simulation core.
//!
//! Layers:
//!
//! * [`rules`] / [`field`] — the 7 on 7 rulebook as data plus enforcement math.
//! * [`team`] — teams, rosters, attributes, 7 on 7 uniform specs.
//! * [`playbook`] — formations, route concepts and defensive calls.
//! * [`sim`] — the continuous-time 7 on 7 snap simulation.
//! * [`game`] — quarters, clock, downs, conversions, overtime, mercy rule.
//! * [`ai`] — head coach play calling and clock management.
//! * [`stats`] — box score, drive charts.

pub mod ai;
pub mod custom;
pub mod difficulty;
pub mod field;
pub mod game;
pub mod math;
pub mod moves;
pub mod playbook;
pub mod rules;
pub mod season;
pub mod sim;
pub mod stats;
pub mod team;
pub mod throwoff;

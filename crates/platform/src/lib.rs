//! What a 7 on 7 client needs from the machine it is running on.
//!
//! These types used to live inside the game binary, which meant nothing else could
//! drive the game: a second window backend, a test harness or a tool could not ask
//! for the pad or the settings because they were private to `main`. None of it is
//! game logic, so it lives here now, and every client imports it instead.

pub mod link;
pub mod pad;
pub mod settings;

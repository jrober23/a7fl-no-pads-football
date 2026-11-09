//! The game itself, as a library.
//!
//! `main.rs` is the command line around this, and it is not the only front end:
//! the Android build drives the same `Arcade` and the same `View` rather than
//! keeping a second copy of either. Everything that draws a frame, reads the pad
//! or calls a play lives here, so a new host only has to open a view.

pub mod arcade;
pub mod menu;
pub mod view;

#[cfg(feature = "bevy_view")]
pub mod bevy_host;
#[cfg(feature = "bevy_view")]
pub mod view_bevy;

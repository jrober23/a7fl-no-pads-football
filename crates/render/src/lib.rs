//! Software broadcast renderer for the A7FL game.
//!
//! The renderer is deliberately dependency-light (just `image` for PNG and `gif`
//! for the highlight package) and draws everything on the CPU: perspective
//! projection, z-buffered triangle rasterisation, a painted field texture and
//! procedural player models wearing A7FL kit — jerseys and shorts, no pads and no
//! helmets.
//!
//! [`broadcast`] ties it together: given a [`a7fl_core::game::GameState`] and one of
//! its [`a7fl_core::game::PlayRecord`]s it replays the exact same deterministic
//! simulation and renders camera framings of it, so a rendered frame always matches
//! the text play-by-play.

pub mod broadcast;
pub mod capture;
pub mod fieldtex;
pub mod font;
pub mod hud;
pub mod players;
pub mod raster;
pub mod scene;
pub mod terminal;

pub use broadcast::{Broadcast, CameraMode};
pub use capture::{write_gif, write_png};
pub use raster::Frame;

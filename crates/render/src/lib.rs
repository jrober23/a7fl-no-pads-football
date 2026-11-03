//! Software broadcast renderer for the game.
//!
//! The renderer is deliberately dependency-light (just `image` for PNG and `gif`
//! for the highlight package) and draws everything on the CPU: perspective
//! projection, z-buffered triangle rasterisation, a painted field texture and
//! procedural player models in no-pads kit — jerseys and shorts, no pads and no
//! helmets.
//!
//! [`broadcast`] ties it together: given a [`nopads_core::game::GameState`] and one of
//! its [`nopads_core::game::PlayRecord`]s it replays the exact same deterministic
//! simulation and renders camera framings of it, so a rendered frame always matches
//! the text play-by-play.

pub mod broadcast;
mod head;
pub mod callsheet;
pub mod capture;
pub mod defsheet;
pub mod fieldtex;
pub mod font;
pub mod hud;
pub mod passing;
pub mod playsheet;
pub mod players;
pub mod post;
pub mod raster;
pub mod scene;
pub mod terminal;
pub mod venues;

pub use broadcast::{Broadcast, CameraMode};
pub use capture::{write_gif, write_png};
pub use raster::Frame;

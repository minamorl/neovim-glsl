//! The editing core the own host owns.
//!
//! Nothing in here knows about msgpack, GLSL or windows. That separation is not
//! tidiness: `pin architecture_choice` says the host speaks Neovim's protocol,
//! and a core that could only be driven through the protocol would make the
//! protocol untestable against anything but itself.

pub mod buffer;
pub mod command;
pub mod editor;
pub mod key;
pub mod motion;

pub use buffer::Buffer;
pub use editor::{Editor, Message, Mode, Request, Scope, Visual};

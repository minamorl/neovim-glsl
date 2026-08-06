//! The editing core the own host owns.
//!
//! Nothing in here knows about msgpack, GLSL or windows. That separation is not
//! tidiness: `pin architecture_choice` says the host speaks Neovim's protocol,
//! and a core that could only be driven through the protocol would make the
//! protocol untestable against anything but itself.

pub mod buffer;
pub mod buffers;
pub mod command;
pub mod diff;
pub mod editor;
pub mod key;
pub mod motion;
pub mod vcs;
pub mod window;

pub use buffer::Buffer;
pub use buffers::{BufferId, BufferStore};
pub use editor::{Editor, Message, Mode, Request, Scope, Visual};
pub use vcs::{HeadLabel, Hunk, SignKind, VcsRequest, VcsState, VcsStatus};
pub use window::{Direction, Layout, Rect, Tabs, WindowId, WindowView};

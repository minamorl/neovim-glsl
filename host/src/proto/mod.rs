//! The Neovim protocol, server side.

pub mod paint;
pub mod server;

pub use server::{serve, Host};

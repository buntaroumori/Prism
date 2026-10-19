//! Tauri command surface.
//!
//! Commands are grouped by domain; every one of them returns
//! `Result<T, PrismError>` so the frontend receives a typed `{ code, message }`
//! failure instead of an opaque string.

pub mod discord;
pub mod library;
pub mod lyrics;
pub mod player;
pub mod plugins;
pub mod system;
pub mod visualiser;

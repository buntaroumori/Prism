//! Tauri command surface.
//!
//! Commands are grouped by domain; every one of them returns
//! `Result<T, PrismError>` so the frontend receives a typed `{ code, message }`
//! failure instead of an opaque string.

pub mod library;
pub mod system;

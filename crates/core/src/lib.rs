//! Meridian's portfolio logic: types, storage, quotes and every number on screen.
//!
//! This crate deliberately links no HTTP server, no Tauri and no desktop library, so the
//! server that wraps it can be deployed to a box with nothing installed on it.

pub mod calc;
pub mod config;
pub mod discover;
pub mod history;
pub mod import;
pub mod optimize;
pub mod par;
pub mod quotes;
pub mod store;
pub mod types;
pub mod universe;

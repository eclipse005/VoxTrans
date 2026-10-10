//! Voxtrans core library.
//!
//! Exposes the backend modules used by the Tauri desktop app (`src/main.rs`).

pub mod app_state;
/// Headless command-line interface (binary: `voxtrans-cli`).
///
/// A peer of `commands` (the Tauri/IPC adapters): both are outer layers that
/// speak DTOs and drive `services`. Unlike `commands`, `cli` must stay free of
/// `AppHandle` — the whole point is running the pipeline with no GUI process —
/// so it never reaches into `commands::workspace`.
pub mod cli;
pub mod commands;
pub mod db;
pub mod domain;
pub mod services;

#[cfg(test)]
mod layering;

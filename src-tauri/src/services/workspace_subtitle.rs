//! Compatibility re-export: the subtitle SoT model now lives in
//! [`crate::domain::workspace_subtitle`] (persisted by `db`, produced by the
//! command layer). `db` and `domain` use the canonical domain path; this shim
//! keeps `services::workspace_subtitle` working for other consumers.

pub use crate::domain::workspace_subtitle::*;

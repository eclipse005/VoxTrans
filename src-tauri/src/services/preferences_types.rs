//! Compatibility re-export: the settings/model vocabulary now lives in
//! [`crate::domain::preferences_types`] (it is shared by the domain, `db` and
//! the command layer, so the innermost layer owns it). This shim keeps the
//! `services::preferences_types` import path working for outward-facing
//! consumers; `db` and `domain` import from the canonical domain path.

pub use crate::domain::preferences_types::*;

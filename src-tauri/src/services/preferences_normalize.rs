//! Compatibility re-export: settings canonicalization now lives in
//! [`crate::domain::preferences_normalize`] (it is pure logic over the domain
//! settings vocabulary — no I/O, no store — and `db` seeds a first-launch row
//! from it, so the innermost layer owns it). This shim keeps the
//! `services::preferences_normalize` import path working for outward-facing
//! consumers; `db` and `domain` import from the canonical domain path.

pub use crate::domain::preferences_normalize::*;

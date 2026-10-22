//! Craft geometry test helpers: assertions and proptest strategies over `craft-geom`.
//!
//! Dev-only (`publish = false`). Promoted from `craft-testkit` with just the parts that
//! depend on nothing above the geometry kernel, so `craft-geom`, `craft-pathops` and
//! `craft-trace` can share property-test infrastructure without pulling in an app stack.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

pub mod geom;
pub mod strategies;

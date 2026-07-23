//! Adapters. The imperative shell.
//!
//! Everything here implements a trait defined in `app`. Nothing here is imported by
//! `app` or `domain` — the dependency arrow points inward only, and `cargo tree`
//! is the proof.

#![cfg_attr(test, allow(clippy::expect_used, clippy::panic))]

pub mod clock;
pub mod db;
pub mod hasher;
pub mod ids;
pub mod repos;
pub mod rows;
pub mod tx;

pub use clock::SystemClock;
pub use db::{Db, connect};
pub use hasher::ScryptHasher;
pub use ids::UuidGen;

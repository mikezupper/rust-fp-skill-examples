//! Test wiring.
//!
//! The same `Ctx` the production binary builds, with two substitutions: an in-memory
//! database and a fixed clock. Nothing else changes — the workflows under test are
//! byte-for-byte the ones that ship, which is the payoff for having pushed every
//! effect behind a trait.

// Each integration-test file is its own crate and compiles this module separately, so
// a helper used by `workflows.rs` but not `http.rs` reads as dead code in the latter.
#![allow(
    dead_code,
    reason = "shared test helpers; each test binary uses a subset"
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use app::Ctx;
use app::ports::{Clock, IdGen};
use chrono::{DateTime, TimeZone, Utc};
use infra::repos::{SqlCartRepo, SqlOrderRepo, SqlProductRepo, SqlSessionRepo, SqlUserRepo};
use infra::tx::SqlDatabase;
use infra::{ScryptHasher, connect};

/// Time stands still unless a test moves it. Every `placed_at` and `created_at` in
/// the suite is therefore predictable, and no assertion has to be written as "within
/// a few seconds of now".
#[derive(Debug)]
pub(crate) struct FixedClock(pub DateTime<Utc>);

impl Default for FixedClock {
    fn default() -> Self {
        Self(
            Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0)
                .single()
                .expect("2026-01-01T12:00:00Z is unambiguous"),
        )
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

/// Sequential ids instead of UUIDs. A failing test names the exact id that broke,
/// and reruns produce the same one.
#[derive(Debug, Default)]
pub(crate) struct SeqIdGen(AtomicU64);

impl IdGen for SeqIdGen {
    fn next(&self) -> String {
        format!("id-{}", self.0.fetch_add(1, Ordering::Relaxed))
    }
}

/// A fresh, isolated database per call. `:memory:` plus a single-connection pool
/// means no test can see another's rows, and no cleanup step can be forgotten.
pub(crate) async fn test_ctx() -> Ctx {
    let db = connect("sqlite::memory:")
        .await
        .expect("in-memory database is always available");

    Ctx {
        users: Arc::new(SqlUserRepo(db.clone())),
        sessions: Arc::new(SqlSessionRepo(db.clone())),
        products: Arc::new(SqlProductRepo(db.clone())),
        carts: Arc::new(SqlCartRepo(db.clone())),
        orders: Arc::new(SqlOrderRepo(db.clone())),
        hasher: Arc::new(ScryptHasher::default()),
        ids: Arc::new(SeqIdGen::default()),
        clock: Arc::new(FixedClock::default()),
        db: Arc::new(SqlDatabase(db)),
    }
}

pub(crate) fn email(raw: &str) -> domain::Email {
    domain::Email::try_new(raw).expect("test fixture email is valid")
}

pub(crate) fn password(raw: &str) -> domain::Password {
    domain::Password::try_from(raw.to_owned()).expect("test fixture password is long enough")
}

pub(crate) fn product_id(raw: &str) -> domain::ProductId {
    domain::ProductId::try_new(raw).expect("test fixture product id is valid")
}

pub(crate) fn qty(n: u32) -> domain::Quantity {
    domain::Quantity::try_new(n).expect("test fixture quantity is in range")
}

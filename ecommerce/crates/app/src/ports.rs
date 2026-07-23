//! Ports: the effects this application is allowed to have.
//!
//! Every trait here is defined by its *consumer*, not its implementor — that is the
//! dependency-inversion arrow that keeps `domain` and `app` free of infrastructure.
//! `infra` implements these; nothing in this crate knows that it exists.
//!
//! ## Why `#[async_trait]` and not native `async fn` in traits
//!
//! Async functions in traits have been stable since Rust 1.75, and for *static*
//! dispatch they are the right default. But AFIT is still not `dyn`-compatible, and
//! this application wires its services as `Arc<dyn Trait>` so that handler signatures
//! stay free of generic parameters and a fake can be swapped in at runtime. Trait
//! objects mean `#[async_trait]` (which boxes the returned future — one allocation
//! per call, irrelevant next to a database round-trip).
//!
//! Reach for native AFIT + `trait-variant` instead when the trait is on a hot path
//! and you can accept generics propagating up through every caller.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::{
    Category, Email, Order, OrderId, Password, PasswordHash, Product, ProductId, Quantity,
    SessionToken, User, UserId,
};

use crate::errors::RepoError;

// ---------------------------------------------------------------------------
// Ambient effects.
//
// Time, identity and randomness are effects, and treating them as such is the
// single highest-leverage application of dependency injection: it turns "wait 30
// minutes and see if the session expired" into a function call. `clippy.toml` bans
// `Utc::now` and `Uuid::new_v4` by path so the shortcut is a build failure.
// ---------------------------------------------------------------------------

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub trait IdGen: Send + Sync {
    fn next(&self) -> String;
}

#[async_trait]
pub trait PasswordHasher: Send + Sync {
    /// Hashing is deliberately expensive, so implementations run it off the async
    /// executor. Fallible because a hasher can be misconfigured — that is a defect,
    /// but one worth surfacing rather than panicking inside a request.
    async fn hash(&self, password: &Password) -> Result<PasswordHash, RepoError>;

    /// Constant-time comparison. Returns a plain `bool`: "the password did not
    /// match" is an expected outcome, not an error.
    async fn verify(&self, password: &Password, stored: &PasswordHash) -> Result<bool, RepoError>;
}

// ---------------------------------------------------------------------------
// Repositories.
//
// Two rules hold for every method below:
//   1. Absence is `Option`, never an error. Whether a missing row is a problem is a
//      decision for the workflow, which has the context; the repository does not.
//   2. Repositories never open a transaction. The workflow owns that boundary.
// ---------------------------------------------------------------------------

#[async_trait]
pub trait UserRepo: Send + Sync {
    async fn find_by_email(&self, email: &Email) -> Result<Option<User>, RepoError>;
    async fn insert(&self, user: &User) -> Result<(), RepoError>;
}

#[async_trait]
pub trait SessionRepo: Send + Sync {
    async fn create(&self, token: &SessionToken, user_id: &UserId) -> Result<(), RepoError>;
    async fn find_user(&self, token: &SessionToken) -> Result<Option<User>, RepoError>;
}

/// Filters arrive as domain types, already parsed. A `None` field means "unfiltered",
/// which is why this is a struct of `Option`s rather than a stringly-typed query.
#[derive(Debug, Clone, Default)]
pub struct ProductFilter {
    pub search: Option<String>,
    pub category_id: Option<domain::CategoryId>,
}

#[async_trait]
pub trait ProductRepo: Send + Sync {
    async fn find_by_id(&self, id: &ProductId) -> Result<Option<Product>, RepoError>;
    async fn list(&self, filter: &ProductFilter) -> Result<Vec<Product>, RepoError>;
    async fn list_categories(&self) -> Result<Vec<Category>, RepoError>;
    async fn find_category_by_slug(&self, slug: &str) -> Result<Option<Category>, RepoError>;
}

#[async_trait]
pub trait CartRepo: Send + Sync {
    async fn items(&self, user_id: &UserId) -> Result<Vec<(Product, Quantity)>, RepoError>;
    async fn set_item(
        &self,
        user_id: &UserId,
        product_id: &ProductId,
        quantity: Quantity,
    ) -> Result<(), RepoError>;
    async fn remove_item(&self, user_id: &UserId, product_id: &ProductId) -> Result<(), RepoError>;
}

#[async_trait]
pub trait OrderRepo: Send + Sync {
    async fn list_by_user(&self, user_id: &UserId) -> Result<Vec<Order>, RepoError>;
    async fn find_by_id(
        &self,
        user_id: &UserId,
        order_id: &OrderId,
    ) -> Result<Option<Order>, RepoError>;
}

// ---------------------------------------------------------------------------
// The transaction boundary.
//
// This is the interesting port. Checkout must read the cart, reserve stock, write an
// order and clear the cart *atomically*, so it needs a transaction — but `app` is
// not allowed to know what a `sqlx::Transaction` is.
//
// The resolution: `Database::begin` hands back a trait object that owns the real
// transaction, exposing only the operations checkout needs. The workflow drives it
// and calls `commit`. If the workflow returns early via `?`, the box is dropped
// without `commit` ever being called, and the adapter's `Drop` rolls back.
//
// That last sentence is the load-bearing one: Rust's ownership semantics and the
// early-exit of `?` line up exactly, so "every failure path rolls back" is enforced
// by the language rather than remembered by the programmer.
// ---------------------------------------------------------------------------

/// The result of attempting to reserve stock.
///
/// A `bool` would have forced the caller to issue a second query to find out how much
/// stock there actually was — a race as well as a round-trip. Returning the available
/// count with the failure makes the error self-describing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StockReservation {
    Reserved,
    Insufficient { available: u32 },
}

#[async_trait]
pub trait Database: Send + Sync {
    async fn begin(&self) -> Result<Box<dyn CheckoutTx + Send + '_>, RepoError>;
}

#[async_trait]
pub trait CheckoutTx: Send {
    async fn cart_items(&mut self, user_id: &UserId)
    -> Result<Vec<(Product, Quantity)>, RepoError>;

    /// Atomic conditional decrement: succeeds only if enough stock remains, in a
    /// single statement. Read-then-write would be a lost-update race under concurrent
    /// checkouts no matter how tight the transaction is.
    async fn try_reserve_stock(
        &mut self,
        product_id: &ProductId,
        quantity: Quantity,
    ) -> Result<StockReservation, RepoError>;

    async fn insert_order(&mut self, order: &Order) -> Result<(), RepoError>;
    async fn clear_cart(&mut self, user_id: &UserId) -> Result<(), RepoError>;

    /// Takes `Box<Self>` so committing consumes the transaction: using it afterwards
    /// is a compile error, not a runtime one.
    async fn commit(self: Box<Self>) -> Result<(), RepoError>;
}

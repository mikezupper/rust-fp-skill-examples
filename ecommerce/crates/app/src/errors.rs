//! The error taxonomy.
//!
//! One enum per use-case, not one enum for the application. A god-enum forces every
//! caller to handle failures that its operation cannot produce, and makes the `E` in
//! a signature carry no information. `CheckoutError` tells you exactly what checkout
//! can do; `RegisterError` tells you exactly what registration can do.
//!
//! Every variant carries the data a handler needs to react — an id, the offending
//! value, the available stock — never a bare message. A message is for a human; a
//! field is for the code that has to decide what to do next.

use std::error::Error as StdError;

use domain::{Email, OrderId, ProductId};

/// Infrastructure failure, already translated out of whatever the adapter used.
///
/// This is the only place a `sqlx::Error` (or an HTTP client error, or an IO error)
/// is allowed to end up, and it is erased on the way in: no workflow signature ever
/// mentions a database type, so swapping SQLite for Postgres cannot ripple upward.
#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    /// The store could not be reached or the query failed. Not a domain outcome —
    /// nothing the user did caused it and nothing they can do will fix it.
    #[error("storage unavailable")]
    Unavailable(#[source] Box<dyn StdError + Send + Sync>),

    /// A row exists but does not satisfy the domain's invariants. This is a *defect*
    /// in disguise: the schema and the domain model are owned by the same team, so a
    /// mismatch means a migration or a deploy is wrong. It is surfaced as an error
    /// rather than a panic so one bad row cannot take the process down.
    #[error("stored data violates a domain invariant: {0}")]
    Corrupt(String),
}

#[derive(Debug, thiserror::Error)]
pub enum RegisterError {
    #[error("email {email} is already registered")]
    EmailTaken { email: Email },
    #[error(transparent)]
    Repo(#[from] RepoError),
}

#[derive(Debug, thiserror::Error)]
pub enum LoginError {
    /// Deliberately identical for "no such user" and "wrong password". Two distinct
    /// errors would turn the login endpoint into an account-enumeration oracle.
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error(transparent)]
    Repo(#[from] RepoError),
}

#[derive(Debug, thiserror::Error)]
pub enum ProductError {
    #[error("product {product_id} not found")]
    NotFound { product_id: ProductId },
    #[error(transparent)]
    Repo(#[from] RepoError),
}

#[derive(Debug, thiserror::Error)]
pub enum CartError {
    #[error("product {product_id} not found")]
    ProductNotFound { product_id: ProductId },
    #[error(transparent)]
    Repo(#[from] RepoError),
}

#[derive(Debug, thiserror::Error)]
pub enum CheckoutError {
    #[error("cannot check out an empty cart")]
    CartEmpty,
    /// Carries `available` so the client can render "only 3 left" without a second
    /// round-trip. This is the difference between an error that is data and an error
    /// that is a string.
    #[error("insufficient stock for {product_id}: requested {requested}, {available} available")]
    InsufficientStock {
        product_id: ProductId,
        requested: u32,
        available: u32,
    },
    #[error(transparent)]
    Repo(#[from] RepoError),
}

#[derive(Debug, thiserror::Error)]
pub enum OrderLookupError {
    #[error("order {order_id} not found")]
    NotFound { order_id: OrderId },
    #[error(transparent)]
    Repo(#[from] RepoError),
}

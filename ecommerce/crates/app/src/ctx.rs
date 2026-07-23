//! The application context — Rust's practical substitute for a Reader monad.
//!
//! Rust has no Reader monad and building one drowns in lifetimes. What actually
//! works is boring: bundle the capabilities into one struct, pass it by reference,
//! and let the single wiring site in `main` decide what goes in it.
//!
//! ## On capability traits
//!
//! Workflows here take `&Ctx`, which means a workflow's signature does not advertise
//! which capabilities it actually uses. The stricter alternative is a capability
//! trait per port:
//!
//! ```ignore
//! pub trait HasClock { fn clock(&self) -> &dyn Clock; }
//! pub trait HasUsers { fn users(&self) -> &dyn UserRepo; }
//!
//! pub async fn register(ctx: &(impl HasUsers + HasClock + HasIds), ..) -> ..
//! ```
//!
//! That is the closest Rust gets to a typed requirements channel: the signature
//! becomes a precise statement of what the function may touch, and a test can pass a
//! context that supplies only those. The cost is a trait and an impl per port, and
//! `impl A + B + C + D` bounds on every workflow.
//!
//! For an application this size the bookkeeping outweighs the benefit, so `Ctx` is
//! concrete. Reach for capability traits when workflows outnumber ports, or when you
//! genuinely need to prove a workflow cannot reach a particular dependency.

use std::sync::Arc;

use crate::ports::{
    CartRepo, Clock, Database, IdGen, OrderRepo, PasswordHasher, ProductRepo, SessionRepo, UserRepo,
};

/// Every effect the application is permitted to have, in one place.
///
/// All fields are `Arc<dyn _>`: dynamic dispatch costs one vtable indirection, which
/// is unmeasurable next to a database round-trip, and buys signatures with no generic
/// parameters plus the ability to substitute a fake at runtime.
#[derive(Clone)]
pub struct Ctx {
    pub users: Arc<dyn UserRepo>,
    pub sessions: Arc<dyn SessionRepo>,
    pub products: Arc<dyn ProductRepo>,
    pub carts: Arc<dyn CartRepo>,
    pub orders: Arc<dyn OrderRepo>,
    pub hasher: Arc<dyn PasswordHasher>,
    pub ids: Arc<dyn IdGen>,
    pub clock: Arc<dyn Clock>,
    pub db: Arc<dyn Database>,
}

/// Hand-written because `dyn Trait` has no `Debug`, and the workspace warns on
/// missing `Debug` impls. Printing the field names is all that would be useful
/// anyway — the values are behaviour, not data.
impl std::fmt::Debug for Ctx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ctx").finish_non_exhaustive()
    }
}

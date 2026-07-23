//! Server state: the application context, plus nothing else.

use app::Ctx;

/// Cheap to clone — every field inside `Ctx` is an `Arc`, so cloning per request
/// bumps refcounts rather than copying anything.
#[derive(Clone, Debug)]
pub struct AppState {
    pub ctx: Ctx,
}

impl AppState {
    #[must_use]
    pub fn new(ctx: Ctx) -> Self {
        Self { ctx }
    }
}

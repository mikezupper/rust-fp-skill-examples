//! The HTTP adapter.
//!
//! Handlers are switchyards: decode the request into domain types, call one workflow,
//! render the result. There is no business logic in this crate — if a handler ever
//! needs an `if`, the decision belongs in a workflow where it can be tested without
//! a socket.

pub mod auth;
pub mod dto;
pub mod error;
pub mod routes;
pub mod state;

pub use error::ApiError;
pub use routes::router;
pub use state::AppState;

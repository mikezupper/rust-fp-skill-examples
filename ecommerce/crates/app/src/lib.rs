//! Use-cases: the railway.
//!
//! Every workflow is a `Result`-returning function over domain types and *ports*
//! (traits). Effects arrive as parameters; nothing here constructs one. That is what
//! lets every workflow be tested against in-memory fakes with no infrastructure, and
//! it is why the transaction boundary lives here rather than being smeared across
//! repositories.

pub mod ctx;
pub mod errors;
pub mod ports;
pub mod workflows;

pub use ctx::Ctx;
pub use errors::{
    CartError, CheckoutError, LoginError, OrderLookupError, ProductError, RegisterError, RepoError,
};

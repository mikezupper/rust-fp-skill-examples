//! The functional core.
//!
//! Pure functions over immutable data. No I/O, no clock, no randomness, no async.
//! Every type in here is either impossible to construct in an invalid state, or
//! constructible only through a fallible parser that rejects invalid input.
//!
//! Nothing in this crate can perform an effect, because nothing it depends on can.

// The deny tier (`unwrap_used`, `expect_used`, `panic`) is relaxed for test code only.
// In a test, an `expect` on a fixture is an assertion: if `ProductId::try_new("p-1")`
// ever fails, the test itself is wrong and a panic is the correct way to say so.
// Scoped with `cfg_attr(test, ..)` so the relaxation cannot leak into shipped code.
#![cfg_attr(test, allow(clippy::expect_used, clippy::panic))]

pub mod cart;
pub mod catalog;
pub mod money;
pub mod order;
pub mod user;

pub use cart::{CartLine, CartView, Quantity, line_total, make_cart_view};
pub use catalog::{
    Category, CategoryId, CategoryTree, DisplayName, Product, ProductId, Sku, Slug,
    build_category_tree,
};
pub use money::Cents;
pub use order::{NonEmpty, Order, OrderId, OrderLine, order_total, to_order_lines};
pub use user::{Email, Password, PasswordHash, SessionToken, User, UserId};

//! Checkout: the transaction boundary of the application, and the reason this
//! architecture is shaped the way it is.

use domain::{CartLine, NonEmpty, Order, OrderId, UserId, make_cart_view, to_order_lines};

use crate::ctx::Ctx;
use crate::errors::{CheckoutError, OrderLookupError, RepoError};
use crate::ports::StockReservation;

/// Read cart -> reserve stock -> write order -> clear cart, atomically.
///
/// # Why the boundary is here
///
/// The workflow opens the transaction because the workflow is the only layer that
/// knows these four operations form one unit of work. A repository that opened its
/// own transaction would make that impossible; a handler that opened one would make
/// the domain rule depend on the transport.
///
/// # How rollback happens
///
/// It is not written down anywhere. Every `?` below returns early, which drops `tx`
/// without calling `commit`, and the adapter's `Drop` issues a `ROLLBACK`. Ownership
/// and railway-oriented early exit are the same mechanism here: **the failure path is
/// correct because it is the path where nothing was committed**, and the compiler is
/// what guarantees a dropped transaction was not committed.
///
/// This is the one place where Rust does something no garbage-collected functional
/// language can: the rollback is structural, not remembered.
#[tracing::instrument(skip(ctx), fields(user_id = %user_id), err)]
pub async fn checkout(ctx: &Ctx, user_id: &UserId) -> Result<Order, CheckoutError> {
    let mut tx = ctx.db.begin().await?;

    let items = tx.cart_items(user_id).await?;
    let cart = make_cart_view(&items); // pure pricing

    // An empty cart is not an order with zero lines — it is not an order at all.
    // `NonEmpty` makes the difference a type error rather than a runtime check that
    // someone might forget downstream.
    let lines: NonEmpty<CartLine> =
        NonEmpty::try_from(cart.lines).map_err(|domain::order::Empty| CheckoutError::CartEmpty)?;

    // Sequential, not concurrent: these statements share one connection, and stock
    // reservation is exactly the kind of contended write that must not interleave.
    // Bounded concurrency is the default elsewhere; here the bound is one.
    for line in &lines {
        let reservation = tx
            .try_reserve_stock(&line.product.id, line.quantity)
            .await?;

        match reservation {
            StockReservation::Reserved => {}
            StockReservation::Insufficient { available } => {
                // Returning here drops `tx`. Every reservation already made in this
                // loop is rolled back by the database — including the ones that
                // succeeded a moment ago. No compensating action to write, none to
                // forget. The atomicity test asserts exactly this.
                return Err(CheckoutError::InsufficientStock {
                    product_id: line.product.id.clone(),
                    requested: line.quantity.into_inner(),
                    available,
                });
            }
        }
    }

    let order = Order::place(
        OrderId::try_new(ctx.ids.next())
            .map_err(|e| RepoError::Corrupt(format!("id generator produced garbage: {e}")))?,
        user_id.clone(),
        to_order_lines(&lines), // pure snapshot of name + price at purchase time
        ctx.clock.now(),
    );

    tx.insert_order(&order).await?;
    tx.clear_cart(user_id).await?;
    tx.commit().await?;

    tracing::info!(
        order_id = %order.id,
        total_cents = order.total_cents.get(),
        lines = order.lines.len(),
        "order placed"
    );

    Ok(order)
}

#[tracing::instrument(skip(ctx), fields(user_id = %user_id), err)]
pub async fn order_history(ctx: &Ctx, user_id: &UserId) -> Result<Vec<Order>, RepoError> {
    ctx.orders.list_by_user(user_id).await
}

#[tracing::instrument(skip(ctx), fields(user_id = %user_id, order_id = %order_id), err)]
pub async fn get_order(
    ctx: &Ctx,
    user_id: &UserId,
    order_id: &OrderId,
) -> Result<Order, OrderLookupError> {
    // Scoped by `user_id`, so another user's order id returns NotFound rather than
    // Forbidden — an authorization check expressed as a query, which cannot be
    // bypassed by forgetting to call it.
    ctx.orders
        .find_by_id(user_id, order_id)
        .await?
        .ok_or_else(|| OrderLookupError::NotFound {
            order_id: order_id.clone(),
        })
}

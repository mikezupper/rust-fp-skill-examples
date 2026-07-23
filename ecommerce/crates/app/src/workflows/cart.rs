//! Cart mutation. Each operation returns the freshly priced cart so the client never
//! has to guess at the new total.

use domain::{CartView, ProductId, Quantity, UserId, make_cart_view};

use crate::ctx::Ctx;
use crate::errors::{CartError, RepoError};

#[tracing::instrument(skip(ctx), fields(user_id = %user_id), err)]
pub async fn get_cart(ctx: &Ctx, user_id: &UserId) -> Result<CartView, RepoError> {
    let items = ctx.carts.items(user_id).await?;
    // Fetch, then price with a pure function. All the arithmetic that could be wrong
    // lives in `domain` where it is property-tested; this function only moves data.
    Ok(make_cart_view(&items))
}

#[tracing::instrument(skip(ctx), fields(user_id = %user_id, product_id = %product_id), err)]
pub async fn set_cart_item(
    ctx: &Ctx,
    user_id: &UserId,
    product_id: &ProductId,
    quantity: Quantity,
) -> Result<CartView, CartError> {
    // Referential integrity is checked before the write. Note what is *not* validated
    // here: that `quantity` is between 1 and 99. `Quantity` cannot hold anything else,
    // so there is nothing to check — the parse happened at the HTTP boundary and the
    // type has carried the proof ever since.
    if ctx.products.find_by_id(product_id).await?.is_none() {
        return Err(CartError::ProductNotFound {
            product_id: product_id.clone(),
        });
    }

    ctx.carts.set_item(user_id, product_id, quantity).await?;
    get_cart(ctx, user_id).await.map_err(CartError::Repo)
}

#[tracing::instrument(skip(ctx), fields(user_id = %user_id, product_id = %product_id), err)]
pub async fn remove_cart_item(
    ctx: &Ctx,
    user_id: &UserId,
    product_id: &ProductId,
) -> Result<CartView, RepoError> {
    // Removing an absent item is a no-op, not an error: the caller's intent ("this
    // should not be in my cart") is already satisfied. Idempotent by design.
    ctx.carts.remove_item(user_id, product_id).await?;
    get_cart(ctx, user_id).await
}

//! Routing and handlers.
//!
//! Every handler below is the same four lines: take already-parsed input, call one
//! workflow, map the result. The `?` operator plus the `From` impls in `error.rs` do
//! the error-track work, so no handler contains a `match` on a failure.

use app::workflows::{auth, cart, catalog, orders};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use domain::{CartView, CategoryTree, Order, OrderId, Product, ProductId};

use crate::auth::CurrentUser;
use crate::dto::{AuthSessionBody, BrowseParams, Credentials, SetCartItem, ValidJson};
use crate::error::ApiError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/catalog/categories", get(categories))
        .route("/catalog/products", get(products))
        .route("/catalog/products/{id}", get(product))
        .route("/cart", get(get_cart))
        .route("/cart/items", put(set_cart_item))
        .route("/cart/items/{product_id}", delete(remove_cart_item))
        .route("/orders", post(checkout).get(order_history))
        .route("/orders/{id}", get(get_order))
        .with_state(state)
}

async fn health() -> StatusCode {
    StatusCode::OK
}

// ---------------------------------------------------------------------------
// auth
// ---------------------------------------------------------------------------

async fn register(
    State(state): State<AppState>,
    ValidJson(body): ValidJson<Credentials>,
) -> Result<(StatusCode, Json<AuthSessionBody>), ApiError> {
    let session = auth::register(&state.ctx, body.email, &body.password).await?;
    Ok((StatusCode::CREATED, Json(session.into())))
}

async fn login(
    State(state): State<AppState>,
    ValidJson(body): ValidJson<Credentials>,
) -> Result<Json<AuthSessionBody>, ApiError> {
    let session = auth::login(&state.ctx, &body.email, &body.password).await?;
    Ok(Json(session.into()))
}

// ---------------------------------------------------------------------------
// catalog (public)
// ---------------------------------------------------------------------------

async fn categories(State(state): State<AppState>) -> Result<Json<Vec<CategoryTree>>, ApiError> {
    Ok(Json(catalog::category_navigation(&state.ctx).await?))
}

async fn products(
    State(state): State<AppState>,
    Query(params): Query<BrowseParams>,
) -> Result<Json<Vec<Product>>, ApiError> {
    let query = catalog::BrowseQuery {
        search: params.search,
        category_slug: params.category,
    };
    Ok(Json(catalog::browse_products(&state.ctx, &query).await?))
}

async fn product(
    State(state): State<AppState>,
    Path(id): Path<ProductId>,
) -> Result<Json<Product>, ApiError> {
    Ok(Json(catalog::get_product(&state.ctx, &id).await?))
}

// ---------------------------------------------------------------------------
// cart (authenticated)
//
// `CurrentUser` in the argument list is the authorization check. There is no way to
// write one of these handlers that skips it, because the user id has to come from
// somewhere and this is the only place it comes from.
// ---------------------------------------------------------------------------

async fn get_cart(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<CartView>, ApiError> {
    Ok(Json(cart::get_cart(&state.ctx, &user.user_id).await?))
}

async fn set_cart_item(
    State(state): State<AppState>,
    user: CurrentUser,
    ValidJson(body): ValidJson<SetCartItem>,
) -> Result<Json<CartView>, ApiError> {
    let view =
        cart::set_cart_item(&state.ctx, &user.user_id, &body.product_id, body.quantity).await?;
    Ok(Json(view))
}

async fn remove_cart_item(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(product_id): Path<ProductId>,
) -> Result<Json<CartView>, ApiError> {
    let view = cart::remove_cart_item(&state.ctx, &user.user_id, &product_id).await?;
    Ok(Json(view))
}

// ---------------------------------------------------------------------------
// orders (authenticated)
// ---------------------------------------------------------------------------

async fn checkout(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<(StatusCode, Json<Order>), ApiError> {
    let order = orders::checkout(&state.ctx, &user.user_id).await?;
    Ok((StatusCode::CREATED, Json(order)))
}

async fn order_history(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<Vec<Order>>, ApiError> {
    Ok(Json(
        orders::order_history(&state.ctx, &user.user_id).await?,
    ))
}

async fn get_order(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<OrderId>,
) -> Result<Json<Order>, ApiError> {
    Ok(Json(
        orders::get_order(&state.ctx, &user.user_id, &id).await?,
    ))
}

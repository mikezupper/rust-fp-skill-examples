//! The anti-corruption layer.
//!
//! A database is an untrusted boundary like any other. It holds flat, nullable
//! primitives; the domain holds newtypes, non-empty collections and derived totals.
//! Trying to make `sqlx` construct domain types directly means either weakening the
//! domain until it matches the schema, or fighting derive macros — so instead every
//! table gets a dumb `*Row` struct that mirrors it exactly, plus a `TryFrom` that is
//! the only way across.
//!
//! The payoff: a schema change breaks `TryFrom`, one obvious place, instead of
//! silently producing a domain value that violates an invariant.

use app::RepoError;
use chrono::{DateTime, Utc};
use domain::order::NonEmpty;
use domain::{
    CartLine, Category, CategoryId, Cents, DisplayName, Order, OrderId, OrderLine, Product,
    ProductId, Quantity, Sku, Slug, User, UserId,
};

/// Every parse failure at this boundary becomes `RepoError::Corrupt`, carrying the
/// column that failed. A row that cannot become a domain value means the schema and
/// the model have drifted — a deploy bug, surfaced loudly rather than papered over
/// with a default.
fn corrupt(field: &str, err: impl std::fmt::Display) -> RepoError {
    RepoError::Corrupt(format!("{field}: {err}"))
}

#[derive(Debug, sqlx::FromRow)]
pub struct ProductRow {
    pub id: String,
    pub sku: String,
    pub name: String,
    pub description: String,
    pub price_cents: i64,
    pub category_id: String,
    pub stock: i64,
}

impl TryFrom<ProductRow> for Product {
    type Error = RepoError;

    fn try_from(row: ProductRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: ProductId::try_new(row.id).map_err(|e| corrupt("products.id", e))?,
            sku: Sku::try_new(row.sku).map_err(|e| corrupt("products.sku", e))?,
            name: DisplayName::try_new(row.name).map_err(|e| corrupt("products.name", e))?,
            description: row.description,
            // The database column is a signed 64-bit integer and therefore can hold
            // -1. `Cents` cannot. This conversion is where that gap is closed.
            price_cents: Cents::try_from_i64(row.price_cents)
                .map_err(|e| corrupt("products.price_cents", e))?,
            category_id: CategoryId::try_new(row.category_id)
                .map_err(|e| corrupt("products.category_id", e))?,
            stock: u32::try_from(row.stock).map_err(|e| corrupt("products.stock", e))?,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct CategoryRow {
    pub id: String,
    pub name: String,
    pub slug: String,
    /// Nullable in the schema, `Option` here, `Option` in the domain. The three agree
    /// because the row type is allowed to be as loose as the table actually is.
    pub parent_id: Option<String>,
}

impl TryFrom<CategoryRow> for Category {
    type Error = RepoError;

    fn try_from(row: CategoryRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: CategoryId::try_new(row.id).map_err(|e| corrupt("categories.id", e))?,
            name: DisplayName::try_new(row.name).map_err(|e| corrupt("categories.name", e))?,
            slug: Slug::try_new(row.slug).map_err(|e| corrupt("categories.slug", e))?,
            // `transpose` turns Option<Result<_>> into Result<Option<_>> so the `?`
            // still short-circuits — the railway runs through the optional field.
            parent_id: row
                .parent_id
                .map(CategoryId::try_new)
                .transpose()
                .map_err(|e| corrupt("categories.parent_id", e))?,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct UserRow {
    pub id: String,
    pub email: String,
    pub password_hash: String,
    pub created_at: DateTime<Utc>,
}

impl TryFrom<UserRow> for User {
    type Error = RepoError;

    fn try_from(row: UserRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: UserId::try_new(row.id).map_err(|e| corrupt("users.id", e))?,
            email: domain::Email::try_new(row.email).map_err(|e| corrupt("users.email", e))?,
            password_hash: domain::PasswordHash::try_new(row.password_hash)
                .map_err(|e| corrupt("users.password_hash", e))?,
            created_at: row.created_at,
        })
    }
}

/// A cart row is a product joined to a quantity — a flat shape that matches neither
/// table exactly. Row structs are free to be query-shaped rather than table-shaped.
#[derive(Debug, sqlx::FromRow)]
pub struct CartItemRow {
    #[sqlx(flatten)]
    pub product: ProductRow,
    pub quantity: i64,
}

impl TryFrom<CartItemRow> for (Product, Quantity) {
    type Error = RepoError;

    fn try_from(row: CartItemRow) -> Result<Self, Self::Error> {
        let quantity = u32::try_from(row.quantity)
            .map_err(|e| corrupt("cart_items.quantity", e))
            .and_then(|q| Quantity::try_new(q).map_err(|e| corrupt("cart_items.quantity", e)))?;
        Ok((Product::try_from(row.product)?, quantity))
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct OrderRow {
    pub id: String,
    pub user_id: String,
    pub total_cents: i64,
    pub placed_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct OrderLineRow {
    pub order_id: String,
    pub product_id: String,
    pub name: String,
    pub unit_price_cents: i64,
    pub quantity: i64,
}

impl TryFrom<OrderLineRow> for OrderLine {
    type Error = RepoError;

    fn try_from(row: OrderLineRow) -> Result<Self, Self::Error> {
        Ok(Self {
            product_id: ProductId::try_new(row.product_id)
                .map_err(|e| corrupt("order_lines.product_id", e))?,
            name: DisplayName::try_new(row.name).map_err(|e| corrupt("order_lines.name", e))?,
            unit_price_cents: Cents::try_from_i64(row.unit_price_cents)
                .map_err(|e| corrupt("order_lines.unit_price_cents", e))?,
            quantity: u32::try_from(row.quantity)
                .map_err(|e| corrupt("order_lines.quantity", e))
                .and_then(|q| {
                    Quantity::try_new(q).map_err(|e| corrupt("order_lines.quantity", e))
                })?,
        })
    }
}

/// Reassembles orders from two flat result sets.
///
/// Two queries plus an in-memory group-by, not one query per order: N+1 is a
/// performance bug that hides behind a clean-looking repository method. The grouping
/// itself is pure, so it costs nothing to reason about.
pub fn assemble_orders(
    order_rows: Vec<OrderRow>,
    line_rows: Vec<OrderLineRow>,
) -> Result<Vec<Order>, RepoError> {
    // Traverse: the first bad line row shunts the whole assembly to the error track.
    let lines: Vec<(String, OrderLine)> = line_rows
        .into_iter()
        .map(|row| {
            let order_id = row.order_id.clone();
            OrderLine::try_from(row).map(|line| (order_id, line))
        })
        .collect::<Result<Vec<_>, RepoError>>()?;

    order_rows
        .into_iter()
        .map(|row| {
            let own_lines: Vec<OrderLine> = lines
                .iter()
                .filter(|(order_id, _)| *order_id == row.id)
                .map(|(_, line)| line.clone())
                .collect();

            // An order row with no line rows is a violated invariant, not an empty
            // order: `Order` cannot hold zero lines, so there is no way to represent
            // what the database is claiming. That is corruption, and it says so.
            let lines = NonEmpty::try_from(own_lines)
                .map_err(|_| RepoError::Corrupt(format!("order {} has no lines", row.id)))?;

            Ok(Order {
                id: OrderId::try_new(row.id).map_err(|e| corrupt("orders.id", e))?,
                user_id: UserId::try_new(row.user_id).map_err(|e| corrupt("orders.user_id", e))?,
                total_cents: Cents::try_from_i64(row.total_cents)
                    .map_err(|e| corrupt("orders.total_cents", e))?,
                lines,
                placed_at: row.placed_at,
            })
        })
        .collect()
}

/// Kept next to the row types because it is the same concern in the other direction:
/// domain value out, primitive in.
#[must_use]
pub fn cart_line_quantity(line: &CartLine) -> i64 {
    i64::from(line.quantity.into_inner())
}

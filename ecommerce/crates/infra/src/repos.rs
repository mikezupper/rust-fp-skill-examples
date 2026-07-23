//! Repository adapters.
//!
//! Every method here does three things and nothing else: run a query, translate the
//! error, parse the rows into domain types. No business decisions, no transactions,
//! no "if this is missing then that is an error" — those belong to the workflow.

use app::RepoError;
use app::ports::{CartRepo, OrderRepo, ProductFilter, ProductRepo, SessionRepo, UserRepo};
use async_trait::async_trait;
use domain::{
    Category, Email, Order, OrderId, Product, ProductId, Quantity, SessionToken, User, UserId,
};

use crate::db::{Db, to_repo_error};
use crate::rows::{
    CartItemRow, CategoryRow, OrderLineRow, OrderRow, ProductRow, UserRow, assemble_orders,
};

#[derive(Debug, Clone)]
pub struct SqlUserRepo(pub Db);

#[async_trait]
impl UserRepo for SqlUserRepo {
    async fn find_by_email(&self, email: &Email) -> Result<Option<User>, RepoError> {
        let row: Option<UserRow> = sqlx::query_as("SELECT * FROM users WHERE email = ?")
            .bind(email.as_ref())
            .fetch_optional(self.0.pool())
            .await
            .map_err(to_repo_error)?;

        // `Option<Row> -> Option<Domain>` without unwrapping: `map` + `transpose`
        // keeps the railway intact through the optional value.
        row.map(User::try_from).transpose()
    }

    async fn insert(&self, user: &User) -> Result<(), RepoError> {
        sqlx::query("INSERT INTO users (id, email, password_hash, created_at) VALUES (?, ?, ?, ?)")
            .bind(user.id.as_ref())
            .bind(user.email.as_ref())
            .bind(user.password_hash.as_ref())
            .bind(user.created_at)
            .execute(self.0.pool())
            .await
            .map_err(to_repo_error)?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SqlSessionRepo(pub Db);

#[async_trait]
impl SessionRepo for SqlSessionRepo {
    async fn create(&self, token: &SessionToken, user_id: &UserId) -> Result<(), RepoError> {
        sqlx::query("INSERT INTO sessions (token, user_id, created_at) VALUES (?, ?, ?)")
            .bind(token.as_ref())
            .bind(user_id.as_ref())
            .bind(chrono::DateTime::UNIX_EPOCH)
            .execute(self.0.pool())
            .await
            .map_err(to_repo_error)?;
        Ok(())
    }

    async fn find_user(&self, token: &SessionToken) -> Result<Option<User>, RepoError> {
        let row: Option<UserRow> = sqlx::query_as(
            "SELECT u.* FROM users u JOIN sessions s ON s.user_id = u.id WHERE s.token = ?",
        )
        .bind(token.as_ref())
        .fetch_optional(self.0.pool())
        .await
        .map_err(to_repo_error)?;

        row.map(User::try_from).transpose()
    }
}

#[derive(Debug, Clone)]
pub struct SqlProductRepo(pub Db);

#[async_trait]
impl ProductRepo for SqlProductRepo {
    async fn find_by_id(&self, id: &ProductId) -> Result<Option<Product>, RepoError> {
        let row: Option<ProductRow> = sqlx::query_as("SELECT * FROM products WHERE id = ?")
            .bind(id.as_ref())
            .fetch_optional(self.0.pool())
            .await
            .map_err(to_repo_error)?;

        row.map(Product::try_from).transpose()
    }

    async fn list(&self, filter: &ProductFilter) -> Result<Vec<Product>, RepoError> {
        // Static SQL with nullable bind parameters rather than string concatenation:
        // there is no code path here that can produce an injection, because there is
        // no code path that builds SQL from input.
        let rows: Vec<ProductRow> = sqlx::query_as(
            "SELECT * FROM products
             WHERE (?1 IS NULL OR category_id = ?1)
               AND (?2 IS NULL OR name LIKE '%' || ?2 || '%')
             ORDER BY name",
        )
        .bind(filter.category_id.as_ref().map(AsRef::as_ref))
        .bind(filter.search.as_deref())
        .fetch_all(self.0.pool())
        .await
        .map_err(to_repo_error)?;

        // Traverse: `collect::<Result<Vec<_>, _>>()` runs every row down the railway
        // and short-circuits on the first that fails to parse.
        rows.into_iter().map(Product::try_from).collect()
    }

    async fn list_categories(&self) -> Result<Vec<Category>, RepoError> {
        let rows: Vec<CategoryRow> = sqlx::query_as("SELECT * FROM categories ORDER BY name")
            .fetch_all(self.0.pool())
            .await
            .map_err(to_repo_error)?;

        rows.into_iter().map(Category::try_from).collect()
    }

    async fn find_category_by_slug(&self, slug: &str) -> Result<Option<Category>, RepoError> {
        let row: Option<CategoryRow> = sqlx::query_as("SELECT * FROM categories WHERE slug = ?")
            .bind(slug)
            .fetch_optional(self.0.pool())
            .await
            .map_err(to_repo_error)?;

        row.map(Category::try_from).transpose()
    }
}

#[derive(Debug, Clone)]
pub struct SqlCartRepo(pub Db);

#[async_trait]
impl CartRepo for SqlCartRepo {
    async fn items(&self, user_id: &UserId) -> Result<Vec<(Product, Quantity)>, RepoError> {
        let rows: Vec<CartItemRow> = sqlx::query_as(
            "SELECT p.*, ci.quantity FROM cart_items ci
             JOIN products p ON p.id = ci.product_id
             WHERE ci.user_id = ? ORDER BY p.name",
        )
        .bind(user_id.as_ref())
        .fetch_all(self.0.pool())
        .await
        .map_err(to_repo_error)?;

        rows.into_iter().map(TryFrom::try_from).collect()
    }

    async fn set_item(
        &self,
        user_id: &UserId,
        product_id: &ProductId,
        quantity: Quantity,
    ) -> Result<(), RepoError> {
        // Upsert: setting a quantity is idempotent, which is what "PUT" promises.
        sqlx::query(
            "INSERT INTO cart_items (user_id, product_id, quantity) VALUES (?, ?, ?)
             ON CONFLICT (user_id, product_id) DO UPDATE SET quantity = excluded.quantity",
        )
        .bind(user_id.as_ref())
        .bind(product_id.as_ref())
        .bind(i64::from(quantity.into_inner()))
        .execute(self.0.pool())
        .await
        .map_err(to_repo_error)?;

        Ok(())
    }

    async fn remove_item(&self, user_id: &UserId, product_id: &ProductId) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM cart_items WHERE user_id = ? AND product_id = ?")
            .bind(user_id.as_ref())
            .bind(product_id.as_ref())
            .execute(self.0.pool())
            .await
            .map_err(to_repo_error)?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SqlOrderRepo(pub Db);

#[async_trait]
impl OrderRepo for SqlOrderRepo {
    async fn list_by_user(&self, user_id: &UserId) -> Result<Vec<Order>, RepoError> {
        let orders: Vec<OrderRow> =
            sqlx::query_as("SELECT * FROM orders WHERE user_id = ? ORDER BY placed_at DESC")
                .bind(user_id.as_ref())
                .fetch_all(self.0.pool())
                .await
                .map_err(to_repo_error)?;

        if orders.is_empty() {
            return Ok(Vec::new());
        }

        // Two queries, then a pure group-by — not one query per order.
        let lines: Vec<OrderLineRow> = sqlx::query_as(
            "SELECT l.* FROM order_lines l
             JOIN orders o ON o.id = l.order_id
             WHERE o.user_id = ?",
        )
        .bind(user_id.as_ref())
        .fetch_all(self.0.pool())
        .await
        .map_err(to_repo_error)?;

        assemble_orders(orders, lines)
    }

    async fn find_by_id(
        &self,
        user_id: &UserId,
        order_id: &OrderId,
    ) -> Result<Option<Order>, RepoError> {
        let orders: Vec<OrderRow> =
            sqlx::query_as("SELECT * FROM orders WHERE id = ? AND user_id = ?")
                .bind(order_id.as_ref())
                .bind(user_id.as_ref())
                .fetch_all(self.0.pool())
                .await
                .map_err(to_repo_error)?;

        if orders.is_empty() {
            return Ok(None);
        }

        let lines: Vec<OrderLineRow> =
            sqlx::query_as("SELECT * FROM order_lines WHERE order_id = ?")
                .bind(order_id.as_ref())
                .fetch_all(self.0.pool())
                .await
                .map_err(to_repo_error)?;

        Ok(assemble_orders(orders, lines)?.into_iter().next())
    }
}

//! The transaction adapter — where "every failure rolls back" stops being a promise
//! and becomes a property of the type system.

use app::RepoError;
use app::ports::{CheckoutTx, Database, StockReservation};
use async_trait::async_trait;
use domain::{Order, Product, ProductId, Quantity, UserId};
use sqlx::{Row, Sqlite, Transaction};

use crate::db::{Db, to_repo_error};
use crate::rows::CartItemRow;

#[derive(Debug, Clone)]
pub struct SqlDatabase(pub Db);

#[async_trait]
impl Database for SqlDatabase {
    async fn begin(&self) -> Result<Box<dyn CheckoutTx + Send + '_>, RepoError> {
        let tx = self.0.pool().begin().await.map_err(to_repo_error)?;
        Ok(Box::new(SqliteCheckoutTx { tx }))
    }
}

/// Owns a live `sqlx::Transaction`.
///
/// **The whole design rests on this type's `Drop`.** `sqlx::Transaction` issues a
/// `ROLLBACK` when it is dropped without `commit()` having been called. Because
/// `commit` here takes `Box<Self>`, committing consumes the value — so at any point
/// where this struct is still alive, the transaction is provably uncommitted, and
/// any early return (every `?` in the checkout workflow) drops it and rolls back.
///
/// No compensating action is written anywhere, and none can be forgotten, because
/// the cleanup is a consequence of ownership rather than a step in a procedure.
#[derive(Debug)]
pub struct SqliteCheckoutTx<'a> {
    tx: Transaction<'a, Sqlite>,
}

#[async_trait]
impl CheckoutTx for SqliteCheckoutTx<'_> {
    async fn cart_items(
        &mut self,
        user_id: &UserId,
    ) -> Result<Vec<(Product, Quantity)>, RepoError> {
        // `&mut *self.tx` is a reborrow. Passing `self.tx` would move the transaction
        // into the query and end this struct; `&mut self.tx` would move the mutable
        // borrow for the rest of the function. The reborrow lends the connection for
        // exactly this call and takes it back — which is why the next statement can
        // use it again.
        let rows: Vec<CartItemRow> = sqlx::query_as(
            "SELECT p.*, ci.quantity FROM cart_items ci
             JOIN products p ON p.id = ci.product_id
             WHERE ci.user_id = ? ORDER BY p.name",
        )
        .bind(user_id.as_ref())
        .fetch_all(&mut *self.tx)
        .await
        .map_err(to_repo_error)?;

        rows.into_iter().map(TryFrom::try_from).collect()
    }

    async fn try_reserve_stock(
        &mut self,
        product_id: &ProductId,
        quantity: Quantity,
    ) -> Result<StockReservation, RepoError> {
        let requested = i64::from(quantity.into_inner());

        // One statement decides and acts. A `SELECT stock` followed by an `UPDATE`
        // would be a lost-update race: two checkouts could both read stock = 1 and
        // both decrement. The `AND stock >= ?` guard makes the check and the write
        // atomic at the database level, so concurrency correctness does not depend
        // on the isolation level.
        let reserved: Option<i64> = sqlx::query(
            "UPDATE products SET stock = stock - ?1
             WHERE id = ?2 AND stock >= ?1
             RETURNING stock",
        )
        .bind(requested)
        .bind(product_id.as_ref())
        .fetch_optional(&mut *self.tx)
        .await
        .map_err(to_repo_error)?
        .map(|row| row.get::<i64, _>("stock"));

        if reserved.is_some() {
            return Ok(StockReservation::Reserved);
        }

        // The update matched nothing, so report how much there actually is. This read
        // is inside the same transaction, so the number is consistent with the failed
        // attempt rather than a fresh guess.
        let available: Option<i64> = sqlx::query("SELECT stock FROM products WHERE id = ?")
            .bind(product_id.as_ref())
            .fetch_optional(&mut *self.tx)
            .await
            .map_err(to_repo_error)?
            .map(|row| row.get::<i64, _>("stock"));

        Ok(StockReservation::Insufficient {
            available: available
                .and_then(|s| u32::try_from(s).ok())
                .unwrap_or_default(),
        })
    }

    async fn insert_order(&mut self, order: &Order) -> Result<(), RepoError> {
        sqlx::query("INSERT INTO orders (id, user_id, total_cents, placed_at) VALUES (?, ?, ?, ?)")
            .bind(order.id.as_ref())
            .bind(order.user_id.as_ref())
            .bind(order.total_cents.as_i64())
            .bind(order.placed_at)
            .execute(&mut *self.tx)
            .await
            .map_err(to_repo_error)?;

        // Sequential, because a transaction is a single connection: there is no
        // concurrency to exploit here, and attempting it would deadlock rather than
        // speed anything up.
        for line in &order.lines {
            sqlx::query(
                "INSERT INTO order_lines
                     (order_id, product_id, name, unit_price_cents, quantity)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(order.id.as_ref())
            .bind(line.product_id.as_ref())
            .bind(line.name.as_ref())
            .bind(line.unit_price_cents.as_i64())
            .bind(i64::from(line.quantity.into_inner()))
            .execute(&mut *self.tx)
            .await
            .map_err(to_repo_error)?;
        }

        Ok(())
    }

    async fn clear_cart(&mut self, user_id: &UserId) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM cart_items WHERE user_id = ?")
            .bind(user_id.as_ref())
            .execute(&mut *self.tx)
            .await
            .map_err(to_repo_error)?;

        Ok(())
    }

    async fn commit(self: Box<Self>) -> Result<(), RepoError> {
        self.tx.commit().await.map_err(to_repo_error)
    }
}

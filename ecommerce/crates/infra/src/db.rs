//! Connection setup, schema, and seed data.

use std::str::FromStr;

use app::RepoError;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Executor, SqlitePool};

/// Thin wrapper over the pool so the concrete driver type does not spread through
/// `infra` either.
#[derive(Debug, Clone)]
pub struct Db {
    pub(crate) pool: SqlitePool,
}

impl Db {
    #[must_use]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

/// Translates `sqlx::Error` into the application's vocabulary.
///
/// This function is the boundary the hard rule refers to: `sqlx::Error` exists on one
/// side of it and nowhere else. Every repository method funnels through it, so no
/// workflow signature can ever mention a database type.
pub(crate) fn to_repo_error(err: sqlx::Error) -> RepoError {
    RepoError::Unavailable(Box::new(err))
}

/// Opens the pool and brings the schema up to date.
///
/// `:memory:` yields a private database per connection, so the pool is capped at one
/// connection for that case — otherwise each pooled connection would get its own
/// empty database and the tests would be a coin flip.
pub async fn connect(url: &str) -> Result<Db, RepoError> {
    let in_memory = url.contains(":memory:");

    let options = SqliteConnectOptions::from_str(url)
        .map_err(to_repo_error)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(if in_memory { 1 } else { 5 })
        .connect_with(options)
        .await
        .map_err(to_repo_error)?;

    migrate(&pool).await?;
    seed(&pool).await?;

    Ok(Db { pool })
}

/// Idempotent DDL, run at startup.
///
/// A real deployment uses `sqlx::migrate!` with numbered, checked-in migration files
/// and a version table. This is inlined so the example runs from a clean checkout
/// with no extra step.
async fn migrate(pool: &SqlitePool) -> Result<(), RepoError> {
    pool.execute(
        r"
        CREATE TABLE IF NOT EXISTS categories (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            slug TEXT NOT NULL UNIQUE,
            parent_id TEXT REFERENCES categories(id)
        );
        CREATE TABLE IF NOT EXISTS products (
            id TEXT PRIMARY KEY,
            sku TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            description TEXT NOT NULL,
            price_cents INTEGER NOT NULL CHECK (price_cents >= 0),
            category_id TEXT NOT NULL REFERENCES categories(id),
            stock INTEGER NOT NULL CHECK (stock >= 0)
        );
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            email TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sessions (
            token TEXT PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id),
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS cart_items (
            user_id TEXT NOT NULL,
            product_id TEXT NOT NULL REFERENCES products(id),
            quantity INTEGER NOT NULL CHECK (quantity BETWEEN 1 AND 99),
            PRIMARY KEY (user_id, product_id)
        );
        CREATE TABLE IF NOT EXISTS orders (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL,
            total_cents INTEGER NOT NULL CHECK (total_cents >= 0),
            placed_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS order_lines (
            order_id TEXT NOT NULL REFERENCES orders(id),
            product_id TEXT NOT NULL,
            name TEXT NOT NULL,
            unit_price_cents INTEGER NOT NULL CHECK (unit_price_cents >= 0),
            quantity INTEGER NOT NULL CHECK (quantity BETWEEN 1 AND 99)
        );
        CREATE INDEX IF NOT EXISTS idx_order_lines_order ON order_lines(order_id);
        CREATE INDEX IF NOT EXISTS idx_orders_user ON orders(user_id);
        ",
    )
    .await
    .map_err(to_repo_error)?;

    Ok(())
}

/// Fixed ids so the examples in the README and the integration tests agree.
///
/// The `CHECK` constraints above mirror the domain's newtype invariants. They are
/// deliberate redundancy: the type system protects this process, the constraints
/// protect the data from anything else that ever touches it.
async fn seed(pool: &SqlitePool) -> Result<(), RepoError> {
    pool.execute(
        r"
        INSERT OR IGNORE INTO categories (id, name, slug, parent_id) VALUES
            ('cat-electronics', 'Electronics', 'electronics', NULL),
            ('cat-laptops', 'Laptops', 'laptops', 'cat-electronics'),
            ('cat-audio', 'Audio', 'audio', 'cat-electronics'),
            ('cat-books', 'Books', 'books', NULL);

        INSERT OR IGNORE INTO products
            (id, sku, name, description, price_cents, category_id, stock) VALUES
            ('p-laptop-pro', 'LAP-PRO-14', 'Laptop Pro 14', 'A very fast laptop', 199900, 'cat-laptops', 5),
            ('p-laptop-air', 'LAP-AIR-13', 'Laptop Air 13', 'A very light laptop', 129900, 'cat-laptops', 10),
            ('p-earbuds', 'AUD-EARB-1', 'Wireless Earbuds', 'Tiny speakers for your ears', 19900, 'cat-audio', 50),
            ('p-headphones', 'AUD-HEAD-1', 'Studio Headphones', 'Big speakers for your ears', 34900, 'cat-audio', 3),
            ('p-dmmf', 'BOOK-DMMF', 'Domain Modeling Made Functional', 'Wlaschin. Read it.', 4999, 'cat-books', 100);
        ",
    )
    .await
    .map_err(to_repo_error)?;

    Ok(())
}

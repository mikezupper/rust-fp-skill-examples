//! The composition root.
//!
//! This is the only file in the workspace that names a concrete implementation, and
//! the only one that reads the environment. Everything below it was written against
//! traits and has no idea that the database is SQLite or that ids are UUIDs.
//!
//! Swapping SQLite for Postgres is an edit to this file plus a new adapter — no
//! workflow, no domain type, and no test of either has to change.

// The composition root is the one place allowed to fail loudly: a process that cannot
// reach its database or bind its port must not start, and must not start *quietly*.
// The relaxation is scoped to this crate root — the library half of this same package
// still denies `expect`, as does everything below it.
#![allow(
    clippy::expect_used,
    reason = "startup wiring: an unmet precondition here must abort the process"
)]

use std::sync::Arc;

use api::{AppState, router};
use app::Ctx;
use infra::repos::{SqlCartRepo, SqlOrderRepo, SqlProductRepo, SqlSessionRepo, SqlUserRepo};
use infra::tx::SqlDatabase;
use infra::{ScryptHasher, SystemClock, UuidGen, connect};
use tokio::net::TcpListener;
use tokio::signal;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // Configuration is read once, here. Nothing deeper in the codebase touches the
    // environment, so nothing deeper can be surprised by it.
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let db_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://ecommerce.db".to_owned());

    // A failure here is a startup failure, not a runtime error: a process that cannot
    // reach its database should not begin serving traffic and report healthy.
    let db = connect(&db_url)
        .await
        .expect("database must be reachable at startup");

    let ctx = Ctx {
        users: Arc::new(SqlUserRepo(db.clone())),
        sessions: Arc::new(SqlSessionRepo(db.clone())),
        products: Arc::new(SqlProductRepo(db.clone())),
        carts: Arc::new(SqlCartRepo(db.clone())),
        orders: Arc::new(SqlOrderRepo(db.clone())),
        hasher: Arc::new(ScryptHasher::default()),
        ids: Arc::new(UuidGen),
        clock: Arc::new(SystemClock),
        db: Arc::new(SqlDatabase(db)),
    };

    // Middleware composes rather than nests: each layer is a function from one
    // `Service` to another, and `Service` is itself `Request -> Result<Response, E>`
    // — the same railway shape as every workflow, one level up.
    //
    // Order matters: the outermost layer sees the request first and the response last.
    let app = router(AppState::new(ctx))
        .layer(CatchPanicLayer::new()) // a defect becomes a 500, not a dead process
        // Every request is bounded in time. A timeout drops the handler's future,
        // which is why the checkout workflow must be safe to cancel at any await —
        // and it is, because cancellation drops the transaction, which rolls back.
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            std::time::Duration::from_secs(15),
        ))
        .layer(RequestBodyLimitLayer::new(64 * 1024))
        .layer(TraceLayer::new_for_http());

    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("port must be bindable at startup");

    tracing::info!(port, "listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server exited with an error");
}

/// Drains in-flight requests before exiting. Without this, a deploy kills requests
/// mid-flight — including a checkout that has reserved stock but not yet committed.
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.ok();
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) = signal::unix::signal(signal::unix::SignalKind::terminate()) {
            sig.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    tracing::info!("shutdown signal received, draining");
}

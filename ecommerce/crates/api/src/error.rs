//! The error track, rendered as HTTP.
//!
//! This module is the *only* place in the workspace where an error becomes a status
//! code and a JSON body. Everywhere upstream, failures are structured values; the
//! translation happens once, at the edge, where HTTP is actually the vocabulary.
//!
//! ## The wire contract
//!
//! Errors serialize as a **tagged union**: a `_tag` discriminant naming the failure,
//! plus that failure's own fields at the top level.
//!
//! ```json
//! { "_tag": "InsufficientStock", "productId": "p-headphones",
//!   "requested": 50, "available": 3 }
//! ```
//!
//! The tag is the *domain* name of the failure, not an HTTP-flavoured code, so a
//! client switches on `_tag` and reads typed fields rather than parsing a message.
//! It is the same shape the domain uses internally — the enum crosses the wire
//! rather than being flattened into prose on the way out.

use app::{
    CartError, CheckoutError, LoginError, OrderLookupError, ProductError, RegisterError, RepoError,
};
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use domain::{Email, OrderId, ProductId};
use serde_json::json;

/// Every way this API can fail, as data.
///
/// One variant per failure a client can meaningfully react to. Each carries the
/// fields a caller needs — `available` on an out-of-stock failure, the id on a
/// not-found — so the body is generated from structure rather than assembled by hand
/// at each call site.
#[derive(Debug)]
pub enum ApiError {
    /// The request body or parameters failed to decode into domain types. This is the
    /// boundary rejecting input, which is exactly what it is for.
    DecodeError(String),
    /// No valid session. Deliberately distinct from `InvalidCredentials`: this means
    /// "your token is not usable", not "your password was wrong".
    Unauthorized,
    InvalidCredentials,
    EmailTaken {
        email: Email,
    },
    ProductNotFound {
        product_id: ProductId,
    },
    OrderNotFound {
        order_id: OrderId,
    },
    CartEmpty,
    InsufficientStock {
        product_id: ProductId,
        requested: u32,
        available: u32,
    },
    /// Infrastructure broke. The cause is logged in full and never sent to the client.
    Internal(RepoError),
}

impl ApiError {
    /// The discriminant and status for each failure, in one table.
    ///
    /// Adding a variant will not compile until it appears here, so no failure can
    /// silently acquire a default status code.
    fn parts(&self) -> (StatusCode, &'static str, serde_json::Value) {
        match self {
            Self::DecodeError(message) => (
                StatusCode::BAD_REQUEST,
                "HttpApiDecodeError",
                json!({ "message": message }),
            ),
            // Contentless by design: an unauthenticated caller must not learn whether
            // the token was unknown, expired, or malformed.
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized", json!({})),
            // Also contentless, and identical for "no such account" and "wrong
            // password" — the login endpoint must not be an enumeration oracle.
            Self::InvalidCredentials => (StatusCode::UNAUTHORIZED, "InvalidCredentials", json!({})),
            Self::EmailTaken { email } => (
                StatusCode::CONFLICT,
                "EmailTaken",
                json!({ "email": email.as_ref() }),
            ),
            Self::ProductNotFound { product_id } => (
                StatusCode::NOT_FOUND,
                "ProductNotFound",
                json!({ "productId": product_id.as_ref() }),
            ),
            Self::OrderNotFound { order_id } => (
                StatusCode::NOT_FOUND,
                "OrderNotFound",
                json!({ "orderId": order_id.as_ref() }),
            ),
            Self::CartEmpty => (StatusCode::CONFLICT, "CartEmpty", json!({})),
            Self::InsufficientStock {
                product_id,
                requested,
                available,
            } => (
                StatusCode::CONFLICT,
                "InsufficientStock",
                json!({
                    "productId": product_id.as_ref(),
                    "requested": requested,
                    "available": available,
                }),
            ),
            Self::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalServerError",
                json!({}),
            ),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // The full chain goes to the log, where operators can see it. The client gets
        // a tag and a status — a leaked `sqlx::Error` discloses schema, table names,
        // and sometimes data.
        if let Self::Internal(err) = &self {
            tracing::error!(error = ?err, "unhandled infrastructure failure");
        }

        let (status, tag, fields) = self.parts();

        let mut body = serde_json::Map::new();
        body.insert("_tag".to_owned(), json!(tag));
        if let serde_json::Value::Object(map) = fields {
            body.extend(map);
        }

        (status, Json(serde_json::Value::Object(body))).into_response()
    }
}

// ---------------------------------------------------------------------------
// Translation from each workflow's error enum.
//
// These impls are where the "which status code" decision lives — once per domain
// failure, not once per handler. A variant added to a workflow error will not compile
// until it is mapped here, so no failure can silently become a 500.
// ---------------------------------------------------------------------------

impl From<RepoError> for ApiError {
    fn from(err: RepoError) -> Self {
        Self::Internal(err)
    }
}

impl From<RegisterError> for ApiError {
    fn from(err: RegisterError) -> Self {
        match err {
            RegisterError::EmailTaken { email } => Self::EmailTaken { email },
            RegisterError::Repo(e) => Self::Internal(e),
        }
    }
}

impl From<LoginError> for ApiError {
    fn from(err: LoginError) -> Self {
        match err {
            LoginError::InvalidCredentials => Self::InvalidCredentials,
            LoginError::Repo(e) => Self::Internal(e),
        }
    }
}

impl From<ProductError> for ApiError {
    fn from(err: ProductError) -> Self {
        match err {
            ProductError::NotFound { product_id } => Self::ProductNotFound { product_id },
            ProductError::Repo(e) => Self::Internal(e),
        }
    }
}

impl From<CartError> for ApiError {
    fn from(err: CartError) -> Self {
        match err {
            CartError::ProductNotFound { product_id } => Self::ProductNotFound { product_id },
            CartError::Repo(e) => Self::Internal(e),
        }
    }
}

impl From<CheckoutError> for ApiError {
    fn from(err: CheckoutError) -> Self {
        match err {
            CheckoutError::CartEmpty => Self::CartEmpty,
            // The structured fields survive from the SQL statement that failed to
            // reserve stock, through two error enums, into the JSON a browser renders.
            CheckoutError::InsufficientStock {
                product_id,
                requested,
                available,
            } => Self::InsufficientStock {
                product_id,
                requested,
                available,
            },
            CheckoutError::Repo(e) => Self::Internal(e),
        }
    }
}

impl From<OrderLookupError> for ApiError {
    fn from(err: OrderLookupError) -> Self {
        match err {
            OrderLookupError::NotFound { order_id } => Self::OrderNotFound { order_id },
            OrderLookupError::Repo(e) => Self::Internal(e),
        }
    }
}

/// Malformed JSON, and JSON that fails a domain type's `Deserialize` validation, both
/// arrive here. Without this, axum's default rejection returns a plain-text body and
/// the API has two error formats — the one it designed, and the one the framework
/// emits when parsing fails.
impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        Self::DecodeError(rejection.body_text())
    }
}

//! Wire types.
//!
//! Requests deserialize *directly into domain types*. `Email`, `Password`, `Quantity`
//! and `ProductId` all validate inside their `Deserialize` impls, so by the time a
//! handler body runs, the input has already been parsed — not "validated and hopefully
//! still correct", but converted into types that cannot hold anything else.
//!
//! That is why there is no `validate_credentials` function anywhere in this codebase.

use app::workflows::auth::AuthSession;
use axum::Json;
use axum::extract::FromRequest;
use domain::{Email, Password, ProductId, Quantity, SessionToken, UserId};
use serde::{Deserialize, Serialize};

/// A `Json` extractor whose rejection is this API's error type.
///
/// Without it, a body that fails validation is rejected by axum's default handler
/// with a plain-text 422, and the API ends up with two error formats — the one it
/// designed and the one the framework emits when parsing fails. Parsing failures are
/// part of the error track, so they get the same envelope as everything else.
#[derive(Debug, Clone, Copy, FromRequest)]
#[from_request(via(Json), rejection(crate::error::ApiError))]
pub struct ValidJson<T>(pub T);

/// `deny_unknown_fields` makes a typo in a client's payload an error rather than a
/// silently ignored field. A request that sets `quantitiy: 5` should fail loudly.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Credentials {
    pub email: Email,
    pub password: Password,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetCartItem {
    pub product_id: ProductId,
    pub quantity: Quantity,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BrowseParams {
    pub search: Option<String>,
    pub category: Option<String>,
}

/// Responses get an explicit DTO rather than serializing an internal type, so a
/// refactor of `AuthSession` cannot silently change the public contract.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSessionBody {
    pub token: SessionToken,
    pub user_id: UserId,
    pub email: Email,
}

impl From<AuthSession> for AuthSessionBody {
    fn from(session: AuthSession) -> Self {
        Self {
            token: session.token,
            user_id: session.user_id,
            email: session.email,
        }
    }
}

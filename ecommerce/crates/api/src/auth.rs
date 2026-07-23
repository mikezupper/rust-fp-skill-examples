//! Authentication as an extractor.
//!
//! `CurrentUser` implements `FromRequestParts`, which means a handler that needs an
//! authenticated user takes one as a parameter — and a handler that takes one cannot
//! be routed without the extractor running first. Authorization stops being something
//! you remember to call at the top of a function and becomes part of the signature.
//!
//! Forgetting the check is not a bug you can write here; it is a handler that takes a
//! different set of arguments.

use app::workflows::auth::authenticate;
use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use domain::{Email, SessionToken, UserId};

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub user_id: UserId,
    pub email: Email,
}

// axum 0.8 uses native `async fn` in traits here — no `#[async_trait]` needed, because
// extractors are resolved by static dispatch rather than through a trait object.
impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let raw = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or(ApiError::Unauthorized)?;

        // A malformed token and an unknown token are the same failure to the caller.
        let token = SessionToken::try_new(raw).map_err(|_| ApiError::Unauthorized)?;

        let user = authenticate(&state.ctx, &token)
            .await?
            .ok_or(ApiError::Unauthorized)?;

        Ok(Self {
            user_id: user.id,
            email: user.email,
        })
    }
}

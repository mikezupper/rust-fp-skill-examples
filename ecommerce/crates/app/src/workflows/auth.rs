//! Registration and login.

use domain::{Email, Password, SessionToken, User, UserId};

use crate::ctx::Ctx;
use crate::errors::{LoginError, RegisterError};

/// What a successful authentication yields. A domain type, not a DTO: the HTTP layer
/// decides how to render it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthSession {
    pub token: SessionToken,
    pub user_id: UserId,
    pub email: Email,
}

/// `err` makes the red track observable for free: any `Err` return emits an ERROR
/// event carrying the error's `Display`, without a single explicit log call on a
/// failure path. `skip_all` keeps the password out of the span — the type redacts its
/// `Debug` too, but defence in depth costs nothing here.
#[tracing::instrument(skip_all, fields(email = %email), err)]
pub async fn register(
    ctx: &Ctx,
    email: Email,
    password: &Password,
) -> Result<AuthSession, RegisterError> {
    // Absence is a repository concern; whether absence is an *error* is this
    // function's decision. Here presence is the failure.
    if ctx.users.find_by_email(&email).await?.is_some() {
        return Err(RegisterError::EmailTaken { email });
    }

    let user = User {
        id: UserId::try_new(ctx.ids.next()).map_err(|e| {
            crate::RepoError::Corrupt(format!("id generator produced garbage: {e}"))
        })?,
        email,
        password_hash: ctx.hasher.hash(password).await?,
        // Injected, never `Utc::now()` — which is why this workflow is deterministic
        // under test.
        created_at: ctx.clock.now(),
    };

    ctx.users.insert(&user).await?;
    tracing::info!(user_id = %user.id, "user registered");

    start_session(ctx, &user).await.map_err(RegisterError::Repo)
}

#[tracing::instrument(skip_all, fields(email = %email), err)]
pub async fn login(
    ctx: &Ctx,
    email: &Email,
    password: &Password,
) -> Result<AuthSession, LoginError> {
    let user = ctx.users.find_by_email(email).await?;

    // Both arms of the credential check produce the *same* error. Distinguishing
    // "no such account" from "wrong password" turns this endpoint into an account
    // enumeration oracle, so the two failures are deliberately indistinguishable —
    // including, ideally, in how long they take.
    let Some(user) = user else {
        return Err(LoginError::InvalidCredentials);
    };

    if !ctx.hasher.verify(password, &user.password_hash).await? {
        return Err(LoginError::InvalidCredentials);
    }

    start_session(ctx, &user).await.map_err(LoginError::Repo)
}

/// Resolves a bearer token to a user. Returns `Option` rather than an error: the
/// HTTP layer owns the decision that an unknown token means 401.
pub async fn authenticate(
    ctx: &Ctx,
    token: &SessionToken,
) -> Result<Option<User>, crate::RepoError> {
    ctx.sessions.find_user(token).await
}

async fn start_session(ctx: &Ctx, user: &User) -> Result<AuthSession, crate::RepoError> {
    let token = SessionToken::try_new(ctx.ids.next()).map_err(|e| {
        crate::RepoError::Corrupt(format!("id generator produced an invalid token: {e}"))
    })?;

    ctx.sessions.create(&token, &user.id).await?;

    Ok(AuthSession {
        token,
        user_id: user.id.clone(),
        email: user.email.clone(),
    })
}

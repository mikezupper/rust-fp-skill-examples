//! Users, credentials, and sessions.

use chrono::{DateTime, Utc};
use nutype::nutype;
use serde::Deserialize;

#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 64),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        AsRef,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct UserId(String);

#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 128),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        AsRef,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct SessionToken(String);

/// A syntactically valid, normalised email address.
///
/// Sanitisation runs before validation, so `"  Alice@Example.COM "` becomes
/// `"alice@example.com"` — which means uniqueness checks and lookups compare the
/// same bytes every time. Normalising at the boundary rather than at each call
/// site is the whole point of parsing into a type.
#[nutype(
    sanitize(trim, lowercase),
    validate(predicate = is_plausible_email, len_char_max = 254),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display, Serialize, Deserialize)
)]
pub struct Email(String);

fn is_plausible_email(candidate: &str) -> bool {
    // Deliberately permissive: RFC 5322 in a regex is a well-known mistake, and the
    // only real proof of deliverability is sending mail. This rejects the obviously
    // malformed and leaves the rest to a confirmation email.
    let mut parts = candidate.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !candidate.contains(char::is_whitespace)
}

/// A plaintext password on its way to being hashed.
///
/// `Debug` is implemented by hand to redact the value, so a password cannot reach a
/// log line, a span field, or an error message by accident. `Serialize` is
/// deliberately NOT derived: this type can be deserialized (it arrives in a request
/// body) but can never be serialized back out.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Password(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("password must be at least 8 characters")]
pub struct PasswordTooShort;

impl TryFrom<String> for Password {
    type Error = PasswordTooShort;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        if raw.chars().count() < 8 {
            return Err(PasswordTooShort);
        }
        Ok(Self(raw))
    }
}

impl Password {
    /// The single, deliberately awkward way to read the secret. Named so that it is
    /// obvious in review when a plaintext password escapes the hashing boundary.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Password {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Password(<redacted>)")
    }
}

#[nutype(
    validate(not_empty, len_char_max = 512),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Serialize, Deserialize)
)]
pub struct PasswordHash(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: UserId,
    pub email: Email,
    pub password_hash: PasswordHash,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn email_is_normalised_on_construction() {
        let email = Email::try_new("  Alice@Example.COM  ").expect("valid");
        assert_eq!(email.as_ref(), "alice@example.com");
    }

    #[test]
    fn malformed_emails_are_rejected() {
        for bad in [
            "",
            "no-at-sign",
            "@example.com",
            "alice@",
            "alice@localhost",
            "two@at@example.com",
            "alice space@example.com",
            "alice@.com",
            "alice@example.",
        ] {
            assert!(Email::try_new(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn a_password_never_prints_its_contents() {
        let password =
            Password::try_from("correct horse battery staple".to_owned()).expect("long enough");
        let rendered = format!("{password:?}");
        assert!(!rendered.contains("horse"), "leaked: {rendered}");
    }

    #[test]
    fn short_passwords_are_rejected() {
        assert!(Password::try_from("short".to_owned()).is_err());
        assert!(Password::try_from("just long".to_owned()).is_ok());
    }

    proptest! {
        /// Idempotence: normalising an already-normalised email is a no-op. Without
        /// this, a user could register twice with the same address.
        #[test]
        fn email_normalisation_is_idempotent(local in "[a-zA-Z]{1,10}", domain in "[a-zA-Z]{1,10}") {
            let once = Email::try_new(format!("{local}@{domain}.com")).expect("valid");
            let twice = Email::try_new(once.as_ref()).expect("already valid");
            prop_assert_eq!(once, twice);
        }

        /// Whitespace and case can never produce two distinct emails for one address.
        #[test]
        fn email_is_case_and_whitespace_insensitive(local in "[a-z]{1,10}", domain in "[a-z]{1,10}") {
            let plain = Email::try_new(format!("{local}@{domain}.com")).expect("valid");
            let noisy = Email::try_new(format!("  {}@{}.COM ", local.to_uppercase(), domain))
                .expect("valid");
            prop_assert_eq!(plain, noisy);
        }
    }
}

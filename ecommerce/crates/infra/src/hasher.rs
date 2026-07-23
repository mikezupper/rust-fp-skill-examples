//! Password hashing.

use app::RepoError;
use app::ports::PasswordHasher;
use async_trait::async_trait;
use domain::{Password, PasswordHash};
use rand::RngCore;
use subtle::ConstantTimeEq;

/// scrypt with interactive-grade parameters.
#[derive(Debug, Clone, Copy)]
pub struct ScryptHasher {
    params: scrypt::Params,
}

impl Default for ScryptHasher {
    fn default() -> Self {
        Self {
            // log_n = 14 (~16 MiB), r = 8, p = 1 — the usual interactive baseline.
            // The `unwrap_or_else` fallback is `Params::recommended()`, so there is no
            // panic path even if these constants were ever edited into an invalid
            // combination.
            params: scrypt::Params::new(14, 8, 1, 32)
                .unwrap_or_else(|_| scrypt::Params::recommended()),
        }
    }
}

#[async_trait]
impl PasswordHasher for ScryptHasher {
    async fn hash(&self, password: &Password) -> Result<PasswordHash, RepoError> {
        let params = self.params;
        // scrypt is intentionally CPU- and memory-hard, which is exactly what must
        // never run on an async executor thread: it would stall every other task on
        // that worker for the duration. `spawn_blocking` moves it to the blocking
        // pool. This is the async equivalent of not doing I/O in a hot loop.
        let plaintext = password.expose().to_owned();

        tokio::task::spawn_blocking(move || {
            let mut salt = [0u8; 16];
            rand::rng().fill_bytes(&mut salt);

            let mut derived = [0u8; 32];
            scrypt::scrypt(plaintext.as_bytes(), &salt, &params, &mut derived)
                .map_err(|e| RepoError::Corrupt(format!("scrypt misconfigured: {e}")))?;

            PasswordHash::try_new(format!("{}:{}", hex(&salt), hex(&derived)))
                .map_err(|e| RepoError::Corrupt(format!("hash encoding: {e}")))
        })
        .await
        .map_err(|e| RepoError::Unavailable(Box::new(e)))?
    }

    async fn verify(&self, password: &Password, stored: &PasswordHash) -> Result<bool, RepoError> {
        let params = self.params;
        let plaintext = password.expose().to_owned();
        let stored = stored.as_ref().to_owned();

        tokio::task::spawn_blocking(move || {
            let Some((salt_hex, expected_hex)) = stored.split_once(':') else {
                // A stored hash that does not parse is corrupt data, not a wrong
                // password — do not silently return `false` and let the user think
                // they mistyped.
                return Err(RepoError::Corrupt("stored hash is malformed".to_owned()));
            };

            let (Some(salt), Some(expected)) = (unhex(salt_hex), unhex(expected_hex)) else {
                return Err(RepoError::Corrupt(
                    "stored hash is not valid hex".to_owned(),
                ));
            };

            let mut derived = [0u8; 32];
            scrypt::scrypt(plaintext.as_bytes(), &salt, &params, &mut derived)
                .map_err(|e| RepoError::Corrupt(format!("scrypt misconfigured: {e}")))?;

            // Constant time: a byte-by-byte `==` leaks how many leading bytes matched,
            // which is enough to forge a hash one byte at a time.
            Ok(bool::from(derived.as_slice().ct_eq(expected.as_slice())))
        })
        .await
        .map_err(|e| RepoError::Unavailable(Box::new(e)))?
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    bytes.iter().fold(String::new(), |mut out, b| {
        // Writing into a String is infallible, so the Result is genuinely dead.
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    // Traverse again: one bad pair fails the whole decode.
    text.as_bytes()
        .chunks(2)
        .map(|pair| {
            let s = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(s, 16).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_password_verifies_against_its_own_hash() {
        let hasher = ScryptHasher::default();
        let password = Password::try_from("correct horse battery staple".to_owned())
            .expect("long enough for a test fixture");

        let hash = hasher.hash(&password).await.expect("hashing succeeds");
        assert!(hasher.verify(&password, &hash).await.expect("verify runs"));
    }

    #[tokio::test]
    async fn a_different_password_does_not_verify() {
        let hasher = ScryptHasher::default();
        let password =
            Password::try_from("correct horse battery staple".to_owned()).expect("valid");
        let other = Password::try_from("incorrect horse battery".to_owned()).expect("valid");

        let hash = hasher.hash(&password).await.expect("hashing succeeds");
        assert!(!hasher.verify(&other, &hash).await.expect("verify runs"));
    }

    #[tokio::test]
    async fn the_same_password_hashes_differently_each_time() {
        // Distinct salts: two users with the same password must not share a hash.
        let hasher = ScryptHasher::default();
        let password = Password::try_from("correct horse battery".to_owned()).expect("valid");

        let first = hasher.hash(&password).await.expect("hashing succeeds");
        let second = hasher.hash(&password).await.expect("hashing succeeds");
        assert_ne!(first.as_ref(), second.as_ref());
    }
}

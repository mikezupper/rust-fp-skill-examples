//! The one place in the workspace allowed to ask what time it is.

use app::ports::Clock;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    // `clippy.toml` bans `chrono::Utc::now` by path across the whole workspace. This
    // adapter is the single sanctioned exception, and the allow is scoped to exactly
    // one function so that the ban stays meaningful everywhere else. A blanket
    // `#![allow]` at crate level would quietly reopen the door.
    #[allow(
        clippy::disallowed_methods,
        reason = "the Clock adapter is by definition where wall-clock time enters the system"
    )]
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

//! The one place in the workspace allowed to generate randomness.

use app::ports::IdGen;

#[derive(Debug, Clone, Copy, Default)]
pub struct UuidGen;

impl IdGen for UuidGen {
    #[allow(
        clippy::disallowed_methods,
        reason = "the IdGen adapter is by definition where nondeterminism enters the system"
    )]
    fn next(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

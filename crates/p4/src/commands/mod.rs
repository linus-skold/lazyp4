//! Typed wrappers over the commands lazyp4 runs.
//!
//! Each one builds argv, runs it tagged, and maps the records onto
//! [`crate::model`] types.
//!
//! The methods sit in one `impl Client` block per verb family. Rust allows
//! several such blocks in the same crate, so a command is grouped with the
//! ones it is read beside, and no signature or caller changes.

mod change;
mod files;
mod query;
mod resolve;
mod shelve;
mod text;

use crate::model::ChangeStatus;

/// Which changelists `changes` should list.
#[derive(Debug, Clone, Default)]
pub struct ChangeFilter {
    pub status: Option<ChangeStatus>,
    pub user: Option<String>,
    pub client: Option<String>,
    /// Stop after this many, newest first.
    pub max: Option<u32>,
    /// Restrict to a depot path, e.g. `//depot/main/...`.
    pub path: Option<String>,
}

impl ChangeFilter {
    pub fn pending() -> Self {
        ChangeFilter {
            status: Some(ChangeStatus::Pending),
            ..Default::default()
        }
    }

    pub fn submitted(max: u32) -> Self {
        ChangeFilter {
            status: Some(ChangeStatus::Submitted),
            max: Some(max),
            ..Default::default()
        }
    }
}

//! Reading tagged records.
//!
//! Perforce reports lists two different ways. Most commands send one record per
//! item; `describe` and `filelog` instead send a single record with the index
//! glued onto the key — `depotFile0`, `depotFile1`, and so on.

use std::str::FromStr;

use p4_sys::ffi::TaggedRecord;

use crate::error::{Error, Result};

pub(crate) trait RecordExt {
    fn field(&self, key: &str) -> Option<&str>;
    fn indexed(&self, prefix: &str, i: usize) -> Option<&str>;

    fn required(&self, key: &str) -> Result<&str> {
        self.field(key)
            .ok_or_else(|| Error::parse(format!("missing field `{key}`")))
    }

    fn parsed<T: FromStr>(&self, key: &str) -> Option<T> {
        self.field(key).and_then(|v| v.parse().ok())
    }

    /// Number of `prefix0`, `prefix1`, … entries before the first gap.
    fn indexed_count(&self, prefix: &str) -> usize {
        (0..).take_while(|i| self.indexed(prefix, *i).is_some()).count()
    }
}

impl RecordExt for TaggedRecord {
    fn field(&self, key: &str) -> Option<&str> {
        self.get(key)
    }

    fn indexed(&self, prefix: &str, i: usize) -> Option<&str> {
        self.get(&format!("{prefix}{i}"))
    }
}

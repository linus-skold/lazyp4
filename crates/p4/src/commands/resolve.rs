//! Commands for files that the server will not accept until a conflict is
//! settled.

use crate::client::Client;
use crate::error::Result;
use crate::model::{Resolution, Unresolved};
use crate::record::RecordExt;

impl Client {
    /// Files that must be resolved before they can be submitted.
    pub fn unresolved(&mut self) -> Result<Vec<Unresolved>> {
        // -n previews; without it `p4 resolve` is interactive and would sit
        // waiting for an answer this client has no way to give.
        let out = self.run_raw("resolve", &["-n"], "")?;
        Ok(out
            .records
            .iter()
            .filter_map(|rec| {
                Some(Unresolved {
                    local_path: rec.field("clientFile")?.to_owned(),
                    from_path: rec.field("fromFile").unwrap_or_default().to_owned(),
                    start_rev: rec.parsed("startFromRev"),
                    end_rev: rec.parsed("endFromRev"),
                    resolve_type: rec.field("resolveType").unwrap_or_default().to_owned(),
                    content_type: rec
                        .field("contentResolveType")
                        .unwrap_or_default()
                        .to_owned(),
                })
            })
            .collect())
    }

    /// Settle files one way or the other.
    ///
    /// Always with an `-a` flag, since a bare `p4 resolve` prompts per file.
    pub fn resolve(&mut self, how: Resolution, paths: &[&str]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut args = vec![how.flag()];
        args.extend_from_slice(paths);
        self.run("resolve", &args)?;
        Ok(())
    }
}

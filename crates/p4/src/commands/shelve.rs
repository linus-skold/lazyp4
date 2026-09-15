//! Commands that move work between the workspace and a shelf on the server.

use crate::client::Client;
use crate::error::Result;
use crate::model::ChangeId;

impl Client {
    /// Copy open files to the server as a shelf on `change`.
    ///
    /// An empty `paths` shelves everything open in the changelist. `-f` is
    /// always passed so shelving again updates what is already there rather
    /// than failing.
    pub fn shelve(&mut self, change: ChangeId, paths: &[&str]) -> Result<()> {
        let id = change.to_string();
        let mut args = vec!["-c", &id, "-f"];
        args.extend_from_slice(paths);
        self.run("shelve", &args)?;
        Ok(())
    }

    /// Replace a shelf with whatever is open now.
    ///
    /// Unlike [`Client::shelve`], files that were shelved but are no longer
    /// open are dropped from the shelf, so it ends up matching the workspace.
    pub fn replace_shelf(&mut self, change: ChangeId) -> Result<()> {
        let id = change.to_string();
        self.run("shelve", &["-r", "-c", &id])?;
        Ok(())
    }

    /// Discard a shelf. The changelist and its open files are untouched.
    pub fn delete_shelf(&mut self, change: ChangeId) -> Result<()> {
        let id = change.to_string();
        self.run("shelve", &["-d", "-c", &id])?;
        Ok(())
    }

    /// Open a shelf's files in `into`, taking a copy of the shelved content.
    ///
    /// The shelf itself is left alone, which is what makes this the way to
    /// carry work between machines.
    pub fn unshelve(&mut self, from: ChangeId, into: ChangeId) -> Result<()> {
        let source = from.to_string();
        let target = into.to_string();
        self.run("unshelve", &["-s", &source, "-c", &target])?;
        Ok(())
    }
}

//! Commands that act on a changelist itself: its spec form, its description,
//! and putting it into the depot or taking it back out.

use crate::client::Client;
use crate::error::{Error, Result};
use crate::model::ChangeId;

/// The number out of `Change 398 created.`
fn parse_created(message: &str) -> Option<u32> {
    let rest = message.strip_prefix("Change ")?;
    rest.split_whitespace().next()?.parse().ok()
}

impl Client {
    /// The changelist spec form, as `p4 change -o` prints it.
    ///
    /// Needs an untagged connection: a tagged reply is a record, not the form
    /// that `save_change_spec` must send back.
    pub fn change_spec(&mut self, change: ChangeId) -> Result<String> {
        let id = change.to_string();
        Ok(self.run("change", &["-o", &id])?.merged_text())
    }

    /// Create an empty pending changelist and return its number.
    ///
    /// The form `p4 change -o` hands back for a new changelist already lists
    /// every file open in the default changelist, and saving it as-is would
    /// sweep all of them in. `Files` is cleared so the caller moves exactly
    /// what it means to.
    pub fn create_change(&mut self, description: &str) -> Result<ChangeId> {
        let form = self.run("change", &["-o"])?.merged_text();
        let form = crate::spec::set_field(&form, "Description", description);
        let form = crate::spec::set_field(&form, "Files", "");

        let out = self.run_with_input("change", &["-i"], &form)?;
        // The server answers "Change 398 created."
        out.info
            .iter()
            .find_map(|line| parse_created(&line.text))
            .map(ChangeId::Number)
            .ok_or_else(|| Error::parse("no changelist number in the reply"))
    }

    /// Delete an empty pending changelist.
    pub fn delete_change(&mut self, change: ChangeId) -> Result<()> {
        let id = change.to_string();
        self.run("change", &["-d", &id])?;
        Ok(())
    }

    /// Write a changelist spec form back.
    pub fn save_change_spec(&mut self, form: &str) -> Result<()> {
        self.run_with_input("change", &["-i"], form)?;
        Ok(())
    }

    /// Replace a pending changelist's description.
    ///
    /// Reads the current form and edits one field of it, so nothing else about
    /// the changelist is disturbed.
    pub fn set_description(&mut self, change: ChangeId, description: &str) -> Result<()> {
        let form = self.change_spec(change)?;
        self.save_change_spec(&crate::spec::set_field(&form, "Description", description))
    }

    /// Open files that reverse a submitted change, into `into`.
    ///
    /// `spec` is a path with a revision range, e.g. `//depot/main/...@=412`
    /// for everything one changelist submitted. Nothing reaches the depot
    /// until the resulting changelist is submitted.
    pub fn undo(&mut self, into: ChangeId, spec: &str) -> Result<()> {
        let id = into.to_string();
        self.run("undo", &["-c", &id, spec])?;
        Ok(())
    }

    /// Submit a pending changelist to the depot.
    ///
    /// Irreversible once it succeeds. It fails if any file needs resolving,
    /// and the server's message says which.
    pub fn submit(&mut self, change: ChangeId) -> Result<()> {
        let id = change.to_string();
        self.run("submit", &["-c", &id])?;
        Ok(())
    }

    /// Submit everything open in the default changelist.
    ///
    /// The default changelist is not a spec and carries no description, so one
    /// has to be given here. `p4 submit` with no `-c` takes whatever is open in
    /// it, which is why the caller has to show that list first.
    pub fn submit_default(&mut self, description: &str) -> Result<()> {
        self.run("submit", &["-d", description])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::parse_created;

    #[test]
    fn reads_the_number_out_of_the_reply() {
        assert_eq!(parse_created("Change 398 created."), Some(398));
        assert_eq!(parse_created("Change 398 created fixing job000123."), Some(398));
    }

    #[test]
    fn ignores_anything_else_the_server_says() {
        assert_eq!(parse_created("Change 398 updated."), Some(398));
        assert_eq!(parse_created("//depot/f.txt#1 - opened for add"), None);
        assert_eq!(parse_created("Change default renamed"), None);
    }
}

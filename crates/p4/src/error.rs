use std::fmt;

use p4_sys::ffi::P4Message;

/// Anything a command can fail with.
#[derive(Debug)]
pub enum Error {
    /// The connection could not be made, or was lost mid-command.
    Connection(String),
    /// The server ran the command and reported warnings or errors.
    Server(Vec<P4Message>),
    /// The server answered, but not in the shape this crate expects.
    Parse(String),
}

impl Error {
    pub(crate) fn parse(what: impl Into<String>) -> Self {
        Error::Parse(what.into())
    }

    pub(crate) fn parse_int(e: std::num::ParseIntError) -> Self {
        Error::Parse(e.to_string())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Connection(msg) => write!(f, "{msg}"),
            Error::Server(msgs) => {
                let mut first = true;
                for msg in msgs {
                    if !first {
                        writeln!(f)?;
                    }
                    first = false;
                    write!(f, "{}", msg.text.trim_end())?;
                }
                Ok(())
            }
            Error::Parse(msg) => write!(f, "unexpected server output: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

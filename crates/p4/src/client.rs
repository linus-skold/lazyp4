use cxx::UniquePtr;
use p4_sys::ffi::{self, RunOutput};

use crate::error::{Error, Result};

/// How to reach the server. Every field left empty falls back to the ambient
/// P4PORT/P4USER/P4CLIENT the way the `p4` binary resolves them, so the common
/// case is `Connection::default()`.
#[derive(Debug, Clone, Default)]
pub struct Connection {
    pub port: Option<String>,
    pub user: Option<String>,
    pub client: Option<String>,
    pub charset: Option<String>,
    pub cwd: Option<String>,
}

/// A connection to a Perforce server.
///
/// Not `Send`: the underlying `ClientApi` must be used from the thread that
/// created it. Own one per worker thread rather than moving it between threads.
pub struct Client {
    inner: UniquePtr<ffi::P4Client>,
}

impl Client {
    pub fn connect(conn: &Connection) -> Result<Self> {
        let mut inner = ffi::new_client();
        {
            let mut c = inner.pin_mut();
            c.as_mut().set_prog("lazyp4");
            c.as_mut().set_version(env!("CARGO_PKG_VERSION"));
            if let Some(v) = &conn.port {
                c.as_mut().set_port(v);
            }
            if let Some(v) = &conn.user {
                c.as_mut().set_user(v);
            }
            if let Some(v) = &conn.client {
                c.as_mut().set_client(v);
            }
            if let Some(v) = &conn.charset {
                c.as_mut().set_charset(v);
            }
            if let Some(v) = &conn.cwd {
                c.as_mut().set_cwd(v);
            }
            // Tagged output must be requested before the handshake.
            c.as_mut().set_tagged(true);
            c.as_mut()
                .connect()
                .map_err(|e| Error::Connection(e.to_string()))?;
        }
        Ok(Client { inner })
    }

    /// True once the server has hung up; the client must be rebuilt.
    pub fn dropped(&mut self) -> bool {
        self.inner.pin_mut().dropped()
    }

    /// Run a command and fail if the server reported a warning or worse.
    pub fn run(&mut self, cmd: &str, args: &[&str]) -> Result<RunOutput> {
        let out = self.run_raw(cmd, args, "")?;
        match out.errors().cloned().collect::<Vec<_>>() {
            msgs if msgs.is_empty() => Ok(out),
            msgs => Err(Error::Server(msgs)),
        }
    }

    /// Run a command, keeping server warnings as data rather than failing.
    ///
    /// Needed wherever a warning is a normal answer — `p4 diff` on an unchanged
    /// file, `p4 opened` with nothing open.
    pub fn run_raw(&mut self, cmd: &str, args: &[&str], input: &str) -> Result<RunOutput> {
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        self.inner
            .pin_mut()
            .run(cmd, &args, input)
            .map_err(|e| Error::Connection(e.to_string()))
    }

    /// Run a spec command that reads a form on stdin, such as `change -i`.
    pub fn run_with_input(&mut self, cmd: &str, args: &[&str], input: &str) -> Result<RunOutput> {
        let out = self.run_raw(cmd, args, input)?;
        match out.errors().cloned().collect::<Vec<_>>() {
            msgs if msgs.is_empty() => Ok(out),
            msgs => Err(Error::Server(msgs)),
        }
    }
}

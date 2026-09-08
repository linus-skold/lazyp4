//! Raw bindings to the Helix Core C++ API.
//!
//! The P4API is driven by subclassing `ClientUser` and overriding its output
//! callbacks. C++ inheritance cannot cross the `cxx` bridge, so the subclass
//! lives in `src/shim.cc` and buffers one command's callbacks into a flat
//! [`ffi::RunOutput`] that Rust reads back after the command finishes.

#[cxx::bridge(namespace = "lazyp4")]
pub mod ffi {
    /// One key/value pair of tagged (`OutputStat`) output.
    #[derive(Debug, Clone)]
    struct TaggedField {
        key: String,
        value: String,
    }

    /// One tagged record — a file, a changelist, a `p4 info` block.
    #[derive(Debug, Clone)]
    struct TaggedRecord {
        fields: Vec<TaggedField>,
    }

    /// A server message. `severity` is the P4API `E_*` scale: 0 empty, 1 info,
    /// 2 warning, 3 failed, 4 fatal.
    #[derive(Debug, Clone)]
    struct P4Message {
        severity: i32,
        generic: i32,
        text: String,
    }

    /// One `OutputInfo` line, and how much `text` had arrived before it.
    ///
    /// The two streams interleave: `p4 diff -du` sends each file's `---`/`+++`
    /// header as info and the `@@` hunks that follow it as text. Keeping the
    /// offset is what lets them be put back in order.
    #[derive(Debug, Clone)]
    struct InfoLine {
        text: String,
        at: u64,
    }

    /// Everything one `run` produced.
    #[derive(Debug, Clone)]
    struct RunOutput {
        records: Vec<TaggedRecord>,
        info: Vec<InfoLine>,
        text: Vec<u8>,
        messages: Vec<P4Message>,
    }

    unsafe extern "C++" {
        include!("p4-sys/include/shim.h");

        type P4Client;

        fn new_client() -> UniquePtr<P4Client>;

        fn set_port(self: Pin<&mut P4Client>, value: &str);
        fn set_user(self: Pin<&mut P4Client>, value: &str);
        fn set_client(self: Pin<&mut P4Client>, value: &str);
        fn set_password(self: Pin<&mut P4Client>, value: &str);
        fn set_charset(self: Pin<&mut P4Client>, value: &str);
        fn set_cwd(self: Pin<&mut P4Client>, value: &str);
        fn set_prog(self: Pin<&mut P4Client>, value: &str);
        fn set_version(self: Pin<&mut P4Client>, value: &str);

        /// Ask the server for tagged output. Must be called before [`connect`].
        fn set_tagged(self: Pin<&mut P4Client>, on: bool);

        fn connect(self: Pin<&mut P4Client>) -> Result<()>;
        fn disconnect(self: Pin<&mut P4Client>) -> Result<()>;
        fn dropped(self: Pin<&mut P4Client>) -> bool;

        /// Run one command. `input` answers both `-i` spec forms
        /// (`ClientUser::InputData`) and password prompts (`ClientUser::Prompt`);
        /// pass `""` when the command needs neither.
        fn run(
            self: Pin<&mut P4Client>,
            cmd: &str,
            args: &Vec<String>,
            input: &str,
        ) -> Result<RunOutput>;
    }
}

impl ffi::TaggedRecord {
    /// First value stored under `key`, if any.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|f| f.key == key)
            .map(|f| f.value.as_str())
    }
}

impl ffi::RunOutput {
    /// Messages the server flagged as warnings or worse.
    pub fn errors(&self) -> impl Iterator<Item = &ffi::P4Message> {
        self.messages.iter().filter(|m| m.severity >= 2)
    }

    /// The info and text streams put back into the order the server sent them,
    /// which is what `p4` itself would have printed.
    ///
    /// Diff output needs this: the file headers arrive as info and the hunk
    /// bodies as text, so neither stream means anything on its own.
    pub fn merged_text(&self) -> String {
        let mut out = String::new();
        let mut copied = 0usize;

        for line in &self.info {
            let at = (line.at as usize).min(self.text.len());
            if at > copied {
                out.push_str(&String::from_utf8_lossy(&self.text[copied..at]));
                copied = at;
            }
            out.push_str(&line.text);
            if !line.text.ends_with('\n') {
                out.push('\n');
            }
        }
        out.push_str(&String::from_utf8_lossy(&self.text[copied..]));
        out
    }
}

//! Milestone 1 gate: prove the FFI, the link and the SSL handshake all work.
//!
//! Uses the ambient P4PORT/P4USER/P4CLIENT from the environment, exactly as the
//! `p4` binary does.
//!
//! ```text
//! cargo run -p p4-sys --example info
//! ```

use p4_sys::ffi;

fn main() {
    let mut client = ffi::new_client();
    let mut c = client.pin_mut();

    c.as_mut().set_prog("lazyp4");
    c.as_mut().set_version(env!("CARGO_PKG_VERSION"));
    c.as_mut().set_tagged(true);

    if let Err(e) = c.as_mut().connect() {
        eprintln!("connect failed: {e}");
        std::process::exit(1);
    }

    let out = match c.as_mut().run("info", &Vec::new(), "") {
        Ok(out) => out,
        Err(e) => {
            eprintln!("run failed: {e}");
            std::process::exit(1);
        }
    };

    for msg in out.errors() {
        eprintln!("[{}] {}", msg.severity, msg.text.trim_end());
    }

    for record in &out.records {
        for field in &record.fields {
            println!("{}: {}", field.key, field.value);
        }
    }

    let _ = c.as_mut().disconnect();
}

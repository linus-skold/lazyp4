{
  lib,
  mkShell,
  rustPlatform,
  cargo,
  clippy,
  rust-analyzer,
  rustc,
  rustfmt,
  openssl,
  p4api,
}:

let
  opensslStatic = openssl.override { static = true; };
in
mkShell {
  # A debug `cargo build` passes no -O, which _FORTIFY_SOURCE needs, so the
  # shim compiles with a #warning from glibc every time. Release builds — the
  # packaged one included — keep the flag.
  hardeningDisable = [ "fortify" ];

  packages = [
    cargo
    clippy
    rust-analyzer
    rustc
    rustfmt
  ];

  # The point of the shell: with these set, a plain `cargo build` downloads
  # nothing and compiles no OpenSSL. They win over an export made before
  # `nix develop`, so reassign one inside the shell to use your own build.
  env = {
    P4API_DIR = "${p4api}";
    OPENSSL_LIB_DIR = "${lib.getLib opensslStatic}/lib";
    RUST_SRC_PATH = "${rustPlatform.rustLibSrc}";
  };
}

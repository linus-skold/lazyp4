{
  lib,
  rustPlatform,
  openssl,
  p4api,
}:

let
  workspace = lib.importTOML ../Cargo.toml;
  crate = lib.importTOML ../crates/lazyp4/Cargo.toml;

  # build.rs asks the linker for `static=ssl` and `static=crypto`, and the
  # ordinary nixpkgs openssl deletes its .a files when it builds shared
  # libraries. This override keeps them, in the same stdenv as the rest of the
  # build — which also means they carry the default `pic` hardening flag that a
  # Rust position-independent link needs.
  opensslStatic = openssl.override { static = true; };
in
rustPlatform.buildRustPackage {
  pname = "lazyp4";
  inherit (workspace.workspace.package) version;

  # Only what the build reads. Keeping the screenshot and docs/ out means
  # editing them does not rebuild anything.
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      # Carries the +crt-static that every object in a Windows build has to
      # agree on. Inert here, but the cargo vendor hook appends to this file
      # rather than replacing it, so it costs nothing to keep the tree whole.
      ../.cargo
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  # Left unset, build.rs downloads the P4API with curl and compiles OpenSSL
  # from source. Neither can happen in a sandbox with no network, and neither
  # needs to. See docs/building.md.
  env = {
    P4API_DIR = "${p4api}";
    OPENSSL_LIB_DIR = "${lib.getLib opensslStatic}/lib";
  };

  # Safe in the sandbox: every test is a unit or headless-render test over
  # canned data, driven through Worker::detached(). Nothing reaches a server.
  doCheck = true;

  meta = {
    inherit (crate.package) description;
    homepage = workspace.workspace.package.repository;
    mainProgram = "lazyp4";
    # The binary statically links the P4API, so the artifact carries its terms
    # as well as lazyp4's own MIT.
    license = with lib.licenses; [
      mit
      unfree
    ];
    sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
    inherit (p4api.meta) platforms;
  };
}

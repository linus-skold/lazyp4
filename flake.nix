{
  description = "A terminal UI for Perforce, in the style of lazygit";

  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      inherit (nixpkgs) lib;

      # What crates/p4-sys/build.rs has a P4API download wired up for, less
      # Windows, which Nix cannot target, and less x86_64-darwin, which
      # nixpkgs 26.11 has dropped. nix/p4api.nix still carries the archive for
      # that platform, so the overlay works against a nixpkgs 26.05 pin.
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      forAllSystems =
        f:
        lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ self.overlays.default ];
              # The P4API ships no licence file, so it is marked unfree, and so
              # is the binary that statically links it. Permit those two rather
              # than turning unfree on for everything. The prefix covers the
              # checks below, which rename the package they override.
              config.allowUnfreePredicate =
                pkg:
                let
                  name = lib.getName pkg;
                in
                name == "p4api" || lib.hasPrefix "lazyp4" name;
            }
          )
        );
    in
    {
      overlays.default = final: _prev: {
        p4api = final.callPackage ./nix/p4api.nix { };
        lazyp4 = final.callPackage ./nix/package.nix { };
      };

      packages = forAllSystems (pkgs: {
        default = pkgs.lazyp4;
        # p4api is exposed on its own so that `nix build .#p4api` isolates a
        # stale hash, rather than meeting it partway through a Rust build.
        inherit (pkgs) lazyp4 p4api;
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.callPackage ./nix/shell.nix { };
      });

      checks = forAllSystems (pkgs: {
        # Builds the binary and runs the test suite.
        inherit (pkgs) lazyp4;

        # Mirrors the Clippy step in .github/workflows/ci.yml, including its
        # lack of `-D warnings`, and for the same reason: the tree carries
        # warnings that predate that workflow.
        clippy = pkgs.lazyp4.overrideAttrs (prev: {
          pname = "${prev.pname}-clippy";
          nativeBuildInputs = prev.nativeBuildInputs ++ [ pkgs.clippy ];
          buildPhase = ''
            runHook preBuild
            cargo clippy --release --all-targets --offline -j $NIX_BUILD_CORES
            runHook postBuild
          '';
          doCheck = false;
          installPhase = "touch $out";
          dontFixup = true;
        });

        # Nix files only. The Rust tree is deliberately not rustfmt-clean, and
        # ci.yml has no formatting step either.
        nixfmt = pkgs.runCommand "nixfmt-check" { nativeBuildInputs = [ pkgs.nixfmt ]; } ''
          nixfmt --check ${./flake.nix} ${./nix}/*.nix
          touch $out
        '';
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt-tree);
    };
}

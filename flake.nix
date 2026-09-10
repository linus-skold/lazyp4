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

      # Not in the overlay: this one is about this repository's files, not
      # something a consumer of the overlay would want in their package set.
      apps = forAllSystems (pkgs: {
        update-p4api = {
          type = "app";
          program = lib.getExe (pkgs.callPackage ./nix/update-p4api.nix { });
          meta.description = "Refresh the pinned P4API hashes in nix/p4api.nix";
        };
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

        # The release and the archive names are written down in build.rs too,
        # and nothing makes the two agree. Compare them, so that a release
        # bump which misses nix/p4api.nix fails here rather than quietly
        # building against a different P4API than cargo does.
        p4api-drift =
          let
            inherit (pkgs.p4api) release srcs;
            pair = system: dist: ''
              if ! grep -qF '("${dist.dir}", "${dist.file}")' flat; then
                echo "  ${system}: nix/p4api.nix wants ${dist.dir}/${dist.file}, build.rs does not pair those" >&2
                fail=1
              fi
            '';
          in
          pkgs.runCommand "p4api-drift-check" { } ''
            # Flattened, so a reflowed match arm in build.rs still matches.
            tr -s '[:space:]' ' ' < ${./crates/p4-sys/build.rs} > flat

            fail=0
            if ! grep -qF 'const RELEASE: &str = "${release}"' flat; then
              echo "  release: nix/p4api.nix pins ${release}, build.rs does not" >&2
              fail=1
            fi
            ${lib.concatStrings (lib.mapAttrsToList pair srcs)}

            if [ "$fail" -ne 0 ]; then
              echo "" >&2
              echo "build.rs and nix/p4api.nix disagree about which P4API to use." >&2
              echo "Reconcile them, then run 'nix run .#update-p4api' to refresh the hashes." >&2
              exit 1
            fi
            touch $out
          '';

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

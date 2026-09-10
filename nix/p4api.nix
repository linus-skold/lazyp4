# The Helix Core C++ API, as crates/p4-sys/build.rs would otherwise download
# it. A Nix build has no network, so the archive is fetched here — where a
# pinned hash makes it a fixed-output derivation — and handed to the build
# script through P4API_DIR.
{
  lib,
  stdenv,
  fetchurl,
}:

let
  # Tracks RELEASE in crates/p4-sys/build.rs.
  release = "r25.1";

  # One entry per case in that file's distribution(), less Windows, which Nix
  # cannot target. The archive names carry the choices build.rs explains: the
  # glibc2.12 build is the one that ships the p4script libraries, and every
  # archive is an openssl3.5 build.
  #
  # Perforce replaces these files in place within a release line — the r25.1
  # archives were last changed on 2026-08-18 — so a rotation upstream arrives
  # as a hash mismatch. Refresh one with:
  #
  #   nix store prefetch-file --json <url>
  srcs = {
    x86_64-linux = {
      dir = "bin.linux26x86_64";
      file = "p4api-glibc2.12-openssl3.5.tgz";
      hash = "sha256-s4QNfkuInkgJKRNHA9IhVAn4qG6OIGFY+2ZRc0CGgF0=";
    };
    aarch64-linux = {
      dir = "bin.linux26aarch64";
      file = "p4api-openssl3.5.tgz";
      hash = "sha256-O7oltEkXNB3bwkFr3GirhBGNLcN8qQ7cXsgpBkreK2M=";
    };
    x86_64-darwin = {
      dir = "bin.macosx12x86_64";
      file = "p4api-openssl3.5.tgz";
      hash = "sha256-LMP6EIkDIJ5XXEBkJIc0zvz0AStau5kx1thrGn/axSI=";
    };
    aarch64-darwin = {
      dir = "bin.macosx12arm64";
      file = "p4api-openssl3.5.tgz";
      hash = "sha256-abr/2K+Slwag/lwnjWCKswNPcb9zx3+N0rqS0d3LIgU=";
    };
  };

  inherit (stdenv.hostPlatform) system;

  dist =
    srcs.${system} or (throw "no P4API distribution is wired up for ${system}; set P4API_DIR instead");
in
stdenv.mkDerivation {
  pname = "p4api";
  version = lib.removePrefix "r" release;

  src = fetchurl {
    url = "https://ftp.perforce.com/perforce/${release}/${dist.dir}/${dist.file}";
    inherit (dist) hash;
  };

  # The archives unpack to a single versioned directory whose name changes with
  # every rebuild upstream, so stay in the parent and find the root instead.
  sourceRoot = ".";

  dontConfigure = true;
  dontBuild = true;

  # Prebuilt vendor archives. Leave them exactly as they were shipped.
  dontStrip = true;

  # Only the headers and the static libraries are used. Locating the root by
  # its header mirrors api_root() in build.rs, which accepts the same layout.
  installPhase = ''
    runHook preInstall

    header=$(find . -type f -path '*/include/p4/clientapi.h' -print -quit)
    if [ -z "$header" ]; then
      echo "no include/p4/clientapi.h in the P4API archive" >&2
      exit 1
    fi
    root=$(cd "$(dirname "$header")/../.." && pwd)

    if ! ls "$root"/lib/*.a >/dev/null 2>&1; then
      echo "no static libraries in $root/lib" >&2
      exit 1
    fi

    mkdir -p "$out"
    cp -r "$root/include" "$root/lib" "$out/"

    runHook postInstall
  '';

  # The whole table, not just this system's row. The update script rewrites
  # hashes from it, and the drift check compares it against build.rs.
  passthru = {
    inherit release srcs;
    baseUrl = "https://ftp.perforce.com/perforce/${release}";
  };

  meta = {
    description = "Helix Core C++ API";
    homepage = "https://www.perforce.com";
    sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
    # The distribution carries no licence file at all, which is issue #10, so
    # take the conservative reading. This also keeps it out of any public
    # binary cache.
    license = lib.licenses.unfree;
    platforms = builtins.attrNames srcs;
  };
}

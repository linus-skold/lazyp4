#!/bin/sh
# Installs lazyp4 from a GitHub release. Needs no Rust.
#
#   curl -fsSL https://raw.githubusercontent.com/linus-skold/lazyp4/main/scripts/install.sh | sh
#
# LAZYP4_VERSION      a tag such as v0.2.0. Default: the latest release
# LAZYP4_INSTALL_DIR  where the binary goes. Default: ~/.local/bin
set -eu

repo="linus-skold/lazyp4"
version="${LAZYP4_VERSION:-latest}"
install_dir="${LAZYP4_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf 'lazyp4: %s\n' "$*" >&2; }
fail() {
  say "$*"
  exit 1
}

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64 | Linux-amd64) asset="lazyp4-linux-x86_64.tar.gz" ;;
  Darwin-arm64 | Darwin-aarch64) asset="lazyp4-macos-arm64.tar.gz" ;;
  *) fail "there is no release for $(uname -s) $(uname -m). To build from source, see https://github.com/$repo/blob/main/docs/building.md" ;;
esac

if [ "$version" = latest ]; then
  base="https://github.com/$repo/releases/latest/download"
else
  base="https://github.com/$repo/releases/download/$version"
fi

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -q -O "$2" "$1"; }
else
  fail "curl or wget is necessary"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

say "downloading $asset ($version)"
fetch "$base/$asset" "$tmp/$asset" || fail "could not download $base/$asset. Is there a published release?"
fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" || fail "could not download $base/SHA256SUMS"

expected=$(awk -v f="$asset" '{ sub(/^\*/, "", $2) } $2 == f { print $1 }' "$tmp/SHA256SUMS")
[ -n "$expected" ] || fail "SHA256SUMS has no entry for $asset"
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/$asset" | awk '{ print $1 }')
else
  actual=$(shasum -a 256 "$tmp/$asset" | awk '{ print $1 }')
fi
[ "$expected" = "$actual" ] || fail "the checksum of $asset is wrong. Nothing was installed"

mkdir -p "$tmp/extract" "$install_dir"
tar -xzf "$tmp/$asset" -C "$tmp/extract"
# Copy, then rename: a rename replaces a binary that is running, a copy onto
# it can fail.
cp "$tmp/extract/lazyp4" "$install_dir/.lazyp4.new"
chmod 755 "$install_dir/.lazyp4.new"
mv -f "$install_dir/.lazyp4.new" "$install_dir/lazyp4"

say "installed $install_dir/lazyp4"
case ":$PATH:" in
  *":$install_dir:"*) say "run it with: lazyp4" ;;
  *)
    say "$install_dir is not on your PATH. Add this line to your shell profile:"
    # shellcheck disable=SC2016 # $PATH is for the profile to expand, not now.
    printf '  export PATH="%s:$PATH"\n' "$install_dir" >&2
    ;;
esac

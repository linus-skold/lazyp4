# Refreshes the pinned P4API hashes in nix/p4api.nix.
#
# Perforce replaces these archives in place within a release line, so the
# hashes go stale on their own. This re-downloads each one and writes the new
# hash back. The systems and file names come from the flake, so this only ever
# changes a hash — never a URL.
#
# Each archive is 40-90 MB, so refreshing all four moves about 250 MB. Name
# systems as arguments to do fewer.
{
  writeShellApplication,
  gawk,
  jq,
}:

writeShellApplication {
  name = "update-p4api";

  runtimeInputs = [
    gawk
    jq
  ];

  # `nix` comes from the ambient environment, as it does for every other
  # update script; it is the thing that invoked this one.
  text = ''
    file=nix/p4api.nix
    if [ ! -f "$file" ]; then
      echo "no $file here — run this from the repository root" >&2
      exit 1
    fi

    srcs=$(nix eval --json .#p4api.srcs)
    base=$(nix eval --raw .#p4api.baseUrl)

    if [ "$#" -gt 0 ]; then
      systems=("$@")
    else
      mapfile -t systems < <(jq -r 'keys[]' <<<"$srcs")
    fi

    changed=0
    for system in "''${systems[@]}"; do
      if ! jq -e --arg s "$system" 'has($s)' >/dev/null <<<"$srcs"; then
        echo "$system is not in $file" >&2
        exit 1
      fi

      dir=$(jq -r --arg s "$system" '.[$s].dir' <<<"$srcs")
      name=$(jq -r --arg s "$system" '.[$s].file' <<<"$srcs")
      old=$(jq -r --arg s "$system" '.[$s].hash' <<<"$srcs")
      url="$base/$dir/$name"

      echo "$system: fetching $url"
      new=$(nix store prefetch-file --json "$url" | jq -r .hash)

      if [ "$new" = "$old" ]; then
        echo "$system: unchanged"
        continue
      fi

      # Replace the hash inside this system's block only. Indentation is
      # matched loosely, so a reformatted file still matches.
      # `sys`, not `system`: gawk has a builtin by that name.
      gawk -v sys="$system" -v new="$new" '
        $0 ~ "^[[:space:]]*" sys "[[:space:]]*=[[:space:]]*\\{" { inblock = 1 }
        inblock && /hash[[:space:]]*=[[:space:]]*"/ {
          sub(/"[^"]*"/, "\"" new "\"")
          inblock = 0
        }
        { print }
      ' "$file" > "$file.tmp"
      mv "$file.tmp" "$file"

      echo "$system: $old -> $new"
      changed=1
    done

    if [ "$changed" -eq 0 ]; then
      echo "every hash was already current"
    else
      echo "done — check the diff, then rebuild to confirm"
    fi
  '';

  meta.description = "Refresh the pinned P4API hashes in nix/p4api.nix";
}

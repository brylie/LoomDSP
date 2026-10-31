#!/bin/sh
# Give this worktree a persistent Cargo target directory on the external drive.
set -eu

if [ "${1:-}" = --all ]; then
    git worktree list --porcelain | sed -n 's/^worktree //p' | while IFS= read -r worktree_dir; do
        "$0" "$worktree_dir"
    done
    exit 0
fi

if [ "$#" -gt 1 ]; then
    echo "Usage: $0 [worktree-path|--all]" >&2
    exit 2
fi
root=$(git -C "${1:-.}" rev-parse --show-toplevel)
volume=/Volumes/External
if [ ! -d "$volume" ] || ! mount | grep -F " on $volume " >/dev/null; then
    echo "External drive is not mounted at $volume" >&2
    exit 1
fi

config="$root/.cargo/config.toml"
if [ -e "$config" ] && ! grep -Fq 'Regenerate with scripts/setup-external-target.sh.' "$config"; then
    echo "Existing Cargo config at $config was not created by this script" >&2
    exit 1
fi

# The common exclude also covers worktrees on branches without this script.
common=$(git -C "$root" rev-parse --path-format=absolute --git-common-dir)
mkdir -p "$common/info"
if ! grep -Fxq '/.cargo/config.toml' "$common/info/exclude" 2>/dev/null; then
    printf '%s\n' '/.cargo/config.toml' >> "$common/info/exclude"
fi

key=$(printf '%s' "$root" | git hash-object --stdin | cut -c 1-16)
target="$volume/LoomDSP/targets/$key"
mkdir -p "$target" "$root/.cargo"
cat > "$config" <<EOF
# Local to this worktree. Regenerate with scripts/setup-external-target.sh.
[build]
target-dir = "$target"
EOF

echo "Cargo target for $root: $target"

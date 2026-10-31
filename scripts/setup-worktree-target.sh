#!/bin/sh
# Give each worktree a persistent Cargo target directory under a chosen root.
set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "Usage: $0 target-root [worktree-path|--all]" >&2
    exit 2
fi
if [ ! -d "$1" ]; then
    echo "Target root does not exist: $1" >&2
    exit 1
fi
target_root=$(cd "$1" && pwd -P)

if [ "${2:-}" = --all ]; then
    git worktree list --porcelain | sed -n 's/^worktree //p' | while IFS= read -r worktree_dir; do
        "$0" "$target_root" "$worktree_dir"
    done
    exit 0
fi

root=$(git -C "${2:-.}" rev-parse --show-toplevel)

config="$root/.cargo/config.toml"
if [ -e "$config" ]; then
    if ! grep -Fq 'Regenerate with scripts/setup-worktree-target.sh.' "$config" &&
        ! grep -Fq 'Regenerate with scripts/setup-external-target.sh.' "$config"; then
        echo "Existing Cargo config at $config was not created by this script" >&2
        exit 1
    fi
fi

# The common exclude also covers worktrees on branches without this script.
common=$(git -C "$root" rev-parse --path-format=absolute --git-common-dir)
mkdir -p "$common/info"
if ! grep -Fxq '/.cargo/config.toml' "$common/info/exclude" 2>/dev/null; then
    printf '%s\n' '/.cargo/config.toml' >> "$common/info/exclude"
fi

key=$(printf '%s' "$root" | git hash-object --stdin | cut -c 1-16)
target="$target_root/$key"
mkdir -p "$target" "$root/.cargo"
escaped_target=$(printf '%s' "$target" | sed 's/\\/\\\\/g; s/"/\\"/g')
cat > "$config" <<EOF
# Local to this worktree. Regenerate with scripts/setup-worktree-target.sh.
[build]
target-dir = "$escaped_target"
EOF

echo "Cargo target for $root: $target"

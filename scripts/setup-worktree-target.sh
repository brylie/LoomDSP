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
    git worktree list --porcelain | awk '
        BEGIN { RS = ""; FS = "\n" }
        {
            for (i = 1; i <= NF; i++) {
                if ($i == "bare" || $i ~ /^prunable( |$)/) next
            }
            path = $1
            sub(/^worktree /, "", path)
            print path
        }
    ' | (
        failed=0
        while IFS= read -r worktree_dir; do
            "$0" "$target_root" "$worktree_dir" || failed=$((failed + 1))
        done
        if [ "$failed" -gt 0 ]; then
            echo "$failed worktree(s) could not be configured" >&2
            exit 1
        fi
    )
    exit 0
fi

root=$(git -C "${2:-.}" rev-parse --show-toplevel)
root=$(cd "$root" && pwd -P)

config="$root/.cargo/config.toml"
if [ -e "$root/.cargo/config" ] || [ -L "$root/.cargo/config" ]; then
    echo "Legacy Cargo config at $root/.cargo/config would override $config" >&2
    exit 1
fi
if [ -e "$config" ]; then
    if ! grep -Fq 'Regenerate with scripts/setup-worktree-target.sh.' "$config" &&
        ! grep -Fq 'Regenerate with scripts/setup-external-target.sh.' "$config"; then
        echo "Existing Cargo config at $config was not created by this script" >&2
        exit 1
    fi
fi

# The common exclude also covers worktrees on branches without this script.
common=$(git -C "$root" rev-parse --path-format=absolute --git-common-dir)
common=$(cd "$common" && pwd -P)
mkdir -p "$common/info"
if ! grep -Fxq '/.cargo/config.toml' "$common/info/exclude" 2>/dev/null; then
    printf '%s\n' '/.cargo/config.toml' >> "$common/info/exclude"
fi

key=$(printf '%s' "$root" | git hash-object --stdin | cut -c 1-16)
target="$target_root/$key"
escaped_target=$(printf '%s' "$target" | sed 's/\\/\\\\/g; s/"/\\"/g')
marker="$target/.loomdsp-worktree-target"
expected_marker=$(printf 'LoomDSP worktree target v1\n%s\n%s' "$root" "$common")

if [ -L "$target" ] || [ -e "$target" ] && [ ! -d "$target" ]; then
    echo "Target is not a plain directory: $target" >&2
    exit 1
fi
if [ -d "$target" ]; then
    if [ -L "$marker" ]; then
        echo "Target marker is a symlink: $marker" >&2
        exit 1
    fi
    if [ -e "$marker" ]; then
        if [ ! -f "$marker" ] || [ "$(cat "$marker")" != "$expected_marker" ]; then
            echo "Target belongs to another worktree or repository: $target" >&2
            exit 1
        fi
    else
        # An old target is adoptable only when its generated config already
        # points to this exact directory. A hash-shaped name is not proof.
        old_config=$(printf '# Local to this worktree. Regenerate with scripts/setup-worktree-target.sh.\n[build]\ntarget-dir = "%s"' "$escaped_target")
        legacy_config=$(printf '# Local to this worktree. Regenerate with scripts/setup-external-target.sh.\n[build]\ntarget-dir = "%s"' "$escaped_target")
        if [ ! -f "$config" ] || {
            [ "$(cat "$config")" != "$old_config" ] &&
                [ "$(cat "$config")" != "$legacy_config" ];
        }; then
            echo "Unmarked target cannot be adopted: $target" >&2
            exit 1
        fi
    fi
fi

mkdir -p "$target" "$root/.cargo"
if [ ! -e "$marker" ]; then
    printf '%s\n' "$expected_marker" > "$marker"
fi
cat > "$config" <<EOF
# Local to this worktree. Regenerate with scripts/setup-worktree-target.sh.
[build]
target-dir = "$escaped_target"
EOF

echo "Cargo target for $root: $target"

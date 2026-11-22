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

if [ -L "$target" ] || { [ -e "$target" ] && [ ! -d "$target" ]; }; then
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
        # The config may hold more settings than the target, from a newer
        # version of this script (jobs, the wrapper).
        if [ ! -f "$config" ] || {
            [ "$(head -n 1 "$config")" != '# Local to this worktree. Regenerate with scripts/setup-worktree-target.sh.' ] &&
                [ "$(head -n 1 "$config")" != '# Local to this worktree. Regenerate with scripts/setup-external-target.sh.' ];
        } || [ "$(sed -n 2p "$config")" != '[build]' ] ||
            ! grep -Fxq "target-dir = \"$escaped_target\"" "$config"; then
            echo "Unmarked target cannot be adopted: $target" >&2
            exit 1
        fi
    fi
fi

mkdir -p "$target" "$root/.cargo"
if [ ! -e "$marker" ]; then
    printf '%s\n' "$expected_marker" > "$marker"
fi
# Half the cores, rounded up, so several agents building on one machine
# leave each other room. `mise run check` runs the tests on as many
# threads. LOOM_BUILD_JOBS overrides it.
cores=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)
jobs=${LOOM_BUILD_JOBS:-$(((cores + 1) / 2))}

# Every worktree compiles through sccache, one cache for the machine
# (scripts/rustc-wrapper.sh). Only when this checkout has the wrapper, so a
# worktree on an older branch never points Cargo at a missing file, and not
# on Windows, where Cargo cannot run a shell script as its wrapper. This
# script runs there under Git Bash, MSYS2, or Cygwin, whose `uname -s`
# starts with MINGW, MSYS, or CYGWIN. Under WSL, Cargo is a Linux program,
# so the wrapper works. LOOM_UNAME stands in for `uname -s` in the tests.
wrapper=""
sccache=""
case ${LOOM_UNAME:-$(uname -s)} in
    MINGW* | MSYS* | CYGWIN*) windows=1 ;;
    *) windows=0 ;;
esac
if [ "$windows" = 0 ] && [ -f "$root/scripts/rustc-wrapper.sh" ]; then
    wrapper=$(printf '%s' "$root/scripts/rustc-wrapper.sh" | sed 's/\\/\\\\/g; s/"/\\"/g')
    sccache=$(cd "$root" && mise which sccache 2>/dev/null || true)
fi

{
    echo '# Local to this worktree. Regenerate with scripts/setup-worktree-target.sh.'
    echo '[build]'
    printf 'target-dir = "%s"\n' "$escaped_target"
    printf 'jobs = %s\n' "$jobs"
    if [ -n "$wrapper" ]; then
        printf 'rustc-wrapper = "%s"\n' "$wrapper"
    fi
    if [ -n "$sccache" ]; then
        echo
        echo '[env]'
        echo '# The sccache mise installed. If mise upgrades it, run this script again.'
        printf 'LOOM_SCCACHE = "%s"\n' "$(printf '%s' "$sccache" | sed 's/\\/\\\\/g; s/"/\\"/g')"
    fi
} > "$config"

echo "Cargo target for $root: $target"

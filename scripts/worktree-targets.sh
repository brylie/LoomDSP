#!/bin/sh
# Inspect per-worktree Cargo targets and explicitly prune confirmed orphans.
set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ] || { [ "$#" -eq 2 ] && [ "$2" != --prune ]; }; then
    echo "Usage: $0 target-root [--prune]" >&2
    exit 2
fi
if [ ! -d "$1" ]; then
    echo "Target root does not exist: $1" >&2
    exit 1
fi
target_root=$(cd "$1" && pwd -P)
common=$(git rev-parse --path-format=absolute --git-common-dir)
common=$(cd "$common" && pwd -P)

# A registered worktree, a path still on disk, or a moved worktree whose
# Cargo config still uses this target is treated as live.
is_live() {
    candidate=$1
    candidate_target=$2
    if [ -e "$candidate" ] || [ -L "$candidate" ]; then
        return 0
    fi
    if ! registered=$(git worktree list --porcelain); then
        return 0
    fi
    [ -n "$registered" ] || return 0
    escaped_target=$(printf '%s' "$candidate_target" | sed 's/\\/\\\\/g; s/"/\\"/g')
    printf '%s\n' "$registered" | awk '/^worktree / { sub(/^worktree /, ""); print }' | (
        while IFS= read -r path; do
            if [ "$path" = "$candidate" ]; then
                exit 0
            fi
            config="$path/.cargo/config.toml"
            if [ -e "$config" ] && [ ! -r "$config" ]; then
                exit 0
            fi
            if [ -f "$config" ] && grep -Fxq "target-dir = \"$escaped_target\"" "$config"; then
                exit 0
            fi
        done
        exit 1
    )
}

owned_target() {
    entry=$1
    [ -d "$entry" ] && [ ! -L "$entry" ] || return 1
    marker="$entry/.loomdsp-worktree-target"
    [ -f "$marker" ] && [ ! -L "$marker" ] || return 1
    magic=$(sed -n '1p' "$marker")
    worktree=$(sed -n '2p' "$marker")
    owner=$(sed -n '3p' "$marker")
    [ "$magic" = 'LoomDSP worktree target v1' ] &&
        [ "$owner" = "$common" ] &&
        [ -n "$worktree" ] &&
        [ "$(printf '%s' "$worktree" | git hash-object --stdin | cut -c 1-16)" = "${entry##*/}" ] &&
        [ "$(wc -l < "$marker" | tr -d ' ')" = 3 ]
}

find "$target_root" -mindepth 1 -maxdepth 1 -print | while IFS= read -r entry; do
    size=$(du -sh "$entry" 2>/dev/null | awk '{ print $1 }')
    if owned_target "$entry"; then
        if is_live "$worktree" "$entry"; then
            printf 'active\t%s\t%s\t%s\n' "$size" "$entry" "$worktree"
        else
            printf 'orphan\t%s\t%s\t%s\n' "$size" "$entry" "$worktree"
            if [ "${2:-}" = --prune ]; then
                # Recheck immediately before deletion. Never follow a target
                # directory symlink or remove a target still in use.
                if owned_target "$entry" && ! is_live "$worktree" "$entry"; then
                    rm -rf "$entry"
                    printf 'pruned\t%s\n' "$entry"
                fi
            else
                printf 'would prune\t%s\n' "$entry"
            fi
        fi
    else
        printf 'unknown\t%s\t%s\n' "$size" "$entry"
    fi
done

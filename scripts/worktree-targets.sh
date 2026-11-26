#!/bin/sh
# Inspect per-worktree Cargo targets, explicitly prune confirmed orphans, and
# sweep stale build output out of live targets.
set -eu

usage() {
    echo "Usage: $0 target-root [--prune | --sweep [--days N] [--delete]]" >&2
    exit 2
}

[ "$#" -ge 1 ] || usage
mode=list
days=14
delete=0
root_arg=$1
shift
while [ "$#" -gt 0 ]; do
    case $1 in
        --prune) [ "$mode" = list ] || usage; mode=prune ;;
        --sweep) [ "$mode" = list ] || usage; mode=sweep ;;
        --days)
            [ "$#" -ge 2 ] || usage
            days=$2
            shift
            ;;
        --delete) delete=1 ;;
        *) usage ;;
    esac
    shift
done
if [ "$mode" != sweep ] && { [ "$delete" = 1 ] || [ "$days" != 14 ]; }; then
    usage
fi
case $days in
    '' | *[!0-9]* | 0)
        echo "Days must be a whole number above 0: $days" >&2
        exit 2
        ;;
esac
set -- "$root_arg"
if [ ! -d "$1" ]; then
    echo "Target root does not exist: $1" >&2
    exit 1
fi
scripts_dir=$(cd "$(dirname "$0")" && pwd -P)
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

# Cargo holds an flock(2) on these files while it builds. Take each one,
# exclusive and without waiting, then run the command while holding them all,
# so no build starts or runs in the target until the command ends. A build
# that starts meanwhile waits ("Blocking waiting for file lock"). Exit 75
# means a lock is taken, so a build is running. perl ships with macOS and the
# common Linux systems and has flock built in; macOS has no flock command.
with_cargo_locks() {
    locks=$1
    shift
    printf '%s\n' "$locks" | perl -e '
        use Fcntl qw(:flock);
        my @held;
        while (my $file = <STDIN>) {
            chomp $file;
            next if $file eq "";
            open(my $lock, "<", $file) or exit 75;
            flock($lock, LOCK_EX | LOCK_NB) or exit 75;
            push @held, $lock;
        }
        # system, not exec: the handles close on exec, which would drop
        # the locks.
        my $status = system(@ARGV);
        exit($status == -1 ? 126 : ($status & 127) ? 128 + ($status & 127) : $status >> 8);
    ' "$@"
}

cargo_locks() {
    find "$1" -maxdepth 4 \
        \( -name build -o -name deps -o -name incremental -o -name .fingerprint \) -prune -o \
        -type f \( -name .cargo-lock -o -name .cargo-build-lock -o -name .cargo-artifact-lock \) \
        -print
}

# Sweep one live target under Cargo's locks. A sweep never removes the
# target itself, only stale folders inside it, and only those nothing
# changed in for $days days, so a build that starts later loses nothing it
# made recently.
sweep() {
    entry=$1
    if [ "$delete" = 1 ]; then
        sweep_mode=delete
    else
        sweep_mode=list
    fi
    # Recheck just before reading or removing anything, as prune does.
    owned_target "$entry" && is_live "$worktree" "$entry" || return 0
    status=0
    if [ "$sweep_mode" = delete ]; then
        with_cargo_locks "$(cargo_locks "$entry")" \
            sh "$scripts_dir/sweep-target.sh" "$entry" "$days" delete || status=$?
    else
        # A dry run only reads, so it checks that no build is running and
        # lets go of the locks before measuring, which can take minutes on
        # a large target. Holding them that long would stall a build.
        with_cargo_locks "$(cargo_locks "$entry")" true || status=$?
        if [ "$status" = 0 ]; then
            sh "$scripts_dir/sweep-target.sh" "$entry" "$days" list || status=$?
        fi
    fi
    case $status in
        0) ;;
        75) printf 'busy\t%s\n' "$entry" ;;
        *)
            printf 'sweep failed\t%s\n' "$entry"
            return 1
            ;;
    esac
}

# One failed sweep does not stop the others; the exit status reports it.
find "$target_root" -mindepth 1 -maxdepth 1 -print | {
    failed=0
    while IFS= read -r entry; do
        size=$(du -sh "$entry" 2>/dev/null | awk '{ print $1 }')
        if owned_target "$entry"; then
            if is_live "$worktree" "$entry"; then
                printf 'active\t%s\t%s\t%s\n' "$size" "$entry" "$worktree"
                if [ "$mode" = sweep ]; then
                    sweep "$entry" || failed=1
                fi
            else
                printf 'orphan\t%s\t%s\t%s\n' "$size" "$entry" "$worktree"
                if [ "$mode" = prune ]; then
                    # Recheck immediately before deletion. Never follow a target
                    # directory symlink or remove a target still in use.
                    if owned_target "$entry" && ! is_live "$worktree" "$entry"; then
                        rm -rf "$entry"
                        printf 'pruned\t%s\n' "$entry"
                    fi
                elif [ "$mode" = list ]; then
                    printf 'would prune\t%s\n' "$entry"
                fi
            fi
        else
            printf 'unknown\t%s\t%s\n' "$size" "$entry"
        fi
    done
    exit "$failed"
}

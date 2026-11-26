#!/bin/sh
# List or remove stale build output inside one Cargo target folder.
#
# scripts/worktree-targets.sh runs this while it holds Cargo's own lock
# files, so no build can start or run in the folder meanwhile. Do not run it
# by hand on a target a build may use.
#
# Cargo never deletes the output of an old configuration. A toolchain
# update, a Cargo.lock change, another feature set, or another flag gives a
# unit a new hash and a new folder, and the old one stays. This finds, in
# every profile folder (debug, release, llvm-cov-target/debug,
# wasm32-unknown-unknown/debug, ...):
#
#   unit         build/<crate>/<hash>/, a unit of the build-dir layout of
#                Cargo 1.100 and later
#   incremental  incremental/<crate>-<id>/, rustc's incremental state
#   old-layout   deps/, .fingerprint/, and build/<crate>-<hash>/ from the
#                layout before Cargo 1.100, only when the profile folder
#                already holds units of the new layout
#
# and treats one as stale when nothing inside it, the folder included, was
# modified in the last DAYS days. Its age is never read from the folder
# alone: Cargo rewrites files inside a unit without touching its folder.
set -eu

if [ "$#" -ne 3 ] || { [ "$3" != list ] && [ "$3" != delete ]; }; then
    echo "Usage: $0 target-folder days list|delete" >&2
    exit 2
fi
target=$1
days=$2
mode=$3
case $days in
    '' | *[!0-9]* | 0)
        echo "Days must be a whole number above 0: $days" >&2
        exit 2
        ;;
esac

# True when nothing in the folder, the folder included, changed in the
# last $days days. `-mtime -N` is "modified less than N days ago".
stale() {
    [ -z "$(find "$1" -mtime "-$days" -print 2>/dev/null | head -n 1)" ]
}

# A hash as Cargo writes it: 16 lowercase hexadecimal digits.
is_hash() {
    case $1 in
        *[!0-9a-f]*) return 1 ;;
    esac
    [ "${#1}" -eq 16 ]
}

# Size in KiB. Units of one build share object files through hard links, so
# the sum overstates what deleting frees: read it as an upper bound.
size_kib() {
    du -sk "$1" 2>/dev/null | awk '{ print $1 }'
}

# Profile folders are the ones that hold Cargo's lock files. Hard-coding
# debug and release would miss coverage and WebAssembly builds, which grow
# the same way.
profiles() {
    find "$target" -maxdepth 4 \
        \( -name build -o -name deps -o -name incremental -o -name .fingerprint \) -prune -o \
        -type f \( -name .cargo-lock -o -name .cargo-build-lock -o -name .cargo-artifact-lock \) \
        -print | sed 's|/[^/]*$||' | sort -u
}

# Print "kind<TAB>path" for each stale folder in one profile folder.
candidates() {
    profile=$1
    new_layout=0
    if [ -d "$profile/build" ]; then
        for crate in "$profile"/build/*/; do
            [ -d "$crate" ] || continue
            crate=${crate%/}
            [ -L "$crate" ] && continue
            units=0
            for unit in "$crate"/*/; do
                [ -d "$unit" ] || continue
                unit=${unit%/}
                is_hash "${unit##*/}" && [ -d "$unit/fingerprint" ] && [ ! -L "$unit" ] || continue
                units=1
                if stale "$unit"; then
                    printf 'unit\t%s\n' "$unit"
                fi
            done
            [ "$units" = 1 ] && new_layout=1
        done
    fi
    if [ -d "$profile/incremental" ]; then
        for session in "$profile"/incremental/*/; do
            [ -d "$session" ] || continue
            session=${session%/}
            [ -L "$session" ] && continue
            if stale "$session"; then
                printf 'incremental\t%s\n' "$session"
            fi
        done
    fi
    # The old layout is only stale once the new one is in use here. An
    # older Cargo building into this folder would still read it.
    [ "$new_layout" = 1 ] || return 0
    for old in "$profile/deps" "$profile/.fingerprint"; do
        if [ -d "$old" ] && [ ! -L "$old" ] && stale "$old"; then
            printf 'old-layout\t%s\n' "$old"
        fi
    done
    for old in "$profile"/build/*-*/; do
        [ -d "$old" ] || continue
        old=${old%/}
        [ -L "$old" ] && continue
        is_hash "${old##*-}" || continue
        # A new-layout crate whose name ends in a hash-like part still
        # holds units, so it is not an old build script folder.
        has_units=0
        for unit in "$old"/*/fingerprint; do
            [ -d "$unit" ] && has_units=1 && break
        done
        [ "$has_units" = 0 ] || continue
        if stale "$old"; then
            printf 'old-layout\t%s\n' "$old"
        fi
    done
}

if [ "$mode" = delete ]; then
    verb=swept
else
    verb='would sweep'
fi
# The whole list is found before anything is removed, so a delete run
# removes exactly what a dry run at the same moment would list. Each path is
# measured before it goes, then awk writes the sizes in K, M, G, or T and
# the total for the target.
list=$(profiles | while IFS= read -r profile; do candidates "$profile"; done)
printf '%s\n' "$list" |
    while IFS="$(printf '\t')" read -r kind path; do
        [ -n "$path" ] || continue
        kib=$(size_kib "$path")
        if [ "$mode" = delete ]; then
            rm -rf "$path"
            # A crate folder left with no units goes too.
            if [ "$kind" = unit ]; then
                rmdir "${path%/*}" 2>/dev/null || true
            fi
        fi
        printf '%s\t%s\t%s\n' "$kind" "${kib:-0}" "$path"
    done |
    awk -F '\t' -v verb="$verb" -v target="$target" '
        function human(k, i) {
            i = 1
            while (k >= 1024 && i < 4) { k /= 1024; i++ }
            return sprintf(i == 1 ? "%d%s" : "%.1f%s", k, substr("KMGT", i, 1))
        }
        { printf "%s\t%s\t%s\t%s\n", verb, $1, human($2), $3; sum += $2 }
        END { printf "total\t%s\t%s\n", human(sum + 0), target }
    '

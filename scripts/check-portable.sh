#!/bin/sh
# Checks that the portable crates use no files, processes, threads, or
# clocks (AGENTS.md, hard rule 6). They must work on wasm32-unknown-unknown,
# where `std` builds these calls as stubs that fail or panic when they run.
# So `mise run wasm` passes even when one is used, and only a look at the
# source can catch it.
#
# Under each portable crate's `src`, a line fails when it names
# `fs::`, `process::`, `thread::`, `Instant`, or `SystemTime`, or is a `use`
# line that names `fs`, `process`, or `thread`. Matching the short path
# catches grouped imports, such as `use std::{fs, io};` followed by
# `fs::read(..)`. Matching `use` lines and `fs as` catches renamed
# imports, such as `use std::fs as disk;`, also inside a `use` group that
# rustfmt splits over several lines. Comment lines are skipped. A crate
# folder with no `src`, or a folder or file the check cannot read, fails
# the check, so no source is skipped without notice.
#
# Test files are skipped: `tests.rs`, `*_tests.rs`, and files under a
# `tests` folder. Test placement (scripts/check-test-placement.sh) keeps
# all test code in those files, and test code never builds for wasm.
#
# Why a grep and not clippy: clippy's `disallowed-methods` names single
# functions, and this rule bans whole modules.
#
# Usage: scripts/check-portable.sh [CRATE_DIR...]
# With no arguments, checks the five portable crates.

set -eu

if [ "$#" -eq 0 ]; then
    set -- crates/loom-core crates/loom-dsp crates/loom-modules crates/loom-patch crates/loom-rt
fi

word='([^A-Za-z0-9_]|$)'
pattern="(^|[^A-Za-z0-9_])(fs|process|thread)::|(^|[^A-Za-z0-9_])(Instant|SystemTime)$word"
pattern="$pattern|^[[:space:]]*(pub(\\([a-z]+\\))? )?use [^;]*[^A-Za-z0-9_](fs|process|thread)$word"
pattern="$pattern|(^|[^A-Za-z0-9_])(fs|process|thread) as [A-Za-z_]"

for crate in "$@"; do
    if [ ! -d "$crate/src" ]; then
        echo "No source folder to check: $crate/src" >&2
        exit 1
    fi
done

# Listed first, on its own, so a `find` error stops the check instead of
# being hidden inside the pipe below.
files=$(
    for crate in "$@"; do
        find "$crate/src" -name '*.rs' ! -name 'tests.rs' ! -name '*_tests.rs' ! -path '*/tests/*' || exit 1
    done
) || {
    echo "Could not list every source file to check." >&2
    exit 1
}

# grep exits 1 when nothing matches and 2 when it cannot read the file.
# Only the first is a pass. The loop reads a here-document, not a pipe, so
# `exit` leaves the script.
matches=""
while read -r file; do
    [ -n "$file" ] || continue
    if lines=$(grep -n -E "$pattern" "$file" /dev/null); then
        matches="$matches$lines
"
    elif [ "$?" -ne 1 ]; then
        echo "Could not read $file" >&2
        exit 1
    fi
done <<SOURCES
$(printf '%s\n' "$files" | sort)
SOURCES

found=$(printf '%s' "$matches" | grep -v -E '^[^:]+:[0-9]+:[[:space:]]*//' || true)

if [ -n "$found" ]; then
    echo "$found"
    echo "A portable crate uses files, processes, threads, or a clock (AGENTS.md, hard rule 6)." >&2
    echo "Move the code to a desktop crate, such as loom-app, and pass the result in." >&2
    exit 1
fi

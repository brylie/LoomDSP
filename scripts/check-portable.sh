#!/bin/sh
# Checks that the portable crates use no files, processes, threads, or
# clocks (AGENTS.md, hard rule 6). They must work on wasm32-unknown-unknown,
# where `std` builds these calls as stubs that fail or panic when they run.
# So `mise run wasm` passes even when one is used, and only a look at the
# source can catch it.
#
# Under each portable crate's `src`, a line fails when it names
# `fs::`, `process::`, `thread::`, `Instant`, or `SystemTime`. Matching the
# short path catches grouped imports too, such as `use std::{fs, io};`
# followed by `fs::read(..)`. Comment lines are skipped.
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

pattern='(^|[^A-Za-z0-9_])(fs|process|thread)::|(^|[^A-Za-z0-9_])(Instant|SystemTime)([^A-Za-z0-9_]|$)'

found=$(
    for crate in "$@"; do
        find "$crate/src" -name '*.rs' ! -name 'tests.rs' ! -name '*_tests.rs' ! -path '*/tests/*'
    done | sort | while read -r file; do
        grep -n -E "$pattern" "$file" /dev/null || true
    done | grep -v -E '^[^:]+:[0-9]+:[[:space:]]*//' || true
)

if [ -n "$found" ]; then
    echo "$found"
    echo "A portable crate uses files, processes, threads, or a clock (AGENTS.md, hard rule 6)." >&2
    echo "Move the code to a desktop crate, such as loom-app, and pass the result in." >&2
    exit 1
fi

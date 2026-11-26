#!/bin/sh
# Checks that each crate builds its integration tests as few binaries as it
# can, and that no test file is left out (CONTRIBUTING.md, Where tests live).
#
# Each file at the top of a crate's `tests/` folder is either:
#
# - a module of `tests/main.rs`, declared there with `mod name;`, or
# - a test binary of its own. Only a file that installs a global allocator
#   needs that, because an allocator applies to its whole binary. One plain
#   file alone is also fine: it is already a single binary.
#
# A crate with `tests/main.rs` sets `autotests = false` and lists its test
# binaries as `[[test]]` targets. Then rustc and Cargo both ignore a file
# that is neither listed nor declared, and its tests never run, so this
# check fails on one.
#
# Usage: scripts/check-test-targets.sh

set -eu

# Prints the `path` of each `[[test]]` target in a manifest, one per line.
# Only active lines count: a commented-out target builds nothing, so a
# plain text search would let its tests vanish unnoticed.
test_paths() {
    awk '
        { sub(/#.*$/, "") }
        /^[[:space:]]*\[/ {
            in_test = ($0 ~ /^[[:space:]]*\[\[test\]\][[:space:]]*$/)
            next
        }
        in_test && /^[[:space:]]*path[[:space:]]*=/ {
            path = $0
            sub(/^[^"]*"/, "", path)
            sub(/".*$/, "", path)
            print path
        }
    ' "$1"
}

fail=0
for dir in crates/*/; do
    crate=${dir%/}
    tests=$crate/tests
    manifest=$crate/Cargo.toml
    [ -d "$tests" ] || continue
    if grep -Eq '^autotests[[:space:]]*=[[:space:]]*false' "$manifest"; then
        autotests=0
    else
        autotests=1
    fi
    main=$tests/main.rs
    targets=$(test_paths "$manifest")
    if [ -f "$main" ] && [ "$autotests" = 1 ]; then
        echo "$manifest: has tests/main.rs, so it needs \`autotests = false\` and a [[test]] target for it."
        fail=1
    fi
    if [ -f "$main" ] && ! printf '%s\n' "$targets" | grep -Fxq 'tests/main.rs'; then
        echo "$manifest: tests/main.rs is not a [[test]] target, so its tests never run."
        fail=1
    fi
    alone=0
    for file in "$tests"/*.rs; do
        [ -f "$file" ] || continue
        name=$(basename "$file" .rs)
        [ "$name" = main ] && continue
        declared=0
        listed=0
        if [ -f "$main" ] && grep -Eq "^(pub )?mod $name;" "$main"; then
            declared=1
        fi
        if [ "$autotests" = 1 ] || printf '%s\n' "$targets" | grep -Fxq "tests/$name.rs"; then
            listed=1
        fi
        if [ "$declared" = 1 ] && [ "$listed" = 1 ]; then
            echo "$file: is both a module of tests/main.rs and a test binary, so it builds twice."
            fail=1
        elif [ "$declared" = 0 ] && [ "$listed" = 0 ]; then
            echo "$file: is not declared in tests/main.rs or listed as a [[test]] target, so its tests never run. Add \`mod $name;\` to tests/main.rs."
            fail=1
        elif [ "$listed" = 1 ] && ! grep -Fq '#[global_allocator]' "$file"; then
            alone=$((alone + 1))
            if [ -f "$main" ]; then
                echo "$file: is a test binary of its own but installs no global allocator. Make it a module of tests/main.rs."
                fail=1
            fi
        fi
    done
    if [ ! -f "$main" ] && [ "$alone" -gt 1 ]; then
        echo "$tests: $alone test files build a binary each. Make them modules of one tests/main.rs, so the crate links once."
        fail=1
    fi
done

if [ "$fail" = 0 ]; then
    echo "Every integration test file is built, in as few binaries as it can be."
fi
exit "$fail"

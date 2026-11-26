#!/bin/sh
# Exercise check-test-targets.sh on small crates in a temporary folder.
set -eu

# Usage: scripts/test-check-test-targets.sh [CHECK_SCRIPT]
script=$(cd "$(dirname "${1:-$0}")" && pwd -P)/$(basename "${1:-check-test-targets.sh}")
scratch=$(mktemp -d "${TMPDIR:-/tmp}/loom-test-targets.XXXXXX")
trap 'rm -rf "$scratch"' EXIT HUP INT TERM

# Writes one case: a manifest, then test files from `name=content` pairs.
case_dir() {
    rm -rf "$scratch/case"
    mkdir -p "$scratch/case/crates/a/tests"
    printf '%b' "$1" > "$scratch/case/crates/a/Cargo.toml"
    shift
    while [ "$#" -gt 0 ]; do
        printf '%b' "$2" > "$scratch/case/crates/a/tests/$1"
        shift 2
    done
}

passes() {
    if ! (cd "$scratch/case" && sh "$script") > /dev/null; then
        echo "expected to pass: $1" >&2
        exit 1
    fi
}

fails() {
    if (cd "$scratch/case" && sh "$script") > /dev/null; then
        echo "expected to fail: $1" >&2
        exit 1
    fi
}

plain='[package]\nname = "a"\n'
merged='[package]\nname = "a"\nautotests = false\n\n[[test]]\nname = "integration"\npath = "tests/main.rs"\n'
alloc='#[global_allocator]\nstatic A: X = X;\n'

case_dir "$plain" one.rs '#[test]\nfn t() {}\n'
passes "one plain file is one binary already"

case_dir "$plain" one.rs '' two.rs ''
fails "two plain files build two binaries"

case_dir "$plain" one.rs '' no_alloc.rs "$alloc"
passes "a file with a global allocator keeps a binary of its own"

case_dir "$merged" main.rs 'mod one;\nmod two;\n' one.rs '' two.rs ''
passes "files declared in tests/main.rs"

case_dir "$merged" main.rs 'mod one;\n' one.rs '' two.rs ''
fails "a file neither declared nor listed never runs"

case_dir "$merged"'\n[[test]]\nname = "no_alloc"\npath = "tests/no_alloc.rs"\n' main.rs 'mod one;\n' one.rs '' no_alloc.rs "$alloc"
passes "a listed file with a global allocator"

case_dir "$merged"'\n[[test]]\nname = "two"\npath = "tests/two.rs"\n' main.rs 'mod one;\n' one.rs '' two.rs ''
fails "a listed file with no global allocator belongs in tests/main.rs"

case_dir "$merged"'\n[[test]]\nname = "one"\npath = "tests/one.rs"\n' main.rs 'mod one;\n' one.rs ''
fails "a file that is both a module and a binary builds twice"

case_dir '[package]\nname = "a"\nautotests = false\n' main.rs 'mod one;\n' one.rs ''
fails "tests/main.rs that is not a target never runs"

case_dir "$plain" main.rs 'mod one;\n' one.rs ''
fails "tests/main.rs with autotests on builds one.rs twice"

echo "check-test-targets.sh behaves as expected."

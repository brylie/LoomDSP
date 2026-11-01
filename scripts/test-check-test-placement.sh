#!/bin/sh
# Exercise check-test-placement.sh on small crates in a temporary folder.
set -eu

# Usage: scripts/test-check-test-placement.sh [CHECK_SCRIPT]
script=$(cd "$(dirname "${1:-$0}")" && pwd -P)/$(basename "${1:-check-test-placement.sh}")
scratch=$(mktemp -d "${TMPDIR:-/tmp}/loom-test-placement.XXXXXX")
trap 'rm -rf "$scratch"' EXIT HUP INT TERM

# Writes one case: crate files from `name=content` pairs, run from a clean folder.
case_dir() {
    rm -rf "$scratch/case"
    mkdir -p "$scratch/case/crates/a/src"
    while [ "$#" -gt 0 ]; do
        file=$1
        mkdir -p "$(dirname "$scratch/case/crates/a/src/$file")"
        printf '%b' "$2" > "$scratch/case/crates/a/src/$file"
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

case_dir lib.rs '#[cfg(test)]\nmod tests;\n' tests.rs '#[test]\nfn t() {}\n'
passes "tests in their own file"

case_dir lib.rs '#[cfg(test)]\n/// Tests.\n\nmod tests;\n' tests.rs '#[test]\nfn t() {}\n'
passes "a doc comment between the attribute and the declaration"

case_dir foo.rs '#[cfg(test)]\nmod tests;\n' foo/tests.rs '#[test]\nfn t() {}\n' lib.rs 'mod foo;\n'
passes "tests for foo.rs in foo/tests.rs"

case_dir lib.rs 'pub mod pitch {\n    #[cfg(test)]\n    mod tests;\n}\n' pitch/tests.rs '#[test]\nfn t() {}\n'
passes "a test file of an inline module"

case_dir lib.rs '#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n'
fails "an inline test module"

case_dir lib.rs '#[cfg(test)]\n/// A fake host.\npub mod fake {\n}\n'
fails "an inline fake after a doc comment"

case_dir lib.rs '#[cfg(all(test, unix))]\nmod tests {\n}\n'
fails "an inline platform-only test module"

case_dir lib.rs '#[test]\nfn t() {}\n'
fails "a test in a code file"

echo "check-test-placement.sh behaves as expected."

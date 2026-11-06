#!/bin/sh
# Exercise check-portable.sh on a small crate in a temporary folder.
set -eu

# Usage: scripts/test-check-portable.sh [CHECK_SCRIPT]
script=$(cd "$(dirname "${1:-$0}")" && pwd -P)/$(basename "${1:-check-portable.sh}")
scratch=$(mktemp -d "${TMPDIR:-/tmp}/loom-portable.XXXXXX")
trap 'rm -rf "$scratch"' EXIT HUP INT TERM

# Writes one case: crate files from `name=content` pairs.
case_dir() {
    rm -rf "$scratch/a"
    mkdir -p "$scratch/a/src"
    while [ "$#" -gt 0 ]; do
        mkdir -p "$(dirname "$scratch/a/src/$1")"
        printf '%b' "$2" > "$scratch/a/src/$1"
        shift 2
    done
}

passes() {
    if ! sh "$script" "$scratch/a" > /dev/null 2>&1; then
        echo "expected to pass: $1" >&2
        exit 1
    fi
}

fails() {
    if sh "$script" "$scratch/a" > /dev/null 2>&1; then
        echo "expected to fail: $1" >&2
        exit 1
    fi
}

case_dir lib.rs 'pub fn f() -> u32 { 1 }\n'
passes "plain code"

case_dir lib.rs 'pub fn f() { let _ = std::fs::read("x"); }\n'
fails "a std::fs call"

case_dir lib.rs 'use std::{fs, io};\npub fn f() { let _ = fs::read("x"); }\n'
fails "a grouped import of fs"

case_dir lib.rs 'pub fn f() -> u32 { std::process::id() }\n'
fails "a process call"

case_dir lib.rs 'use std::thread;\npub fn f() { thread::yield_now(); }\n'
fails "a thread call"

case_dir lib.rs 'use std::time::{Duration, Instant};\n'
fails "a grouped import of Instant"

case_dir lib.rs 'pub fn f() { let _ = std::time::SystemTime::now(); }\n'
fails "SystemTime"

case_dir lib.rs '/// Reads nothing: no `std::fs` here.\n// thread::spawn is not used.\npub fn f() {}\n'
passes "comments that name the calls"

case_dir lib.rs 'pub fn process_block() {}\npub struct Instants;\npub fn f(x: &dyn Module) { x.process(); }\npub trait Module { fn process(&self); }\n'
passes "names that only contain the words"

case_dir lib.rs '#[cfg(test)]\nmod tests;\n' tests.rs '#[test]\nfn t() { let _ = std::fs::read("x"); }\n'
passes "a tests.rs file"

case_dir lib.rs 'mod foo;\n' foo.rs '#[cfg(test)]\nmod tests;\n' foo/tests/data.rs 'pub fn d() { let _ = std::fs::read("x"); }\n'
passes "a file under a tests folder"

case_dir lib.rs '#[cfg(test)]\nmod slew_tests;\n' slew_tests.rs '#[test]\nfn t() { let _ = std::fs::read("x"); }\n'
passes "a *_tests.rs file"

case_dir lib.rs 'mod foo;\n' foo.rs 'pub fn f() { let _ = std::fs::read("x"); }\n'
fails "a call in a module file"

echo "check-portable.sh: all cases behave"

#!/bin/sh
# Checks that tests live in their own files, so code modules hold only code
# (CONTRIBUTING.md, Where tests live). Under each crate's `src`:
#
# - No inline test module: `#[cfg(test)]` (or `#[cfg(all(test, ...))]`)
#   followed by `mod name {`. This includes test fakes. Write
#   `#[cfg(test)] mod name;` and put the code in its own file.
# - No `#[test]` outside a test file. A test file is one declared with
#   `#[cfg(test)] mod name;`, or a file under that module's folder.
#
# Integration tests in `crates/*/tests` and benches are not checked: they
# are test files already. CI runs this.
#
# Usage: scripts/check-test-placement.sh

set -eu

fail=0
test_files=$(mktemp)
trap 'rm -f "$test_files"' EXIT

sources=$(find crates/*/src -name '*.rs' | sort)

# Inline test modules, and the files that test module declarations point to.
for file in $sources; do
    dir=$(dirname "$file")
    case $(basename "$file") in
        lib.rs | main.rs | mod.rs) base=$dir ;;
        *) base=${file%.rs} ;;
    esac
    # Attributes may sit between `#[cfg(test)]` and the `mod` line.
    # Inline modules nest their children in a folder of the same name.
    awk -v file="$file" -v base="$base" '
        # `#[cfg(test)]`, or `#[cfg(all(test, ...))]` for platform-only tests.
        /^[[:space:]]*#\[cfg\((test|all\(test[,)].*)\)\][[:space:]]*$/ { pending = 1; next }
        # Doc comments, comments, and blank lines may sit there too.
        pending && /^[[:space:]]*(#\[|\/\/|$)/ { next }
        pending {
            pending = 0
            if (match($0, /^[[:space:]]*(pub(\([a-z]+\))? )?mod [a-z_0-9]+ *\{/)) {
                printf "INLINE %s:%d\n", file, NR
            } else if (match($0, /^[[:space:]]*(pub(\([a-z]+\))? )?mod [a-z_0-9]+;/)) {
                name = $0
                sub(/^[[:space:]]*(pub(\([a-z]+\))? )?mod /, "", name)
                sub(/;.*/, "", name)
                prefix = base
                for (i = 1; i <= depth; i++) prefix = prefix "/" stack[i]
                printf "TEST %s/%s\n", prefix, name
            }
        }
        # Track inline (non-test) modules so nested declarations resolve.
        /^[[:space:]]*(pub(\([a-z]+\))? )?mod [a-z_0-9]+ *\{/ {
            name = $0
            sub(/^[[:space:]]*(pub(\([a-z]+\))? )?mod /, "", name)
            sub(/ *\{.*/, "", name)
            indent = match($0, /[^[:space:]]/) - 1
            stack[++depth] = name
            indents[depth] = indent
        }
        /^[[:space:]]*\}[[:space:]]*$/ && depth > 0 {
            indent = match($0, /[^[:space:]]/) - 1
            if (indent == indents[depth]) depth--
        }
    ' "$file"
done > "$test_files"

if grep -q '^INLINE ' "$test_files"; then
    grep '^INLINE ' "$test_files" | while read -r _ where; do
        echo "$where: inline test module. Move it to its own file and declare it with \`#[cfg(test)] mod name;\`."
    done
    fail=1
fi

# `#[test]` outside a test file or its folder.
for file in $sources; do
    grep -q '#\[test\]' "$file" || continue
    stem=${file%.rs}
    stem=${stem%/mod}
    allowed=0
    for test in $(sed -n 's/^TEST //p' "$test_files"); do
        case $stem in
            "$test" | "$test"/*) allowed=1 ;;
        esac
    done
    if [ "$allowed" = 0 ]; then
        echo "$file: has #[test] but is not a test file. Move the tests to a file declared with \`#[cfg(test)] mod name;\`."
        fail=1
    fi
done

if [ "$fail" = 0 ]; then
    echo "Tests are in their own files."
fi
exit "$fail"

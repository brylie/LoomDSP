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

# One awk pass over every file. Starting a process per file was most of
# the cost: about 2,800 processes for 631 files, which took 40 s or more on
# a loaded machine (#863).
sources=$(find crates/*/src -name '*.rs' | sort)
[ -n "$sources" ] || { echo "Tests are in their own files."; exit 0; }

# Unquoted on purpose: one argument per file. Crate paths have no spaces.
awk '
    FNR == 1 {
        nfiles++
        files[nfiles] = FILENAME
        dir = FILENAME
        sub(/\/[^\/]*$/, "", dir)
        name = FILENAME
        sub(/^.*\//, "", name)
        base = (name == "lib.rs" || name == "main.rs" || name == "mod.rs") ? dir : substr(FILENAME, 1, length(FILENAME) - 3)
        pending = 0
        depth = 0
    }
    # Files with a test function. Checked before the rules below skip lines.
    /#\[test\]/ { has_test[FILENAME] = 1 }
    # `#[cfg(test)]`, or `#[cfg(all(test, ...))]` for platform-only tests.
    # Attributes may sit between it and the `mod` line.
    /^[[:space:]]*#\[cfg\((test|all\(test[,)].*)\)\][[:space:]]*$/ { pending = 1; next }
    # Doc comments, comments, and blank lines may sit there too.
    pending && /^[[:space:]]*(#\[|\/\/|$)/ { next }
    pending {
        pending = 0
        if (match($0, /^[[:space:]]*(pub(\([a-z]+\))? )?mod [a-z_0-9]+ *\{/)) {
            inline[++ninline] = FILENAME ":" FNR
        } else if (match($0, /^[[:space:]]*(pub(\([a-z]+\))? )?mod [a-z_0-9]+;/)) {
            mod = $0
            sub(/^[[:space:]]*(pub(\([a-z]+\))? )?mod /, "", mod)
            sub(/;.*/, "", mod)
            prefix = base
            for (i = 1; i <= depth; i++) prefix = prefix "/" stack[i]
            tests[++ntests] = prefix "/" mod
        }
    }
    # Track inline (non-test) modules so nested declarations resolve.
    # Inline modules nest their children in a folder of the same name.
    /^[[:space:]]*(pub(\([a-z]+\))? )?mod [a-z_0-9]+ *\{/ {
        mod = $0
        sub(/^[[:space:]]*(pub(\([a-z]+\))? )?mod /, "", mod)
        sub(/ *\{.*/, "", mod)
        stack[++depth] = mod
        indents[depth] = match($0, /[^[:space:]]/) - 1
    }
    /^[[:space:]]*\}[[:space:]]*$/ && depth > 0 {
        if (match($0, /[^[:space:]]/) - 1 == indents[depth]) depth--
    }
    END {
        fail = 0
        for (i = 1; i <= ninline; i++) {
            print inline[i] ": inline test module. Move it to its own file and declare it with `#[cfg(test)] mod name;`."
            fail = 1
        }
        # `#[test]` outside a test file or its folder.
        for (f = 1; f <= nfiles; f++) {
            file = files[f]
            if (!(file in has_test)) continue
            stem = substr(file, 1, length(file) - 3)
            sub(/\/mod$/, "", stem)
            allowed = 0
            for (t = 1; t <= ntests && !allowed; t++) {
                if (stem == tests[t] || substr(stem, 1, length(tests[t]) + 1) == tests[t] "/") allowed = 1
            }
            if (!allowed) {
                print file ": has #[test] but is not a test file. Move the tests to a file declared with `#[cfg(test)] mod name;`."
                fail = 1
            }
        }
        if (!fail) print "Tests are in their own files."
        exit fail
    }
' $sources

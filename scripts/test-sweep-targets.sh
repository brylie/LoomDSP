#!/bin/sh
# Exercise the sweep of stale build output on fake target folders, in an
# isolated temporary repository.
set -eu

scripts_dir=$(cd "$(dirname "$0")" && pwd -P)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/loom-sweep-targets.XXXXXX")
scratch=$(cd "$scratch" && pwd -P)
holder=""
cleanup() {
    # Stop only the lock holder this test started, by its own ID.
    if [ -n "$holder" ]; then
        kill "$holder" 2>/dev/null || true
    fi
    rm -rf "$scratch"
}
trap cleanup EXIT HUP INT TERM
repo="$scratch/repo with spaces"
targets="$scratch/target root with spaces"
worktree="$scratch/worktree with spaces"
mkdir -p "$repo" "$targets"
git -C "$repo" init -q
git -C "$repo" -c user.name=Test -c user.email=test@example.invalid commit -q --allow-empty -m init
git -C "$repo" worktree add -q -b test-worktree "$worktree"
"$scripts_dir/setup-worktree-target.sh" "$targets" "$worktree" >/dev/null
key=$(printf '%s' "$worktree" | git hash-object --stdin | cut -c 1-16)
target="$targets/$key"

fail() {
    echo "$*" >&2
    exit 1
}

# Write a file, making its folders.
put() {
    mkdir -p "$(dirname "$1")"
    printf 'build output\n' > "$1"
}

# A long-lived target with every kind of folder the sweep looks at.
debug="$target/debug"
for lock in .cargo-lock .cargo-build-lock .cargo-artifact-lock; do
    put "$debug/$lock"
done
old_unit="$debug/build/foo/aaaaaaaaaaaaaaaa"
fresh_unit="$debug/build/foo/bbbbbbbbbbbbbbbb"
touched_unit="$debug/build/bar/cccccccccccccccc"
only_unit="$debug/build/gone/eeeeeeeeeeeeeeee"
old_script="$debug/build/bar-dddddddddddddddd"
old_session="$debug/incremental/foo-0abc"
fresh_session="$debug/incremental/bar-1def"
for unit in "$old_unit" "$fresh_unit" "$touched_unit" "$only_unit"; do
    put "$unit/fingerprint/lib"
    put "$unit/out/lib.rlib"
done
put "$old_script/out/generated.rs"
put "$old_script/output"
put "$debug/deps/libfoo-aaaaaaaaaaaaaaaa.rlib"
put "$debug/.fingerprint/foo-aaaaaaaaaaaaaaaa/lib"
put "$old_session/s-1-abc/query-cache.bin"
put "$old_session/s-1-abc.lock"
put "$fresh_session/s-2-def/query-cache.bin"
put "$debug/loom-app"
# A profile still on the layout before Cargo 1.100 keeps that layout, since
# an older Cargo would read it. Its incremental state is swept all the same.
release="$target/release"
put "$release/.cargo-lock"
put "$release/deps/libbaz-ffffffffffffffff.rlib"
put "$release/build/baz-ffffffffffffffff/output"
put "$release/incremental/baz-2abc/s-3-abc/query-cache.bin"
# Profile folders are found by their lock files, at any depth Cargo uses.
coverage_unit="$target/llvm-cov-target/debug/build/foo/9999999999999999"
put "$target/llvm-cov-target/debug/.cargo-lock"
put "$coverage_unit/fingerprint/lib"

# A target that is not live and one with no marker are never swept.
orphan_key=$(printf '%s' "$scratch/removed worktree" | git hash-object --stdin | cut -c 1-16)
orphan_unit="$targets/$orphan_key/debug/build/foo/aaaaaaaaaaaaaaaa"
put "$targets/$orphan_key/debug/.cargo-lock"
put "$orphan_unit/fingerprint/lib"
common=$(git -C "$repo" rev-parse --path-format=absolute --git-common-dir)
common=$(cd "$common" && pwd -P)
printf 'LoomDSP worktree target v1\n%s\n%s\n' "$scratch/removed worktree" "$common" > "$targets/$orphan_key/.loomdsp-worktree-target"
unknown_unit="$targets/unmarked legacy/debug/build/foo/aaaaaaaaaaaaaaaa"
put "$targets/unmarked legacy/debug/.cargo-lock"
put "$unknown_unit/fingerprint/lib"

# Age everything, folders included, then bring back what a recent build
# wrote: two whole folders, and one file in an otherwise old unit. Appending
# changes the file and leaves its folder's own time old.
find "$targets" -exec touch -t 202001010000 {} +
for fresh in "$fresh_unit" "$fresh_session"; do
    find "$fresh" -exec touch {} +
done
printf 'rebuilt\n' >> "$touched_unit/out/lib.rlib"

# A symlink in a build folder is never followed or removed.
mkdir "$scratch/outside"
put "$scratch/outside/keep"
touch -t 202001010000 "$scratch/outside" "$scratch/outside/keep"
ln -s "$scratch/outside" "$debug/build/linked"

swept="$old_unit
$only_unit
$old_script
$debug/deps
$debug/.fingerprint
$old_session
$release/incremental/baz-2abc
$coverage_unit"
kept="$fresh_unit
$touched_unit
$fresh_session
$debug/loom-app
$release/deps
$release/build/baz-ffffffffffffffff
$orphan_unit
$unknown_unit
$scratch/outside/keep"

# Each path in the list exists, or none does.
all_exist() {
    printf '%s\n' "$1" | while IFS= read -r path; do
        [ -e "$path" ] || fail "missing: $path"
    done
}
none_exist() {
    printf '%s\n' "$1" | while IFS= read -r path; do
        [ ! -e "$path" ] || fail "not swept: $path"
    done
}

# The flags are checked before anything is read.
for args in "--delete" "--days 3" "--sweep --days 0" "--sweep --days x" "--prune --sweep" "--sweep --bogus"; do
    # shellcheck disable=SC2086
    if "$scripts_dir/worktree-targets.sh" "$targets" $args >/dev/null 2>&1; then
        fail "accepted: $args"
    fi
done

cd "$repo"
# The dry run lists what it would remove, with sizes, and removes nothing.
"$scripts_dir/worktree-targets.sh" "$targets" --sweep > "$scratch/dry.txt"
grep -Fq "active	" "$scratch/dry.txt" || fail 'dry run lists no active target'
grep -Fq "total	" "$scratch/dry.txt" || fail 'dry run gives no total'
if grep -Eq '^(would prune|pruned)	' "$scratch/dry.txt"; then
    fail 'sweep mode pruned or offered to prune'
fi
grep '^would sweep	' "$scratch/dry.txt" | while IFS="$(printf '\t')" read -r _ kind size path; do
    [ -n "$kind" ] && [ -n "$size" ] && [ -n "$path" ] || fail "incomplete line for $path"
done
awk -F '\t' '$1 == "would sweep" { print $4 }' "$scratch/dry.txt" | sort > "$scratch/dry-paths.txt"
printf '%s\n' "$swept" | sort > "$scratch/expected-paths.txt"
cmp -s "$scratch/dry-paths.txt" "$scratch/expected-paths.txt" ||
    fail "dry run listed the wrong folders: $(diff "$scratch/expected-paths.txt" "$scratch/dry-paths.txt")"
grep -Fq "would sweep	unit	" "$scratch/dry.txt" || fail 'no unit kind'
grep -Fq "would sweep	incremental	" "$scratch/dry.txt" || fail 'no incremental kind'
grep -Fq "would sweep	old-layout	" "$scratch/dry.txt" || fail 'no old-layout kind'
all_exist "$swept"
all_exist "$kept"

# While a build holds one of Cargo's locks, the target is skipped whole,
# even by a delete run. This holder takes the lock as Cargo does, with
# flock(2), and says when it has it.
perl -e '
    use Fcntl qw(:flock);
    open(my $lock, "<", $ARGV[0]) or die "open: $!";
    flock($lock, LOCK_EX) or die "flock: $!";
    open(my $ready, ">", $ARGV[1]) or die "ready: $!";
    close $ready;
    sleep 60;
' "$debug/.cargo-build-lock" "$scratch/held" &
holder=$!
tries=0
while [ ! -e "$scratch/held" ]; do
    tries=$((tries + 1))
    [ "$tries" -le 100 ] || fail 'the lock holder never took the lock'
    sleep 0.1
done
"$scripts_dir/worktree-targets.sh" "$targets" --sweep --delete > "$scratch/busy.txt"
grep -Fxq "busy	$target" "$scratch/busy.txt" || fail 'a held lock did not make the target busy'
if grep -q '^swept	' "$scratch/busy.txt"; then
    fail 'swept a target whose lock was held'
fi
all_exist "$swept"
"$scripts_dir/worktree-targets.sh" "$targets" --sweep > "$scratch/busy-dry.txt"
grep -Fxq "busy	$target" "$scratch/busy-dry.txt" || fail 'a dry run read a target whose lock was held'
kill "$holder"
wait "$holder" 2>/dev/null || true
holder=""

# The delete run removes exactly what the dry run listed, and nothing else.
"$scripts_dir/worktree-targets.sh" "$targets" --sweep --delete > "$scratch/delete.txt"
awk -F '\t' '$1 == "swept" { print $4 }' "$scratch/delete.txt" | sort > "$scratch/delete-paths.txt"
cmp -s "$scratch/delete-paths.txt" "$scratch/dry-paths.txt" ||
    fail "delete run removed other folders than the dry run listed: $(diff "$scratch/dry-paths.txt" "$scratch/delete-paths.txt")"
none_exist "$swept"
all_exist "$kept"
# A crate folder left with no units goes too; one with units stays.
[ ! -e "$debug/build/gone" ] || fail 'left an empty crate folder'
[ -d "$debug/build/foo" ] || fail 'removed a crate folder that still has units'
[ -L "$debug/build/linked" ] || fail 'removed a symlink'
[ -f "$target/.loomdsp-worktree-target" ] || fail 'removed the ownership marker'
for lock in .cargo-lock .cargo-build-lock .cargo-artifact-lock; do
    [ -f "$debug/$lock" ] || fail "removed $lock"
done

# A second sweep finds nothing left.
"$scripts_dir/worktree-targets.sh" "$targets" --sweep > "$scratch/again.txt"
if grep -q '^would sweep	' "$scratch/again.txt"; then
    fail 'a second sweep still found stale folders'
fi
grep -Fxq "total	0K	$target" "$scratch/again.txt" || fail 'a second sweep gave a total above zero'

# A longer age keeps what a shorter one would take.
put "$old_unit/fingerprint/lib"
touch -t 202001010000 "$old_unit" "$old_unit/fingerprint" "$old_unit/fingerprint/lib"
"$scripts_dir/worktree-targets.sh" "$targets" --sweep --days 100000 > "$scratch/long.txt"
if grep -q '^would sweep	' "$scratch/long.txt"; then
    fail '--days did not set the age'
fi

printf 'sweep target tests passed\n'

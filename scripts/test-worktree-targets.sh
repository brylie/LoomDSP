#!/bin/sh
# Exercise target ownership and pruning in an isolated temporary repository.
set -eu

scripts_dir=$(cd "$(dirname "$0")" && pwd -P)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/loom-worktree-targets.XXXXXX")
scratch=$(cd "$scratch" && pwd -P)
trap 'rm -rf "$scratch"' EXIT HUP INT TERM
repo="$scratch/repo with spaces"
targets="$scratch/target root with spaces"
worktree="$scratch/worktree with spaces"
mkdir -p "$repo" "$targets"
git -C "$repo" init -q
git -C "$repo" -c user.name=Test -c user.email=test@example.invalid commit -q --allow-empty -m init
git -C "$repo" worktree add -q -b test-worktree "$worktree"

key=$(printf '%s' "$worktree" | git hash-object --stdin | cut -c 1-16)
target="$targets/$key"
"$scripts_dir/setup-worktree-target.sh" "$targets" "$worktree" >/dev/null
[ -f "$target/.loomdsp-worktree-target" ]
grep -Fxq "$worktree" "$target/.loomdsp-worktree-target"

# A generated config can safely adopt its own old unmarked target.
rm "$target/.loomdsp-worktree-target"
"$scripts_dir/setup-worktree-target.sh" "$targets" "$worktree" >/dev/null
[ -f "$target/.loomdsp-worktree-target" ]

mkdir "$targets/unmarked legacy"
printf 'keep\n' > "$targets/unmarked legacy/build"
mkdir "$scratch/unrelated"
printf 'keep\n' > "$scratch/unrelated/build"
ln -s "$scratch/unrelated" "$targets/symlink target"
foreign_worktree="$scratch/foreign worktree"
foreign_key=$(printf '%s' "$foreign_worktree" | git hash-object --stdin | cut -c 1-16)
mkdir "$targets/$foreign_key"
printf 'LoomDSP worktree target v1\n%s\n%s\n' "$foreign_worktree" "$scratch/other-repository" > "$targets/$foreign_key/.loomdsp-worktree-target"

cd "$repo"
"$scripts_dir/worktree-targets.sh" "$targets" > "$scratch/live.txt"
grep -Fq "active	" "$scratch/live.txt"
grep -Fq "$target" "$scratch/live.txt"
grep -Fq "unknown	" "$scratch/live.txt"
grep -Fq "$targets/unmarked legacy" "$scratch/live.txt"
grep -Fq "$targets/symlink target" "$scratch/live.txt"
[ -d "$target" ]
"$scripts_dir/worktree-targets.sh" "$targets" --prune > "$scratch/active-prune.txt"
[ -d "$target" ]
[ -f "$targets/unmarked legacy/build" ]
[ -f "$scratch/unrelated/build" ]

git worktree remove --force "$worktree"
"$scripts_dir/worktree-targets.sh" "$targets" > "$scratch/dry-run.txt"
grep -Fq "orphan	" "$scratch/dry-run.txt"
grep -Fq "would prune	$target" "$scratch/dry-run.txt"
[ -d "$target" ]

"$scripts_dir/worktree-targets.sh" "$targets" --prune > "$scratch/prune.txt"
grep -Fq "pruned	$target" "$scratch/prune.txt"
[ ! -e "$target" ]
[ -f "$targets/unmarked legacy/build" ]
[ -f "$scratch/unrelated/build" ]
[ -f "$targets/$foreign_key/.loomdsp-worktree-target" ]
[ -L "$targets/symlink target" ]

printf 'worktree target tests passed\n'

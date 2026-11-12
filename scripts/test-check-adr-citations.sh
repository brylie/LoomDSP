#!/bin/sh
# Exercise check-adr-citations.sh on small repositories in a temporary folder.
set -eu

# Usage: scripts/test-check-adr-citations.sh [CHECK_SCRIPT]
script=$(cd "$(dirname "${1:-$0}")" && pwd -P)/$(basename "${1:-check-adr-citations.sh}")
scratch=$(mktemp -d "${TMPDIR:-/tmp}/loom-test-adr-citations.XXXXXX")
trap 'rm -rf "$scratch"' EXIT HUP INT TERM

# Writes one case: three ADRs (0002 superseded by 0003), then the files from
# `name=content` pairs, all tracked by git unless the name starts with `!`.
case_dir() {
    rm -rf "$scratch/case"
    mkdir -p "$scratch/case/docs/adr"
    printf -- '- **Status:** Accepted\n' > "$scratch/case/docs/adr/0001-one.md"
    printf -- '- **Status:** Superseded by ADR-0003\n' > "$scratch/case/docs/adr/0002-two.md"
    printf -- '- **Status:** Accepted\n' > "$scratch/case/docs/adr/0003-three.md"
    git -C "$scratch/case" init -q
    while [ "$#" -gt 0 ]; do
        file=${1#!}
        mkdir -p "$(dirname "$scratch/case/$file")"
        printf '%b' "$2" > "$scratch/case/$file"
        [ "$file" = "$1" ] && git -C "$scratch/case" add "$file"
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

case_dir AGENTS.md 'See ADR-0001 and ADR-0003.\n'
passes "current ADRs"

case_dir AGENTS.md 'Text.\nADR-0002 decides this.\n'
fails "a superseded ADR in AGENTS.md"

case_dir .claude/skills/x/SKILL.md 'Read ADR-0002.\n'
fails "a superseded ADR in a skill"

case_dir .claude/skills/x/references/lenses.md 'Read ADR-0002.\n'
fails "a superseded ADR in a skill reference"

case_dir crates/a/CLAUDE.md 'Read ADR-0002.\n'
fails "a superseded ADR in a nested CLAUDE.md"

case_dir AGENTS.md 'ADR-0003, which superseded ADR-0002.\n'
passes "a line that says the ADR was superseded"

case_dir AGENTS.md 'Read ADR-0009.\n'
fails "an ADR that does not exist"

case_dir docs/design/spec.md 'Superseding ADR-0002 is the point.\nADR-0002 again.\n'
passes "a spec, which is not an instruction file"

case_dir '!AGENTS.md' 'Read ADR-0002.\n'
passes "an untracked file"

echo "check-adr-citations.sh behaves as expected."

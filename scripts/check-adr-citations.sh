#!/bin/sh
# Checks the ADRs that agent instruction files cite: AGENTS.md and CLAUDE.md
# at any level, and the Markdown files under .claude/ (skills and their
# references). Each cited ADR must exist and must not be superseded, unless
# the same line says that it was superseded. Why: an agent follows these
# files as written. A skill that still describes a replaced decision steers
# every run wrong, and no test notices, because nothing else reads the skill
# when the ADR changes. Specs and other docs may cite old ADRs freely.
#
# Reads the files git tracks, so worktrees under .claude/worktrees and
# untracked notes are left out. Run it from the repository root.
#
# Usage: scripts/check-adr-citations.sh

set -eu

adr_dir=docs/adr
scratch=$(mktemp -d "${TMPDIR:-/tmp}/loom-adr-citations.XXXXXX")
trap 'rm -rf "$scratch"' EXIT HUP INT TERM

git ls-files -- AGENTS.md CLAUDE.md '*/AGENTS.md' '*/CLAUDE.md' '.claude/*.md' > "$scratch/files"
: > "$scratch/problems"

while IFS= read -r file; do
    # Each match prints as LINE:ADR-NNNN.
    grep -noE 'ADR-[0-9]{4}' "$file" | while IFS=: read -r line ref; do
        number=${ref#ADR-}
        set -- "$adr_dir/$number"-*.md
        if [ ! -e "$1" ]; then
            echo "$file:$line: cites $ref, which does not exist in $adr_dir" >> "$scratch/problems"
            continue
        fi
        status=$(sed -n 's/^- \*\*Status:\*\*[[:space:]]*//p' "$1" | sed 's/[[:space:]]*$//')
        case $status in
            Superseded*)
                text=$(sed -n "${line}p" "$file")
                case $text in
                    *[Ss]upersed*) ;;
                    *) echo "$file:$line: cites $ref, whose status is '$status'. Point to the ADR that replaced it, or say on this line that $ref was superseded." >> "$scratch/problems" ;;
                esac
                ;;
        esac
    done
done < "$scratch/files"

if [ -s "$scratch/problems" ]; then
    cat "$scratch/problems"
    exit 1
fi
echo "ADR citations in agent instruction files are current."

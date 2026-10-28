#!/bin/sh
# Checks the status of every ADR (docs/adr/README.md, Editing rules and
# Status values). Each ADR has exactly one `- **Status:**` line, it holds one
# allowed value and nothing else, and the ADR's row in the index says the
# same. Proposed is refused, because an ADR merges into main only once it is
# decided. CI runs this, so a pull request with an undecided ADR cannot merge.
#
# Usage:
#   scripts/check-adr-status.sh            every ADR in docs/adr
#   scripts/check-adr-status.sh FILE...    only these files (the commit hook)

set -eu

if [ "$#" = 0 ]; then
    set -- docs/adr/[0-9][0-9][0-9][0-9]-*.md
fi
fail=0

for file in "$@"; do
    name=$(basename "$file")
    dir=$(dirname "$file")
    index="$dir/README.md"
    # Only ADR files are checked; the hook may pass the index or others.
    case $name in
        [0-9][0-9][0-9][0-9]-*.md) ;;
        *) continue ;;
    esac
    number=${name%%-*}
    # The template shows the format, so it stays Proposed.
    [ "$number" = 0000 ] && continue

    count=$(grep -c '^- \*\*Status:\*\*' "$file" || true)
    if [ "$count" != 1 ]; then
        echo "$name: needs exactly one '- **Status:**' line, found $count"
        fail=1
        continue
    fi
    status=$(sed -n 's/^- \*\*Status:\*\*[[:space:]]*//p' "$file" | sed 's/[[:space:]]*$//')

    case $status in
        Accepted | Deferred | Rejected) ;;
        "Superseded by ADR-"[0-9][0-9][0-9][0-9])
            target=${status#Superseded by ADR-}
            if ! ls "$dir/$target"-*.md > /dev/null 2>&1; then
                echo "$name: superseded by ADR-$target, which does not exist"
                fail=1
            fi
            ;;
        Proposed)
            echo "$name: status is Proposed. Decide the ADR before it merges (docs/adr/README.md, Editing rules)."
            fail=1
            ;;
        *)
            echo "$name: status '$status' is not one of Accepted, Deferred, Rejected, or Superseded by ADR-NNNN. Put dates on the Date line and reasons in the ADR's sections."
            fail=1
            ;;
    esac

    # Index rows look like: | [0024](0024-....md) | Title | Status |
    row=$(grep "^| \[$number\](" "$index" || true)
    if [ -z "$row" ]; then
        echo "$name: has no row in $index"
        fail=1
        continue
    fi
    indexed=$(printf '%s\n' "$row" | awk -F'|' '{ s = $(NF - 1); gsub(/^[ \t]+|[ \t]+$/, "", s); print s }')
    if [ "$indexed" != "$status" ]; then
        echo "$name: the index says '$indexed', the ADR says '$status'"
        fail=1
    fi
done

if [ "$fail" = 0 ]; then
    echo "ADR status lines are valid."
fi
exit "$fail"

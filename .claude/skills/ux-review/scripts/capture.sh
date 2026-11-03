#!/bin/sh
# Capture what a UX review looks at: the main views of the running app, every
# module at 100% zoom, what the UI shows as JSON, and simple measures.
# Usage: capture.sh OUT_DIR [PATCH] [MODULE]
set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 3 ]; then
    echo "usage: $0 OUT_DIR [PATCH] [MODULE]" >&2
    exit 2
fi

here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$1"
out=$(cd "$1" && pwd)
patch=${2:-examples/patches/subtractive.ron}
module=${3:-4}

cargo build --release -q -p loom-app
app() {
    cargo run --release -q -p loom-app -- "$@"
}

# Zoom 1, because below about 0.83 labels draw as grey bars, and a review would
# judge those instead of the words. The size comes from the patch, because a
# fixed one would silently cut off the rows that new modules add.
app examples/dev/all-modules.ron --dump-ui >"$out/gallery.json"
app examples/dev/all-modules.ron --screenshot "$out/gallery.png" --zoom 1 \
    --size "$(python3 "$here/measure.py" size "$out/gallery.json")"
app "$patch" --dump-ui --select "$module" >"$out/patch.json"
python3 "$here/measure.py" dump "$out/gallery.json" "$out/patch.json" >"$out/measures.txt"

# The preferences file keeps the tour away from the user's real settings, and
# --no-recovery keeps it out of their autosave copies.
LOOM_UX_OUT="$out" LOOM_UX_MODULE="$module" app "$patch" --fake-audio --no-recovery \
    --preferences "$out/preferences.ron" --control-script "$here/tour.jsonl" >"$out/tour.log"

# With no patch, the app opens as a new user first meets it.
LOOM_UX_OUT="$out" app --fake-audio --no-recovery \
    --preferences "$out/preferences.ron" --control-script "$here/first-launch.jsonl" >"$out/first-launch.log"

if grep -q '"ok":false' "$out/tour.log" "$out/first-launch.log"; then
    echo "some tour steps failed; see the logs in $out" >&2
    exit 1
fi
echo "captured in $out"

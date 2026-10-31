#!/bin/sh
# Packs the release-mode binaries into the download for one system. The
# release workflow runs it once per target; you can run it by hand too.
# What goes in each archive is in docs/design/releases.md.
#
# Usage:
#   scripts/package-release.sh OS ARCH [TARGET_TRIPLE]
#
#   OS            macos, windows, or linux
#   ARCH          arm64 or x86_64 (a label for the file name)
#   TARGET_TRIPLE the cargo --target the binaries were built with; leave it
#                 out when they were built for the machine you are on
#
# Environment:
#   VERSION       the version for the file name (default: from Cargo.toml)
#
# Build first, for example:
#   cargo build --release -p loom-app -p loom-cli
#
# The archive is written to dist/, and its path is printed last.
set -eu

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
    echo "usage: package-release.sh OS ARCH [TARGET_TRIPLE]" >&2
    exit 2
fi
os="$1"
arch="$2"
triple="${3:-}"

cd "$(dirname "$0")/.."

version="${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)}"
target=$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
if [ -z "$target" ]; then
    echo "package-release.sh: could not find Cargo target directory" >&2
    exit 1
fi
if [ -n "$triple" ]; then
    target="$target/$triple"
fi
target="$target/release"

name="LoomDSP-$version-$os-$arch"
stage="dist/$name"

rm -rf "$stage"
mkdir -p "$stage"

case "$os" in
macos)
    # The app bundle is the download. `loom` is left out: a bare binary in
    # a zip is not what a Mac user expects, and the app does everything.
    # macOS wants a plain number for the bundle version, so it gets the
    # Cargo.toml version and not a label like -alpha.1.
    LOOM_SKIP_BUILD=1 LOOM_TARGET="$triple" LOOM_VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)" \
        LOOM_BUNDLE_ID=io.github.brylie.loomdsp \
        sh scripts/bundle-macos.sh >/dev/null
    # ditto keeps the permissions and attributes that a plain copy drops.
    ditto --norsrc --noextattr --noqtn "$target/LoomDSP.app" "$stage/LoomDSP.app"
    first_launch="Open LoomDSP.app by right-clicking it and choosing Open, then Open again.
macOS warns because this build is not signed yet. Without that step it
says the developer cannot be verified. You can also allow it under
System Settings, Privacy & Security."
    ;;
windows)
    cp "$target/loom-app.exe" "$target/loom.exe" "$stage/"
    first_launch="Double-click loom-app.exe.
Windows may show \"Windows protected your PC\" because this build is not
signed yet. Choose More info, then Run anyway."
    ;;
linux)
    cp "$target/loom-app" "$target/loom" "$stage/"
    chmod +x "$stage/loom-app" "$stage/loom"
    first_launch="Run ./loom-app from a terminal in this folder.
If it does not start, run: chmod +x loom-app"
    ;;
*)
    echo "package-release.sh: unknown OS '$os' (use macos, windows, or linux)" >&2
    exit 2
    ;;
esac

cp -R examples "$stage/examples"
# The app can be translocated away from this folder on macOS. The marker
# identifies these patches as bundled examples when the player opens one.
: > "$stage/examples/patches/.loomdsp-bundled-examples"
cp LICENSE "$stage/LICENSE"

cat > "$stage/README.txt" <<TEXT
LoomDSP $version

A free, open-source modular synthesizer. This is an early alpha build.
Patches made now may stop opening in later versions.

How to open it
--------------
$first_launch

Then open one of the patches in the examples folder, such as
examples/patches/subtractive.ron. Connect a MIDI keyboard before you start,
or click "Hold note" in the toolbar, and turn the filter's Cutoff knob.

More help
---------
Getting started: https://github.com/brylie/LoomDSP/blob/main/docs/guide/getting-started.md
Source and issues: https://github.com/brylie/LoomDSP
Licence: Apache-2.0 (see LICENSE)
TEXT

case "$os" in
macos)
    archive="dist/$name.zip"
    rm -f "$archive"
    (cd dist && ditto -c -k --norsrc --noextattr --noqtn --keepParent "$name" "$name.zip")
    ;;
windows)
    archive="dist/$name.zip"
    rm -f "$archive"
    (cd dist && 7z a -tzip -bso0 "$name.zip" "$name")
    ;;
linux)
    archive="dist/$name.tar.gz"
    rm -f "$archive"
    tar -C dist -czf "$archive" "$name"
    ;;
esac

echo "$archive"

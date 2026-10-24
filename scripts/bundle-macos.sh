#!/bin/sh
# Builds loom-app in release mode and wraps it in a minimal LoomDSP.app, so
# macOS desktop automation tools (which find apps by bundle) can see and
# drive the window.
#
# No signing, no notarisation, no icon. The release workflow
# (docs/design/releases.md) uses it too, with these optional variables:
#   LOOM_TARGET=x86_64-apple-darwin   build for another target triple
#   LOOM_SKIP_BUILD=1                 use the binary that is already built
#   LOOM_BUNDLE_ID=io.github.brylie.loomdsp   the bundle identifier
#   LOOM_VERSION=0.1.0                the version shown by macOS
#
# Usage:
#   scripts/bundle-macos.sh
#   open target/release/LoomDSP.app --args "$PWD/examples/patches/fm-feedback.ron"
#
# `open` starts the app in `/`, so give it absolute paths. You can also run
# the binary inside the bundle directly, with relative paths:
#   target/release/LoomDSP.app/Contents/MacOS/loom-app examples/patches/fm-feedback.ron
set -eu

cd "$(dirname "$0")/.."

if [ "$(uname -s)" != Darwin ]; then
    echo "bundle-macos.sh: app bundles are for macOS only" >&2
    exit 1
fi

triple="${LOOM_TARGET:-}"
if [ -z "${LOOM_SKIP_BUILD:-}" ]; then
    if [ -n "$triple" ]; then
        cargo build --release -p loom-app --target "$triple"
    else
        cargo build --release -p loom-app
    fi
fi

target="${CARGO_TARGET_DIR:-target}"
if [ -n "$triple" ]; then
    target="$target/$triple"
fi
target="$target/release"
app="$target/LoomDSP.app"
version="${LOOM_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)}"
bundle_id="${LOOM_BUNDLE_ID:-io.github.brylie.loomdsp.dev}"

# Start from an empty bundle, so nothing from an older build is left in it.
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "$target/loom-app" "$app/Contents/MacOS/loom-app"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>$bundle_id</string>
    <key>CFBundleName</key>
    <string>LoomDSP</string>
    <key>CFBundleDisplayName</key>
    <string>LoomDSP</string>
    <key>CFBundleExecutable</key>
    <string>loom-app</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleShortVersionString</key>
    <string>$version</string>
    <key>CFBundleVersion</key>
    <string>$version</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

echo "$app"

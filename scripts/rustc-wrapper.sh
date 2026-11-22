#!/bin/sh
# Runs rustc through sccache, the compile cache that every worktree on this
# machine shares, so a new worktree reuses the dependency builds of the
# others. Cargo calls it as `rustc-wrapper` with rustc's path and arguments
# (scripts/setup-worktree-target.sh writes it into the worktree's Cargo
# config, with LOOM_SCCACHE set to the sccache that mise installed).
#
# A missing sccache never breaks a build: the wrapper runs rustc itself.
# That happens when mise has upgraded sccache since the setup ran; run the
# setup script again to use the new one.
#
# sccache does not cache incremental builds, so it caches the dependencies
# and release builds, not the workspace's own crates in a debug build.

if [ -n "${LOOM_SCCACHE:-}" ] && [ -x "$LOOM_SCCACHE" ]; then
    exec "$LOOM_SCCACHE" "$@"
fi
if command -v sccache > /dev/null 2>&1; then
    exec sccache "$@"
fi
exec "$@"

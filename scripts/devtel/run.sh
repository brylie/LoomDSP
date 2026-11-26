#!/bin/sh
# Run a check, record it as a span for the development telemetry
# (docs/dev/devtel.md), and reuse its pass when it already passed on the
# same tree (docs/dev/check-reuse.md). Usage:
#   sh scripts/devtel/run.sh NAME COMMAND [ARGS...]
#
# prek.toml wraps each check with this. When the check runs, its output and
# exit code are unchanged. When a pass is reused, it prints one line saying
# so and exits 0. In CI the command always runs directly (unless
# LOOM_DEVTEL=1 asks for telemetry there), and a pass is never reused. With
# telemetry and reuse both off (LOOM_DEVTEL=0 and LOOM_CHECK_REUSE=0), or
# with no python3 on PATH, the command also runs directly.
name="$1"
shift
if [ -n "${CI:-}" ]; then
  [ "${LOOM_DEVTEL:-}" = "1" ] || exec "$@"
elif [ "${LOOM_DEVTEL:-}" = "0" ] && [ "${LOOM_CHECK_REUSE:-}" = "0" ]; then
  exec "$@"
fi
devtel_py="$(dirname "$0")/devtel.py"
if command -v python3 >/dev/null 2>&1 && [ -f "$devtel_py" ]; then
  exec python3 "$devtel_py" run "$name" -- "$@"
fi
exec "$@"

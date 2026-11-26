#!/bin/sh
# Run a check and record it as a span for the development telemetry
# (docs/dev/devtel.md). Usage: sh scripts/devtel/run.sh NAME COMMAND [ARGS...]
#
# prek.toml wraps each check with this. The command's output and exit code
# are unchanged. With devtel off (LOOM_DEVTEL=0, or CI), or with no python3
# on PATH, the command runs directly.
name="$1"
shift
if [ "${LOOM_DEVTEL:-}" = "0" ] || { [ -n "${CI:-}" ] && [ "${LOOM_DEVTEL:-}" != "1" ]; }; then
  exec "$@"
fi
devtel_py="$(dirname "$0")/devtel.py"
if command -v python3 >/dev/null 2>&1 && [ -f "$devtel_py" ]; then
  exec python3 "$devtel_py" run "$name" -- "$@"
fi
exec "$@"

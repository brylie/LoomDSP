#!/bin/sh
# Entry point for the Claude Code hooks in .claude/settings.json. Runs
# devtel.py with the Python that mise.toml pins. Telemetry must never get in
# an agent's way, so this prints nothing and always exits 0, even when mise
# is missing or not trusted in this worktree. See docs/dev/devtel.md.
dir="$(cd "$(dirname "$0")" && pwd)"
payload="$(cat)"
printf '%s' "$payload" | mise exec -C "$dir" -- python3 "$dir/devtel.py" claude "$@" >/dev/null 2>&1 \
  || printf '%s' "$payload" | python3 "$dir/devtel.py" claude "$@" >/dev/null 2>&1
exit 0

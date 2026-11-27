#!/bin/sh
# Claude Code SessionStart hook (.claude/settings.json): puts mise's shims
# first on PATH for every Bash command of the session, its subagents
# included. Claude Code runs commands from a snapshot of the user's shell,
# and `mise activate` in hook mode updates PATH only when a prompt is
# drawn, which an agent's shell never does. Without this, tools that
# mise.toml installs (ast-grep, typos, prek) are "command not found"
# unless called through `mise exec` or `mise run`. A shim picks the
# version from the mise.toml of the folder the command runs in, so each
# worktree gets its own. Prints nothing and always exits 0: a missing or
# untrusted mise must never stop a session.
[ -n "$CLAUDE_ENV_FILE" ] || exit 0
line="$(mise activate bash --shims 2>/dev/null)" || exit 0
case "$line" in
  "export PATH="*) printf '%s\n' "$line" >>"$CLAUDE_ENV_FILE" ;;
esac
exit 0

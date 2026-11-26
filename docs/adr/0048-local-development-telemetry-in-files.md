# ADR-0048: Local development telemetry as OpenTelemetry spans in files

- **Status:** Accepted
- **Date:** 2026-11-26
- **Related decisions:** ADR-0006 (dependency and licence policy)
- **Specification:** [Development telemetry](../dev/devtel.md)

## Context

Most changes to LoomDSP are made by coding agents, often several at once, in separate worktrees. We cannot yet see where their time goes. Do they test, wait for a person, re-run a check on code that has not changed, or loop on one command? Which model did the work, and how large was the change? Without numbers we cannot set targets for efficiency or tell whether a process change helped.

The data exists, but in pieces:

- Claude Code writes a transcript of each session and subagent. It deletes them after 30 days.
- mise runs our checks, prek runs them inside `git commit` and `git push`, and git knows the size of each commit. None of them records anything.
- These steps nest. An agent's Bash call runs `git commit`, which runs prek, which runs `mise run`, which runs cargo. A tool that sees only one level counts the same work twice, or misses the waste inside a commit.

Constraints: no service may have to run for data to be recorded, every tool must come through mise, recording must never slow down or break a commit, a check, or an agent, and nothing may leave the machine.

## Options considered

1. **A collector service, run with Docker Compose.** Every tool sends OTLP to it. Claude Code and mise (2026.9.13 or later) can export OTLP themselves. But data is lost whenever the service is not running, it needs Docker, and neither tool can write OTLP to a file.
2. **Each tool's own log, joined later.** Simple, but there is no shared trace ID, so nesting must be guessed from timestamps everywhere. Each log also has its own format, and the transcripts still expire.
3. **OpenTelemetry spans appended to local files, plus an archive.** Small writers append OTLP JSON lines to a spool folder outside the repository: a wrapper around each mise task step, prek hooks for git events, and a Claude Code SessionStart hook. Trace context passes down through the `TRACEPARENT` environment variable, as the OpenTelemetry environment carrier specification describes. An import copies the spool and the Claude Code transcripts into a DuckDB file, and viewers read that file only while someone looks.

## Decision

We use option 3.

- **Format.** Spans use the OTLP JSON encoding, with semantic convention names where they exist: `gen_ai.*`, `process.*`, `vcs.*`, and `cicd.*`. Any OpenTelemetry tool can read the files.
- **Store.** The spool and the archive live in one folder per user, outside the repository, shared by every worktree.
- **Writers.** They run in the background or take a few milliseconds, never print, and never fail the command they watch.
- **Archive contents.** Transcript records are reduced to metadata and command lines. Prompt text, replies, and tool output are not kept.
- **Readers.** The DuckDB UI, a text report, and otel-desktop-viewer, all on demand.
- **Scope.** Telemetry is on by default for contributors and off in CI.

## Consequences

- Nothing has to run for data to be recorded, and history outlives Claude Code's 30-day cleanup.
- Any tool that reads OTLP JSON can use the files. A collector can be added later by pointing its `otlpjsonfile` receiver at the spool.
- Python and DuckDB become development tools pinned in `mise.toml`. They are used only by these scripts, never by the product.
- Contributors get a SessionStart hook and new git hook types. After pulling this change, they must run `prek install` again to install the hooks.
- Command lines can contain paths or arguments a person would not share. The archive stays on the machine, and the specification says how to turn recording off.
- The `TRACEPARENT` that the SessionStart hook sets points at the session, not at a single tool call. The import places a task under the Bash call that ran it by time. When two Bash calls run in parallel, a task can land under the wrong one.

# Development telemetry (devtel)

**Status: Agreed.** Decision: [ADR-0048](../adr/0048-local-development-telemetry-in-files.md). Code: [`scripts/devtel`](../../scripts/devtel).

Devtel records what happens while people and agents work on LoomDSP: Claude Code sessions and their tool calls, mise tasks, prek hooks, and git events. It stores them as OpenTelemetry spans in local files, with no service running, and lets you query them. The goal is to see where development time goes, find waste such as re-running a check on unchanged code, and set targets that later changes can be measured against.

Everything stays on your machine. Nothing is sent anywhere.

## Quick start

```sh
mise install                 # Python for the scripts (DuckDB and the viewer come on first use)
prek install                 # once more after pulling this change: it adds the post-commit and other git hooks
mise run devtel-report       # the efficiency report for the last 14 days (--days N to change)
mise run devtel              # the DuckDB UI over the archive, for your own queries
mise run devtel-traces       # every session as a trace waterfall in otel-desktop-viewer
```

The report reads Claude Code's own transcripts too, so it shows months of history before any hook has run.

To stop recording, set `LOOM_DEVTEL=0` in your shell. Recording is off in CI unless `LOOM_DEVTEL=1` is set.

## How it works

```text
Claude Code session ──── SessionStart hook ── exports TRACEPARENT, LOOM_DEVTEL_SESSION
   │ Bash tool call
   └─ git commit ── prek ── mise run fmt ── devtel-sh ── span ─┐
   └─ mise run check ────────────────────── devtel-sh ── span ─┤
   └─ git push ── prek ── devtel git pre-push ────────── span ─┤
                                                               ▼
        ~/.local/share/loomdsp/devtel/spool/<day>/<source>.jsonl  (OTLP JSON lines)
                                                               │
 ~/.claude/projects/<repo>*/…/*.jsonl  (Claude transcripts) ───┤  mise run devtel-import
                                                               ▼  (also at every session start)
                     ~/.local/share/loomdsp/devtel/devtel.duckdb  (archive + views)
                                                               │
                        devtel-report · devtel (DuckDB UI) · devtel-traces (waterfall)
```

### Writers

Writers append spans to the spool. Each one takes a few milliseconds, prints nothing, and never changes the exit code of the command it watches.

| Writer | Where it hooks in | What it records |
|---|---|---|
| Task wrapper (`bin/devtel-sh`) | mise runs every inline task step with it (`unix_default_inline_shell_args` in `mise.toml`) | Task name, command, exit code, duration, working directory, branch, commit, and a hash of the working tree |
| Git hooks (`devtel.py git`) | prek, in the `post-commit`, `post-checkout`, `post-merge`, `post-rewrite`, and `pre-push` stages | Commit size (files, insertions, deletions), parent count, whether Claude co-authored it, branch switches, rewrites, and the remote name and branch of a push |
| Claude Code hook (`hook.sh`) | `SessionStart` in `.claude/settings.json` | The session's start, model, and source; sets the trace context for the session; starts an import in the background |

The Claude Code transcripts are the fourth source. They hold every tool call with its timing, the model and tokens of each turn, and the subagents. The import reads them, so no hook per tool call is needed.

### Trace context

A Claude Code session is one trace, and its ID is the session UUID. That UUID is 128 bits, like a trace ID, so the hook, the import, and the views all derive the same IDs with no shared state. The SessionStart hook writes `TRACEPARENT` (pointing at the session's root span) and `LOOM_DEVTEL_SESSION` into `CLAUDE_ENV_FILE`. Claude Code then runs every Bash command of the session with them set.

Each writer reads `TRACEPARENT`, records its span as a child of it, and passes its own span on to the processes it starts. That follows the OpenTelemetry environment carrier specification, which mise also follows. So a `mise run` inside a prek hook inside a `git commit` nests correctly at any depth.

The trace context set by the hook names the session, not the single tool call, because a hook cannot change a command's environment per call without rewriting it. The `devtel_spans` view therefore moves a span whose parent is the session root under the Bash call it ran inside, found by time. Two Bash calls running at once can make that placement wrong.

Work outside an agent session (your own terminal) gets a new trace for each top-level command.

### Store

The store holds the spool and the archive. It lives in `$LOOM_DEVTEL_DIR`, or by default `$XDG_DATA_HOME/loomdsp/devtel` (`~/.local/share/loomdsp/devtel`). It is outside the repository, so every worktree shares it and it outlives them all.

- `spool/<day>/<source>.jsonl` holds OTLP JSON, one `TracesData` object per line. Writers append with a single write call, so lines written at the same time do not mix.
- `devtel.duckdb` is the archive. Only the import writes to it.
- `import.lock` lets only one import run at a time.

### Import

`mise run devtel-import` runs the import. Every reader runs it first, and so does the SessionStart hook, in the background.

- **It is idempotent.** Every row has a natural key: a transcript record's session and UUID, a subagent's session and agent ID, or a span's trace and span ID. The import only inserts rows whose key is new, so a second run changes nothing.
- **It only adds.** A transcript that Claude Code deleted keeps every row it already gave. Run any devtel command at least once a month, so nothing expires before it is imported.
- **It is incremental.** A file is read again only when its size or time changed.
- **It keeps metadata and commands only.** It keeps tool names, command lines (up to 2000 characters), file paths, search patterns, subagent types and models, timestamps, errors, models, and token counts. It does not keep prompt text, replies, or tool output. A person's prompt is kept only as "a prompt of N characters".

The views are created again on every import, so improving a view changes how all of history reads.

### Readers

| Command | What it shows |
|---|---|
| `mise run devtel-report` | Time by kind of work, re-runs, possible loops, models and tokens, commits, and the largest sessions |
| `mise run devtel` | The DuckDB UI, a notebook for your own SQL. The page loads its code from `ui.duckdb.org`, but queries and data stay local. Not on Intel Macs. |
| `mise run devtel-traces` | otel-desktop-viewer, with each session as a waterfall. It runs only while open and holds the spans in memory. On the first run, pick a wide time range: the page starts on the last hour. No Intel Mac build. |
| `python3 scripts/devtel/devtel.py export FILE` | Every span as OTLP JSON lines, for any other OpenTelemetry tool |

## Views

| View | One row per | Use it for |
|---|---|---|
| `devtel_tools` | Tool call | Timing, kind of work, errors, and the command, normalised |
| `devtel_spans` | Span, from every source | Traces: sessions, subagents, tool calls, tasks, and git events, in one OpenTelemetry shape |
| `devtel_sessions` | Claude Code session | Size and cost: time, tool calls, subagents, tokens, waiting, checking, commits, and lines changed |
| `devtel_reruns` | Check run again with nothing changed | Waste (see below) |
| `devtel_loops` | Command run 3 or more times within 10 calls of one agent | Agents stuck in a loop |

### Kinds of work

Each tool call gets one kind (`devtel_category` in `views.sql`):

- **wait: person:** a question to you, or a plan waiting for approval.
- **test, check, build:** cargo, mise, prek, and the linters.
- **git commit** and **git push:** these include the hooks that run inside them.
- **git** and **github:** other git commands, and `gh`.
- **edit, read, web, agents, mcp:** the other tools.
- **shell:** any other command.

A command that does several things counts as the first match in the order `views.sql` checks them.

### Re-runs and loops

A re-run is a check that ran again when nothing had changed, so it could not tell anything new. There are two measures:

- **same tree** is exact. The task wrapper hashes the working tree before each step, with `git add -A` into a temporary index and `git write-tree`. That takes about 80 ms and leaves your index alone. Two runs of the same step on the same tree, with the same result, count as one run and one re-run.
- **no edit** is an estimate from the transcripts: an agent ran the same check command again with no Edit or Write call in between. Code can also change through git or a shell command, so it is an upper bound. It covers history from before the task wrapper existed.

A loop is the same normalised command run 3 or more times within 10 tool calls of one agent. Normalising removes a leading `cd`, output redirection, and a trailing `head`, `tail`, `grep`, or `wc`.

## Measuring efficiency

Raw counts invite gaming, so compare like with like:

- **Size:** a session's commits, insertions, and deletions, and later the issue's Size field.
- **Rate:** re-run minutes, loop minutes, and waiting minutes per session or per 100 lines changed.
- **Trend:** the same report over two periods, such as before and after a change to a skill or a hook.

The first targets will come from the first weeks of data. They are not set yet.

## Out of scope for now

- GitHub: pull requests, reviews, and CI runs. Trace context could reach CI through a pull request label or a commit trailer later.
- A collector service or a hosted backend.
- Commands typed in your own terminal outside mise and git.
- A hook per Claude Code tool call. The transcripts already hold the timing. A `PostToolUse` hook would add `duration_ms` without permission waits, so it is a possible later step.

## Testing

`mise run devtel-test` runs the unit tests (encoding, trace context, the writers) and, when mise is available, an import into a temporary store from a made-up transcript. The import tests check that importing is idempotent, that deleted transcripts keep their rows, and that no prompt or output text is stored.

# Development telemetry (devtel)

**Status: Agreed.** Decision: [ADR-0048](../adr/0048-local-development-telemetry-in-files.md). Code: [`scripts/devtel`](../../scripts/devtel).

Devtel records what happens while people and agents work on LoomDSP: Claude Code sessions and their tool calls, the checks that git hooks run, and git events. It stores them as OpenTelemetry spans in local files, with no service running, and lets you query them. The goal is to see where development time goes, find waste such as re-running a check on unchanged code, and set targets that later changes can be measured against.

Everything stays on your machine. Nothing is sent anywhere.

## Quick start

```sh
mise install                 # Python for the scripts (DuckDB, the storage, and the viewer come on first use)
prek install                 # once more after pulling this change: it adds the post-commit and other git hooks
mise run devtel-report       # the efficiency report for the last 14 days (--days N to change)
mise run devtel              # every session as a trace waterfall in otel-desktop-viewer (--days N, default 14)
```

The report also reads Claude Code's own transcripts, so it shows months of history before any hook has run.

To stop recording, set `LOOM_DEVTEL=0` in your shell. Recording is off in CI unless `LOOM_DEVTEL=1` is set.

## How it works

```text
Claude Code session ──── SessionStart hook ── exports TRACEPARENT, LOOM_DEVTEL_SESSION
   │ Bash tool call (timed in the transcript)
   ├─ git commit ── prek ── run.sh clippy ─── mise run clippy ──── span ─┐
   │                     └─ git post-commit ─────────────────────── span ─┤
   ├─ git push ──── prek ── run.sh typos ──── mise exec -- typos ── span ─┤
   │                     └─ git pre-push ────────────────────────── span ─┤
   └─ sh scripts/devtel/run.sh NAME CMD (a check run by hand) ───── span ─┤
                                                                          ▼
              ~/.local/share/loomdsp/devtel/spool/<day>/<source>.jsonl  (OTLP JSON lines)
                                                                          │
 ~/.claude/projects/<repo>*/…/*.jsonl  (Claude Code transcripts) ─────────┤  mise run devtel-import
                                                                          ▼  (also at each session start)
                           ~/.local/share/loomdsp/devtel/devtel.duckdb  (archive and views)
                                                                          │
                              devtel-report (text) · devtel (otel-desktop-viewer waterfall)
```

### Writers

Writers append spans to the spool. They print nothing and never change the exit code of the command they watch. The one exception is the check runner when it reuses a pass on the same tree: it does not run the check, prints one `reused:` line, and exits 0, as the check did on the same inputs ([check reuse](check-reuse.md), ADR-0049).

| Writer | Where it hooks in | What it records | Cost |
|---|---|---|---|
| Check runner (`run.sh`) | Each check's `entry` in `prek.toml`, in the commit, push, and manual stages | Check name, command, exit code, duration, branch, commit, a hash of the working tree, whether it ran in a git hook or before a push, and whether it reused an earlier pass (`loom.devtel.reused`, with the reused run's time in `loom.devtel.reused_ms`) | About 150 ms per check: 90 ms for the tree hash, 45 ms for git, and the rest to start Python |
| Git hooks (`devtel.py git`) | prek, in the `post-commit`, `post-checkout`, `post-merge`, `post-rewrite`, and `pre-push` stages | Commit size (files, insertions, deletions), parent count, whether Claude co-authored it, branch switches, rewrites, and the remote name and branch of a push | About 0.1 s to find the git process; the rest (about 0.4 s) runs in the background, so git does not wait |
| Claude Code hook (`hook.sh`) | `SessionStart` in `.claude/settings.json` | The session's start, model, and source. Sets the trace context for the session and starts an import in the background | About 0.35 s, once per session |
| Test runner (`scripts/test_run.py`) | The test runs of `mise run check`, `test`, and `coverage` ([test runs](test-runs.md)) | Service `tests`: the wait for the machine-wide test queue, and the run with its selection, crates, and exit code | Nothing measurable: two spans per test run |

The Claude Code transcripts are the fourth source. They hold every tool call with its timing, the model, effort, and tokens of each response, the subagents, the pull requests a session opened, and Claude Code's own totals for cost and lines changed. The import reads them, so no hook per tool call is needed.

Checks are wrapped in `prek.toml` and not through mise, for two reasons. mise 2026 ignores a project's `unix_default_inline_shell_args` for security reasons. A `shell` on each task would also run a `sh` script as the shell on Windows, where CI runs `mise run test`. A `mise run` started outside a git hook and outside an agent session is therefore not recorded. mise's own OpenTelemetry export, available from mise 2026.9.13, is the likely route to cover it, but it needs a receiver to be running.

The prek hooks that run a builtin check (whitespace, line endings, merge conflicts) have no entry to wrap, so they are not recorded one by one. The time of the `git commit` or `git push` Bash call covers them.

### Trace context

A Claude Code session is one trace, and its ID is the session UUID. That UUID is 128 bits, like a trace ID, so the hook, the import, and the views all derive the same IDs with no shared state. The SessionStart hook writes `TRACEPARENT` (pointing at the session's root span) and `LOOM_DEVTEL_SESSION` into `CLAUDE_ENV_FILE`. Claude Code then runs every Bash command of the session with them set.

Each writer reads `TRACEPARENT`, records its span as a child of it, and passes its own span on to the processes it starts. That follows the OpenTelemetry environment carrier specification, which mise also follows. So a check inside a prek hook inside a `git commit` nests correctly at any depth.

The trace context set by the hook names the session, not the single tool call, because a hook cannot change a command's environment per call without rewriting it. The `devtel_spans` view therefore moves a span whose parent is the session root under the Bash call it ran inside, found by time. Two Bash calls running at once can make that placement wrong.

Outside an agent session (your own terminal, or a session that started before the hook existed), the spans of one git command share a trace. Each check and git event walks up the process tree with `ps` to the `git` process that ran its hook. The host name, the process ID, and the start time of that process name the trace, so every hook of one `git commit` agrees on it with no shared state. No hook sees the git command end, so the `devtel_spans` view adds the root span ("git commit", "git push", ...) from its children's times. A check run by hand outside both gets a trace of its own.

### Store

The store holds the spool and the archive. It lives in `$LOOM_DEVTEL_DIR`, or by default `$XDG_DATA_HOME/loomdsp/devtel` (`~/.local/share/loomdsp/devtel`). It is outside the repository, so every worktree shares it and it outlives them all.

- `spool/<day>/<source>.jsonl` holds OTLP JSON, one `TracesData` object per line. Writers append with a single write call, so lines written at the same time do not mix.
- `devtel.duckdb` is the archive. Only the import writes to it. The readers (the report and the export for the trace viewer) open it read-only, so they can run at once. DuckDB allows many readers or one writer, so an import that finds a reader open is skipped with a note, and its data comes in at the next import. A reader that finds an import running waits up to 30 seconds for it.
- `import.lock` lets only one import run at a time.

### Import

`mise run devtel-import` runs the import. Every reader runs it first, and so does the SessionStart hook, in the background.

- **It is idempotent.** Every row has a natural key: a transcript record's session and UUID (or a hash of the record, for records without a UUID), a subagent's session and agent ID, or a span's trace and span ID. The import only inserts rows whose key is new, so a second run changes nothing.
- **It only adds.** A transcript that Claude Code deleted keeps every row it already gave. Run any devtel command at least once a month, so nothing expires before it is imported.
- **It is incremental.** A file is read again only when its size or time changed.
- **It keeps metadata and commands only.** It keeps tool names, command lines (up to 2000 characters), file paths, search patterns, subagent types and models, timestamps, errors, models, effort, token counts, pull request numbers, session titles, and cost totals. It does not keep prompt text, replies, or tool output. A person's prompt is kept only as "a prompt of N characters". Claude Code's own bookkeeping records (request bodies, file snapshots, attachments) are skipped.
- **Columns are only added.** The archive outlives the transcripts it came from, so a field the import drops is lost for good. A new column goes into `schema.sql` as an `alter table ... add column`, never by changing a table, so an existing archive gains it.

The views are created again on every import, so improving a view changes how all of history reads.

Claude Code writes one transcript record per content block of a response, and each one repeats the response's token usage. Count tokens per message (`devtel_turns`), never per record, or they come out about twice too high.

### Readers

| Command | What it shows |
|---|---|
| `mise run devtel-report` | Time by kind of work, re-runs, reused passes and the time they saved, possible loops, models and tokens, tokens and model by subagent type (a model change shows as a new row with its own dates), commits, and the largest sessions |
| `mise run devtel` | otel-desktop-viewer, with each session as a waterfall. It runs only while open, holds the spans in memory, and stops when you stop the task. Pick a wide time range: the page starts on the last hour. No Intel Mac build. |
| `python3 scripts/devtel/devtel.py export FILE` | Every span as OTLP JSON lines, for any other OpenTelemetry tool |

DuckDB is only the storage. To query the archive with SQL yourself, open it read-only, for example `mise exec $LOOM_DEVTEL_DUCKDB -- duckdb -readonly ~/.local/share/loomdsp/devtel/devtel.duckdb`.

## Views

| View | One row per | Use it for |
|---|---|---|
| `devtel_tools` | Tool call | Timing, kind of work, errors, the command (normalised), and the git operation |
| `devtel_turns` | Response from the model | Model, effort, thinking time, and tokens, counted once |
| `devtel_prs` | Pull request a session opened or worked on | Linking sessions to pull requests, from Claude Code's own records |
| `devtel_spans` | Span, from every source | Traces: sessions, subagents, tool calls, checks, and git events, in one OpenTelemetry shape |
| `devtel_sessions` | Claude Code session | Size and cost: time, tool calls, subagents, tokens, cost, lines changed, pull requests, waiting, checking, and commits |
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

A re-run is a check that ran again when nothing had changed, so it could not tell anything new. `devtel_reruns` has three measures:

- **same tree** is exact. `run.sh` hashes the working tree before each check, with `git add -A` into a temporary index and `git write-tree`. That leaves your index alone. It writes each new file version into `.git/objects` once, and `git gc` prunes them. Two runs of a check on the same tree count as one run and one re-run, whether by hand, in the commit hook, or before push. A run that reused an earlier pass ([check reuse](check-reuse.md)) is not a re-run: the report counts it under "reused a pass", with the time it saved. prek splits a check's files into batches. Batches that overlap in time, or follow each other within a second, count as one run, so two separate runs less than a second apart also count as one.
- **no change** is an estimate from the transcripts: the same check command ran again in a session with no tool call in between that may have changed the code. Agents change code through Bash as often as through the Edit tool, so `devtel_may_write` also counts in-place editors, scripts on stdin, heredocs, redirects into files, git commands that change the tree, and formatters. That rule is broad, so this measure is a lower bound.
- **hook after check** is an estimate from the transcripts: a `git commit` or `git push` that came right after the session ran a test or check, with no change in between, and that did not itself run a check. Its hooks re-checked code that had just been checked. The hooks run only some of the checks, so this is time spent re-checking, not all waste.

A loop is the same normalised command run 3 or more times within 10 tool calls of one agent. Normalising removes a leading `cd` or `git add`, environment assignments, `time`, `nice`, and `mise exec --`, output redirection, and a trailing `head`, `tail`, `grep`, or `wc`.

## Commits, branches, and squash merges

Pull requests are squash merged today, which may change. A squash merge puts one new commit on `main` for the whole pull request, so the commits recorded on a branch never reach `main` under the same IDs. So devtel links work to a pull request by the branch (`vcs.ref.head.name` on every check and git event) and by the session that opened it (`devtel_prs`), never by a commit ID. The squash commit itself is recorded when you pull it (`git post-merge`). If the project moves to merge commits or rebasing, commit IDs would survive, and the branch link still works.

## Measuring efficiency

Raw counts invite gaming, so compare like with like:

- **Size:** a session's commits, lines changed, and pull requests, and later the issue's Size field.
- **Rate:** re-run minutes, loop minutes, and waiting minutes per session or per 100 lines changed.
- **Cost:** tokens and cost per pull request or per 100 lines changed, by model.
- **Trend:** the same report over two periods, such as before and after a change to a skill or a hook.

The first targets will come from the first weeks of data. They are not set yet.

## Out of scope for now

- GitHub: reviews and CI runs. Trace context could reach CI through a pull request label or a commit trailer later.
- A collector service or a hosted backend.
- Commands typed in your own terminal outside git hooks, and `mise run` outside a git hook or an agent session (see [Writers](#writers)).
- A hook per Claude Code tool call. The transcripts already hold the timing. A `PostToolUse` hook would add `duration_ms` without permission waits, and the working tree's hash for checks an agent runs by hand, which would make "by hand, then in the hook" exact. It waits until the SessionStart hook has run in real sessions for a while.

## Testing

`mise run devtel-test` runs the unit tests (encoding, trace context, the writers, `run.sh`) and, when mise is available, an import into a temporary store from a made-up transcript. The import tests check that importing is idempotent, that deleted transcripts keep their rows, that no prompt or output text is stored, that a check under a Bash call nests under it, and that a check on the same tree is a re-run.

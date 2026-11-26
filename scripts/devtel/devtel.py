#!/usr/bin/env python3
"""Local development telemetry for LoomDSP (devtel).

Records what happens while people and agents work on the repository, as
OpenTelemetry spans in plain files, with no daemon. See docs/dev/devtel.md
for the design and ADR-0048 for the decision.

Writers (fast, append only, never fail the caller):
  run        runs a check and records it (called by run.sh from prek.toml)
  git        records a git event (called by prek post-* and pre-push hooks)
  claude     handles a Claude Code hook event (called by hook.sh)

Readers (run on demand):
  import     copies Claude Code transcripts and the spool into the archive
  report     prints the efficiency report
  ui         opens the DuckDB UI over the archive
  traces     opens otel-desktop-viewer with every span loaded
  export     writes the archive as OTLP JSON lines

Standard library only, and Python 3.9 compatible, so a hook still works
when it runs outside mise.
"""

import argparse
import datetime
import hashlib
import json
import os
import re
import socket
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
SQL_DIR = os.path.join(HERE, "sql")
SCOPE = {"name": "loomdsp.devtel", "version": "1"}
# The longest command line kept on a span. Long heredocs say little more.
MAX_COMMAND = 2000
TRACEPARENT_RE = re.compile(r"^00-([0-9a-f]{32})-([0-9a-f]{16})-([0-9a-f]{2})$")


# ---------------------------------------------------------------------------
# Settings
# ---------------------------------------------------------------------------


def enabled():
    """Whether recording is on: off when LOOM_DEVTEL=0, and in CI unless LOOM_DEVTEL=1."""
    flag = os.environ.get("LOOM_DEVTEL", "")
    if flag == "0":
        return False
    if os.environ.get("CI") and flag != "1":
        return False
    return True


def store_dir():
    """The folder that holds the spool and the archive, shared by every worktree."""
    if os.environ.get("LOOM_DEVTEL_DIR"):
        return os.path.expanduser(os.environ["LOOM_DEVTEL_DIR"])
    base = os.environ.get("XDG_DATA_HOME") or os.path.join(os.path.expanduser("~"), ".local", "share")
    return os.path.join(base, "loomdsp", "devtel")


def archive_path():
    """The DuckDB archive file."""
    return os.path.join(store_dir(), "devtel.duckdb")


def duckdb_cmd():
    """The command that runs the DuckDB CLI pinned in mise.toml."""
    tool = os.environ.get("LOOM_DEVTEL_DUCKDB", "github:duckdb/duckdb@2.0.1")
    return ["mise", "exec", tool, "--", "duckdb", "-no-agent"]


def viewer_cmd():
    """The command that runs otel-desktop-viewer, pinned in mise.toml."""
    tool = os.environ.get("LOOM_DEVTEL_VIEWER", "github:CtrlSpice/otel-desktop-viewer@0.8.0")
    return ["mise", "exec", tool, "--", "otel-desktop-viewer"]


# ---------------------------------------------------------------------------
# OTLP JSON encoding (https://opentelemetry.io/docs/specs/otlp/, JSON
# Protobuf Encoding): hex IDs, lowerCamelCase keys, 64-bit integers as
# decimal strings, enums as integers, one TracesData object per line.
# ---------------------------------------------------------------------------


def derived_id(text, hex_chars=16):
    """A stable ID from a name, so every writer and the import agree on it."""
    return hashlib.md5(text.encode("utf-8")).hexdigest()[:hex_chars]


def session_trace_id(session_id):
    """A Claude Code session UUID is 128 bits, so it is the trace ID as it is."""
    return session_id.replace("-", "").lower()


def session_root_span_id(session_id):
    """The span ID of a session's root span."""
    return derived_id("session:" + session_id)


def new_span_id():
    """A random 64-bit span ID, as hex."""
    return os.urandom(8).hex()


def new_trace_id():
    """A random 128-bit trace ID, as hex."""
    return os.urandom(16).hex()


def parse_traceparent(value):
    """Return (trace_id, parent_span_id) from a W3C traceparent, or None."""
    match = TRACEPARENT_RE.match((value or "").strip().lower())
    if not match or set(match.group(1)) == {"0"} or set(match.group(2)) == {"0"}:
        return None
    return match.group(1), match.group(2)


def traceparent(trace_id, span_id):
    """Format a W3C traceparent for child processes (env carrier spec)."""
    return "00-%s-%s-01" % (trace_id, span_id)


def any_value(value):
    """Encode one attribute value as an OTLP AnyValue."""
    if isinstance(value, bool):
        return {"boolValue": value}
    if isinstance(value, int):
        return {"intValue": str(value)}
    if isinstance(value, float):
        return {"doubleValue": value}
    return {"stringValue": str(value)}


def key_values(attributes):
    """Encode a dict as an OTLP attribute list, leaving out empty values."""
    return [
        {"key": key, "value": any_value(value)}
        for key, value in sorted(attributes.items())
        if value is not None and value != ""
    ]


def span(trace_id, span_id, parent_id, name, start_ns, end_ns, attributes, error=False):
    """Build one OTLP span (kind INTERNAL)."""
    record = {
        "traceId": trace_id,
        "spanId": span_id,
        "name": name,
        "kind": 1,
        "startTimeUnixNano": str(int(start_ns)),
        "endTimeUnixNano": str(int(end_ns)),
        "attributes": key_values(attributes),
        "status": {"code": 2} if error else {},
    }
    if parent_id:
        record["parentSpanId"] = parent_id
    return record


def traces_line(service, resource, spans):
    """One line of an OTLP JSON file: a TracesData object."""
    attributes = dict(resource)
    attributes["service.name"] = service
    return json.dumps(
        {
            "resourceSpans": [
                {
                    "resource": {"attributes": key_values(attributes)},
                    "scopeSpans": [{"scope": SCOPE, "spans": spans}],
                }
            ]
        },
        separators=(",", ":"),
    )


def write_spool(source, line):
    """Append one line to today's spool file for this source.

    One write call with O_APPEND, so lines from processes running at the
    same time do not interleave on a local file system.
    """
    day = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d")
    folder = os.path.join(store_dir(), "spool", day)
    os.makedirs(folder, exist_ok=True)
    path = os.path.join(folder, "%s.jsonl" % source)
    fd = os.open(path, os.O_WRONLY | os.O_APPEND | os.O_CREAT, 0o600)
    try:
        os.write(fd, (line + "\n").encode("utf-8"))
    finally:
        os.close(fd)


def emit(source, service, spans_list, resource=None):
    """Write spans to the spool. Never raises: telemetry must not break work."""
    try:
        base = common_resource()
        base.update(resource or {})
        write_spool(source, traces_line(service, base, spans_list))
    except Exception as error:  # noqa: BLE001 - fail open, but say why
        if os.environ.get("LOOM_DEVTEL_DEBUG"):
            print("devtel: %s" % error, file=sys.stderr)


# ---------------------------------------------------------------------------
# Context from git and the environment
# ---------------------------------------------------------------------------


def git(*args, cwd=None, env=None):
    """Run git and return stripped stdout, or "" when it fails."""
    try:
        result = subprocess.run(
            ["git"] + list(args), cwd=cwd, env=env, capture_output=True, text=True, timeout=10
        )
    except (OSError, subprocess.SubprocessError):
        return ""
    return result.stdout.strip() if result.returncode == 0 else ""


def common_resource():
    """Resource attributes on every span: the host and the worktree."""
    top = git("rev-parse", "--show-toplevel")
    return {
        "host.name": socket.gethostname(),
        "vcs.repository.name": "LoomDSP",
        "loom.devtel.worktree": os.path.basename(top) if top else "",
    }


def vcs_attributes(cwd=None):
    """Branch and commit, with the semantic convention names."""
    return {
        "vcs.ref.head.name": git("branch", "--show-current", cwd=cwd),
        "vcs.ref.head.revision": git("rev-parse", "HEAD", cwd=cwd),
    }


def tree_hash(cwd=None):
    """The git tree of the working copy as it is now, untracked files included.

    Two runs of a check with the same tree hash checked the same code, so
    the second told us nothing new. Uses a temporary index, so the real
    index and staging area are not touched.
    """
    index = git("rev-parse", "--git-path", "index", cwd=cwd)
    if not index:
        return ""
    if not os.path.isabs(index):
        index = os.path.join(cwd or os.getcwd(), index)
    handle, temp = tempfile.mkstemp(prefix="devtel-index-")
    os.close(handle)
    try:
        if os.path.exists(index):
            with open(index, "rb") as source, open(temp, "wb") as target:
                target.write(source.read())
        env = dict(os.environ, GIT_INDEX_FILE=temp)
        if git("add", "-A", cwd=cwd, env=env) == "" and not os.path.exists(temp):
            return ""
        return git("write-tree", cwd=cwd, env=env)
    finally:
        try:
            os.unlink(temp)
        except OSError:
            pass


def agent_attributes():
    """The Claude Code session a command runs in, from the SessionStart hook's variables."""
    return {"gen_ai.conversation.id": os.environ.get("LOOM_DEVTEL_SESSION", "")}


def parent_context():
    """The trace and parent span inherited through TRACEPARENT, or a new trace."""
    parsed = parse_traceparent(os.environ.get("TRACEPARENT", ""))
    if parsed:
        return parsed
    return new_trace_id(), None


# ---------------------------------------------------------------------------
# Writers
# ---------------------------------------------------------------------------


def run_step(name, command):
    """Run one check and record it as a span named after it.

    prek.toml wraps each check with run.sh, which calls this. The command's
    output and exit code are passed through unchanged. Child processes get
    TRACEPARENT pointing at this span, so anything they record nests under
    it. The working tree's hash is taken first, so two runs of a check on
    the same code can be found (devtel_reruns, "same tree").
    """
    trace_id, parent_id = parent_context()
    span_id = new_span_id()
    # Nothing that only feeds the span may stop the check from running.
    attributes = {}
    try:
        attributes = {
            "cicd.pipeline.task.name": name,
            "loom.devtel.step": name,
            "process.command_line": " ".join(command)[:MAX_COMMAND],
            "process.working_directory": os.getcwd(),
            "loom.devtel.tree": tree_hash(),
            # prek sets PRE_COMMIT=1 for every hook it runs, and the remote
            # name only before a push.
            "loom.devtel.in_git_hook": os.environ.get("PRE_COMMIT") == "1",
            "loom.devtel.in_push": bool(os.environ.get("PRE_COMMIT_REMOTE_NAME")),
        }
        attributes.update(vcs_attributes())
        attributes.update(agent_attributes())
    except Exception:  # noqa: BLE001 - fail open
        pass
    env = dict(os.environ, TRACEPARENT=traceparent(trace_id, span_id))
    start = time.time_ns()
    try:
        code = run_forwarding_signals(list(command), env)
    except OSError as error:
        print("devtel: cannot run %s: %s" % (command[0] if command else "", error), file=sys.stderr)
        code = 127
    end = time.time_ns()
    attributes["process.exit.code"] = code
    emit("hooks", "checks", [span(trace_id, span_id, parent_id, name, start, end, attributes, code != 0)])
    return code


def run_forwarding_signals(command, env):
    """Run a command, pass on the signals that stop it, and return its exit code.

    The wrapper sits between mise and the step, so a SIGTERM or SIGHUP sent
    to it (when mise cancels a task, or a terminal closes) must reach the
    step, or cargo would keep running after the task was stopped. A signal
    exit is returned as 128 plus the signal number, as a shell does.
    """
    import signal

    child = subprocess.Popen(command, env=env)
    forwarded = (signal.SIGTERM, signal.SIGHUP, signal.SIGINT)
    previous = {}

    def forward(number, _frame):
        try:
            child.send_signal(number)
        except OSError:
            pass

    for number in forwarded:
        previous[number] = signal.signal(number, forward)
    try:
        while True:
            try:
                code = child.wait()
                break
            except InterruptedError:
                continue
    finally:
        for number, handler in previous.items():
            signal.signal(number, handler)
    return 128 - code if code < 0 else code


def commit_stats(revision):
    """Files, insertions, and deletions of one commit, from git's numstat."""
    output = git("show", "--numstat", "--format=", revision)
    files = insertions = deletions = 0
    for line in output.splitlines():
        parts = line.split("\t")
        if len(parts) < 3:
            continue
        files += 1
        if parts[0].isdigit():
            insertions += int(parts[0])
        if parts[1].isdigit():
            deletions += int(parts[1])
    return files, insertions, deletions


def git_event(stage):
    """Record a git event, called by a prek hook in the given stage."""
    stage = stage or "event"
    trace_id, parent_id = parent_context()
    now = time.time_ns()
    attributes = vcs_attributes()
    attributes.update(agent_attributes())
    attributes["loom.devtel.git.event"] = stage
    env = os.environ.get
    if stage == "post-commit":
        revision = attributes["vcs.ref.head.revision"]
        files, insertions, deletions = commit_stats(revision)
        parents = git("rev-list", "--parents", "-n", "1", revision).split()
        body = git("log", "-1", "--format=%B", revision)
        attributes.update(
            {
                "loom.devtel.commit.files": files,
                "loom.devtel.commit.insertions": insertions,
                "loom.devtel.commit.deletions": deletions,
                "loom.devtel.commit.parents": max(len(parents) - 1, 0),
                "loom.devtel.commit.agent_coauthored": "Co-Authored-By: Claude" in body,
            }
        )
    elif stage == "post-checkout":
        attributes.update(
            {
                "loom.devtel.git.from": env("PRE_COMMIT_FROM_REF", ""),
                "loom.devtel.git.to": env("PRE_COMMIT_TO_REF", ""),
                "loom.devtel.git.branch_checkout": env("PRE_COMMIT_CHECKOUT_TYPE", "") == "1",
            }
        )
    elif stage == "post-merge":
        attributes["loom.devtel.git.squash"] = env("PRE_COMMIT_IS_SQUASH_MERGE", "") == "1"
    elif stage == "post-rewrite":
        attributes["loom.devtel.git.rewrite"] = env("PRE_COMMIT_REWRITE_COMMAND", "")
    elif stage == "pre-push":
        # The remote name only: a remote URL can hold a token.
        attributes.update(
            {
                "loom.devtel.git.remote": env("PRE_COMMIT_REMOTE_NAME", ""),
                "loom.devtel.git.remote_branch": env("PRE_COMMIT_REMOTE_BRANCH", ""),
            }
        )
    name = "git %s" % (stage or "event")
    emit("git", "git", [span(trace_id, new_span_id(), parent_id, name, now, now, attributes)])
    return 0


def claude_hook(event):
    """Handle a Claude Code hook event, given its JSON on stdin.

    SessionStart writes TRACEPARENT and LOOM_DEVTEL_SESSION into
    CLAUDE_ENV_FILE, so every Bash command of the session runs inside the
    session's trace, then starts an import in the background so
    transcripts reach the archive before Claude Code's cleanup removes
    them. Always succeeds and prints nothing.
    """
    try:
        payload = json.load(sys.stdin)
    except ValueError:
        return 0
    session_id = payload.get("session_id") or ""
    if not session_id:
        return 0
    trace_id = session_trace_id(session_id)
    root = session_root_span_id(session_id)
    if event == "session-start":
        env_file = os.environ.get("CLAUDE_ENV_FILE")
        if env_file:
            with open(env_file, "a", encoding="utf-8") as handle:
                handle.write("export TRACEPARENT=%s\n" % traceparent(trace_id, root))
                handle.write("export LOOM_DEVTEL_SESSION=%s\n" % session_id)
        now = time.time_ns()
        attributes = {
            "gen_ai.conversation.id": session_id,
            "gen_ai.request.model": payload.get("model") or "",
            "gen_ai.agent.name": payload.get("agent_type") or "",
            "loom.devtel.claude.source": payload.get("source") or "",
            "loom.devtel.claude.permission_mode": payload.get("permission_mode") or "",
        }
        attributes.update(vcs_attributes(payload.get("cwd")))
        emit("claude", "claude-code", [span(trace_id, new_span_id(), root, "claude session start", now, now, attributes)])
        start_background_import()
    return 0


def start_background_import():
    """Start an import that outlives the hook, with its output discarded."""
    try:
        subprocess.Popen(
            [sys.executable, os.path.abspath(__file__), "import", "--quiet"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except OSError:
        pass


# ---------------------------------------------------------------------------
# Import into the archive
# ---------------------------------------------------------------------------


def repo_root():
    """The main checkout, which every worktree shares."""
    common = git("rev-parse", "--path-format=absolute", "--git-common-dir")
    if common:
        return os.path.dirname(common.rstrip("/"))
    return git("rev-parse", "--show-toplevel") or os.getcwd()


def claude_project_prefix(root):
    """How Claude Code names the project folders of a checkout and its worktrees."""
    return re.sub(r"[^A-Za-z0-9]", "-", root).lower()


def claude_files():
    """Every transcript and subagent metadata file of this repository's sessions."""
    base = os.environ.get("LOOM_DEVTEL_CLAUDE_PROJECTS") or os.path.join(os.path.expanduser("~"), ".claude", "projects")
    prefix = claude_project_prefix(repo_root())
    found = []
    if not os.path.isdir(base):
        return found
    for name in os.listdir(base):
        if not name.lower().startswith(prefix):
            continue
        for folder, _dirs, files in os.walk(os.path.join(base, name)):
            for file in files:
                if file.endswith(".jsonl") or file.endswith(".meta.json"):
                    found.append(os.path.join(folder, file))
    return found


def spool_files():
    """Every spool file."""
    found = []
    for folder, _dirs, files in os.walk(os.path.join(store_dir(), "spool")):
        for file in files:
            if file.endswith(".jsonl"):
                found.append(os.path.join(folder, file))
    return found


def sql_literal(text):
    """Quote a string for SQL."""
    return "'" + text.replace("'", "''") + "'"


def sql_list(paths):
    """A SQL list of file paths."""
    return "[" + ", ".join(sql_literal(path) for path in paths) + "]"


class ArchiveBusy(RuntimeError):
    """Another process holds the archive in a way this call cannot share."""


def run_duckdb(sql, json_output=False, database=None, readonly=False, wait=0.0):
    """Run SQL in DuckDB against the archive and return stdout.

    DuckDB lets many read-only connections share a file, or one writer have
    it alone. Readers open it read-only, so the DuckDB UI, a report, and the
    trace viewer can all be open at once. A call that meets a lock held by
    another process tries again for up to `wait` seconds (an import takes a
    few), then raises ArchiveBusy.
    """
    command = duckdb_cmd() + (["-json"] if json_output else []) + (["-readonly"] if readonly else [])
    command.append(database or archive_path())
    deadline = time.time() + wait
    while True:
        result = subprocess.run(command, input=".bail on\n" + sql, capture_output=True, text=True)
        if result.returncode == 0 and "Error:" not in result.stderr:
            return result.stdout
        if "Could not set lock" not in result.stderr:
            raise RuntimeError(result.stderr.strip() or "duckdb failed")
        if time.time() >= deadline:
            raise ArchiveBusy(result.stderr.strip())
        time.sleep(0.5)


def read_sql(name):
    """One of the SQL files that ship with this script."""
    with open(os.path.join(SQL_DIR, name), encoding="utf-8") as handle:
        return handle.read()


def changed_files(kind, paths):
    """The files whose size or modification time differs from the last import."""
    stats = []
    for path in paths:
        try:
            info = os.stat(path)
        except OSError:
            continue
        stats.append((path, info.st_size, int(info.st_mtime)))
    if not stats:
        return [], []
    known = {}
    output = run_duckdb(
        "select path, size, mtime from imported_files where kind = %s;" % sql_literal(kind), json_output=True
    )
    for row in json.loads(output or "[]"):
        known[row["path"]] = (row["size"], row["mtime"])
    changed = [entry for entry in stats if known.get(entry[0]) != (entry[1], entry[2])]
    return changed, stats


def record_files(kind, entries):
    """SQL that marks files as imported at their current size."""
    if not entries:
        return ""
    values = ", ".join(
        "(%s, %s, %d, %d, now())" % (sql_literal(kind), sql_literal(path), size, mtime) for path, size, mtime in entries
    )
    return "insert or replace into imported_files values %s;\n" % values


def import_all(quiet=False):
    """Copy new Claude Code records and spool spans into the archive.

    Idempotent: every row has a natural key and the import only inserts,
    so running it twice changes nothing, and a transcript Claude Code has
    deleted keeps every row it already gave.
    """
    os.makedirs(store_dir(), exist_ok=True)
    lock_path = os.path.join(store_dir(), "import.lock")
    lock = open(lock_path, "w")
    try:
        import fcntl

        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except (ImportError, OSError):
        if not quiet:
            print("devtel: another import is running; try again shortly", file=sys.stderr)
        return 1
    try:
        return import_locked(quiet)
    except ArchiveBusy:
        if not quiet:
            print(
                "devtel: the archive is open elsewhere (the DuckDB UI or the trace viewer), so the import was "
                "skipped; showing the data of the last import. Close the other window to import new data.",
                file=sys.stderr,
            )
        return 0
    finally:
        lock.close()


def import_locked(quiet):
    """The import itself, run while holding import.lock."""
    run_duckdb(read_sql("schema.sql"))
    claude = claude_files()
    transcripts = [path for path in claude if path.endswith(".jsonl")]
    metas = [path for path in claude if path.endswith(".meta.json")]
    sql = []
    counts = {}
    for kind, paths, template in (
        ("claude", transcripts, "import_claude.sql"),
        ("claude-meta", metas, "import_agents.sql"),
        ("spool", spool_files(), "import_spool.sql"),
    ):
        changed, _all = changed_files(kind, paths)
        counts[kind] = len(changed)
        # Batches keep each statement's file list to a reasonable size.
        for start in range(0, len(changed), 200):
            batch = changed[start : start + 200]
            sql.append(read_sql(template).replace("$FILES", sql_list([entry[0] for entry in batch])))
            sql.append(record_files(kind, batch))
    sql.append(read_sql("views.sql"))
    run_duckdb("begin;\n" + "\n".join(sql) + "\ncommit;\n")
    if not quiet:
        print(
            "devtel: imported %d transcripts, %d agent files, %d spool files into %s"
            % (counts["claude"], counts["claude-meta"], counts["spool"], archive_path())
        )
    return 0


# ---------------------------------------------------------------------------
# Readers
# ---------------------------------------------------------------------------


def report(days):
    """Print the efficiency report for the last given days."""
    import_all(quiet=True)
    sql = read_sql("report.sql").replace("$DAYS", str(int(days)))
    sys.stdout.write(run_duckdb(".mode box\n" + sql, readonly=True, wait=30))
    return 0


def open_ui():
    """Open the DuckDB UI over the archive, after an import."""
    import_all()
    print("devtel: opening the DuckDB UI. The page loads its assets from ui.duckdb.org; your data stays local.")
    print("devtel: start with `from devtel_sessions` or `from devtel_reruns`. Quit with Ctrl-D.")
    # Read-only, so reports, imports of other windows, and the trace viewer
    # can still read the archive while the UI is open.
    return subprocess.call(duckdb_cmd() + ["-readonly", "-ui", archive_path()])


def export_otlp(path, days):
    """Write the archive's spans as OTLP JSON lines, one trace per line."""
    import_all(quiet=True)
    sql = read_sql("export.sql").replace("$DAYS", str(int(days)))
    rows = json.loads(run_duckdb(sql, json_output=True, readonly=True, wait=30) or "[]")
    by_trace = {}
    for row in rows:
        by_trace.setdefault((row["service"], row["trace_id"]), []).append(row)
    with open(path, "w", encoding="utf-8") as handle:
        for (service, _trace), items in sorted(by_trace.items()):
            spans_list = []
            for row in items:
                attributes = json.loads(row["attributes"] or "{}")
                spans_list.append(
                    span(
                        row["trace_id"],
                        row["span_id"],
                        row["parent_span_id"],
                        row["name"],
                        int(row["start_ns"]),
                        int(row["end_ns"]),
                        attributes,
                        bool(row["error"]),
                    )
                )
            handle.write(traces_line(service, {"vcs.repository.name": "LoomDSP"}, spans_list) + "\n")
    return len(rows)


def wait_for_port(port, seconds):
    """Wait until something listens on a local port."""
    deadline = time.time() + seconds
    while time.time() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.5):
                return True
        except OSError:
            time.sleep(0.25)
    return False


def open_traces(days, port, open_browser=True):
    """Open otel-desktop-viewer with the archive's spans loaded.

    The viewer keeps data in memory and is filled again on every start,
    so it never holds a second copy that could drift from the archive.
    """
    export = os.path.join(store_dir(), "export", "traces.jsonl")
    os.makedirs(os.path.dirname(export), exist_ok=True)
    count = export_otlp(export, days)
    print("devtel: %d spans from the last %d days" % (count, days), flush=True)
    # Its OTLP receivers get ports next to the page, so they do not clash
    # with a collector already on the standard ports 4317 and 4318.
    import signal

    viewer = subprocess.Popen(
        viewer_cmd()
        + ["--browser-port", str(port), "--grpc", str(port + 1), "--http", str(port + 2), "--open-browser=false"]
    )

    def stop(_number, _frame):
        raise KeyboardInterrupt

    # Stopped by Ctrl-C, a closed terminal, or mise cancelling the task, the
    # viewer must stop too, or it keeps the port.
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGHUP, stop)
    try:
        if not wait_for_port(port, 60):
            print("devtel: the viewer did not start on port %d" % port, file=sys.stderr)
            return 1
        subprocess.call(viewer_cmd() + ["import", export, "--endpoint", "http://localhost:%d" % port])
        url = "http://localhost:%d" % port
        print("devtel: traces at %s (pick a wide time range). Stop with Ctrl-C." % url, flush=True)
        if open_browser:
            opener = "open" if sys.platform == "darwin" else "xdg-open"
            subprocess.call([opener, url], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return viewer.wait()
    except KeyboardInterrupt:
        return 0
    finally:
        if viewer.poll() is None:
            viewer.terminate()
            try:
                viewer.wait(timeout=10)
            except subprocess.TimeoutExpired:
                viewer.kill()


def main(argv):
    """Command-line entry point."""
    parser = argparse.ArgumentParser(prog="devtel", description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run", help="run a check and record it (used by run.sh)")
    run.add_argument("name")
    run.add_argument("argv", nargs=argparse.REMAINDER, metavar="command")
    git_parser = commands.add_parser("git", help="record a git event")
    git_parser.add_argument("stage", nargs="?", default="")
    claude = commands.add_parser("claude", help="handle a Claude Code hook event (JSON on stdin)")
    claude.add_argument("event")
    importer = commands.add_parser("import", help="copy transcripts and the spool into the archive")
    importer.add_argument("--quiet", action="store_true")
    report_parser = commands.add_parser("report", help="print the efficiency report")
    report_parser.add_argument("--days", type=int, default=14)
    commands.add_parser("ui", help="open the DuckDB UI over the archive")
    traces = commands.add_parser("traces", help="open otel-desktop-viewer with the spans loaded")
    traces.add_argument("--days", type=int, default=14)
    traces.add_argument("--port", type=int, default=8000)
    traces.add_argument("--no-open", action="store_true", help="do not open a browser")
    export = commands.add_parser("export", help="write the spans as OTLP JSON lines")
    export.add_argument("path")
    export.add_argument("--days", type=int, default=3650)
    args = parser.parse_args(argv)

    if args.command == "run":
        command = args.argv[1:] if args.argv[:1] == ["--"] else args.argv
        if not command:
            return 2
        if not enabled():
            os.execvp(command[0], command)
        return run_step(args.name, command)
    if args.command == "git":
        return git_event(args.stage) if enabled() else 0
    if args.command == "claude":
        if enabled():
            try:
                claude_hook(args.event)
            except Exception:  # noqa: BLE001 - a hook must never fail the session
                pass
        return 0
    try:
        return readers(args)
    except RuntimeError as error:
        print("devtel: %s" % error, file=sys.stderr)
        return 1


def readers(args):
    """Run one of the commands that read the archive."""
    if args.command == "import":
        return import_all(args.quiet)
    if args.command == "report":
        return report(args.days)
    if args.command == "ui":
        return open_ui()
    if args.command == "traces":
        return open_traces(args.days, args.port, not args.no_open)
    if args.command == "export":
        print("devtel: wrote %d spans to %s" % (export_otlp(args.path, args.days), args.path))
        return 0
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

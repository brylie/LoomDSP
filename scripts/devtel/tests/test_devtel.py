"""Tests for scripts/devtel/devtel.py (mise run devtel-test).

The unit tests need only Python. The archive tests run DuckDB through
`mise exec`, and are skipped when mise is not on PATH.
"""

import hashlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import devtel  # noqa: E402

SESSION = "0a1b2c3d-1111-2222-3333-444455556666"


class StoreTestCase(unittest.TestCase):
    """Gives each test its own store folder and a clean devtel environment."""

    def setUp(self):
        """Point devtel at a temporary store, with recording on."""
        self.dir = tempfile.mkdtemp(prefix="devtel-test-")
        self.env = mock.patch.dict(
            os.environ,
            {"LOOM_DEVTEL_DIR": os.path.join(self.dir, "store"), "LOOM_DEVTEL": "1"},
        )
        self.env.start()
        for name in ("TRACEPARENT", "LOOM_DEVTEL_SESSION", "CLAUDE_ENV_FILE", "MISE_TASK_NAME"):
            os.environ.pop(name, None)

    def tearDown(self):
        """Remove the temporary store."""
        self.env.stop()
        shutil.rmtree(self.dir, ignore_errors=True)

    def spool_spans(self, source):
        """Every span written to today's spool file for a source."""
        spans = []
        for folder, _dirs, files in os.walk(os.path.join(self.dir, "store", "spool")):
            for file in files:
                if file == source + ".jsonl":
                    with open(os.path.join(folder, file), encoding="utf-8") as handle:
                        for line in handle:
                            data = json.loads(line)
                            for resource in data["resourceSpans"]:
                                for scope in resource["scopeSpans"]:
                                    spans.extend(scope["spans"])
        return spans


def attributes_of(span):
    """A span's attributes as a plain dict of the encoded values."""
    return {item["key"]: list(item["value"].values())[0] for item in span["attributes"]}


class EncodingTests(unittest.TestCase):
    """OTLP JSON encoding and trace context."""

    def test_traceparent_round_trip(self):
        """A traceparent written for a child parses back to the same IDs."""
        trace_id, span_id = "a" * 32, "b" * 16
        self.assertEqual(devtel.parse_traceparent(devtel.traceparent(trace_id, span_id)), (trace_id, span_id))

    def test_traceparent_rejects_invalid(self):
        """All-zero IDs, wrong lengths, and other versions are ignored."""
        for value in ("", "garbage", "00-%s-%s-01" % ("0" * 32, "b" * 16), "00-%s-%s-01" % ("a" * 31, "b" * 16)):
            self.assertIsNone(devtel.parse_traceparent(value), value)

    def test_line_follows_otlp_json_encoding(self):
        """Hex IDs, string nanoseconds, integer enums, and typed values (OTLP spec)."""
        record = devtel.span("a" * 32, "b" * 16, "c" * 16, "x", 1, 2, {"n": 3, "ok": True, "s": "t", "empty": ""}, True)
        line = json.loads(devtel.traces_line("svc", {}, [record]))
        span = line["resourceSpans"][0]["scopeSpans"][0]["spans"][0]
        self.assertEqual(span["startTimeUnixNano"], "1")
        self.assertEqual(span["kind"], 1)
        self.assertEqual(span["status"], {"code": 2})
        self.assertEqual(span["parentSpanId"], "c" * 16)
        values = {item["key"]: item["value"] for item in span["attributes"]}
        self.assertEqual(values["n"], {"intValue": "3"})
        self.assertEqual(values["ok"], {"boolValue": True})
        self.assertNotIn("empty", values)
        resource = {item["key"]: item["value"] for item in line["resourceSpans"][0]["resource"]["attributes"]}
        self.assertEqual(resource["service.name"], {"stringValue": "svc"})

    def test_session_ids_match_the_sql_views(self):
        """The hook and views.sql derive the same root span ID (md5, first 16 hex)."""
        expected = hashlib.md5(("session:" + SESSION).encode()).hexdigest()[:16]
        self.assertEqual(devtel.session_root_span_id(SESSION), expected)
        self.assertEqual(devtel.session_trace_id(SESSION), SESSION.replace("-", ""))

    def test_project_prefix_matches_claude_code_folders(self):
        """Claude Code turns every character but letters and digits into a dash."""
        self.assertEqual(devtel.claude_project_prefix("/Users/a/code/LoomDSP"), "-users-a-code-loomdsp")


class SwitchTests(unittest.TestCase):
    """When recording is on."""

    def test_off_in_ci_unless_asked(self):
        """CI turns recording off; LOOM_DEVTEL=1 turns it back on; 0 always wins."""
        with mock.patch.dict(os.environ, {"CI": "true", "LOOM_DEVTEL": ""}):
            self.assertFalse(devtel.enabled())
        with mock.patch.dict(os.environ, {"CI": "true", "LOOM_DEVTEL": "1"}):
            self.assertTrue(devtel.enabled())
        with mock.patch.dict(os.environ, {"CI": "", "LOOM_DEVTEL": "0"}):
            self.assertFalse(devtel.enabled())


class WriterTests(StoreTestCase):
    """The task wrapper, git events, and the Claude Code hook."""

    def test_run_passes_exit_code_and_records_a_span(self):
        """A failing check keeps its exit code and is recorded as an error."""
        code = devtel.run_step("demo", ["sh", "-c", "exit 3"])
        self.assertEqual(code, 3)
        (span,) = self.spool_spans("hooks")
        self.assertEqual(span["name"], "demo")
        self.assertEqual(span["status"], {"code": 2})
        values = attributes_of(span)
        self.assertEqual(values["process.exit.code"], "3")
        self.assertEqual(values["loom.devtel.step"], "demo")

    def test_run_children_inherit_its_span(self):
        """A child process sees TRACEPARENT naming the check's own span."""
        os.environ["TRACEPARENT"] = devtel.traceparent("a" * 32, "b" * 16)
        out = os.path.join(self.dir, "tp")
        devtel.run_step("demo", ["sh", "-c", 'printf "%%s" "$TRACEPARENT" > "%s"' % out])
        (span,) = self.spool_spans("hooks")
        self.assertEqual(span["traceId"], "a" * 32)
        self.assertEqual(span["parentSpanId"], "b" * 16)
        with open(out, encoding="utf-8") as handle:
            self.assertEqual(devtel.parse_traceparent(handle.read()), ("a" * 32, span["spanId"]))

    def test_run_reports_a_missing_command(self):
        """A command that cannot start fails like a shell would, with 127."""
        self.assertEqual(devtel.run_step("demo", ["devtel-no-such-command"]), 127)

    def test_run_sh_runs_directly_when_off(self):
        """With devtel off, run.sh runs the command and records nothing."""
        script = os.path.join(os.path.dirname(devtel.__file__), "run.sh")
        env = dict(os.environ, LOOM_DEVTEL="0")
        result = subprocess.run(["sh", script, "demo", "sh", "-c", "echo hi; exit 4"], env=env, capture_output=True, text=True)
        self.assertEqual((result.returncode, result.stdout), (4, "hi\n"))
        self.assertEqual(self.spool_spans("hooks"), [])

    def test_git_event_records_stage(self):
        """A git event names its stage and the branch."""
        devtel.git_event("post-merge")
        (span,) = self.spool_spans("git")
        self.assertEqual(attributes_of(span)["loom.devtel.git.event"], "post-merge")

    def test_session_start_exports_trace_context(self):
        """SessionStart writes the session's trace into CLAUDE_ENV_FILE."""
        env_file = os.path.join(self.dir, "env")
        os.environ["CLAUDE_ENV_FILE"] = env_file
        payload = json.dumps({"session_id": SESSION, "source": "startup", "model": "m"})
        with mock.patch.object(devtel, "start_background_import") as started, mock.patch("sys.stdin", io.StringIO(payload)):
            devtel.claude_hook("session-start")
        started.assert_called_once()
        with open(env_file, encoding="utf-8") as handle:
            text = handle.read()
        root = devtel.session_root_span_id(SESSION)
        self.assertIn("export TRACEPARENT=00-%s-%s-01" % (devtel.session_trace_id(SESSION), root), text)
        self.assertIn("export LOOM_DEVTEL_SESSION=%s" % SESSION, text)

    def test_hook_survives_bad_input(self):
        """The hook entry point exits 0 on input that is not JSON."""
        with mock.patch("sys.stdin", io.StringIO("not json")):
            self.assertEqual(devtel.main(["claude", "session-start"]), 0)


def iso(seconds):
    """An ISO timestamp like the ones in Claude Code transcripts."""
    return time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime(seconds)) + ".000Z"


@unittest.skipUnless(shutil.which("mise"), "the archive tests run DuckDB through mise")
class ArchiveTests(StoreTestCase):
    """The import into DuckDB and the views, on a small made-up history."""

    def setUp(self):
        """Write a made-up transcript, subagent file, and task span."""
        super().setUp()
        projects = os.path.join(self.dir, "projects")
        os.environ["LOOM_DEVTEL_CLAUDE_PROJECTS"] = projects
        folder = os.path.join(projects, devtel.claude_project_prefix(devtel.repo_root()) + "--claude-worktrees-demo")
        os.makedirs(os.path.join(folder, SESSION, "subagents"))
        self.transcript = os.path.join(folder, SESSION + ".jsonl")
        now = time.time() - 60
        base = {"sessionId": SESSION, "gitBranch": "demo", "cwd": "/repo", "version": "1"}
        records = [
            dict(base, type="user", uuid="u1", timestamp=iso(now), message={"role": "user", "content": "secret prompt"}),
            dict(base, type="assistant", uuid="a1", timestamp=iso(now + 1), message={
                "model": "claude-test", "usage": {"input_tokens": 5, "output_tokens": 7},
                "content": [{"type": "text", "text": "private reply"},
                            {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "cd x && cargo test -p a 2>&1 | tail -5"}}]}),
            dict(base, type="user", uuid="u2", timestamp=iso(now + 20), message={"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": "private output"}]}),
            dict(base, type="assistant", uuid="a2", timestamp=iso(now + 21), message={"model": "claude-test", "content": [
                {"type": "tool_use", "id": "t2", "name": "Bash", "input": {"command": "cargo test -p a"}}]}),
            dict(base, type="user", uuid="u3", timestamp=iso(now + 30), message={"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t2", "content": "ok"}]}),
        ]
        with open(self.transcript, "w", encoding="utf-8") as handle:
            for record in records:
                handle.write(json.dumps(record) + "\n")
        with open(os.path.join(folder, SESSION, "subagents", "agent-abc.meta.json"), "w", encoding="utf-8") as handle:
            json.dump({"agentType": "Explore", "toolUseId": "t1", "model": "claude-small"}, handle)
        # A check that ran inside the first Bash call, in the session's
        # trace, and the same check again later on the same tree.
        os.environ["TRACEPARENT"] = devtel.traceparent(devtel.session_trace_id(SESSION), devtel.session_root_span_id(SESSION))
        start = int((now + 5) * 1e9)
        check = {"loom.devtel.step": "clippy", "loom.devtel.tree": "t" * 40, "process.exit.code": 0}
        devtel.emit("hooks", "checks", [devtel.span(devtel.session_trace_id(SESSION), "f" * 16, devtel.session_root_span_id(SESSION),
                                                    "clippy", start, start + int(1e9), check)])
        devtel.emit("hooks", "checks", [devtel.span(devtel.new_trace_id(), "e" * 16, None,
                                                    "clippy", start + int(40e9), start + int(41e9), check)])
        del os.environ["TRACEPARENT"]

    def query(self, sql):
        """Run SQL against the archive and return the rows."""
        return json.loads(devtel.run_duckdb(sql, json_output=True) or "[]")

    def test_import_is_idempotent_and_outlives_its_sources(self):
        """Importing twice adds nothing, and deleted transcripts keep their rows."""
        self.assertEqual(devtel.import_all(quiet=True), 0)
        first = self.query("select count(*) as n from claude_records")[0]["n"]
        self.assertEqual(devtel.import_all(quiet=True), 0)
        self.assertEqual(self.query("select count(*) as n from claude_records")[0]["n"], first)
        os.unlink(self.transcript)
        self.assertEqual(devtel.import_all(quiet=True), 0)
        self.assertEqual(self.query("select count(*) as n from claude_records")[0]["n"], first)
        self.assertEqual(first, 5)

    def test_archive_keeps_metadata_not_content(self):
        """Prompts, replies, and tool output are not stored (ADR-0048)."""
        devtel.import_all(quiet=True)
        dump = json.dumps(self.query("select * from claude_records"))
        for text in ("secret prompt", "private reply", "private output"):
            self.assertNotIn(text, dump)
        self.assertIn("cargo test -p a", dump)

    def test_views_join_the_sources(self):
        """Tool calls get times and kinds; spans nest; a repeated check is a re-run."""
        devtel.import_all(quiet=True)
        tools = self.query("select tool_use_id, category, normalized, epoch_ms(end_ts) - epoch_ms(start_ts) as ms from devtel_tools order by 1")
        self.assertEqual([row["category"] for row in tools], ["test", "test"])
        self.assertEqual(tools[0]["normalized"], "cargo test -p a")
        self.assertEqual(tools[0]["ms"], 19000)
        parent = self.query("select parent_span_id from devtel_spans where span_id = '%s'" % ("f" * 16))[0]["parent_span_id"]
        self.assertEqual(parent, devtel.derived_id("tool:t1"))
        agent = self.query("select parent_span_id, attributes['gen_ai.agent.name'] as name from devtel_spans where category = 'agent'")
        self.assertEqual(agent, [])  # no records carry the agent's ID, so it has no span
        reruns = self.query("select kind, command from devtel_reruns order by kind")
        self.assertEqual(reruns, [{"kind": "no change", "command": "cargo test -p a"}, {"kind": "same tree", "command": "clippy"}])

    def test_report_and_export_run(self):
        """The report prints, and the export writes valid OTLP lines."""
        devtel.import_all(quiet=True)
        output = subprocess.run(
            [sys.executable, devtel.__file__, "report", "--days", "30"], capture_output=True, text=True, env=os.environ
        )
        self.assertEqual(output.returncode, 0, output.stderr)
        self.assertIn("Where the time goes", output.stdout)
        path = os.path.join(self.dir, "out.jsonl")
        self.assertGreater(devtel.export_otlp(path, 30), 0)
        with open(path, encoding="utf-8") as handle:
            for line in handle:
                self.assertIn("resourceSpans", json.loads(line))

    def test_readers_share_the_archive_and_an_import_waits_its_turn(self):
        """While another command reads the archive, the import skips and readers still work."""
        import select

        devtel.import_all(quiet=True)
        holder = subprocess.Popen(
            devtel.duckdb_cmd() + ["-readonly", "-noheader", "-list", devtel.archive_path()],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        try:
            # The holder answers a query only once it has the archive open.
            holder.stdin.write("select 4242;\n")
            holder.stdin.flush()
            ready, _, _ = select.select([holder.stdout], [], [], 60)
            self.assertTrue(ready, "the holder never answered")
            self.assertEqual(holder.stdout.readline().strip(), "4242")
            # A span written now is new data the skipped import must leave out.
            devtel.emit("hooks", "checks", [devtel.span("a" * 32, "d" * 16, None, "late", 1, 2, {})])
            self.assertEqual(devtel.import_all(quiet=True), 0)
            count = "select count(*) as n from spool_spans where span_id = '%s'" % ("d" * 16)
            self.assertEqual(json.loads(devtel.run_duckdb(count, json_output=True, readonly=True))[0]["n"], 0)
            self.assertGreater(devtel.export_otlp(os.path.join(self.dir, "out.jsonl"), 30), 0)
        finally:
            holder.stdin.close()
            holder.wait(timeout=30)
        # Once the other reader closes, the next import brings the new span in.
        self.assertEqual(devtel.import_all(quiet=True), 0)
        self.assertEqual(json.loads(devtel.run_duckdb(count, json_output=True, readonly=True))[0]["n"], 1)
    def test_import_skips_while_another_import_holds_the_lock(self):
        """Only one import writes at a time; another one skips and adds nothing."""
        devtel.import_all(quiet=True)
        devtel.emit("hooks", "checks", [devtel.span("a" * 32, "c" * 16, None, "late", 1, 2, {})])
        count = "select count(*) as n from spool_spans where span_id = '%s'" % ("c" * 16)
        lock = devtel.take_import_lock(wait=5)
        self.assertIsNotNone(lock)
        try:
            self.assertEqual(devtel.import_all(quiet=True), 1)
            self.assertEqual(json.loads(devtel.run_duckdb(count, json_output=True, readonly=True))[0]["n"], 0)
        finally:
            lock.close()
        self.assertEqual(devtel.import_all(quiet=True), 0)
        self.assertEqual(json.loads(devtel.run_duckdb(count, json_output=True, readonly=True))[0]["n"], 1)


if __name__ == "__main__":
    unittest.main()

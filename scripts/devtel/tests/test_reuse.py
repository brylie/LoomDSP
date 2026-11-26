"""Tests for reusing a pass on the same tree (scripts/devtel/reuse.py).

Each test works in a temporary git repository, so no pass record is ever
written into the repository the tests run in. The tool versions are a
fixed stub, so the tests need no mise, except the one for run.sh.
"""

import contextlib
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
import reuse  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
RUN_SH = os.path.join(HERE, "..", "run.sh")


def git(cwd, *args):
    """Run git in a test repository."""
    subprocess.run(["git"] + list(args), cwd=cwd, check=True, capture_output=True)


class ReuseTestCase(unittest.TestCase):
    """A temporary repository as the working folder, a temporary store, and reuse on."""

    def setUp(self):
        """Make a repository with one commit and change into it."""
        self.dir = tempfile.mkdtemp(prefix="devtel-reuse-test-")
        self.repo = os.path.join(self.dir, "repo")
        os.makedirs(self.repo)
        git(self.repo, "init", "-q")
        git(self.repo, "config", "user.email", "test@example.com")
        git(self.repo, "config", "user.name", "Test")
        with open(os.path.join(self.repo, ".gitignore"), "w", encoding="utf-8") as handle:
            handle.write("/.cargo/config.toml\n")
        self.write("src.rs", "fn main() {}\n")
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-q", "-m", "first")
        self.cargo_home = os.path.join(self.dir, "cargo-home")
        os.makedirs(self.cargo_home)
        self.env = mock.patch.dict(
            os.environ,
            {
                "LOOM_DEVTEL_DIR": os.path.join(self.dir, "store"),
                "LOOM_DEVTEL": "1",
                "LOOM_CHECK_REUSE": "",
                "CI": "",
                "CARGO_HOME": self.cargo_home,
            },
        )
        self.env.start()
        for name in reuse.ENV_KEYS + ("TRACEPARENT", "LOOM_DEVTEL_SESSION", "PRE_COMMIT"):
            os.environ.pop(name, None)
        self.versions = mock.patch.object(reuse, "tool_versions", return_value='{"rust": ["1.100.0"]}')
        self.versions.start()
        self.old_cwd = os.getcwd()
        os.chdir(self.repo)
        # The check counts its runs in a file outside the tree, so counting
        # does not change the tree.
        self.counter = os.path.join(self.dir, "runs")

    def tearDown(self):
        """Go back, stop the patches, and remove the temporary folders."""
        os.chdir(self.old_cwd)
        self.versions.stop()
        self.env.stop()
        shutil.rmtree(self.dir, ignore_errors=True)

    def write(self, name, text):
        """Write a file in the test repository."""
        path = os.path.join(self.repo, name)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(text)

    def check(self, exit_code=0, name="demo"):
        """Run a check that counts itself and exits with exit_code; return (code, stdout)."""
        command = ["sh", "-c", 'echo run >> "%s"; exit %d' % (self.counter, exit_code)]
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = devtel.run_step(name, command)
        return code, out.getvalue()

    def runs(self):
        """How many times the check really ran."""
        try:
            with open(self.counter, encoding="utf-8") as handle:
                return len(handle.readlines())
        except FileNotFoundError:
            return 0

    def records(self):
        """The file names in the pass record folder, temporary files left out."""
        folder = reuse.records_dir()
        if not os.path.isdir(folder):
            return []
        return [name for name in os.listdir(folder) if not name.startswith(".tmp-")]

    def check_spans(self):
        """Every check span in the temporary store's spool."""
        spans = []
        for folder, _dirs, files in os.walk(os.path.join(self.dir, "store", "spool")):
            for file in files:
                if file == "hooks.jsonl":
                    with open(os.path.join(folder, file), encoding="utf-8") as handle:
                        for line in handle:
                            for resource in json.loads(line)["resourceSpans"]:
                                for scope in resource["scopeSpans"]:
                                    spans.extend(scope["spans"])
        return spans


class ReuseTests(ReuseTestCase):
    """When a pass is reused, and when the check runs."""

    def test_pass_is_reused_on_the_same_key(self):
        """The second run on the same tree is skipped, says so, and exits 0."""
        self.assertEqual(self.check(), (0, ""))
        code, out = self.check()
        self.assertEqual((code, self.runs()), (0, 1))
        self.assertIn("reused: passed on this tree", out)
        self.assertIn("LOOM_CHECK_REUSE=0", out)

    def test_reuse_is_recorded_on_the_span(self):
        """The reused run's span says it was reused and how long the pass took."""
        self.check()
        self.check()
        first, second = self.check_spans()
        values = {item["key"]: item["value"] for item in second["attributes"]}
        self.assertNotIn("loom.devtel.reused", {item["key"] for item in first["attributes"]})
        self.assertEqual(values["loom.devtel.reused"], {"boolValue": True})
        self.assertIn("loom.devtel.reused_ms", values)

    def test_changed_tree_runs_the_check(self):
        """Editing a file, or adding an untracked one, changes the key."""
        self.check()
        self.write("src.rs", "fn main() { }\n")
        self.check()
        self.write("new.rs", "\n")
        self.check()
        self.assertEqual(self.runs(), 3)

    def test_changed_tools_run_the_check(self):
        """A new tool version changes the key."""
        self.check()
        reuse.tool_versions.return_value = '{"rust": ["1.101.0"]}'
        self.check()
        self.assertEqual(self.runs(), 2)

    def test_changed_environment_runs_the_check(self):
        """A listed variable such as RUSTFLAGS changes the key."""
        self.check()
        os.environ["RUSTFLAGS"] = "-D warnings"
        self.check()
        self.assertEqual(self.runs(), 2)

    def test_changed_cargo_config_outside_the_tree_runs_the_check(self):
        """The ignored worktree config and CARGO_HOME's config change the key."""
        self.check()
        self.write(".cargo/config.toml", "[build]\njobs = 2\n")
        self.check()
        with open(os.path.join(self.cargo_home, "config.toml"), "w", encoding="utf-8") as handle:
            handle.write("[build]\n")
        self.check()
        self.assertEqual(self.runs(), 3)

    def test_other_command_line_runs_the_check(self):
        """The command line, file names included, is part of the key."""
        self.check()
        self.check(name="other")
        self.assertEqual(self.runs(), 2)
        key_a = reuse.key("typos", ["typos", "a.md"], "t", versions="v")
        key_b = reuse.key("typos", ["typos", "b.md"], "t", versions="v")
        self.assertNotEqual(key_a, key_b)

    def test_failure_is_never_recorded(self):
        """A failed check runs again, and keeps its exit code."""
        self.assertEqual(self.check(exit_code=1)[0], 1)
        self.assertEqual(self.check(exit_code=1)[0], 1)
        self.assertEqual((self.runs(), self.records()), (2, []))

    def test_check_that_changes_the_tree_is_not_recorded(self):
        """A formatter exits 0 after it rewrote files, so that is no pass."""
        fixer = ["sh", "-c", 'echo run >> "%s"; echo formatted > src.rs' % self.counter]
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(devtel.run_step("fmt", fixer), 0)
        self.assertEqual(self.records(), [])

    def test_switch_off_and_ci_always_run(self):
        """LOOM_CHECK_REUSE=0 and CI both run the check, even with a record."""
        self.check()
        os.environ["LOOM_CHECK_REUSE"] = "0"
        self.check()
        os.environ["LOOM_CHECK_REUSE"] = "1"
        os.environ["CI"] = "true"
        self.check()
        self.assertEqual(self.runs(), 3)

    def test_reuse_works_with_telemetry_off(self):
        """LOOM_DEVTEL=0 stops the spans, not the reuse."""
        os.environ["LOOM_DEVTEL"] = "0"
        self.check()
        self.check()
        self.assertEqual((self.runs(), self.check_spans()), (1, []))

    def test_damaged_record_runs_the_check(self):
        """A record that is not valid JSON, or names another key, is ignored."""
        self.check()
        (name,) = self.records()
        path = os.path.join(reuse.records_dir(), name)
        with open(path, "w", encoding="utf-8") as handle:
            handle.write("{not json")
        self.check()
        with open(path, "w", encoding="utf-8") as handle:
            json.dump({"key": "other", "passed_at": time.time()}, handle)
        self.check()
        self.assertEqual(self.runs(), 3)

    def test_old_records_are_not_reused_and_are_pruned(self):
        """A record past MAX_AGE_SECONDS runs the check, and the next write deletes it."""
        old = time.time() - reuse.MAX_AGE_SECONDS - 10
        reuse.store("a" * 64, {"check": "x"}, now=old)
        self.assertIsNone(reuse.lookup("a" * 64))
        path = os.path.join(reuse.records_dir(), "a" * 64)
        os.utime(path, (old, old))
        reuse.store("b" * 64, {"check": "y"})
        self.assertEqual(self.records(), ["b" * 64])

    def test_clear_deletes_every_record(self):
        """reuse-clear forgets every pass, so the next run checks again."""
        self.check()
        self.assertEqual(reuse.clear(), 1)
        self.check()
        self.assertEqual(self.runs(), 2)

    def test_records_live_in_the_git_common_folder(self):
        """Every worktree of a clone shares one record folder."""
        expected = os.path.realpath(os.path.join(self.repo, ".git", reuse.FOLDER))
        self.assertEqual(os.path.realpath(reuse.records_dir()), expected)

    def test_skip_line_names_the_age_and_the_switch(self):
        """The line gives the age of the pass, how long it took, and how to force a run."""
        line = reuse.skip_line({"passed_at": 1000.0, "duration_ms": 31200}, now=1240.0)
        self.assertEqual(line, "reused: passed on this tree 4 min ago (31.2 s); LOOM_CHECK_REUSE=0 runs it")
        self.assertEqual([reuse.ago(s) for s in (5, 3600, 200000)], ["5 s", "1 h", "2 days"])


class RunShTests(ReuseTestCase):
    """run.sh, as prek calls it."""

    def run_sh(self, **env):
        """Run the counting check through run.sh with extra environment."""
        command = ["sh", RUN_SH, "demo", "sh", "-c", 'echo run >> "%s"' % self.counter]
        return subprocess.run(command, env=dict(os.environ, **env), capture_output=True, text=True, cwd=self.repo)

    def test_ci_runs_directly(self):
        """In CI, run.sh never reuses, even when telemetry is asked for."""
        self.run_sh(CI="true")
        self.run_sh(CI="true", LOOM_DEVTEL="1")
        self.assertEqual((self.runs(), self.records()), (2, []))

    @unittest.skipUnless(shutil.which("mise"), "needs mise for the tool versions")
    def test_reuses_with_telemetry_off(self):
        """With only LOOM_DEVTEL=0, run.sh still goes through the reuse."""
        self.run_sh(LOOM_DEVTEL="0")
        second = self.run_sh(LOOM_DEVTEL="0")
        self.assertEqual(self.runs(), 1)
        self.assertIn("reused:", second.stdout)


if __name__ == "__main__":
    unittest.main()

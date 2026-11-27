"""Tests for scripts/test_run.py (mise run test-run-test, docs/dev/test-runs.md).

They need only Python and git. The queue tests start real processes that
hold and wait for slots in a temporary folder.
"""

import errno
import glob
import io
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "..", "test_run.py")
sys.path.insert(0, os.path.join(HERE, ".."))
import test_run  # noqa: E402

MEMBERS = {"dsp": "loom-dsp", "core": "loom-core", "app": "loom-app", "test": "loom-test", "mods": "loom-modules"}


def fake_metadata():
    """A small workspace: app -> mods -> core -> dsp, and mods dev-depends on test."""

    def package(name, folder):
        return {"id": name + " 0.1", "name": name, "manifest_path": "/w/crates/%s/Cargo.toml" % folder}

    def node(name, deps):
        return {"id": name + " 0.1", "deps": [{"pkg": d + " 0.1"} for d in deps]}

    packages = [package(name, folder) for folder, name in MEMBERS.items()]
    packages.append({"id": "serde 1.0", "name": "serde", "manifest_path": "/reg/serde/Cargo.toml"})
    return {
        "workspace_root": "/w",
        "workspace_members": [p["id"] for p in packages if p["name"] != "serde"],
        "packages": packages,
        "resolve": {
            "nodes": [
                node("loom-dsp", ["serde"]),
                node("loom-core", ["loom-dsp"]),
                node("loom-test", ["loom-core"]),
                node("loom-modules", ["loom-core", "loom-test"]),
                node("loom-app", ["loom-modules"]),
            ]
        },
    }


def read_text(path):
    """A file's text, or "" when it does not exist yet."""
    try:
        with open(path, encoding="utf-8") as file:
            return file.read()
    except OSError:
        return ""


class CleanEnv(unittest.TestCase):
    """Removes the settings that change the script's behaviour, for each test."""

    def setUp(self):
        """Clear the queue, CI, and devtel variables, and give the test a folder."""
        self.dir = tempfile.mkdtemp(prefix="test-run-test-")
        self.addCleanup(shutil.rmtree, self.dir, True)
        names = [n for n in os.environ if n.startswith(("LOOM_TEST", "LOOM_DEVTEL")) or n in ("CI", "TRACEPARENT")]
        patcher = mock.patch.dict(os.environ, {}, clear=False)
        patcher.start()
        self.addCleanup(patcher.stop)
        for name in names:
            del os.environ[name]
        os.environ["LOOM_DEVTEL"] = "0"
        os.environ["LOOM_TEST_QUEUE_DIR"] = os.path.join(self.dir, "queue")


class ClassifyTests(unittest.TestCase):
    """Each rule of docs/dev/test-runs.md, "What a file reaches"."""

    def test_files_read_across_crates_run_everything(self):
        """Lock file, root manifest, tool config, examples, and module help pages run every test."""
        for path in [
            "Cargo.lock",
            "Cargo.toml",
            "mise.toml",
            "rust-toolchain.toml",
            ".config/nextest.toml",
            "examples/patches/sine.ron",
            "docs/modules/loom.osc.sine.md",
        ]:
            self.assertEqual(test_run.classify(path, MEMBERS), ("all", None), path)

    def test_a_crate_file_selects_its_crate(self):
        """Any file under crates/NAME/ belongs to that member."""
        self.assertEqual(test_run.classify("crates/dsp/src/lib.rs", MEMBERS), ("crate", "loom-dsp"))
        self.assertEqual(test_run.classify("crates/app/tests/data/x.ron", MEMBERS), ("crate", "loom-app"))

    def test_a_crate_manifest_runs_everything(self):
        """A crate's Cargo.toml can change shared features, so it runs every test."""
        self.assertEqual(test_run.classify("crates/app/Cargo.toml", MEMBERS), ("all", None))

    def test_an_unknown_crate_folder_runs_everything(self):
        """A folder under crates/ that is no member is not trusted."""
        self.assertEqual(test_run.classify("crates/new-thing/src/lib.rs", MEMBERS), ("all", None))
        self.assertEqual(test_run.classify("crates/README.md", MEMBERS), ("all", None))

    def test_files_no_test_reads_select_nothing(self):
        """Docs (outside docs/modules), scripts, CI, and root Markdown select nothing."""
        for path in ["docs/dev/devtel.md", "scripts/test_run.py", ".github/workflows/ci.yml", "AGENTS.md", "prek.toml"]:
            self.assertEqual(test_run.classify(path, MEMBERS), ("none", None), path)

    def test_an_unlisted_file_runs_everything(self):
        """The allow list is fail-safe: anything else is tested in full."""
        for path in ["new-folder/data.bin", "build.rs", "notes.txt"]:
            self.assertEqual(test_run.classify(path, MEMBERS), ("all", None), path)


class SelectTests(unittest.TestCase):
    """The reverse closure and the selection built on it."""

    def setUp(self):
        """Read the fake workspace."""
        self.members, self.deps, self.names = test_run.workspace(fake_metadata())

    def test_workspace_keeps_only_members(self):
        """Folders map to names, and registry crates are left out of the graph."""
        self.assertEqual(self.members, MEMBERS)
        self.assertEqual(self.deps["loom-dsp"], set())
        self.assertNotIn("serde", self.names)

    def test_closure_follows_dev_dependencies(self):
        """A change to loom-test reaches loom-modules through a dev dependency, and loom-app beyond it."""
        self.assertEqual(test_run.reverse_closure({"loom-test"}, self.deps), {"loom-test", "loom-modules", "loom-app"})

    def test_a_leaf_change_runs_only_itself(self):
        """A crate nothing depends on selects only itself."""
        sel = test_run.select(["crates/app/src/main.rs"], self.members, self.deps, self.names, "origin/main")
        self.assertEqual((sel.kind, sel.run), ("some", ["loom-app"]))
        self.assertEqual(sel.filterset(), "package(=loom-app)")
        self.assertIn("loom-dsp", sel.skip)

    def test_the_output_names_what_runs_and_what_is_skipped(self):
        """The changed crate is marked, the skipped ones listed, and the way to run all given."""
        sel = test_run.select(["crates/core/src/lib.rs", "docs/x.md"], self.members, self.deps, self.names, "origin/main")
        text = "\n".join(sel.describe())
        self.assertIn("4 of 5 crates", text)
        self.assertIn("loom-core (changed)", text)
        self.assertIn("skip: loom-dsp", text)
        self.assertIn("LOOM_TEST_ALL=1", text)

    def test_one_run_everything_file_wins(self):
        """The first run-everything file is named as the reason."""
        sel = test_run.select(["crates/app/src/main.rs", "examples/patches/sine.ron"], self.members, self.deps, self.names, "b")
        self.assertEqual(sel.kind, "all")
        self.assertIn("examples/patches/sine.ron changed", sel.describe()[0])

    def test_docs_only_runs_no_tests(self):
        """Only files no test reads: no Rust tests, and the output says so."""
        sel = test_run.select(["docs/dev/x.md"], self.members, self.deps, self.names, "b")
        self.assertEqual(sel.kind, "none")
        self.assertIn("only files no Rust test reads", sel.describe()[0])

    def test_no_merge_base_runs_everything(self):
        """Without a merge base nothing can be left out."""
        sel = test_run.select(None, self.members, self.deps, self.names, "origin/main")
        self.assertEqual(sel.kind, "all")
        self.assertIn("no merge base with origin/main", sel.describe()[0])


class ChangedFilesTests(unittest.TestCase):
    """The changed set, from a real temporary repository."""

    def setUp(self):
        """A repository with a base branch and a feature branch on top of it."""
        self.repo = tempfile.mkdtemp(prefix="test-run-git-")
        self.addCleanup(shutil.rmtree, self.repo, True)
        cwd = os.getcwd()
        self.addCleanup(os.chdir, cwd)
        os.chdir(self.repo)
        env = {"GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
        patcher = mock.patch.dict(os.environ, env)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.git("init", "-q", "-b", "main")
        for name in ["old.txt", "kept.txt", "staged.txt"]:
            self.write(name, name)
        self.git("add", ".")
        self.commit("base")
        self.git("branch", "base")

    def git(self, *args):
        """Run git in the repository."""
        subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", *args], check=True, capture_output=True)

    def commit(self, message):
        """Commit what is staged."""
        self.git("commit", "-q", "--no-verify", "-m", message)

    def write(self, name, text):
        """Write a file in the repository."""
        os.makedirs(os.path.dirname(name) or ".", exist_ok=True)
        with open(name, "w", encoding="utf-8") as file:
            file.write(text)

    def test_committed_staged_unstaged_untracked_and_renamed(self):
        """Every kind of change counts, with both paths of a rename."""
        self.write("committed.txt", "c")
        self.git("add", "committed.txt")
        self.git("mv", "old.txt", "new.txt")
        self.commit("work")
        self.write("staged.txt", "changed")
        self.git("add", "staged.txt")
        self.write("kept.txt", "unstaged")
        self.write("dir/untracked.txt", "u")
        self.assertEqual(
            test_run.changed_files("base"),
            ["committed.txt", "dir/untracked.txt", "kept.txt", "new.txt", "old.txt", "staged.txt"],
        )

    def test_nothing_changed(self):
        """A clean branch at the base has no changed files."""
        self.assertEqual(test_run.changed_files("base"), [])

    def test_a_missing_base(self):
        """A base that does not exist gives None, so every test runs."""
        self.assertIsNone(test_run.changed_files("origin/nowhere"))


class QueueSettingsTests(CleanEnv):
    """When the queue is on, and the thread count."""

    def setUp(self):
        """Pretend flock exists, so these tests mean the same on Windows."""
        super().setUp()
        if test_run.fcntl is None:
            patcher = mock.patch.object(test_run, "fcntl", object())
            patcher.start()
            self.addCleanup(patcher.stop)

    def test_on_by_default(self):
        """No settings: the queue is on."""
        self.assertIsNone(test_run.queue_off_reason())

    def test_no_flock(self):
        """Without flock the queue is off, and says why."""
        with mock.patch.object(test_run, "fcntl", None):
            self.assertEqual(test_run.queue_off_reason(), "no flock on this system")

    def test_switches(self):
        """LOOM_TEST_QUEUE=0, CI, and a held slot turn it off; LOOM_TEST_QUEUE=1 turns it on in CI."""
        with mock.patch.dict(os.environ, {"LOOM_TEST_QUEUE": "0"}):
            self.assertEqual(test_run.queue_off_reason(), "LOOM_TEST_QUEUE=0")
        with mock.patch.dict(os.environ, {"CI": "true"}):
            self.assertEqual(test_run.queue_off_reason(), "CI")
        with mock.patch.dict(os.environ, {"CI": "true", "LOOM_TEST_QUEUE": "1"}):
            self.assertIsNone(test_run.queue_off_reason())
        with mock.patch.dict(os.environ, {"LOOM_TEST_SLOT_HELD": "1"}):
            self.assertEqual(test_run.queue_off_reason(), "held")

    def test_folder_follows_xdg(self):
        """Without LOOM_TEST_QUEUE_DIR, the folder sits next to devtel's store."""
        del os.environ["LOOM_TEST_QUEUE_DIR"]
        with mock.patch.dict(os.environ, {"XDG_DATA_HOME": "/data"}):
            self.assertEqual(test_run.queue_dir(), os.path.join("/data", "loomdsp", "test-queue"))

    def test_threads(self):
        """Every core with a slot, half without, none passed in CI, LOOM_TEST_THREADS above all."""
        with mock.patch("os.cpu_count", return_value=10):
            self.assertEqual(test_run.nextest_threads(True, False), ["--test-threads", "10"])
            self.assertEqual(test_run.nextest_threads(False, False), ["--test-threads", "5"])
            self.assertEqual(test_run.nextest_threads(False, True), [])
            with mock.patch.dict(os.environ, {"LOOM_TEST_THREADS": "3"}):
                self.assertEqual(test_run.nextest_threads(True, True), ["--test-threads", "3"])

    def test_slot_count(self):
        """LOOM_TEST_SLOTS, at least 1, and 1 when it is not a number."""
        for value, expected in [("2", 2), ("0", 1), ("x", 1)]:
            with mock.patch.dict(os.environ, {"LOOM_TEST_SLOTS": value}):
                self.assertEqual(test_run.slot_count(), expected)


@unittest.skipIf(test_run.fcntl is None, "the queue needs flock")
class BuildArgumentTests(unittest.TestCase):
    """Which extra arguments the build before the slot takes."""

    def test_build_arguments_are_kept(self):
        """Release, features, and the cargo profile change the build, in either form."""
        arguments = ["--release", "--features", "a,b", "--cargo-profile=ci", "-F", "c"]
        self.assertEqual(test_run.build_arguments(arguments), arguments)

    def test_run_arguments_are_left_to_the_run(self):
        """--no-fail-fast, a thread count, a nextest profile, and test arguments are for the run."""
        arguments = ["--no-fail-fast", "--test-threads", "2", "--profile", "ci", "--release", "--", "--features"]
        self.assertEqual(test_run.build_arguments(arguments), ["--release"])


class QueueTests(CleanEnv):
    """Real processes holding and waiting for a slot."""

    def start_holder(self):
        """Start a queued command that holds the slot until killed; return (wrapper, child PID)."""
        pid_file = os.path.join(self.dir, "child.pid")
        child = "import os, time; open(%r, 'w').write(str(os.getpid())); time.sleep(60)" % pid_file
        wrapper = subprocess.Popen(
            [sys.executable, SCRIPT, "queue", "--", sys.executable, "-c", child],
            stdout=subprocess.DEVNULL,
            env=dict(os.environ),
        )
        self.addCleanup(wrapper.wait)
        self.addCleanup(self.kill, wrapper.pid)
        deadline = time.time() + 20
        while not read_text(pid_file):
            self.assertLess(time.time(), deadline, "the holder never started")
            time.sleep(0.05)
        child_pid = int(read_text(pid_file))
        self.addCleanup(self.kill, child_pid)
        return wrapper, child_pid

    def kill(self, pid):
        """Kill a process if it still runs."""
        try:
            os.kill(pid, signal.SIGKILL)
        except OSError:
            pass

    def start_waiter(self):
        """Start a queued `true` that must wait for the slot."""
        return subprocess.Popen(
            [sys.executable, SCRIPT, "queue", "--", sys.executable, "-c", "pass"],
            stdout=subprocess.PIPE,
            text=True,
            env=dict(os.environ),
        )

    def wait_for_line(self, process, text):
        """Read lines until one contains the text."""
        deadline = time.time() + 20
        lines = []
        while time.time() < deadline:
            line = process.stdout.readline()
            lines.append(line)
            if text in line:
                return lines
        self.fail("never printed %r: %r" % (text, lines))

    def test_a_second_run_waits_and_names_the_holder(self):
        """The waiter says who holds the slot, and runs when the holder ends."""
        wrapper, child_pid = self.start_holder()
        waiter = self.start_waiter()
        lines = self.wait_for_line(waiter, "slot 0:")
        self.assertIn("waiting for a test slot (1 of 1 in use)", "".join(lines))
        self.assertIn(os.path.basename(os.getcwd()), lines[-1])
        self.assertIsNone(waiter.poll())
        os.kill(child_pid, signal.SIGTERM)
        self.assertEqual(waiter.wait(timeout=20), 0)
        waiter.stdout.close()
        wrapper.wait(timeout=20)

    def test_a_killed_wrapper_frees_the_slot(self):
        """kill -9 of the wrapper frees the slot at once; its child does not keep it."""
        wrapper, _child_pid = self.start_holder()
        os.kill(wrapper.pid, signal.SIGKILL)
        wrapper.wait(timeout=20)
        waiter = self.start_waiter()
        self.assertEqual(waiter.wait(timeout=20), 0)
        waiter.stdout.close()

    def test_two_slots_let_two_runs_go(self):
        """With LOOM_TEST_SLOTS=2 a second run does not wait."""
        os.environ["LOOM_TEST_SLOTS"] = "2"
        self.start_holder()
        waiter = self.start_waiter()
        self.assertEqual(waiter.wait(timeout=20), 0)
        self.assertNotIn("waiting", waiter.stdout.read())
        waiter.stdout.close()

    def test_a_held_slot_is_not_taken_twice(self):
        """A run inside a run that holds the slot does not queue for it."""
        self.start_holder()
        os.environ["LOOM_TEST_SLOT_HELD"] = "1"
        waiter = self.start_waiter()
        self.assertEqual(waiter.wait(timeout=20), 0)
        waiter.stdout.close()

    def test_the_exit_code_passes_through(self):
        """The command's exit code is the script's."""
        code = subprocess.run([sys.executable, SCRIPT, "queue", "--", sys.executable, "-c", "raise SystemExit(3)"]).returncode
        self.assertEqual(code, 3)

    def test_an_unwritable_folder_runs_without_the_queue(self):
        """The queue failing never blocks a run."""
        blocker = os.path.join(self.dir, "file")
        open(blocker, "w").close()
        os.environ["LOOM_TEST_QUEUE_DIR"] = os.path.join(blocker, "queue")
        out = io.StringIO()
        with mock.patch("sys.stdout", out):
            code = test_run.queued([sys.executable, "-c", "pass"], "t", test_run.Spans())
        self.assertEqual(code, 0)
        self.assertIn("test queue: off", out.getvalue())

    def test_a_lock_failure_that_is_not_contention_runs_without_the_queue(self):
        """ENOLCK is no busy slot: the run goes on without the queue, not waiting for good."""
        os.environ["LOOM_TEST_QUEUE_DIR"] = self.dir
        failure = OSError(errno.ENOLCK, "No locks available")
        out = io.StringIO()
        with mock.patch.object(test_run.fcntl, "flock", side_effect=failure), mock.patch("sys.stdout", out):
            code = test_run.queued([sys.executable, "-c", "pass"], "t", test_run.Spans())
        self.assertEqual(code, 0)
        self.assertIn("test queue: off", out.getvalue())

    def test_a_word_for_the_thread_count_is_kept(self):
        """LOOM_TEST_THREADS=num-cpus, which nextest takes, does not fail the run."""
        os.environ["LOOM_TEST_QUEUE"] = "0"
        os.environ["LOOM_TEST_THREADS"] = "num-cpus"
        code = test_run.queued([sys.executable, "-c", "pass"], "t", test_run.Spans(), test_run.nextest_threads)
        self.assertEqual(code, 0)


class SpanTests(CleanEnv):
    """The devtel spans (docs/dev/test-runs.md, "Devtel")."""

    def test_wait_and_run_are_recorded(self):
        """A queued run writes a wait span and a run span, with service `tests`."""
        os.environ["LOOM_DEVTEL"] = "1"
        os.environ["LOOM_DEVTEL_DIR"] = os.path.join(self.dir, "devtel")
        if test_run.fcntl is None:
            os.environ["LOOM_TEST_QUEUE"] = "0"
        code = test_run.queued([sys.executable, "-c", "pass"], "t", test_run.Spans(), None, {"loom.test.selection": "all"})
        self.assertEqual(code, 0)
        lines = []
        for path in glob.glob(os.path.join(self.dir, "devtel", "spool", "*", "tests.jsonl")):
            with open(path, encoding="utf-8") as file:
                lines += [json.loads(line) for line in file]
        spans = [s for line in lines for r in line["resourceSpans"] for ss in r["scopeSpans"] for s in ss["spans"]]
        names = [s["name"] for s in spans]
        self.assertIn("test run", names)
        if test_run.fcntl is not None:
            self.assertIn("test queue wait", names)
        services = {
            a["value"]["stringValue"]
            for line in lines
            for r in line["resourceSpans"]
            for a in r["resource"]["attributes"]
            if a["key"] == "service.name"
        }
        self.assertEqual(services, {"tests"})


if __name__ == "__main__":
    unittest.main()

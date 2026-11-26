#!/usr/bin/env python3
"""Run the workspace tests through a machine-wide queue, and select what to run.

See docs/dev/test-runs.md (ADR-0050, ADR-0051).

  test_run.py              the test step of `mise run check`: select, build, queue, run
  test_run.py --all        the same with every test (`mise run test`, or LOOM_TEST_ALL=1)
  test_run.py --explain    print what the test step would run and why
  test_run.py queue -- CMD take a slot, run CMD with it held, free it (`mise run coverage`)

Arguments after `--` in the first two forms go to `cargo nextest run`.
"""

import argparse
import datetime
import json
import os
import signal
import subprocess
import sys
import time

try:
    import fcntl
except ImportError:  # Windows: the queue turns itself off.
    fcntl = None

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "devtel"))
try:
    import devtel
except Exception:  # noqa: BLE001 - telemetry is optional
    devtel = None

# How often a waiting run tries the slots again, and how often it says so.
POLL_SECONDS = 0.2
REPORT_SECONDS = 60.0

# ---------------------------------------------------------------------------
# Selection (docs/dev/test-runs.md, "Selection")
# ---------------------------------------------------------------------------

# Files that any crate's build or tests may read, or that change every build.
# A path matches an entry that ends in "/" by prefix, and any other exactly.
RUN_ALL = (
    "Cargo.lock",
    "Cargo.toml",
    "mise.toml",
    "rust-toolchain",
    "rust-toolchain.toml",
    ".config/",
    ".cargo/",
    "examples/",
    "docs/modules/",
)

# Files no Rust test reads. An allow list: a path in neither list runs
# everything, so a new kind of file is tested in full until it is listed.
RUN_NONE = (
    "docs/",
    "scripts/",
    "site/",
    ".github/",
    ".claude/",
    "fuzz/",
    "prek.toml",
    "_typos.toml",
    "rustfmt.toml",
    "deny.toml",
    "codecov.yml",
    "skills-lock.json",
    ".gitignore",
    "LICENSE",
)


def matches(path, entries):
    """Whether a path matches one of the entries (folder prefixes end in '/')."""
    for entry in entries:
        if entry.endswith("/"):
            if path.startswith(entry):
                return True
        elif path == entry:
            return True
    return False


def classify(path, members):
    """What one changed file reaches: ("all", None), ("crate", name), or ("none", None).

    `members` maps a crate's folder under crates/ to its package name.
    """
    if matches(path, RUN_ALL):
        return ("all", None)
    if path.startswith("crates/"):
        folder = path.split("/")[1] if path.count("/") >= 2 else ""
        if folder in members:
            # A crate's manifest can change the features of shared
            # dependencies, and so the build of every crate.
            if path == "crates/%s/Cargo.toml" % folder:
                return ("all", None)
            return ("crate", members[folder])
        return ("all", None)
    if matches(path, RUN_NONE):
        return ("none", None)
    if "/" not in path and path.endswith(".md"):
        return ("none", None)
    return ("all", None)


def reverse_closure(changed, depends_on):
    """The changed crates and every crate that depends on one, directly or not.

    `depends_on` maps a crate to the workspace crates it depends on through
    normal, dev, or build dependencies.
    """
    dependents = {}
    for crate, deps in depends_on.items():
        for dep in deps:
            dependents.setdefault(dep, set()).add(crate)
    seen = set()
    stack = list(changed)
    while stack:
        crate = stack.pop()
        if crate in seen:
            continue
        seen.add(crate)
        stack.extend(dependents.get(crate, ()))
    return seen


def workspace(metadata):
    """From `cargo metadata` output: (folder -> name, name -> workspace deps, names in order)."""
    root = metadata["workspace_root"]
    by_id = {}
    members = {}
    for package in metadata["packages"]:
        if package["id"] not in metadata["workspace_members"]:
            continue
        by_id[package["id"]] = package["name"]
        folder = os.path.relpath(os.path.dirname(package["manifest_path"]), root)
        parts = folder.split(os.sep)
        if len(parts) == 2 and parts[0] == "crates":
            members[parts[1]] = package["name"]
    depends_on = {name: set() for name in by_id.values()}
    for node in (metadata.get("resolve") or {}).get("nodes", []):
        if node["id"] not in by_id:
            continue
        for dep in node.get("deps", []):
            if dep["pkg"] in by_id:
                depends_on[by_id[node["id"]]].add(by_id[dep["pkg"]])
    return members, depends_on, sorted(by_id.values())


def git(*args):
    """Run git and return its output, or None when it fails."""
    try:
        result = subprocess.run(["git", *args], capture_output=True, check=True)
    except (OSError, subprocess.CalledProcessError):
        return None
    return result.stdout.decode("utf-8", "replace")


def changed_files(base):
    """Every path that differs from the merge base with `base`, or None without one.

    Committed, staged, unstaged, and untracked files, with both paths of a rename.
    """
    merge_base = git("merge-base", "HEAD", base)
    if merge_base is None or not merge_base.strip():
        return None
    diff = git("diff", "--name-status", "-z", "--find-renames", merge_base.strip())
    untracked = git("ls-files", "--others", "--exclude-standard", "-z")
    if diff is None or untracked is None:
        return None
    paths = set()
    fields = diff.split("\0")
    i = 0
    while i < len(fields) and fields[i]:
        status = fields[i]
        count = 2 if status[0] in "RC" else 1
        paths.update(fields[i + 1 : i + 1 + count])
        i += 1 + count
    paths.update(p for p in untracked.split("\0") if p)
    return sorted(paths)


class Selection:
    """What to run: `kind` is all, some, or none, with the crates and the reason."""

    def __init__(self, kind, reason, run=(), skip=(), changed=()):
        self.kind = kind
        self.reason = reason
        self.run = sorted(run)
        self.skip = sorted(skip)
        self.changed = sorted(changed)

    def filterset(self):
        """The nextest filter for a partial run."""
        return " | ".join("package(=%s)" % name for name in self.run)

    def describe(self):
        """The lines printed before the build."""
        hint = "  LOOM_TEST_ALL=1 or `mise run check -- --all` runs every test."
        if self.kind == "all":
            return ["test selection: every test (%s)" % self.reason]
        if self.kind == "none":
            return ["test selection: no Rust tests (%s)" % self.reason, hint]
        changed = set(self.changed)
        lines = [
            "test selection: %d of %d crates (%s)" % (len(self.run), len(self.run) + len(self.skip), self.reason),
            "  run:  %s" % ", ".join(name + (" (changed)" if name in changed else "") for name in self.run),
        ]
        if self.skip:
            lines.append("  skip: %s (no changed crate reaches them)" % ", ".join(self.skip))
        lines.append(hint)
        return lines


def select(paths, members, depends_on, names, base):
    """Decide what to run for a list of changed paths (None: no merge base)."""
    if paths is None:
        return Selection("all", "no merge base with %s" % base)
    changed = set()
    for path in paths:
        kind, crate = classify(path, members)
        if kind == "all":
            return Selection("all", "%s changed" % path)
        if kind == "crate":
            changed.add(crate)
    if not changed:
        if paths:
            return Selection("none", "only files no Rust test reads changed since the merge base with %s" % base)
        return Selection("none", "nothing changed since the merge base with %s" % base)
    run = reverse_closure(changed, depends_on)
    return Selection(
        "some",
        "changed since the merge base with %s" % base,
        run=run,
        skip=set(names) - run,
        changed=changed,
    )


def selection_from_repository(base):
    """Run git and `cargo metadata` and decide what to run."""
    paths = changed_files(base)
    if paths is None:
        return select(None, {}, {}, [], base)
    try:
        output = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked", "--offline"],
            capture_output=True,
            check=True,
        ).stdout
        members, depends_on, names = workspace(json.loads(output))
    except (OSError, subprocess.CalledProcessError, ValueError, KeyError) as error:
        return Selection("all", "cargo metadata failed: %s" % error)
    return select(paths, members, depends_on, names, base)


# ---------------------------------------------------------------------------
# The queue (docs/dev/test-runs.md, "The queue")
# ---------------------------------------------------------------------------


def queue_dir():
    """The shared folder for the lock files, next to devtel's store."""
    if os.environ.get("LOOM_TEST_QUEUE_DIR"):
        return os.path.expanduser(os.environ["LOOM_TEST_QUEUE_DIR"])
    base = os.environ.get("XDG_DATA_HOME") or os.path.join(os.path.expanduser("~"), ".local", "share")
    return os.path.join(base, "loomdsp", "test-queue")


def queue_off_reason():
    """Why the queue is off, or None when it is on."""
    flag = os.environ.get("LOOM_TEST_QUEUE", "")
    if flag == "0":
        return "LOOM_TEST_QUEUE=0"
    if os.environ.get("CI") and flag != "1":
        return "CI"
    if os.environ.get("LOOM_TEST_SLOT_HELD") == "1":
        return "held"
    if fcntl is None:
        return "no flock on this system"
    return None


def slot_count():
    """How many runs may run at once (LOOM_TEST_SLOTS, at least 1)."""
    try:
        return max(1, int(os.environ.get("LOOM_TEST_SLOTS", "1")))
    except ValueError:
        return 1


def read_holder(folder, index):
    """The informational holder record of one slot, or None."""
    try:
        with open(os.path.join(folder, "slot-%d.json" % index), encoding="utf-8") as file:
            return json.load(file)
    except (OSError, ValueError):
        return None


def describe_holder(holder, now):
    """One line about who holds a slot."""
    if not holder:
        return "unknown holder"
    since = holder.get("since", 0)
    waited = int(max(0, now - since))
    clock = datetime.datetime.fromtimestamp(since).strftime("%H:%M:%S")
    what = holder.get("what", "tests")
    return "%s on %s, %s, since %s (%dm %02ds)" % (
        os.path.basename(holder.get("worktree", "?")),
        holder.get("branch", "?"),
        what,
        clock,
        waited // 60,
        waited % 60,
    )


class Slot:
    """A held slot: the open, locked file, and its index."""

    def __init__(self, fd, index, folder):
        self.fd = fd
        self.index = index
        self.folder = folder

    def release(self):
        """Free the slot."""
        try:
            os.remove(os.path.join(self.folder, "slot-%d.json" % self.index))
        except OSError:
            pass
        os.close(self.fd)


def try_slots(folder, slots):
    """Take the first free slot without waiting, or return None."""
    for index in range(slots):
        fd = os.open(os.path.join(folder, "slot-%d.lock" % index), os.O_RDWR | os.O_CREAT, 0o644)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            os.close(fd)
            continue
        return Slot(fd, index, folder)
    return None


def acquire(folder, slots, what, out=sys.stdout, clock=time.time):
    """Wait for a slot, saying who holds them, and write this run's holder record.

    Returns the slot and the holder record of slot 0 when the wait began
    (None when there was no wait). The lock file is not inherited by the
    command: a long-lived process it starts, such as the sccache server,
    would otherwise keep the slot after the run.
    """
    os.makedirs(folder, exist_ok=True)
    slot = try_slots(folder, slots)
    next_report = 0.0
    first_holder = None
    while slot is None:
        now = clock()
        if first_holder is None:
            first_holder = read_holder(folder, 0) or {}
        if now >= next_report:
            print("test queue: waiting for a test slot (%d of %d in use)" % (slots, slots), file=out)
            for index in range(slots):
                print("  slot %d: %s" % (index, describe_holder(read_holder(folder, index), now)), file=out)
            print("  LOOM_TEST_QUEUE=0 skips the queue.", file=out, flush=True)
            next_report = now + REPORT_SECONDS
        time.sleep(POLL_SECONDS)
        slot = try_slots(folder, slots)
    holder = {
        "worktree": os.getcwd(),
        "branch": (git("rev-parse", "--abbrev-ref", "HEAD") or "?").strip(),
        "pid": os.getpid(),
        "since": clock(),
        "what": what,
    }
    try:
        path = os.path.join(folder, "slot-%d.json" % slot.index)
        with open(path + ".tmp", "w", encoding="utf-8") as file:
            json.dump(holder, file)
        os.replace(path + ".tmp", path)
    except OSError:
        pass
    return slot, first_holder


# ---------------------------------------------------------------------------
# Running
# ---------------------------------------------------------------------------


def run_child(command, env):
    """Run a command, pass on the signals that stop it, and return its exit code."""
    try:
        child = subprocess.Popen(command, env=env)
    except OSError as error:
        print("test_run: cannot run %s: %s" % (command[0], error), file=sys.stderr)
        return 127
    previous = {}

    def forward(number, _frame):
        try:
            child.send_signal(number)
        except OSError:
            pass

    for number in (signal.SIGTERM, signal.SIGINT) + ((signal.SIGHUP,) if hasattr(signal, "SIGHUP") else ()):
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


class Spans:
    """Records the wait and the run as devtel spans (docs/dev/test-runs.md, "Devtel")."""

    def __init__(self):
        self.on = devtel is not None and devtel.enabled()
        if self.on:
            self.trace_id, self.parent_id = devtel.parent_context()

    def record(self, name, start_ns, end_ns, attributes, error=False):
        """Write one span; never raises."""
        if not self.on:
            return
        try:
            attributes = dict(attributes)
            attributes.update(devtel.vcs_attributes())
            attributes.update(devtel.agent_attributes())
            span = devtel.span(self.trace_id, devtel.new_span_id(), self.parent_id, name, start_ns, end_ns, attributes, error)
            devtel.emit("tests", "tests", [span])
        except Exception:  # noqa: BLE001 - telemetry must not break a run
            pass


def queued(command, what, spans, threads_flag=None, extra_attributes=None):
    """Take a slot (unless the queue is off), run the command, and record the spans.

    `threads_flag` is a function from "held" (bool) and "ci" (bool) to the
    extra arguments that set the thread count.
    """
    off = queue_off_reason()
    env = dict(os.environ)
    slot = None
    wait_start = time.time_ns()
    if off is None:
        try:
            slot, holder = acquire(queue_dir(), slot_count(), what)
        except KeyboardInterrupt:
            spans.record("test queue wait", wait_start, time.time_ns(), {"loom.test.cancelled": True})
            return 130
        except OSError as error:
            print("test queue: off (%s: %s)" % (queue_dir(), error.strerror or error))
            off = "folder"
        else:
            waited = time.time_ns() - wait_start
            if waited > 2e9:
                print("test queue: got slot %d after %ds" % (slot.index, waited // 1_000_000_000), flush=True)
            spans.record(
                "test queue wait",
                wait_start,
                time.time_ns(),
                {
                    "loom.test.wait_ms": waited // 1_000_000,
                    "loom.test.slots": slot_count(),
                    "loom.test.holder": os.path.basename((holder or {}).get("worktree", "")),
                },
            )
            env["LOOM_TEST_SLOT_HELD"] = "1"
    if off == "no flock on this system":
        print("test queue: off (no flock on this system)", flush=True)
    held = slot is not None or off == "held"
    threads = threads_flag(held, off == "CI") if threads_flag else []
    full = list(command) + threads
    start = time.time_ns()
    try:
        code = run_child(full, env)
    finally:
        if slot:
            slot.release()
    attributes = {"process.exit.code": code, "process.command_line": " ".join(full)[:2000]}
    if threads:
        attributes["loom.test.threads"] = int(threads[1])
    attributes.update(extra_attributes or {})
    spans.record("test run", start, time.time_ns(), attributes, code != 0)
    return code


def nextest_threads(held, ci):
    """--test-threads: every core with a slot, half without, nextest's own in CI."""
    if os.environ.get("LOOM_TEST_THREADS"):
        return ["--test-threads", os.environ["LOOM_TEST_THREADS"]]
    if ci:
        return []
    cores = os.cpu_count() or 2
    return ["--test-threads", str(cores if held else (cores + 1) // 2)]


def run_tests(run_all, nextest_args):
    """The test step: select, build outside the queue, then run in it."""
    spans = Spans()
    base = os.environ.get("LOOM_TEST_BASE", "origin/main")
    if run_all or os.environ.get("LOOM_TEST_ALL") == "1":
        selection = Selection("all", "asked for every test")
    else:
        selection = selection_from_repository(base)
    for line in selection.describe():
        print(line, flush=True)
    attributes = {
        "loom.test.selection": selection.kind,
        "loom.test.crates": ",".join(selection.run),
        "loom.test.reason": selection.reason,
    }
    if selection.kind == "none":
        now = time.time_ns()
        spans.record("test run", now, now, attributes)
        return 0
    command = ["cargo", "nextest", "run", "--workspace"]
    if selection.kind == "some":
        # A selected crate may have no tests of its own; that is no failure.
        command += ["-E", selection.filterset(), "--no-tests=warn"]
    command += list(nextest_args)
    if queue_off_reason() is None:
        # Build before taking a slot, so only the run waits.
        code = run_child(["cargo", "nextest", "run", "--workspace", "--no-run"], dict(os.environ))
        if code != 0:
            return code
    what = "every test" if selection.kind == "all" else "the tests of %d crates" % len(selection.run)
    return queued(command, what, spans, nextest_threads, attributes)


def main(argv=None):
    """Parse the command line and run."""
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv[:1] == ["queue"]:
        rest = argv[1:]
        if rest[:1] == ["--"]:
            rest = rest[1:]
        if not rest:
            print("usage: test_run.py queue -- COMMAND", file=sys.stderr)
            return 2
        return queued(rest, "`%s`" % " ".join(rest)[:60], Spans())
    extra = []
    if "--" in argv:
        extra = argv[argv.index("--") + 1 :]
        argv = argv[: argv.index("--")]
    parser = argparse.ArgumentParser(description="Run the workspace tests (docs/dev/test-runs.md).")
    parser.add_argument("--all", action="store_true", help="run every test, with no selection")
    parser.add_argument("--explain", action="store_true", help="print what would run and why, and exit")
    args = parser.parse_args(argv)
    if args.explain:
        for line in selection_from_repository(os.environ.get("LOOM_TEST_BASE", "origin/main")).describe():
            print(line)
        return 0
    return run_tests(args.all, extra)


if __name__ == "__main__":
    sys.exit(main())

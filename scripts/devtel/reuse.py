"""Reuse a check that already passed on the same tree (docs/dev/check-reuse.md).

devtel.py's runner calls this around each check that prek.toml wraps. When
a pass record with the same key exists, the check is not run again. The key
covers the check's command line, the working tree, the tool versions mise
resolves, and the settings outside the tree that change what a check
reports. Every error here means the check runs: reuse fails open.

Never on in CI (ADR-0049). LOOM_CHECK_REUSE=0 turns it off.
Standard library only, and Python 3.9 compatible, like devtel.py.
"""

import hashlib
import json
import os
import subprocess
import tempfile
import time

# Bump when the key's contents change, so old records stop matching.
FORMAT = "1"
# A record older than this is neither reused nor kept.
MAX_AGE_SECONDS = 7 * 24 * 3600
FOLDER = "loom-check-passes"
# Environment variables that change what fmt, clippy, or the scripts report.
ENV_KEYS = (
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "RUSTDOCFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
    "CLIPPY_CONF_DIR",
    "RUSTUP_TOOLCHAIN",
)


def enabled():
    """Whether reuse is on: off with LOOM_CHECK_REUSE=0, and always off in CI."""
    if os.environ.get("CI"):
        return False
    return os.environ.get("LOOM_CHECK_REUSE", "") != "0"


def _git(*args, cwd=None):
    """Run git and return its output, or raise when it fails."""
    out = subprocess.run(
        ["git"] + list(args), cwd=cwd, capture_output=True, text=True, check=True, timeout=10
    )
    return out.stdout.strip()


def records_dir(cwd=None):
    """The folder of pass records, in the git common folder every worktree shares."""
    common = _git("rev-parse", "--git-common-dir", cwd=cwd)
    if not os.path.isabs(common):
        common = os.path.join(cwd or os.getcwd(), common)
    return os.path.join(common, FOLDER)


def tool_versions(cwd=None):
    """The tool versions mise resolves here. mise.toml pins some to `latest`."""
    out = subprocess.run(
        ["mise", "ls", "--current", "--json"],
        cwd=cwd,
        capture_output=True,
        text=True,
        check=True,
        timeout=30,
    )
    # Keep only names and versions: paths and flags say nothing about results.
    data = json.loads(out.stdout)
    versions = {name: sorted(str(entry.get("version", "")) for entry in entries) for name, entries in data.items()}
    return json.dumps(versions, sort_keys=True)


def _file_text(path):
    """A hash of a file's contents, or a marker when it does not exist."""
    try:
        with open(path, "rb") as handle:
            return hashlib.sha256(handle.read()).hexdigest()
    except FileNotFoundError:
        return "-"


def outside_files(cwd=None):
    """Hashes of the Cargo configuration files that git does not see."""
    top = _git("rev-parse", "--show-toplevel", cwd=cwd)
    cargo_home = os.environ.get("CARGO_HOME") or os.path.join(os.path.expanduser("~"), ".cargo")
    paths = {
        "worktree.config.toml": os.path.join(top, ".cargo", "config.toml"),
        "worktree.config": os.path.join(top, ".cargo", "config"),
        "cargo_home.config.toml": os.path.join(cargo_home, "config.toml"),
        "cargo_home.config": os.path.join(cargo_home, "config"),
    }
    return ["file.%s=%s" % (label, _file_text(path)) for label, path in paths.items()]


def key(name, command, tree, cwd=None, versions=None):
    """The key of one check on one tree, or None when the tree is unknown."""
    if not tree:
        return None
    lines = [
        "format=" + FORMAT,
        "check=" + name,
        "command=" + json.dumps(list(command)),
        "tree=" + tree,
        "tools=" + (versions if versions is not None else tool_versions(cwd)),
    ]
    lines += ["env.%s=%s" % (k, os.environ.get(k, "")) for k in ENV_KEYS]
    lines += outside_files(cwd)
    return hashlib.sha256("\n".join(lines).encode("utf-8")).hexdigest()


def lookup(record_key, cwd=None, now=None):
    """The pass record for a key, if it exists and is fresh; otherwise None."""
    now = time.time() if now is None else now
    path = os.path.join(records_dir(cwd), record_key)
    try:
        with open(path, encoding="utf-8") as handle:
            record = json.load(handle)
    except (OSError, ValueError):
        return None
    if not isinstance(record, dict) or record.get("key") != record_key:
        return None
    passed_at = record.get("passed_at")
    if not isinstance(passed_at, (int, float)) or not now - MAX_AGE_SECONDS <= passed_at <= now + 60:
        return None
    return record


def store(record_key, record, cwd=None, now=None):
    """Write a pass record atomically, then prune records too old to reuse."""
    now = time.time() if now is None else now
    folder = records_dir(cwd)
    os.makedirs(folder, exist_ok=True)
    data = dict(record, key=record_key, passed_at=now)
    handle, temp = tempfile.mkstemp(prefix=".tmp-", dir=folder)
    try:
        with os.fdopen(handle, "w", encoding="utf-8") as out:
            json.dump(data, out)
        os.replace(temp, os.path.join(folder, record_key))
    except BaseException:
        try:
            os.unlink(temp)
        except OSError:
            pass
        raise
    prune(folder, now)


def prune(folder, now):
    """Delete records, and temporary files left by a crash, older than MAX_AGE_SECONDS."""
    for entry in os.listdir(folder):
        path = os.path.join(folder, entry)
        try:
            if os.path.getmtime(path) < now - MAX_AGE_SECONDS:
                os.unlink(path)
        except OSError:
            pass


def clear(cwd=None):
    """Delete every pass record of this clone, and return how many there were."""
    folder = records_dir(cwd)
    if not os.path.isdir(folder):
        return 0
    count = 0
    for entry in os.listdir(folder):
        try:
            os.unlink(os.path.join(folder, entry))
            count += 1
        except OSError:
            pass
    return count


def ago(seconds):
    """A short, rounded age such as "12 s", "4 min", or "3 h"."""
    seconds = max(0, int(seconds))
    if seconds < 60:
        return "%d s" % seconds
    if seconds < 3600:
        return "%d min" % (seconds // 60)
    if seconds < 172800:
        return "%d h" % (seconds // 3600)
    return "%d days" % (seconds // 86400)


def skip_line(record, now=None):
    """The one line a reused check prints, so nobody takes it for a fresh run."""
    now = time.time() if now is None else now
    took = record.get("duration_ms", 0) / 1000.0
    return "reused: passed on this tree %s ago (%.1f s); LOOM_CHECK_REUSE=0 runs it" % (
        ago(now - record.get("passed_at", now)),
        took,
    )

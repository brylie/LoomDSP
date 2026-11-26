#!/usr/bin/env python3
"""Measure the health of the LoomDSP test suite for a test review.

Prints Markdown (or JSON with --json). The numbers are a map of where to
look, not findings: a slow test that renders a whole patch may be fine.

Run from the repository root:

    python3 .claude/skills/review-tests/scripts/measure_tests.py [options]

Sections, each skipped with a note when its tool is missing:

  machine    cores and load, sampled again beside every timing
  inventory  test counts, ignored tests, sleeps, polls, temp folders,
             machine dependencies, duplicated helpers (no build needed)
  specs      acceptance criteria per spec: ticked, open, manual
  ci         per-test times from the nextest lines in a CI run's logs,
             per OS, from runners with a clean temp folder (needs `gh`)
  coverage   per crate, per file, and public functions never run, from
             the `coverage` artifact of the same CI run (never run here)
  probe      (--probe) the fixed cost of one test process per binary,
             user versus kernel CPU time, run once with the shared temp
             folder and once with an empty one; what is left behind
  local      (--local) one full nextest run here, refused when the
             machine is too loaded for its times to mean anything
  devtel     weekly agent time spent waiting on tests (docs/dev/devtel.md)

Uses only the Python standard library. Load and per-process CPU time need a
Unix system; on Windows the script says so and reports wall time only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from collections import Counter, defaultdict
from pathlib import Path

TOP = 15
# Load per core above which wall-clock times are not compared with anything.
BUSY = 1.0
# Load per core above which the full local run is refused without --force.
TOO_LOADED = 1.5
ANSI = re.compile(r"\x1b\[[0-9;]*m")
# nextest result line: "PASS [   0.024s] (   1/4450) loom-analysis tests::name"
RESULT = re.compile(
    r"^\s*(PASS|FAIL|FLAKY|TRY \d+ \w+|SLOW|TIMEOUT|LEAK|SIGSEGV|SIGABRT|ABORT)\s*"
    r"\[\s*([\d.]+)s\]\s*(?:\(\s*\d+/\d+\))?\s+(\S+)\s+(\S+)"
)
FINISHED = re.compile(r"Finished\s+`test` profile .* in (?:(\d+)m )?([\d.]+)s")
STARTING = re.compile(r"Starting\s+(\d+) tests across (\d+) binaries(?: \((\d+) tests? skipped)?")
SUMMARY = re.compile(r"Summary\s+\[\s*([\d.]+)s\]")


# --- machine ----------------------------------------------------------------


def cores() -> int:
    """Logical cores, as the load average counts them."""
    return os.cpu_count() or 1


def load() -> dict:
    """The load averages now, per machine and per core, with a verdict."""
    if not hasattr(os, "getloadavg"):
        return {"available": False, "verdict": "unknown (no load average on this system)"}
    one, five, fifteen = os.getloadavg()
    per_core = one / cores()
    if per_core <= 0.5:
        verdict = "quiet"
    elif per_core <= BUSY:
        verdict = "busy"
    elif per_core <= TOO_LOADED:
        verdict = "loaded: wall times are not comparable"
    else:
        verdict = "overloaded: wall times are not comparable"
    return {
        "available": True,
        "one": round(one, 1),
        "five": round(five, 1),
        "fifteen": round(fifteen, 1),
        "per_core": round(per_core, 2),
        "verdict": verdict,
        "at": time.strftime("%Y-%m-%d %H:%M:%S"),
    }


def load_note(sample: dict) -> str:
    """One short phrase for a load sample, for beside a number."""
    if not sample.get("available"):
        return "load unknown"
    return f"load {sample['one']} on {cores()} cores, {sample['per_core']}/core, {sample['verdict']}"


# --- inventory ----------------------------------------------------------------


def crate_of(path: Path) -> str:
    """The crate a path belongs to, from the `crates/<name>` segment."""
    parts = path.parts
    if "crates" in parts and parts.index("crates") + 1 < len(parts):
        return parts[parts.index("crates") + 1]
    return parts[0] if parts else "?"


def is_test_file(path: Path) -> bool:
    """Whether a file holds tests by this project's placement rule."""
    name = path.name
    return name == "tests.rs" or name.endswith("_tests.rs") or "tests" in path.parts


def rust_files(root: Path) -> list[Path]:
    """Every `.rs` file under `root`, skipping build output."""
    return [p for p in sorted(root.rglob("*.rs")) if "target" not in p.parts]


PATTERNS = {
    "sleep": re.compile(r"\bthread::sleep\b|\bsleep\(Duration"),
    "poll": re.compile(r"\brecv_timeout\b|\.elapsed\(\)\s*[<>]|Instant::now\(\)\s*\+|\bdeadline\b"),
    "temp folder": re.compile(r"\btemp_dir\(\)|\btempfile\b|\bTempDir\b|\bmkdtemp\b"),
    "GPU": re.compile(r"\brequest_adapter\b|\bLOOM_REQUIRE_GPU\b|\bwgpu::Instance\b|\bheadless_device\b|\bcapture\("),
    "audio device": re.compile(r"\bcpal::\b|\bdefault_host\(\)|\bdefault_output_device\b"),
    "system fonts": re.compile(r"\bload_system_fonts\b|\bsystem_fonts\b|\bFontCollection::new\b|\bfontdb\b"),
    "environment": re.compile(r"\benv::var\(\s*\"(HOME|USER|TMPDIR|XDG_[A-Z_]+|APPDATA)\""),
}
TEST_ATTR = re.compile(r"^\s*#\[(?:\w+::)*test\b")
IGNORE_ATTR = re.compile(r"^\s*#\[ignore(?:\s*=\s*\"([^\"]*)\")?\]")
INLINE_TESTS = re.compile(r"^\s*mod tests\s*\{")
FN_DEF = re.compile(r"^(\s*)(?:pub(?:\([^)]*\))?\s+)?fn\s+(\w+)")
CLEANUP = re.compile(r"\bremove_dir_all\b|\bremove_file\b|\bimpl Drop\b|\bTempDir\b")


def function_bodies(lines: list[str]) -> list[tuple[str, int, str]]:
    """(name, line, body) for each top-level or impl-level non-test function."""
    found = []
    i = 0
    while i < len(lines):
        match = FN_DEF.match(lines[i])
        if match and len(match.group(1)) <= 4 and "{" in lines[i]:
            prev = lines[i - 1] if i else ""
            depth, j, body = 0, i, []
            while j < len(lines):
                body.append(lines[j])
                depth += lines[j].count("{") - lines[j].count("}")
                if depth <= 0 and j > i:
                    break
                j += 1
            if not TEST_ATTR.match(prev) and len(body) >= 4:
                found.append((match.group(2), i + 1, "\n".join(s.strip() for s in body[1:])))
            i = j + 1
            continue
        i += 1
    return found


def inventory(root: Path) -> dict:
    """Static counts over the test code. Needs no build."""
    crates: dict[str, Counter] = defaultdict(Counter)
    hits: dict[str, list[str]] = defaultdict(list)
    ignored: list[dict] = []
    inline: list[str] = []
    no_cleanup: list[str] = []
    helpers: dict[str, list[str]] = defaultdict(list)
    for path in rust_files(root / "crates"):
        rel = path.relative_to(root)
        crate = crate_of(rel)
        text = path.read_text(encoding="utf-8", errors="replace")
        lines = text.splitlines()
        test_file = is_test_file(rel)
        tests_here = 0
        for number, line in enumerate(lines, 1):
            if TEST_ATTR.match(line):
                tests_here += 1
            ignore = IGNORE_ATTR.match(line)
            if ignore:
                ignored.append({"where": f"{rel}:{number}", "reason": ignore.group(1) or ""})
            if not test_file and INLINE_TESTS.match(line):
                inline.append(f"{rel}:{number}")
            if test_file:
                for name, pattern in PATTERNS.items():
                    if pattern.search(line) and not line.lstrip().startswith("//"):
                        hits[name].append(f"{rel}:{number}")
        crates[crate]["tests"] += tests_here
        crates[crate]["test files"] += 1 if test_file and tests_here else 0
        if test_file and PATTERNS["temp folder"].search(text) and not CLEANUP.search(text):
            no_cleanup.append(str(rel))
        if test_file:
            for name, line, body in function_bodies(lines):
                digest = hashlib.sha1(body.encode()).hexdigest()[:12]
                helpers[digest].append(f"{rel}:{line} {name}")
    duplicates = sorted(
        (places for places in helpers.values() if len({p.split(":")[0] for p in places}) >= 2),
        key=len,
        reverse=True,
    )
    return {
        "crates": {c: dict(v) for c, v in sorted(crates.items())},
        "patterns": {k: v for k, v in sorted(hits.items())},
        "ignored": ignored,
        "inline_tests": inline,
        "temp_without_cleanup": no_cleanup,
        "duplicate_helpers": duplicates[:TOP],
        "duplicate_helper_groups": len(duplicates),
    }


# --- specs ----------------------------------------------------------------------


def specs(root: Path) -> list[dict]:
    """Acceptance criteria per spec in docs/design: ticked, open, manual."""
    rows = []
    for path in sorted((root / "docs" / "design").glob("*.md")):
        text = path.read_text(encoding="utf-8", errors="replace")
        status = re.search(r"Status:\**\s*([A-Za-z]+)", text)
        ticked = opened = manual = 0
        in_section = False
        for line in text.splitlines():
            if line.startswith("#"):
                in_section = "acceptance" in line.lower()
                continue
            if in_section:
                if re.match(r"\s*- \[x\]", line, re.I):
                    ticked += 1
                elif re.match(r"\s*- \[ \]", line):
                    opened += 1
                if "manual check" in line.lower():
                    manual += 1
        if ticked or opened:
            rows.append({
                "spec": str(path.relative_to(root)),
                "status": status.group(1).strip() if status else "?",
                "ticked": ticked,
                "open": opened,
                "manual": manual,
            })
    return rows


# --- CI -----------------------------------------------------------------------


def gh(*args: str) -> str | None:
    """Run `gh` and return stdout, or None when it is missing or fails."""
    if not shutil.which("gh"):
        return None
    result = subprocess.run(["gh", *args], capture_output=True, text=True)
    return result.stdout if result.returncode == 0 else None


def parse_nextest(text: str) -> dict:
    """Per-test results, build time, and totals from nextest output."""
    tests, statuses = [], Counter()
    build = started = summary = None
    for raw in text.splitlines():
        line = ANSI.sub("", raw)
        # GitHub prefixes each log line with a timestamp.
        line = re.sub(r"^\d{4}-\d\d-\d\dT[\d:.]+Z ", "", line)
        match = RESULT.match(line)
        if match:
            status, seconds, binary, name = match.groups()
            statuses[status.split()[0]] += 1
            if status in ("PASS", "FAIL", "FLAKY"):
                tests.append({"binary": binary, "test": name, "seconds": float(seconds), "status": status})
            continue
        if build is None and (m := FINISHED.search(line)):
            build = int(m.group(1) or 0) * 60 + float(m.group(2))
        elif (m := STARTING.search(line)):
            started = {"tests": int(m.group(1)), "binaries": int(m.group(2)), "skipped": int(m.group(3) or 0)}
        elif (m := SUMMARY.search(line)):
            summary = float(m.group(1))
    return {"tests": tests, "statuses": dict(statuses), "build_seconds": build, "start": started, "run_seconds": summary}


def by_binary(tests: list[dict]) -> list[dict]:
    """Test count, total, and slowest per binary, slowest total first."""
    groups: dict[str, list[float]] = defaultdict(list)
    for t in tests:
        groups[t["binary"]].append(t["seconds"])
    rows = [
        {"binary": b, "tests": len(s), "total": round(sum(s), 1), "median": round(statistics.median(s), 3), "max": max(s)}
        for b, s in groups.items()
    ]
    return sorted(rows, key=lambda r: -r["total"])


def ci(run_id: str | None) -> dict:
    """Per-test times from the latest green CI run on main, or the one named."""
    if gh("--version") is None:
        return {"available": False, "why": "`gh` is missing or not signed in"}
    if run_id is None:
        listing = gh("run", "list", "--branch", "main", "--workflow", "ci.yml", "--status", "success",
                     "--limit", "1", "--json", "databaseId")
        if not listing or not json.loads(listing):
            return {"available": False, "why": "no green CI run on main found"}
        run_id = str(json.loads(listing)[0]["databaseId"])
    meta = json.loads(gh("run", "view", run_id, "--json", "headSha,createdAt,url,jobs") or "{}")
    # The run's own URL names the repository, so no other `gh` call can
    # disagree with it.
    found = re.search(r"github\.com/([^/]+/[^/]+)/actions", meta.get("url", ""))
    repo = found.group(1) if found else ""
    jobs, missing = {}, []
    for job in meta.get("jobs", []):
        if not re.match(r"Test \(|Coverage$", job["name"]):
            continue
        # nextest colours its output. gh 2.98 and later refuse to print
        # escape sequences unless told to; older gh has no such flag.
        url = f"repos/{repo}/actions/jobs/{job['databaseId']}/logs"
        log = gh("api", "--allow-escape-sequences", url) or gh("api", url)
        if log is None:
            missing.append(job["name"])
            continue
        parsed = parse_nextest(log)
        if not parsed["tests"]:
            # A log that parses to nothing is a failed read, not a run with
            # no tests: the format changed, or the output was refused.
            missing.append(f"{job['name']} (log read, no test lines found)")
            continue
        parsed["slowest"] = sorted(parsed["tests"], key=lambda t: -t["seconds"])[:TOP]
        parsed["binaries"] = by_binary(parsed["tests"])
        jobs[job["name"]] = parsed
    recent = json.loads(gh("run", "list", "--branch", "main", "--workflow", "ci.yml", "--limit", "50",
                           "--json", "databaseId,attempt,conclusion,headSha") or "[]")
    return {
        "available": True,
        "run": run_id,
        "commit": meta.get("headSha", "")[:8],
        "date": meta.get("createdAt", ""),
        "url": meta.get("url", ""),
        "jobs": jobs,
        "missing_logs": missing,
        "recent_runs": len(recent),
        "recent_failed": sum(1 for r in recent if r["conclusion"] == "failure"),
        "recent_reattempted": sum(1 for r in recent if r.get("attempt", 1) > 1),
    }


# --- coverage -------------------------------------------------------------------


# `const fn` is left out: it often runs at compile time (in a `static`
# descriptor), where coverage cannot count it.
PUB_FN = re.compile(r"\s*pub\s+(?:unsafe\s+)?fn\s+(\w+)")


_SOURCES: dict[str, str] = {}


def with_callers(root: Path, entry: str) -> str:
    """Add where a function is called from, which says what its zero means.

    No caller: dead code. Callers only in other crates: probably a coverage
    artefact, because the compiler may inline a small function into the
    calling crate (Rust does so for small functions by itself), and llvm-cov
    then counts no hits in its own file. Callers in its own crate: a gap.
    """
    if not _SOURCES:
        for path in rust_files(root / "crates"):
            _SOURCES[str(path.relative_to(root))] = path.read_text(encoding="utf-8", errors="replace")
    where, name = entry.split(" ", 1)
    home = where.split(":")[0]
    call = re.compile(rf"(?:\.|::|\b){re.escape(name)}\s*(?:::<[^>]*>)?\(")
    same = other = 0
    for path, text in _SOURCES.items():
        hits = len(call.findall(text)) - (1 if path == home else 0)
        if hits <= 0:
            continue
        if crate_of(Path(path)) == crate_of(Path(home)):
            same += hits
        else:
            other += hits
    if not same and not other:
        verdict = "no callers: dead code?"
    elif not same:
        verdict = f"{other} calls from other crates only: likely inlined there"
    else:
        verdict = f"{same} calls in its crate, {other} elsewhere"
    return f"{entry} ({verdict})"


def body_end(lines: list[str], start: int) -> int:
    """Index of the line that closes the brace block opened at or after `start`."""
    depth, opened = 0, False
    for i in range(start, len(lines)):
        depth += lines[i].count("{") - lines[i].count("}")
        opened = opened or "{" in lines[i]
        if opened and depth <= 0:
            return i
    return len(lines) - 1


def coverage(root: Path, run_id: str | None, lcov: Path | None) -> dict:
    """Line coverage per crate and file, and public functions never run."""
    if lcov is None:
        if run_id is None:
            return {"available": False, "why": "no CI run to read the coverage artifact from"}
        # The artifact is about 24 MB. It goes in a folder that is removed
        # afterwards, so this review does not add to the temp folder it checks.
        with tempfile.TemporaryDirectory(prefix="loom-test-review-") as folder:
            if gh("run", "download", run_id, "-n", "coverage", "-D", folder) is None:
                return {"available": False, "why": f"run {run_id} has no `coverage` artifact (expired, or the job failed)"}
            report = coverage(root, run_id, Path(folder) / "lcov.info")
            report["source"] = f"the `coverage` artifact of run {run_id}"
            return report
    files: dict[str, dict] = {}
    current = None
    for line in lcov.read_text(encoding="utf-8", errors="replace").splitlines():
        if line.startswith("SF:"):
            path = line[3:].replace("\\", "/")
            # The runner's absolute path: keep only the part from `crates/`.
            current = path[path.find("crates/"):] if "crates/" in path else path
            files[current] = {"lines": 0, "hit": 0, "fns": {}, "fn_hits": {}, "hit_lines": set()}
        elif current is None:
            continue
        elif line.startswith("DA:"):
            _, count = line[3:].split(",")[:2]
            files[current]["lines"] += 1
            files[current]["hit"] += 1 if int(count) > 0 else 0
            if int(count) > 0:
                files[current]["hit_lines"].add(int(_))
        elif line.startswith("FN:"):
            number, name = line[3:].split(",", 1)
            files[current]["fns"][name] = int(number)
        elif line.startswith("FNDA:"):
            count, name = line[5:].split(",", 1)
            files[current]["fn_hits"][name] = files[current]["fn_hits"].get(name, 0) + int(count)
    crates: dict[str, list[int]] = defaultdict(lambda: [0, 0])
    unrun_pub: dict[str, list[str]] = defaultdict(list)
    for path, f in files.items():
        if is_test_file(Path(path)):
            continue
        crate = crate_of(Path(path))
        crates[crate][0] += f["lines"]
        crates[crate][1] += f["hit"]
        source = root / path
        if not source.exists():
            continue
        lines = source.read_text(encoding="utf-8", errors="replace").splitlines()
        # A generic function has one entry per instantiation: it is unrun
        # only when every instantiation on that line is.
        hits_by_line: dict[int, int] = defaultdict(int)
        for name, number in f["fns"].items():
            hits_by_line[number] += f["fn_hits"].get(name, 0)
        for number, hits in sorted(hits_by_line.items()):
            if hits or number > len(lines):
                continue
            match = PUB_FN.match(lines[number - 1])
            # Function records miss instances compiled into another crate's
            # test binary; the merged line counts do not. So a function is
            # unrun only when no line of its body ran.
            end = body_end(lines, number - 1) + 1
            # An `#[inline]` function called from another crate is compiled
            # there, and llvm-cov counts no hits for it here.
            if any("#[inline" in lines[k] for k in range(max(0, number - 4), number - 1)):
                continue
            if match and not any(n in f["hit_lines"] for n in range(number, end + 1)):
                unrun_pub[crate].append(f"{path}:{number} {match.group(1)}")
    per_crate = sorted(
        ({"crate": c, "lines": n, "percent": round(100 * h / n, 1) if n else 0.0, "unrun_pub_fns": len(unrun_pub[c])}
         for c, (n, h) in crates.items()),
        key=lambda r: r["percent"],
    )
    lowest = sorted(
        ({"file": p, "lines": f["lines"], "percent": round(100 * f["hit"] / f["lines"], 1)}
         for p, f in files.items() if f["lines"] >= 50 and not is_test_file(Path(p))),
        key=lambda r: (r["percent"], -r["lines"]),
    )[:TOP]
    total_lines = sum(n for n, _ in crates.values())
    total_hit = sum(h for _, h in crates.values())
    return {
        "available": True,
        "source": str(lcov),
        "total_percent": round(100 * total_hit / total_lines, 1) if total_lines else 0.0,
        "crates": per_crate,
        "lowest_files": lowest,
        "unrun_pub_fns": {c: [with_callers(root, entry) for entry in v[:TOP]] for c, v in unrun_pub.items() if v},
    }


# --- probe ----------------------------------------------------------------------


def nextest_list(root: Path) -> list[dict] | None:
    """Every test binary nextest knows, with its path, cwd, and tests."""
    result = subprocess.run(
        ["mise", "exec", "--", "cargo", "nextest", "list", "--workspace", "--message-format", "json"],
        cwd=root, capture_output=True, text=True,
    )
    if result.returncode != 0:
        return None
    data = json.loads(result.stdout)
    binaries = []
    for suite in data.get("rust-suites", {}).values():
        tests = [name for name, t in suite.get("testcases", {}).items() if not t.get("ignored")]
        binaries.append({"id": suite["binary-id"], "path": suite["binary-path"], "cwd": suite["cwd"], "tests": tests})
    return binaries


def run_once(path: str, test: str, cwd: str, env: dict, timeout: float) -> dict:
    """Run one test in its own process, as nextest does, and time it."""
    start = time.monotonic()
    proc = subprocess.Popen([path, "--exact", test, "--nocapture"], cwd=cwd, env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if not hasattr(os, "wait4"):
        try:
            code = proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            proc.kill()  # our own child, by its handle
            proc.wait()
            code = "timeout"
        return {"wall": round(time.monotonic() - start, 3), "user": None, "sys": None, "code": code}
    deadline = start + timeout
    while True:
        pid, status, usage = os.wait4(proc.pid, os.WNOHANG)
        if pid:
            break
        if time.monotonic() > deadline:
            proc.kill()  # our own child, by its process ID
            _, status, usage = os.wait4(proc.pid, 0)
            status = "timeout"
            break
        time.sleep(0.01)
    proc.returncode = 0  # reaped by wait4; stop Popen reaping it again
    return {
        "wall": round(time.monotonic() - start, 3),
        "user": round(usage.ru_utime, 3),
        "sys": round(usage.ru_stime, 3),
        "code": status if status == "timeout" else os.waitstatus_to_exitcode(status),
    }


def median_of(runs: list[dict], key: str):
    """Median of one field over several runs, or None when unmeasured."""
    values = [r[key] for r in runs if r[key] is not None]
    return round(statistics.median(values), 3) if values else None


def probe(root: Path, ci_tests: dict[str, float], repeat: int, timeout: float, only: str | None) -> dict:
    """The cost of one test process at a few points of each binary.

    Each binary runs a few of its tests alone: the fastest, the median, and
    the 90th percentile by CI time, and in a binary of 200 tests or more also
    the 25th, 75th, and 97th (fewer when tests repeat). The fastest shows the
    binary's floor; the others sample the setup its tests share, which is
    where a harness's cost hides. The temp-folder sweep of 2026-11 ran only
    in tests that opened the UI harness: the fastest and the median test of
    `loom-app` missed it, and only the 90th-percentile pick caught it. Each runs `repeat` times with the shared
    temp folder and `repeat` times with an empty one. Wall time depends on
    the load; user and kernel CPU time much less, and kernel time far above
    user time points at file-system work such as walking a folder.
    """
    binaries = nextest_list(root)
    if binaries is None:
        return {"available": False, "why": "`cargo nextest list` failed: build the tests first"}
    shared_tmp = tempfile.gettempdir()
    shared_entries = len(os.listdir(shared_tmp))
    rows = []
    for binary in binaries:
        if only and only not in binary["id"]:
            continue
        if not binary["tests"]:
            continue
        timed = sorted((ci_tests[f"{binary['id']} {name}"], name) for name in binary["tests"]
                       if f"{binary['id']} {name}" in ci_tests)
        if timed:
            # More points in a large binary: a harness used by only part of
            # its tests (as the UI harness was) may miss the median.
            points = {"fastest": 0.0, "median": 0.5, "p90": 0.9}
            if len(timed) >= 200:
                points.update({"p25": 0.25, "p75": 0.75, "p97": 0.97})
            picks = {label: timed[min(int(q * len(timed)), len(timed) - 1)][1] for label, q in points.items()}
        else:
            picks = {"first": binary["tests"][0]}
        seen: set[str] = set()
        for label, test in picks.items():
            if test in seen:
                continue
            seen.add(test)
            rows.append(probe_one(binary, test, label, ci_tests, repeat, timeout))
    return {
        "available": True,
        "shared_tmp_entries": shared_entries,
        "repeat": repeat,
        "rows": sorted(rows, key=lambda r: -(r["shared"]["wall"] or 0)),
    }


def probe_one(binary: dict, test: str, label: str, ci_tests: dict[str, float], repeat: int, timeout: float) -> dict:
    """Time one test alone, with the shared temp folder and with an empty one."""
    env = dict(os.environ, CARGO_MANIFEST_DIR=binary["cwd"], NEXTEST="1", RUST_TEST_THREADS="1")
    before = load()
    shared = [run_once(binary["path"], test, binary["cwd"], env, timeout) for _ in range(repeat)]
    empty_dir = tempfile.mkdtemp(prefix="loom-test-review-")
    clean_env = dict(env, TMPDIR=empty_dir, TMP=empty_dir, TEMP=empty_dir)
    clean = [run_once(binary["path"], test, binary["cwd"], clean_env, timeout) for _ in range(repeat)]
    # What one run left behind: the folder held all `repeat` runs.
    left = len(os.listdir(empty_dir)) / repeat
    shutil.rmtree(empty_dir, ignore_errors=True)
    return {
        "binary": binary["id"],
        "test": test,
        "pick": label,
        "tests_in_binary": len(binary["tests"]),
        "ci_seconds": ci_tests.get(f"{binary['id']} {test}"),
        "shared": {k: median_of(shared, k) for k in ("wall", "user", "sys")},
        "empty": {k: median_of(clean, k) for k in ("wall", "user", "sys")},
        "exit": sorted({str(r["code"]) for r in shared + clean}),
        "left_in_empty_tmp": round(left, 1),
        "load_before": before,
    }


# --- local full run -----------------------------------------------------------------


def local_run(root: Path, force: bool) -> dict:
    """One full nextest run here, refused when the machine is too loaded."""
    before = load()
    if before.get("available") and before["per_core"] > TOO_LOADED and not force:
        return {"available": False, "why": f"refused: {load_note(before)}. Rerun when it is quieter, or pass --force."}
    start = time.monotonic()
    result = subprocess.run(
        ["mise", "exec", "--", "cargo", "nextest", "run", "--workspace", "--no-fail-fast"],
        cwd=root, capture_output=True, text=True,
    )
    parsed = parse_nextest(result.stdout + result.stderr)
    parsed["slowest"] = sorted(parsed["tests"], key=lambda t: -t["seconds"])[:TOP]
    parsed["binaries"] = by_binary(parsed["tests"])
    parsed.update(available=True, wall=round(time.monotonic() - start, 1), load_before=before, load_after=load(),
                  exit=result.returncode)
    return parsed


# --- devtel -----------------------------------------------------------------------


def devtel(root: Path, days: int) -> dict:
    """Weekly agent time on tests, from the devtel archive, read-only."""
    sys.path.insert(0, str(root / "scripts" / "devtel"))
    try:
        import devtel as dt  # noqa: PLC0415  (the project's own helper)
    except ImportError as error:
        return {"available": False, "why": f"scripts/devtel not importable: {error}"}
    if not os.path.exists(dt.archive_path()):
        return {"available": False, "why": "no devtel archive yet (run `mise run devtel-import`)"}
    sql = f"""
        select strftime(date_trunc('week', start_ts), '%Y-%m-%d') as week,
               category,
               count(*) as calls,
               round(sum(epoch(end_ts - start_ts)) / 60, 0) as minutes,
               round(median(epoch(end_ts - start_ts)), 1) as median_s,
               round(quantile_cont(epoch(end_ts - start_ts), 0.9), 1) as p90_s,
               round(100.0 * avg(case when error then 1 else 0 end), 1) as error_pct
        from devtel_tools
        where category in ('test', 'check', 'build')
          and start_ts > now() - interval {int(days)} day
          and end_ts is not null
        group by all order by week, category;
    """
    try:
        rows = json.loads(dt.run_duckdb(sql, json_output=True, readonly=True, wait=30) or "[]")
    except Exception as error:  # noqa: BLE001  (the archive may be busy or missing a tool)
        return {"available": False, "why": f"devtel query failed: {error}"}
    return {"available": True, "days": days, "weeks": rows}


# --- report -------------------------------------------------------------------------


def ratio(times: dict) -> str:
    """Kernel time over user time, when both are big enough to divide."""
    user, kernel = times.get("user"), times.get("sys")
    if user is None or kernel is None or max(user, kernel) < 0.05:
        return ""
    return f"{kernel / max(user, 0.001):.1f}"


def table(headers: list[str], rows: list[list]) -> str:
    """A Markdown table."""
    out = ["| " + " | ".join(headers) + " |", "|" + "---|" * len(headers)]
    out += ["| " + " | ".join("" if v is None else str(v) for v in row) + " |" for row in rows]
    return "\n".join(out)


def markdown(report: dict) -> str:
    """The whole report as Markdown."""
    out = [f"# Test health measurements\n\nCommit `{report['commit']}`, {report['started']}.\n"]
    m = report["machine"]
    out.append(f"## Machine\n\n{cores()} cores. At start: {load_note(m['start'])}. At end: {load_note(m['end'])}.\n"
               f"Wall times next to a load above {BUSY}/core are not comparable with other runs. "
               "CPU times (user, kernel) hold up better.\n")

    inv = report["inventory"]
    out.append("## Inventory\n")
    out.append(table(["Crate", "Tests", "Test files"],
                     [[c, v.get("tests", 0), v.get("test files", 0)] for c, v in inv["crates"].items()]))
    out.append("\n**Patterns in test files** (places to read, not findings):\n")
    out.append(table(["Pattern", "Lines", "First places"],
                     [[k, len(v), ", ".join(v[:4])] for k, v in inv["patterns"].items()]))
    out.append(f"\n**Ignored tests:** {len(inv['ignored'])}. Without a reason: "
               f"{sum(1 for i in inv['ignored'] if not i['reason'])}.")
    for item in inv["ignored"][:TOP]:
        out.append(f"- `{item['where']}` {item['reason']}")
    out.append(f"\n**Inline `mod tests {{` outside test files:** {len(inv['inline_tests'])} {inv['inline_tests'][:5]}")
    out.append(f"\n**Test files that make temp folders and never remove one:** {len(inv['temp_without_cleanup'])}")
    for path in inv["temp_without_cleanup"][:TOP]:
        out.append(f"- `{path}`")
    out.append(f"\n**Helpers with the same body in two or more test files:** {inv['duplicate_helper_groups']} groups")
    for group in inv["duplicate_helpers"][:8]:
        out.append(f"- {len(group)}x: " + "; ".join(f"`{g}`" for g in group[:4]))

    rows = report["specs"]
    out.append("\n## Acceptance criteria in specs\n")
    out.append(f"{len(rows)} specs with criteria: {sum(r['ticked'] for r in rows)} ticked, "
               f"{sum(r['open'] for r in rows)} open, {sum(r['manual'] for r in rows)} marked manual. "
               "Criteria name no test, so whether a ticked one is tested takes reading.\n")
    out.append(table(["Spec", "Status", "Ticked", "Open", "Manual"],
                     [[r["spec"], r["status"], r["ticked"], r["open"], r["manual"]]
                      for r in sorted(rows, key=lambda r: -r["open"])[:TOP]]))

    c = report["ci"]
    out.append("\n## CI timings\n")
    if not c["available"]:
        out.append(f"Not measured: {c['why']}.")
    else:
        out.append(f"Run {c['run']} on `{c['commit']}` ({c['date']}). Runners start with an empty temp folder, so "
                   "costs that grow with the machine's state never show here.\n")
        if c["missing_logs"]:
            out.append(f"**Could not read the logs of:** {', '.join(c['missing_logs'])}. "
                       "Their rows are missing below, and the probe has no CI times for them.\n")
        out.append(f"Last {c['recent_runs']} CI runs on main: {c['recent_failed']} failed, "
                   f"{c['recent_reattempted']} re-run by hand. nextest retries are off, so FLAKY never appears in a log.\n")
        out.append(table(["Job", "Build s", "Run s", "Tests", "Binaries", "Skipped", "Statuses"],
                         [[n, j["build_seconds"], j["run_seconds"], (j["start"] or {}).get("tests"),
                           (j["start"] or {}).get("binaries"), (j["start"] or {}).get("skipped"), j["statuses"]]
                          for n, j in c["jobs"].items()]))
        for name, job in c["jobs"].items():
            out.append(f"\n### {name}: slowest tests\n")
            out.append(table(["Seconds", "Binary", "Test"],
                             [[t["seconds"], t["binary"], t["test"]] for t in job["slowest"]]))
            out.append(f"\n### {name}: binaries by total test time\n")
            out.append(table(["Binary", "Tests", "Total s", "Median s", "Max s"],
                             [[b["binary"], b["tests"], b["total"], b["median"], b["max"]] for b in job["binaries"][:TOP]]))

    cov = report["coverage"]
    out.append("\n## Coverage (from CI)\n")
    if not cov["available"]:
        out.append(f"Not read: {cov['why']}.")
    else:
        ci_commit = report["ci"].get("commit", "")
        same = "the same commit as here" if ci_commit and report["commit"].startswith(ci_commit) else \
            f"commit `{ci_commit or '?'}`, not `{report['commit']}`, so line numbers may have moved"
        out.append(f"Total {cov['total_percent']}% of lines, test files left out. Measured by CI on {same}. "
                   "Unrun public functions: `pub fn` with no line of its body run; never `pub(crate)`, `const fn`, or "
                   "`#[inline]` (llvm-cov does not count those when another crate calls them). In the `loom-app` "
                   "binary, `pub` is still private to the crate.\n")
        out.append(table(["Crate", "Lines", "Covered %", "Public fns never run"],
                         [[r["crate"], r["lines"], r["percent"], r["unrun_pub_fns"]] for r in cov["crates"]]))
        out.append("\n**Lowest files** (50 lines or more):\n")
        out.append(table(["File", "Lines", "Covered %"], [[r["file"], r["lines"], r["percent"]] for r in cov["lowest_files"]]))
        out.append("\n**Public functions no test runs** (first few per crate):\n")
        for crate, fns in sorted(cov["unrun_pub_fns"].items(), key=lambda kv: -len(kv[1])):
            out.append(f"- {crate}: " + "; ".join(f"`{f}`" for f in fns[:5]))

    p = report.get("probe")
    if p:
        out.append("\n## Fixed cost per test process (probe)\n")
        if not p["available"]:
            out.append(f"Not measured: {p['why']}.")
        else:
            out.append(f"Per binary, its fastest, median, and 90th-percentile test by CI time (and the 25th, 75th, "
                       f"and 97th in a binary of 200 tests or more), each run alone "
                       f"{p['repeat']} times; medians shown. "
                       f"`shared` uses the system temp folder ({p['shared_tmp_entries']} entries at start); "
                       "`empty` uses a new empty one. A large gap means the test depends on the machine's state. "
                       "`left` is what one test left in the empty folder.\n")
            out.append(table(
                ["Binary", "Pick", "Test", "CI s", "Wall s", "User s", "Kernel s", "Kernel/user", "Empty: wall", "Empty: kernel", "Left",
                 "Exit", "Load/core"],
                [[r["binary"], r["pick"], r["test"], r["ci_seconds"], r["shared"]["wall"], r["shared"]["user"], r["shared"]["sys"], ratio(r["shared"]),
                  r["empty"]["wall"], r["empty"]["sys"], r["left_in_empty_tmp"], ",".join(r["exit"]),
                  r["load_before"].get("per_core")] for r in p["rows"]]))

    lr = report.get("local")
    if lr:
        out.append("\n## Local full run\n")
        if not lr["available"]:
            out.append(lr["why"])
        else:
            out.append(f"Wall {lr['wall']} s, exit {lr['exit']}. Before: {load_note(lr['load_before'])}. "
                       f"After: {load_note(lr['load_after'])}.\n")
            out.append(table(["Seconds", "Binary", "Test"], [[t["seconds"], t["binary"], t["test"]] for t in lr["slowest"]]))

    d = report["devtel"]
    out.append("\n## Agent time on tests (devtel)\n")
    if not d["available"]:
        out.append(f"Not read: {d['why']}.")
    else:
        out.append(f"Last {d['days']} days, by week. Tool time of agents' test, check, and build calls.\n")
        out.append(table(["Week", "Kind", "Calls", "Minutes", "Median s", "p90 s", "Error %"],
                         [[w["week"], w["category"], w["calls"], w["minutes"], w["median_s"], w["p90_s"], w["error_pct"]]
                          for w in d["weeks"]]))
    return "\n".join(out) + "\n"


def main() -> int:
    """Parse arguments, measure, and print the report."""
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--json", action="store_true", help="print JSON instead of Markdown")
    parser.add_argument("--ci-run", help="CI run ID to read (default: the latest green run on main)")
    parser.add_argument("--no-ci", action="store_true", help="skip CI timings and coverage (no network)")
    parser.add_argument("--lcov", type=Path, help="read this lcov file instead of downloading the artifact")
    parser.add_argument("--probe", action="store_true", help="time one test process per binary (needs built tests)")
    parser.add_argument("--probe-binary", help="probe only binaries whose ID contains this text")
    parser.add_argument("--repeat", type=int, default=3, help="runs per probe (default 3)")
    parser.add_argument("--timeout", type=float, default=120.0, help="seconds before a probe run is stopped")
    parser.add_argument("--local", action="store_true", help="one full nextest run here (refused when overloaded)")
    parser.add_argument("--force", action="store_true", help="run --local even when the machine is overloaded")
    parser.add_argument("--days", type=int, default=56, help="devtel window in days (default 56)")
    args = parser.parse_args()

    root = Path.cwd()
    if not (root / "Cargo.toml").exists() or not (root / "crates").is_dir():
        print("Run from the repository root.", file=sys.stderr)
        return 2
    commit = subprocess.run(["git", "rev-parse", "--short=8", "HEAD"], capture_output=True, text=True).stdout.strip()
    report: dict = {"commit": commit or "?", "started": time.strftime("%Y-%m-%d %H:%M"), "machine": {"start": load()}}
    report["inventory"] = inventory(root)
    report["specs"] = specs(root)
    report["ci"] = {"available": False, "why": "skipped (--no-ci)"} if args.no_ci else ci(args.ci_run)
    run_id = report["ci"].get("run") if report["ci"]["available"] else None
    if args.lcov or not args.no_ci:
        report["coverage"] = coverage(root, run_id, args.lcov)
    else:
        report["coverage"] = {"available": False, "why": "skipped (--no-ci)"}
    if args.probe:
        times: dict[str, float] = {}
        for job in report["ci"].get("jobs", {}).values():
            for t in job["tests"]:
                times.setdefault(f"{t['binary']} {t['test']}", t["seconds"])
        report["probe"] = probe(root, times, args.repeat, args.timeout, args.probe_binary)
    if args.local:
        report["local"] = local_run(root, args.force)
    report["devtel"] = devtel(root, args.days)
    report["machine"]["end"] = load()
    if args.json:
        for job in report["ci"].get("jobs", {}).values():
            job.pop("tests", None)
        print(json.dumps(report, indent=2))
    else:
        print(markdown(report))
    return 0


if __name__ == "__main__":
    sys.exit(main())

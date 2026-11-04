#!/usr/bin/env python3
"""Measure the shape of the LoomDSP workspace for an architecture review.

Prints Markdown (or JSON with --json) with sizes, shapes, churn, rule
checks, duplication seams, and the crate graph. The numbers are a map of
where to read, not findings: a large file with one concern is fine.

Run from the repository root:

    python3 .claude/skills/review-architecture/scripts/measure.py [PATH ...] [--json] [--since DAYS]

Needs only Python 3. Uses `git` for churn and `cargo metadata` for the
crate graph when they are available, and says so when they are not. With
`rust-code-analysis-cli` available (`mise run metrics` installs it), the
function tables come from its parser, with cognitive and cyclomatic
complexity; without it they fall back to a brace-counting heuristic.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

PORTABLE_CRATES = ("loom-rt", "loom-dsp", "loom-core", "loom-modules", "loom-patch")
PORTABLE_CALLS = re.compile(
    r"\bstd::(fs|process|thread|net|env)\b|\bstd::time::(Instant|SystemTime)\b|\bInstant::now\b"
)
TOP = 15


def rust_files(roots: list[Path]) -> list[Path]:
    """Every `.rs` file under the given roots, in a stable order."""
    files: list[Path] = []
    for root in roots:
        if root.is_file() and root.suffix == ".rs":
            files.append(root)
            continue
        for path in sorted(root.rglob("*.rs")):
            if "target" in path.parts:
                continue
            files.append(path)
    return files


def crate_of(path: Path) -> str:
    """The crate a path belongs to, from the `crates/<name>` segment."""
    parts = path.parts
    if "crates" in parts:
        index = parts.index("crates")
        if index + 1 < len(parts):
            return parts[index + 1]
    return parts[0] if parts else "?"


def is_test_file(path: Path) -> bool:
    """Whether a file holds tests by this project's placement rule."""
    name = path.name
    return name == "tests.rs" or name.endswith("_tests.rs") or "tests" in path.parts


# --- shapes ---------------------------------------------------------------

STRUCT_RE = re.compile(r"^(?:pub(?:\([^)]*\))?\s+)?struct\s+(\w+)(?:<[^>]*>)?\s*\{")
ENUM_RE = re.compile(r"^(?:pub(?:\([^)]*\))?\s+)?enum\s+(\w+)(?:<[^>]*>)?\s*\{")
FIELD_RE = re.compile(r"^\s{4}(?:pub(?:\([^)]*\))?\s+)?[a-z_][a-z0-9_]*\s*:")
VARIANT_RE = re.compile(r"^\s{4}[A-Z]\w*")
IMPL_RE = re.compile(r"^impl(?:<[^>]*>)?\s+(?:[\w:]+\s+for\s+)?(\w+)")
DESCRIPTOR_RE = re.compile(r"^(?:pub\s+)?static\s+([A-Z][A-Z0-9_]*)\s*:\s*ModuleDescriptor\s*=")
MODULE_IMPL_RE = re.compile(r"^impl\s+Module\s+for\s+(\w+)")
FN_RE = re.compile(r"^(\s*)(?:pub(?:\([^)]*\))?\s+)?(?:const\s+|async\s+|unsafe\s+)*fn\s+(\w+)")


def block_end(lines: list[str], start: int) -> int:
    """Index of the line that closes the brace block opened on `start`."""
    depth = 0
    seen = False
    for i in range(start, len(lines)):
        line = lines[i].split("//", 1)[0]
        depth += line.count("{") - line.count("}")
        if "{" in line:
            seen = True
        if seen and depth <= 0:
            return i
    return len(lines) - 1


def shapes(files: list[Path]) -> dict:
    """Field, variant, impl-spread, and function-length counts."""
    structs: list[tuple[int, str, str]] = []
    enums: list[tuple[int, str, str]] = []
    impl_files: dict[str, set[str]] = defaultdict(set)
    functions: list[tuple[int, str, str]] = []
    for path in files:
        text = path.read_text(encoding="utf-8", errors="replace")
        lines = text.splitlines()
        rel = str(path)
        for i, line in enumerate(lines):
            m = STRUCT_RE.match(line)
            if m:
                end = block_end(lines, i)
                count = sum(1 for l in lines[i + 1 : end] if FIELD_RE.match(l))
                structs.append((count, m.group(1), f"{rel}:{i + 1}"))
                continue
            m = ENUM_RE.match(line)
            if m:
                end = block_end(lines, i)
                count = sum(1 for l in lines[i + 1 : end] if VARIANT_RE.match(l))
                enums.append((count, m.group(1), f"{rel}:{i + 1}"))
                continue
            m = IMPL_RE.match(line)
            if m:
                impl_files[m.group(1)].add(rel)
            m = FN_RE.match(line)
            if m and not is_test_file(path):
                # rustfmt wraps a long signature one parameter per line, so the
                # body's `{` can come several lines later. A `;` first means a
                # declaration with no body, as in a trait.
                for j in range(i, min(i + 40, len(lines))):
                    sig = lines[j].split("//", 1)[0]
                    if "{" in sig:
                        end = block_end(lines, j)
                        functions.append((end - i + 1, m.group(2), f"{rel}:{i + 1}"))
                        break
                    if sig.rstrip().endswith(";"):
                        break
    spread = sorted(((len(v), k, sorted(v)) for k, v in impl_files.items() if len(v) >= 3), reverse=True)
    return {
        "structs": sorted(structs, reverse=True)[:TOP],
        "enums": sorted(enums, reverse=True)[:TOP],
        "impl_spread": [(n, name, files_) for n, name, files_ in spread[:TOP]],
        "functions": [f for f in sorted(functions, reverse=True) if f[0] >= 80][:TOP],
    }


# --- complexity -----------------------------------------------------------

def metrics_command() -> list[str] | None:
    """How to run rust-code-analysis-cli here: on the path, or through mise.

    mise.toml scopes the tool to the `metrics` task, so outside that task
    `mise x` names it in full; mise installs it on first use, which compiles
    it and takes a few minutes once.
    """
    if shutil.which("rust-code-analysis-cli"):
        return ["rust-code-analysis-cli"]
    if shutil.which("mise"):
        command = ["mise", "x", "cargo:rust-code-analysis-cli@latest", "--", "rust-code-analysis-cli"]
        probe = subprocess.run([*command, "--version"], capture_output=True, text=True, env={**os.environ, "MISE_YES": "1"})
        if probe.returncode == 0:
            return command
    return None


def complexity(paths: list[str]) -> dict:
    """Per-function complexity from rust-code-analysis-cli, or `available: False`.

    Closures are left out (the tool names them `<anonymous>`), and so are
    test files. `cognitive` is the sum for the function, nested closures
    included, which is what a reader meets.
    """
    command = metrics_command()
    if command is None:
        return {"available": False, "functions": []}
    out_dir = tempfile.mkdtemp(prefix="loom-metrics-")
    try:
        run = subprocess.run(
            [*command, "-m", "-O", "json", "-I", "*.rs", "-X", "**/target/**", "-o", out_dir, "-p", *paths],
            capture_output=True, text=True,
        )
        if run.returncode != 0:
            return {"available": False, "functions": [], "error": run.stderr.strip()[-200:]}
        functions: list[dict] = []
        for json_path in sorted(Path(out_dir).rglob("*.json")):
            data = json.loads(json_path.read_text(encoding="utf-8"))
            source = Path(data["name"])
            if is_test_file(source):
                continue
            stack = list(data.get("spaces", []))
            while stack:
                space = stack.pop()
                stack.extend(space.get("spaces", []))
                if space["kind"] != "function" or space["name"] == "<anonymous>":
                    continue
                m = space["metrics"]
                functions.append({
                    "cognitive": int(m["cognitive"]["sum"]),
                    "cyclomatic": int(m["cyclomatic"]["sum"]),
                    "lines": int(m["loc"]["sloc"]),
                    "args": int(m["nargs"]["total_functions"]),
                    "exits": int(m["nexits"]["sum"]),
                    "name": space["name"],
                    "where": f"{source}:{space['start_line']}",
                })
        return {"available": True, "functions": functions}
    finally:
        shutil.rmtree(out_dir, ignore_errors=True)


# --- sizes ----------------------------------------------------------------

def sizes(files: list[Path]) -> dict:
    """Lines per crate and the largest non-test source files."""
    per_crate: Counter = Counter()
    per_file: list[tuple[int, str]] = []
    for path in files:
        n = sum(1 for _ in path.open(encoding="utf-8", errors="replace"))
        per_crate[crate_of(path)] += n
        if not is_test_file(path):
            per_file.append((n, str(path)))
    return {
        "per_crate": sorted(per_crate.items(), key=lambda kv: -kv[1]),
        "largest_files": sorted(per_file, reverse=True)[:TOP],
        "total": sum(per_crate.values()),
    }


# --- churn ----------------------------------------------------------------

def churn(since_days: int, paths: list[str]) -> dict:
    """Files with the most commits in the window, from git, under the given paths."""
    try:
        out = subprocess.run(
            ["git", "log", f"--since={since_days} days ago", "--name-only", "--pretty=format:", "--", *paths],
            capture_output=True, text=True, check=True,
        ).stdout
    except (OSError, subprocess.CalledProcessError):
        return {"available": False, "files": []}
    counts = Counter(line.strip() for line in out.splitlines() if line.strip().endswith(".rs"))
    return {"available": True, "files": counts.most_common(TOP)}


# --- rules ----------------------------------------------------------------

def rules(files: list[Path]) -> dict:
    """Unsafe blocks, portable-crate calls, and smoothed params without param_glide."""
    unsafe_total: Counter = Counter()
    unsafe_without_safety: list[str] = []
    portable_calls: list[str] = []
    smooth_no_glide: list[str] = []
    for path in files:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        crate = crate_of(path)
        rel = str(path)
        text = "\n".join(lines)
        for i, line in enumerate(lines):
            code = line.split("//", 1)[0]
            if re.search(r"\bunsafe\s*\{", code) and not is_test_file(path):
                unsafe_total[crate] += 1
                window = "\n".join(lines[max(0, i - 6) : i + 1])
                if "SAFETY" not in window:
                    unsafe_without_safety.append(f"{rel}:{i + 1}")
            if crate in PORTABLE_CRATES and not is_test_file(path) and PORTABLE_CALLS.search(code):
                portable_calls.append(f"{rel}:{i + 1}: {line.strip()[:90]}")
        if crate == "loom-modules" and not is_test_file(path) and ".smooth(" in text:
            smoothed: set[str] = set()
            for i, line in enumerate(lines):
                m = DESCRIPTOR_RE.match(line)
                if m:
                    end = block_end(lines, i)
                    if ".smooth(" in "\n".join(lines[i : end + 1]):
                        smoothed.add(m.group(1).replace("_", ""))
            for i, line in enumerate(lines):
                m = MODULE_IMPL_RE.match(line)
                if not m or m.group(1).upper() not in smoothed:
                    continue
                end = block_end(lines, i)
                if "fn param_glide" not in "\n".join(lines[i : end + 1]):
                    smooth_no_glide.append(f"{rel}:{i + 1}: {m.group(1)}")
    return {
        "unsafe_per_crate": sorted(unsafe_total.items()),
        "unsafe_without_safety": unsafe_without_safety,
        "portable_calls": portable_calls,
        "smooth_no_glide": smooth_no_glide,
    }


# --- seams ----------------------------------------------------------------

# A `use` line, or a line inside a multi-line `use` block (names, commas, braces only).
IMPORT_LINE_RE = re.compile(r"^\s*(use\s|[\w:,{}\s]+,?\s*$)")
SEAMS = {
    "WAV writers (`WavWriter`)": re.compile(r"\bWavWriter\b"),
    "WAV readers (`WavReader`)": re.compile(r"\bWavReader\b"),
    "Thread spawns": re.compile(r"\bthread::spawn\b|\bthread::Builder\b"),
    "Temp-directory helpers in tests (`process::id()`)": re.compile(r"process::id\(\)"),
    "Message text matched by substring (`.contains(\"...\")`)": re.compile(r"\.contains\(\"[^\"]*[a-z ]{6,}"),
    "Transport found by id string": re.compile(r"\bTRANSPORT_MODULE\b|\"loom\.clock\.transport\""),
    "`Box::from_raw`": re.compile(r"Box::from_raw"),
}


def seams(files: list[Path]) -> dict:
    """Sites of each duplication or stringly-typed pattern."""
    found: dict[str, list[str]] = {name: [] for name in SEAMS}
    for path in files:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        for i, line in enumerate(lines):
            code = line.split("//", 1)[0]
            for name, pattern in SEAMS.items():
                if not pattern.search(code):
                    continue
                wants_tests = "Temp-directory" in name
                if wants_tests != is_test_file(path):
                    continue
                if "Transport" in name and ("const TRANSPORT_MODULE" in code or IMPORT_LINE_RE.match(code)):
                    continue
                found[name].append(f"{path}:{i + 1}")
    return found


# --- crates ---------------------------------------------------------------

def crate_graph() -> dict:
    """Each workspace crate's dependencies on the others, from cargo metadata."""
    try:
        out = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"],
            capture_output=True, text=True, check=True,
        ).stdout
    except (OSError, subprocess.CalledProcessError):
        return {"available": False, "crates": {}}
    meta = json.loads(out)
    names = {p["name"] for p in meta["packages"]}
    graph = {}
    for p in meta["packages"]:
        deps = [d["name"] for d in p["dependencies"] if d["name"] in names and d["name"] != p["name"]]
        dev = [
            d["name"] for d in p["dependencies"] if d["name"] in names and d["name"] != p["name"] and d.get("kind") == "dev"
        ]
        graph[p["name"]] = {"deps": sorted(set(deps) - set(dev)), "dev": sorted(set(dev))}
    return {"available": True, "crates": graph}


# --- output ---------------------------------------------------------------

def head_commit() -> str:
    try:
        return subprocess.run(["git", "rev-parse", "--short", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return "unknown"


def markdown(report: dict) -> str:
    out: list[str] = []
    w = out.append
    w(f"# Workspace measurements at {report['commit']}\n")
    w("Numbers are a map of where to read, not findings.\n")

    s = report["sizes"]
    w("## Size\n")
    w(f"Total Rust lines under the given paths: {s['total']}\n")
    w("| Crate | Lines |\n|---|---:|")
    for crate, n in s["per_crate"]:
        w(f"| {crate} | {n} |")
    w("\nLargest non-test files:\n")
    w("| Lines | File |\n|---:|---|")
    for n, path in s["largest_files"]:
        w(f"| {n} | `{path}` |")

    sh = report["shapes"]
    w("\n## Shapes\n")
    w("Structs with the most fields:\n")
    w("| Fields | Struct | Where |\n|---:|---|---|")
    for n, name, where in sh["structs"]:
        w(f"| {n} | `{name}` | `{where}` |")
    w("\nEnums with the most variants:\n")
    w("| Variants | Enum | Where |\n|---:|---|---|")
    for n, name, where in sh["enums"]:
        w(f"| {n} | `{name}` | `{where}` |")
    w("\nTypes with `impl` blocks in three or more files:\n")
    w("| Files | Type | Files |\n|---:|---|---|")
    for n, name, files_ in sh["impl_spread"]:
        w(f"| {n} | `{name}` | {', '.join(f'`{f}`' for f in files_)} |")
    cx = report["complexity"]
    if cx["available"]:
        by_cognitive = sorted(cx["functions"], key=lambda f: (-f["cognitive"], -f["lines"]))[:TOP]
        w("\nMost complex functions, by cognitive complexity (how much nesting and branching a reader must hold; 15 is the usual warning level, 25 and up is hard to change safely), from rust-code-analysis:\n")
        w("| Cognitive | Cyclomatic | Lines | Args | Exits | Function | Where |\n|---:|---:|---:|---:|---:|---|---|")
        for f in by_cognitive:
            w(f"| {f['cognitive']} | {f['cyclomatic']} | {f['lines']} | {f['args']} | {f['exits']} | `{f['name']}` | `{f['where']}` |")
        by_lines = sorted(cx["functions"], key=lambda f: (-f["lines"], -f["cognitive"]))[:TOP]
        w("\nLongest functions (source lines, from rust-code-analysis):\n")
        w("| Lines | Cognitive | Function | Where |\n|---:|---:|---|---|")
        for f in by_lines:
            w(f"| {f['lines']} | {f['cognitive']} | `{f['name']}` | `{f['where']}` |")
    else:
        w("\nLongest functions (80 lines or more, brace-counted, so attributes and comments count). Run `mise run metrics` once to install `rust-code-analysis-cli`, for parsed lengths and cognitive complexity:\n")
        if cx.get("error"):
            w(f"\nrust-code-analysis-cli failed: `{cx['error']}`\n")
        w("| Lines | Function | Where |\n|---:|---|---|")
        for n, name, where in sh["functions"]:
            w(f"| {n} | `{name}` | `{where}` |")

    c = report["churn"]
    w(f"\n## Churn (last {report['since_days']} days)\n")
    if not c["available"]:
        w("git log was not available.")
    else:
        w("| Commits | File |\n|---:|---|")
        for path, n in c["files"]:
            w(f"| {n} | `{path}` |")

    r = report["rules"]
    w("\n## Rules\n")
    w("`unsafe { }` blocks per crate (non-test): " + (", ".join(f"{k} {v}" for k, v in r["unsafe_per_crate"]) or "none"))
    w("\n`unsafe` without a `SAFETY` comment within six lines above:\n")
    w("\n".join(f"- `{x}`" for x in r["unsafe_without_safety"]) or "- none")
    w("\nFilesystem, process, thread, network, or clock calls in the portable crates (check each is gated or belongs elsewhere):\n")
    w("\n".join(f"- `{x}`" for x in r["portable_calls"]) or "- none")
    w("\nModules whose descriptor declares a `.smooth(` parameter but whose `impl Module` has no `fn param_glide` (matched by name, so a module whose descriptor static is not its own name in capitals is missed; read each, because one case may be harmless when traced):\n")
    w("\n".join(f"- `{x}`" for x in r["smooth_no_glide"]) or "- none")

    w("\n## Seams\n")
    for name, sites in report["seams"].items():
        w(f"**{name}:** {len(sites)}")
        for site in sites[:TOP]:
            w(f"- `{site}`")
        if len(sites) > TOP:
            w(f"- and {len(sites) - TOP} more")
        w("")

    g = report["crates"]
    w("## Crates\n")
    if not g["available"]:
        w("cargo metadata was not available.")
    else:
        w("| Crate | Depends on | Dev only |\n|---|---|---|")
        for name, d in g["crates"].items():
            w(f"| {name} | {', '.join(d['deps']) or '-'} | {', '.join(d['dev']) or '-'} |")
    return "\n".join(out) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("paths", nargs="*", default=["crates"], help="directories or files to measure (default: crates)")
    parser.add_argument("--json", action="store_true", help="print JSON instead of Markdown")
    parser.add_argument("--since", type=int, default=90, help="churn window in days (default: 90)")
    args = parser.parse_args()

    roots = [Path(p) for p in args.paths]
    missing = [str(r) for r in roots if not r.exists()]
    if missing:
        print(f"not found: {', '.join(missing)}", file=sys.stderr)
        return 2
    files = rust_files(roots)
    if not files:
        print("no Rust files found", file=sys.stderr)
        return 2

    report = {
        "commit": head_commit(),
        "since_days": args.since,
        "sizes": sizes(files),
        "shapes": shapes(files),
        "complexity": complexity(args.paths),
        "churn": churn(args.since, args.paths),
        "rules": rules(files),
        "seams": seams(files),
        "crates": crate_graph(),
    }
    if args.json:
        print(json.dumps(report, indent=2))
    else:
        print(markdown(report))
    return 0


if __name__ == "__main__":
    os.chdir(os.getcwd())
    raise SystemExit(main())

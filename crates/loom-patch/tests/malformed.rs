//! Malformed patches give an error, never a panic or a hang (#44,
//! NFR-REL-04, NFR-SEC-01).
//!
//! The fuzz target in `fuzz/` searches for such inputs, but it needs a
//! nightly compiler, so CI does not run it. This test runs on stable, in
//! every CI job. It loads three kinds of input the same way the fuzz
//! target does:
//!
//! - every file in `tests/malformed/`: inputs the fuzzer or a person found
//!   that once broke the loader, kept so the bug stays fixed;
//! - hand-written edge cases: extreme IDs, numbers, sizes, and nesting;
//! - seeded random changes of every example patch: cut, deleted, copied,
//!   and replaced bytes, and swapped tokens.

use loom_core::{EngineConfig, create_engine};
use loom_patch::Patch;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Longest time one input may take to load and play. Far more than any
/// input needs, so it fails only on a hang or a runaway loop.
const LIMIT: Duration = Duration::from_secs(5);

/// Random changes made to each example patch.
const MUTATIONS: usize = 400;

/// Loads `text` as the fuzz target does: parse it, load it into an engine
/// with every built-in module, and play a few buffers if it loads. Returns
/// true when the patch loaded. Panics if it takes longer than [`LIMIT`] or
/// the output is not finite.
fn load_and_play(text: &str, what: &str) -> bool {
    let started = Instant::now();
    let loaded = match Patch::from_ron(text) {
        Ok(patch) => {
            let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
            let loaded = patch.load_into(&mut parts.controller).is_ok();
            if loaded {
                let (mut left, mut right) = ([0.0f32; 256], [0.0f32; 256]);
                for _ in 0..4 {
                    parts.engine.process(&mut left, &mut right);
                }
                assert!(
                    left.iter().chain(&right).all(|s| s.is_finite()),
                    "{what}: the output is finite"
                );
            }
            loaded
        }
        Err(_) => false,
    };
    assert!(started.elapsed() < LIMIT, "{what}: took {:?}", started.elapsed());
    loaded
}

/// The folder of a crate-relative path.
fn crate_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// The files in `folder` with extension `ron`, sorted.
fn ron_files(folder: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", folder.display()))
        .map(|entry| entry.expect("a folder entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "ron"))
        .collect();
    files.sort();
    files
}

/// Inputs that once broke the loader still give an error or a working
/// engine. Each file says in a comment what it found.
#[test]
fn saved_inputs_that_broke_the_loader_load_safely() {
    let files = ron_files(&crate_path("tests/malformed"));
    assert!(!files.is_empty(), "tests/malformed holds inputs");
    for file in files {
        let bytes = std::fs::read(&file).expect("a readable file");
        // Fuzz inputs may not be UTF-8; the fuzz target skips those too.
        if let Ok(text) = std::str::from_utf8(&bytes) {
            load_and_play(text, &file.display().to_string());
        }
    }
}

/// A patch with one module, made of the given module fields.
fn one_module(fields: &str) -> String {
    format!("Patch(version: 1, modules: [PatchModule({fields})])")
}

/// Edge cases a person thought of: numbers at their limits, IDs that
/// clash, references to nothing, very large and very deep input.
#[test]
fn edge_cases_load_safely() {
    let sine = "kind: \"loom.osc.sine@1\"";
    let mut cases: Vec<(String, bool)> = vec![
        (String::new(), false),
        ("Patch()".into(), false),
        ("Patch(version: 1)".into(), true),
        ("Patch(version: 4294967295)".into(), false),
        ("Patch(version: -1)".into(), false),
        (one_module(&format!("id: 0, {sine}")), true),
        (one_module(&format!("id: 4294967295, {sine}")), false),
        (one_module(&format!("id: 4294967296, {sine}")), false),
        (one_module(&format!("id: 1, {sine}, params: {{\"level\": 1.0}}")), false),
        (one_module("id: 1, kind: \"loom.osc.sine\""), false),
        (one_module("id: 1, kind: \"@1\""), false),
        (one_module("id: 1, kind: \"loom.osc.sine@99999999999\""), false),
        (one_module("id: 1, kind: \"loom.osc.sine@0\""), true),
        (
            one_module("id: 1, kind: \"loom.filter.svf@1\", params: {\"cutoff\": NaN, \"resonance\": inf}"),
            true,
        ),
        (
            one_module("id: 1, kind: \"loom.filter.svf@1\", params: {\"cutoff\": -inf, \"resonance\": 1e38}"),
            true,
        ),
        (one_module(&format!("id: 1, {sine}, position: Some((NaN, inf))")), true),
        (
            format!("Patch(version: 1, modules: [PatchModule(id: 1, {sine}), PatchModule(id: 1, {sine})])"),
            false,
        ),
        (
            format!(
                "Patch(version: 1, modules: [PatchModule(id: 1, {sine})], cables: [PatchCable(from: (1, \"out\"), to: (1, \"phase\"))])"
            ),
            true,
        ),
        (
            format!(
                "Patch(version: 1, modules: [PatchModule(id: 1, {sine})], cables: [PatchCable(from: (2, \"out\"), to: (1, \"phase\"))])"
            ),
            false,
        ),
        (
            "Patch(version: 1, modules: [PatchModule(id: 1, kind: \"loom.filter.svf@1\")], modulation: [PatchModulation(source: (1, \"lowpass\"), target: (1, \"cutoff\"), depth: NaN)])".to_string(),
            true,
        ),
        (
            "Patch(version: 1, modules: [PatchModule(id: 1, kind: \"loom.filter.svf@1\")], midi: [PatchMidiMapping(target: (1, \"cutoff\"), channel: 255, controller: 255)])".to_string(),
            false,
        ),
    ];
    // Many modules, more than the engine has slots for.
    let many: Vec<String> = (1..=2_000).map(|id| format!("PatchModule(id: {id}, {sine})")).collect();
    cases.push((format!("Patch(version: 1, modules: [{}])", many.join(", ")), false));
    // Deep nesting, which must not overflow the stack.
    let deep = 100_000;
    cases.push((
        format!("Patch(version: 1, name: {}{})", "[".repeat(deep), "]".repeat(deep)),
        false,
    ));
    cases.push((
        format!("{}Patch(version: 1){}", "(".repeat(deep), ")".repeat(deep)),
        false,
    ));
    // A very long string.
    cases.push((format!("Patch(version: 1, name: \"{}\")", "x".repeat(1_000_000)), true));
    // Only a check that the cases mean what they say: what matters is
    // that none of them panics.
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(text, should_load)| {
            let what = if text.len() > 200 { &text[..200] } else { text };
            let loaded = load_and_play(text, what);
            (loaded != *should_load).then(|| format!("{what}: loaded {loaded}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// A small seeded random number generator (xorshift64), so the mutations
/// are the same on every run and every platform (AGENTS.md hard rule 3).
struct Random(u64);

impl Random {
    /// A number below `n`, which must not be 0.
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// Tokens a mutation may put in: numbers at their limits, brackets, and
/// pieces of the format.
const TOKENS: &[&str] = &[
    "0",
    "-1",
    "4294967295",
    "4294967296",
    "18446744073709551616",
    "255",
    "256",
    "NaN",
    "inf",
    "-inf",
    "1e39",
    "1e-45",
    "(",
    ")",
    "[",
    "]",
    "{",
    "}",
    ",",
    ":",
    "\"",
    "Some(",
    "None",
    "true",
    "\"loom.time.delay1@1\"",
    "\"loom.io.audio-out@1\"",
    "PatchModule(id: 4294967295, kind: \"loom.osc.sine@1\"),",
    "PatchCable(from: (1, \"out\"), to: (1, \"in\")),",
];

/// One random change to `text`: cut it short, delete a range, copy a
/// range, put a token in, or replace a byte. The result may not be valid
/// UTF-8 at a cut; such changes are skipped by the caller.
fn mutate(text: &[u8], random: &mut Random) -> Vec<u8> {
    let mut out = text.to_vec();
    let len = out.len().max(1);
    let at = random.below(len);
    match random.below(5) {
        0 => out.truncate(at),
        1 => {
            let end = (at + random.below(64) + 1).min(out.len());
            out.drain(at.min(end)..end);
        }
        2 => {
            let end = (at + random.below(256) + 1).min(out.len());
            let copy = out[at.min(end)..end].to_vec();
            let to = random.below(len);
            out.splice(to..to, copy);
        }
        3 => {
            let token = TOKENS[random.below(TOKENS.len())];
            out.splice(at..at, token.bytes());
        }
        _ => {
            if let Some(byte) = out.get_mut(at) {
                *byte = b"0123456789()[]{},:\"-.e"[random.below(22)];
            }
        }
    }
    out
}

/// Random changes to every example patch give an error or a working
/// engine. Several changes are stacked, so a patch can break in more than
/// one place.
#[test]
fn changed_example_patches_load_safely() {
    let patches = ron_files(&crate_path("../../examples/patches"));
    assert!(!patches.is_empty(), "examples/patches holds patches");
    let mut random = Random(0x5EED_1234_ABCD_0042);
    let mut loaded = 0;
    for patch in &patches {
        let original = std::fs::read(patch).expect("a readable file");
        assert!(
            load_and_play(std::str::from_utf8(&original).expect("UTF-8"), "the example"),
            "{} loads",
            patch.display()
        );
        for i in 0..MUTATIONS {
            let mut text = original.clone();
            for _ in 0..=random.below(3) {
                text = mutate(&text, &mut random);
            }
            if let Ok(text) = std::str::from_utf8(&text) {
                let what = format!("{} change {i}", patch.display());
                loaded += usize::from(load_and_play(text, &what));
            }
        }
    }
    // Some changes still load, so the test also plays odd patches, not
    // only parse errors.
    assert!(loaded > 0, "some changed patches still load");
}

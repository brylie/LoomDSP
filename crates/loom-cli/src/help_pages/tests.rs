use super::*;
use loom_core::PortDefault;

/// The committed folder of help pages.
fn docs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/modules")
}

/// A fresh, empty folder for one test, in the system's temporary folder.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("loom-help-pages-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The pages in `docs/modules` match the descriptors. When this fails,
/// a descriptor changed and its page was not regenerated.
#[test]
fn committed_pages_are_current() {
    let problems = stale_pages(&loom_modules::registry(), &docs_dir()).unwrap();
    assert!(
        problems.is_empty(),
        "The module help pages in docs/modules do not match the module descriptors:\n  {}\n\
             Regenerate them with `{REGENERATE}` (or `cargo run -p loom-cli -- modules --markdown docs/modules`), \
             check the diff, and commit the pages.",
        problems.join("\n  ")
    );
}

/// Only a whole line counts as the marker. A page that quotes it inside
/// other text has no marker, so its hand-written text is never cut.
#[test]
fn the_marker_counts_only_as_a_whole_line() {
    assert_eq!(notes_of(&format!("# A\n{MARKER}\nnotes\n")).as_deref(), Some("notes\n"));
    assert_eq!(
        notes_of(&format!("# A\r\n{MARKER}\r\nnotes\r\n")).as_deref(),
        Some("notes\r\n")
    );
    assert_eq!(notes_of(&format!("# A\n{MARKER}")).as_deref(), Some(""));
    assert_eq!(
        notes_of(&format!("Keep the line `{MARKER}` when you edit.\nmine\n")),
        None
    );
    assert_eq!(notes_of("# A\nno marker\n"), None);
}

/// Notes below the marker survive regeneration, and a second run writes
/// nothing.
#[test]
fn regenerating_keeps_notes() {
    let registry = loom_modules::registry();
    let dir = temp_dir("notes");
    let first = write_pages(&registry, &dir).unwrap();
    assert_eq!(first.written.len(), registry.len() + 1);
    assert!(stale_pages(&registry, &dir).unwrap().is_empty());

    let d = registry.iter().next().unwrap();
    let path = dir.join(file_name(d));
    let notes = "## Notes\n\nPlay it slowly.\n";
    let text = std::fs::read_to_string(&path).unwrap();
    let edited = text.replacen(&format!("# {}", d.name), "# Old title", 1) + notes;
    std::fs::write(&path, edited).unwrap();
    assert_eq!(stale_pages(&registry, &dir).unwrap().len(), 1);

    let second = write_pages(&registry, &dir).unwrap();
    assert_eq!(second.written, vec![path.clone()]);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with(&format!("# {}\n", d.name)));
    assert!(text.ends_with(&format!("{MARKER}\n{notes}")));
    assert!(write_pages(&registry, &dir).unwrap().written.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A page without the marker is left alone, and pages of unknown modules
/// are reported, not deleted.
#[test]
fn unmarked_and_unknown_pages_are_kept() {
    let registry = loom_modules::registry();
    let dir = temp_dir("unknown");
    let d = registry.iter().next().unwrap();
    let page = dir.join(file_name(d));
    std::fs::write(&page, "My own page.\n").unwrap();
    let old = dir.join("loom.gone.module.md");
    std::fs::write(&old, "Old notes.\n").unwrap();

    let report = write_pages(&registry, &dir).unwrap();
    assert_eq!(report.no_marker, vec![page.clone()]);
    assert_eq!(report.unknown, vec![old.clone()]);
    assert_eq!(std::fs::read_to_string(&page).unwrap(), "My own page.\n");
    assert!(old.exists());
    let problems = stale_pages(&registry, &dir).unwrap();
    assert_eq!(problems.len(), 2, "{problems:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Windows line endings in a checkout do not make pages stale.
#[test]
fn crlf_pages_are_current() {
    let registry = loom_modules::registry();
    let dir = temp_dir("crlf");
    write_pages(&registry, &dir).unwrap();
    let path = dir.join(INDEX);
    let text = std::fs::read_to_string(&path).unwrap().replace('\n', "\r\n");
    std::fs::write(&path, text).unwrap();
    assert!(stale_pages(&registry, &dir).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A page lists every port and parameter with its label, name, range,
/// and unit.
#[test]
fn page_lists_ports_and_parameters() {
    let registry = loom_modules::registry();
    for d in registry.iter() {
        let text = page(d);
        assert!(text.starts_with(&format!("# {}\n", d.name)));
        for p in d.inputs.iter().chain(d.outputs) {
            assert!(text.contains(&port_name(p)), "{} {}", d.id, p.name);
        }
        for p in d.params {
            assert!(text.contains(&format!("| {} | {} |", name(p.label, p.name), cell(&p.range_text()))));
        }
    }
}

/// Helpers that turn descriptor values into words.
#[test]
fn values_read_as_words() {
    assert_eq!(cell("a | b"), "a \\| b");
    assert_eq!(loom_core::number_text(-0.0), "0");
    assert_eq!(loom_core::number_text(0.25), "0.25");
    assert_eq!(loom_core::number_text(20000.0), "20000");
    assert_eq!(PortDefault::BarPhase.text(), "bar phase");
    assert_eq!(PortDefault::BeatPhase.text(), "beat phase");
    assert_eq!(PortDefault::Playing.text(), "play gate");
    assert_eq!(PortDefault::Value(1.0).text(), "1");
    assert_eq!(name("", "x"), "`x`");
    assert_eq!(notes_of("no marker"), None);
    assert_eq!(notes_of(&format!("a\n{MARKER}\nb\n")).as_deref(), Some("b\n"));
    assert_eq!(
        extract_ron_snippet("```ron\nPatch()\n```\n").as_deref(),
        Some("Patch()\n")
    );
    assert_eq!(
        extract_ron_snippet("```ron\nPatch()\n// ```not a close\n```\n").as_deref(),
        Some("Patch()\n// ```not a close\n")
    );
    assert_eq!(extract_ron_snippet("no fence"), None);
}

/// Extracts a RON snippet enclosed in ```ron ... ``` from markdown notes.
fn extract_ron_snippet(notes: &str) -> Option<String> {
    let normalized = notes.replace("\r\n", "\n");
    let start_tag = "```ron\n";
    let start = normalized.find(start_tag)?;
    let rest = &normalized[start + start_tag.len()..];
    let mut len = 0;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches('\n').trim_end();
        if trimmed.len() >= 3 && trimmed.chars().all(|c| c == '`') {
            return Some(rest[..len].to_string());
        }
        len += line.len();
    }
    None
}

/// Every built-in module has an example patch in its help page (FR-LIB-11).
/// The patch must parse, contain the module, and compile cleanly into a schedule.
#[test]
fn example_patches_in_pages_are_valid() {
    let registry = loom_modules::registry();
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/modules"));
    for d in registry.iter() {
        let path = dir.join(file_name(d));
        let text = read_normalized(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
            .unwrap_or_else(|| panic!("{} is missing", path.display()));
        let notes = notes_of(&text).unwrap_or_else(|| panic!("{} has no marker line", path.display()));
        let ron_snippet = extract_ron_snippet(&notes)
            .unwrap_or_else(|| panic!("{} has no ```ron example patch in its notes", path.display()));
        let patch = loom_patch::Patch::from_ron(&ron_snippet)
            .unwrap_or_else(|e| panic!("{} example patch failed to parse: {e}", path.display()));
        let expected_prefix = format!("{}@", d.id);
        let has_module = patch
            .modules
            .iter()
            .any(|m| m.kind.starts_with(&expected_prefix) || m.kind == d.id);
        assert!(
            has_module,
            "{} example patch does not contain module {}",
            path.display(),
            d.id
        );
        let mut parts = loom_core::create_engine(registry.clone(), loom_core::EngineConfig::default());
        patch
            .load_into(&mut parts.controller)
            .unwrap_or_else(|e| panic!("{} example patch failed to load into controller: {e}", path.display()));
    }
}

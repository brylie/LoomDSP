use super::*;

/// Longest stats line, in columns, that `format_stats` may give for any
/// realistic value. Wider lines wrap in a narrow terminal panel.
const STATS_MAX_COLUMNS: usize = 60;

/// The stats line fits in a narrow terminal even for extreme values, so
/// it never wraps and the in-place update stays readable.
#[test]
fn stats_line_fits_narrow_terminal() {
    let worst = StatsSnapshot {
        last_load: 1.0,
        max_load: 9.999,
        smallest_buffer: 16,
        largest_buffer: 16384,
        overruns: 99_999,
        stream_errors: 99_999,
        ..Default::default()
    };
    let line = format_stats(&worst);
    assert!(line.chars().count() <= STATS_MAX_COLUMNS, "{line}");
    let typical = format_stats(&StatsSnapshot::default());
    assert!(typical.chars().count() <= 50, "{typical}");
}

/// The flags choose the plan: `--note` first, then `--no-midi`, then
/// `--midi`, and every port by default. Each plan opens what it says.
#[test]
fn midi_plans_open_what_the_flags_ask() {
    assert_eq!(MidiPlan::new(Some(60), true, Some("x")), MidiPlan::Hold(60));
    assert_eq!(MidiPlan::new(None, true, Some("x")), MidiPlan::Off);
    assert_eq!(MidiPlan::new(None, false, Some("key")), MidiPlan::Named("key"));
    assert_eq!(MidiPlan::new(None, false, None), MidiPlan::Every);

    fn name(c: &String) -> &str {
        c
    }
    let one = |wanted: &str| Ok(Some(format!("{wanted} 88")));
    let none = |_: &str| -> Result<Option<String>> { Err("no port matches".into()) };
    let every = || {
        Ok((
            vec!["Keys".to_string(), "Pad".to_string()],
            vec!["Drums: busy".to_string()],
        ))
    };
    let never = || -> Result<(Vec<String>, Vec<String>)> { panic!("not asked for") };

    let (open, lines) = open_midi(MidiPlan::Every, none, every, name).unwrap();
    assert_eq!(open, ["Keys", "Pad"]);
    assert_eq!(lines.len(), 3, "{lines:?}");
    let (open, lines) = open_midi(MidiPlan::Named("Key"), one, never, name).unwrap();
    assert_eq!(open, ["Key 88"]);
    assert_eq!(lines, ["midi:   Key 88"]);
    assert!(open_midi(MidiPlan::Named("x"), none, never, name).is_err());
    let (open, lines) = open_midi(MidiPlan::Off, none, never, name).unwrap();
    assert!(open.is_empty());
    assert_eq!(lines, ["midi:   off"]);
    let (_, lines) = open_midi(MidiPlan::Hold(60), none, never, name).unwrap();
    assert!(lines[0].contains("holding note 60"), "{lines:?}");
}

/// Every opened MIDI port gets its own line, a port that failed is
/// reported after them, and the hint shows only when nothing opened.
#[test]
fn midi_lines_list_every_port_and_each_failure() {
    let lines = midi_lines(&["Keys", "Pad"], &["Drums: busy".to_string()]);
    assert_eq!(
        lines,
        ["midi:   Keys", "midi:   Pad", "midi:   could not open Drums: busy"]
    );
    let none = midi_lines(&[], &[]);
    assert_eq!(none.len(), 1);
    assert!(none[0].contains("no MIDI input found"), "{none:?}");
}

/// The play line fits in 50 columns for ordinary audio numbers.
#[test]
fn play_line_fits_50_columns_for_ordinary_values() {
    let ordinary = StatsSnapshot {
        last_load: 0.999,
        max_load: 0.999,
        smallest_buffer: 4096,
        largest_buffer: 4096,
        ..Default::default()
    };
    let line = format_stats(&ordinary);
    assert!(line.chars().count() <= 50, "{line}");
}

/// The whole stress line fits in one row for an ordinary run: up to 99 s,
/// 9,999 edits, and usual audio numbers. Extreme values may still wrap.
#[test]
fn stress_line_fits_narrow_terminal() {
    let ordinary = StatsSnapshot {
        last_load: 0.999,
        max_load: 0.999,
        smallest_buffer: 4096,
        largest_buffer: 4096,
        ..Default::default()
    };
    let line = format_stress_line(99.0, 9_999, &ordinary);
    assert!(line.chars().count() <= 50, "{line}");
}

/// Verbose output shows each label and description, and no line is wider
/// than the narrow-terminal limit.
#[test]
fn verbose_modules_show_text_within_columns() {
    let mut out = Vec::new();
    write_modules(&mut out, &loom_modules::registry(), true).unwrap();
    let text = String::from_utf8(out).unwrap();
    let d = loom_modules::registry().iter().find(|d| !d.inputs.is_empty()).unwrap();
    let p = &d.inputs[0];
    assert!(text.contains(&format!("{}: ", p.label)), "{}", p.label);
    let word = p.description.split_whitespace().next().unwrap();
    assert!(text.contains(word));
    // Module summary lines are long by design; only port text is held to the limit.
    for line in text.lines().filter(|l| l.starts_with(' ')) {
        assert!(line.chars().count() <= MODULES_MAX_COLUMNS, "{line}");
    }
}

/// Without `--verbose`, only the module lines are written.
#[test]
fn plain_modules_have_no_port_lines() {
    let mut out = Vec::new();
    write_modules(&mut out, &loom_modules::registry(), false).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.lines().all(|l| !l.starts_with(' ')));
}

/// Long text wraps onto several indented lines.
#[test]
fn long_text_wraps() {
    let mut out = Vec::new();
    write_text(&mut out, "Label", &"word ".repeat(30)).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.lines().count() > 1);
    assert!(
        text.lines()
            .all(|l| l.starts_with("        ") && l.chars().count() <= MODULES_MAX_COLUMNS)
    );
}

/// A single word wider than the line is split, not left to wrap.
#[test]
fn oversized_word_is_split() {
    let mut out = Vec::new();
    write_text(&mut out, "", &"x".repeat(120)).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.lines().count() >= 3);
    assert!(text.lines().all(|l| l.chars().count() <= MODULES_MAX_COLUMNS));
}

/// Registry notes print under `module notes:`; none prints nothing.
#[test]
fn notes_print_only_when_present() {
    use loom_core::{Channels, ModuleDescriptor, ModuleRole, Panel, PortDefault, PortSpec, SignalType};
    static QUIET: ModuleDescriptor = ModuleDescriptor {
        id: "test.quiet",
        version: 1,
        name: "Quiet",
        category: "Test",
        family: loom_core::Family::Interface,
        description: "",
        inputs: &[PortSpec {
            name: "in",
            label: "In",
            signal: SignalType::Audio,
            description: "",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        }],
        outputs: &[PortSpec {
            name: "out",
            label: "Out",
            signal: SignalType::Audio,
            description: "Output.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        }],
        params: &[],
        role: ModuleRole::UnitDelay,
        panel: Panel::EMPTY,
        displays: &[],
    };
    let mut out = Vec::new();
    write_notes(&mut out, &Registry::new()).unwrap();
    assert!(out.is_empty());
    let mut registry = Registry::new();
    registry.register(&QUIET).unwrap();
    write_notes(&mut out, &registry).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "  module notes:\n    test.quiet: input `in` has no description\n"
    );
}

/// `--markdown` writes the pages, `--check` then passes, and fails once
/// a page is out of date or for an unknown module.
#[test]
fn markdown_writes_then_checks() {
    let dir = std::env::temp_dir().join(format!("loom-cli-markdown-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(markdown(&dir, true).unwrap(), ExitCode::FAILURE);
    assert_eq!(markdown(&dir, false).unwrap(), ExitCode::SUCCESS);
    assert_eq!(markdown(&dir, true).unwrap(), ExitCode::SUCCESS);
    assert_eq!(markdown(&dir, false).unwrap(), ExitCode::SUCCESS);
    std::fs::write(dir.join("loom.gone.module.md"), "Old notes.\n").unwrap();
    assert_eq!(markdown(&dir, false).unwrap(), ExitCode::FAILURE);
    assert_eq!(markdown(&dir, true).unwrap(), ExitCode::FAILURE);
    let _ = std::fs::remove_dir_all(&dir);
}

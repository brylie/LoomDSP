use super::*;
use loom_patch::{FlatIds, SubPatch};

/// The example patch's path for a built-in.
fn example(builtin: &Builtin) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/patches")
        .join(format!("{}.ron", builtin.stem))
}

/// Every built-in reads, flattens, carries its credit and a description,
/// and holds no linked definition, so it never follows a link into the
/// player's library folder (sub-patches.md, Built-in abstractions).
#[test]
fn every_builtin_reads_flattens_and_has_no_links() {
    let registry = loom_modules::registry();
    for builtin in ALL {
        let patch = builtin.patch();
        assert_eq!(patch.metadata.name, builtin.name, "{}", builtin.stem);
        assert_eq!(patch.metadata.license.as_deref(), Some("CC0-1.0"));
        assert_eq!(patch.metadata.authors, ["LoomDSP contributors"]);
        assert!(!builtin.description().is_empty());
        assert!(!patch.has_links(), "{} has a linked definition", builtin.stem);
        assert!(patch.roles.is_empty(), "a built-in declares no role yet");
        patch
            .flatten(&registry, &mut FlatIds::new())
            .unwrap_or_else(|e| panic!("{} flattens: {e}", builtin.stem));
    }
}

/// Each example patch carries its built-in's definition, so the two cannot
/// drift apart.
#[test]
fn each_example_patch_holds_its_builtins_definition() {
    let registry = loom_modules::registry();
    for builtin in ALL {
        let text = std::fs::read_to_string(example(builtin)).expect("the example patch");
        let example = loom_patch::Patch::from_ron(&text).expect("it reads");
        let (definition, _) = SubPatch::from_patch(&builtin.patch(), &registry, None);
        let key = builtin.name.to_lowercase();
        assert_eq!(example.subpatches.get(&key), Some(&definition), "{}", builtin.stem);
    }
}

/// The Synth voice shows its planned surface, in order, with the Filter
/// slot's cutoff and resonance first.
#[test]
fn the_synth_voice_exposes_its_surface_in_order() {
    let patch = ALL[0].patch();
    let names: Vec<&str> = patch.exposed.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "cutoff",
            "resonance",
            "env amount",
            "amp attack",
            "amp decay",
            "amp sustain",
            "amp release",
            "fine tune",
            "level"
        ]
    );
    let patch = ALL[1].patch();
    let names: Vec<&str> = patch.exposed.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["effect 1 mix", "effect 2 mix", "effect 3 mix"]);
}

/// The browser lists the built-ins first, marked `Built in`, with their own
/// description and no menu; Enter on the first inserts the Synth voice as
/// a copy, selected, whose tile menu offers `Swap Filter to Ladder` and
/// leaves out Dissolve sub-patch while it holds slots (sub-patches.md,
/// Built-in abstractions).
#[test]
fn the_browser_inserts_a_builtin_as_a_copy() {
    use crate::{Message, browser};
    let mut ui = crate::ui_test::Ui::open(r#"Patch(version: 1, metadata: Metadata(name: "Empty"))"#);
    ui.send(Message::OpenBrowser);
    ui.redraw();
    let model = ui
        .app()
        .overlays
        .browser()
        .expect("the browser is open")
        .model()
        .clone();
    let first: Vec<(&str, &str, bool)> = model
        .rows
        .iter()
        .take(2)
        .map(|r| (r.name.as_str(), r.mark, r.library))
        .collect();
    assert_eq!(
        first,
        [("Synth voice", "Built in", false), ("Effect chain", "Built in", false)]
    );
    assert_eq!(
        model.rows[0].id,
        browser::browser_id().with("builtin").with("synth-voice")
    );
    assert_eq!(model.rows[0].description, ALL[0].description());
    let about = model.about.expect("the first row is highlighted");
    assert_eq!(about.detail, "Built in, inserted as a copy");
    ui.send(Message::Browser(browser::Message::Submit));
    ui.redraw();
    assert_eq!(ui.app().alerts.status(), "Inserted Synth voice");
    let tile = *ui
        .app()
        .session
        .selection
        .iter()
        .next()
        .expect("the new tile is selected");
    let hierarchy = &ui.app().session.hierarchy;
    assert_eq!(
        hierarchy.library_of(loom_core::ModuleId(tile)),
        None,
        "a copy, not linked"
    );
    let items: Vec<String> = ui.app().tile_menu_items(tile).into_iter().map(|(l, _)| l).collect();
    assert!(items.contains(&"Swap Filter to Ladder".to_string()), "{items:?}");
    assert!(!items.contains(&"Dissolve sub-patch".to_string()), "{items:?}");
    assert!(!items.iter().any(|l| l.contains("Remove part")), "{items:?}");
    // A search by the mark finds both.
    ui.send(Message::OpenBrowser);
    ui.redraw();
    ui.send(Message::Browser(browser::Message::Query("built in".into())));
    let names: Vec<String> = ui
        .app()
        .overlays
        .browser()
        .unwrap()
        .model()
        .rows
        .iter()
        .map(|r| r.name.clone())
        .collect();
    assert_eq!(names, ["Synth voice", "Effect chain"]);
}

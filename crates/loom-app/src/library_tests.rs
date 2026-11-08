//! The library and linked sub-patches, as the app meets them
//! (`docs/design/sub-patches.md`, Library, Linked library items, Copies
//! and editing; acceptance criteria under Copies, library, and linking).
//! Every test works in a temporary library folder of its own, on any
//! machine.

use crate::files::fake::{self, Asked};
use crate::recovery::tests::TempDir;
use crate::ui_test::Ui;
use crate::{App, Loaded, Message, Source, browser};
use loom_core::{EngineConfig, ModuleId, ModuleNames};
use loom_patch::{ModulePath, Patch};
use std::path::{Path, PathBuf};

/// A library item: a pitch port into a sine through a multiply whose gain
/// is exposed, out through a port.
const VOICE_FILE: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Voice", authors: ["Ada"]),
    modules: [
        PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("pitch"), position: Some((0.0, 0.0))),
        PatchModule(id: 2, kind: "loom.phase.generator@1", position: Some((200.0, 0.0))),
        PatchModule(id: 3, kind: "loom.osc.sine@1", position: Some((400.0, 0.0))),
        PatchModule(id: 4, kind: "loom.math.mul@1", params: {"gain": 0.5}, position: Some((600.0, 0.0))),
        PatchModule(id: 5, kind: "loom.struct.output@1", label: Some("out"), position: Some((800.0, 0.0))),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "pitch")),
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "out"), to: (4, "a")),
        PatchCable(from: (4, "out"), to: (5, "in")),
    ],
    exposed: [Exposed(name: "gain", module: 4, param: "gain")],
)"#;

/// [`VOICE_FILE`] with a Transport driving the sine and a Scale and tuning
/// module feeding the phase generator, as a file saved from a top-level
/// patch would have them.
const ARP_FILE: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Arp"),
    modules: [
        PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("pitch"), position: Some((0.0, 0.0))),
        PatchModule(id: 2, kind: "loom.phase.generator@1", position: Some((200.0, 0.0))),
        PatchModule(id: 3, kind: "loom.osc.sine@1", position: Some((400.0, 0.0))),
        PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out"), position: Some((600.0, 0.0))),
        PatchModule(id: 5, kind: "loom.clock.transport@1", position: Some((0.0, 200.0))),
        PatchModule(id: 6, kind: "loom.pitch.scale@1", position: Some((0.0, 300.0))),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "pitch")),
        PatchCable(from: (5, "beat-phase"), to: (3, "phase")),
        PatchCable(from: (6, "pitch"), to: (2, "reset")),
        PatchCable(from: (3, "out"), to: (4, "in")),
    ],
)"#;

/// A flat host patch: a note, the audio output, and the Transport.
const HOST: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Host"),
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1", position: Some((0.0, 0.0))),
        PatchModule(id: 2, kind: "loom.io.audio-out@1", position: Some((600.0, 0.0))),
        PatchModule(id: 3, kind: "loom.clock.transport@1", position: Some((0.0, 300.0))),
    ],
)"#;

/// The app with `text` open and no audio, reading linked sub-patches from
/// `library`.
fn app_with(text: &str, library: &Path) -> App {
    let patch = Patch::from_ron(text).unwrap();
    let loaded = Loaded::read(
        EngineConfig::default(),
        false,
        Source::Recovered(Box::new(patch)),
        &crate::patch_file::FolderLibrary {
            root: library.to_path_buf(),
        },
    )
    .unwrap();
    let mut app = App::assemble(
        Err("test".to_string()),
        48_000.0,
        loaded,
        1,
        &crate::tests::app_args(),
        crate::settings::Rig::empty(),
        false,
    );
    use_library(&mut app, library);
    app
}

/// Points the app's library preference at `dir`.
fn use_library(app: &mut App, dir: &Path) {
    let mut preferences = app.rig.store.saved().clone();
    preferences.library = Some(dir.to_path_buf());
    // With no preferences file the save fails, but the value is kept for
    // the run.
    let _ = app.rig.store.save(preferences);
}

/// Writes `text` at `relative` under `dir`, making the folders.
fn write(dir: &Path, relative: &str, text: &str) -> PathBuf {
    let path = dir.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    path
}

/// A temporary library folder with `Voices/Voice.ron` in it.
fn library() -> TempDir {
    let dir = TempDir::new("library");
    write(&dir.0, "Voices/Voice.ron", VOICE_FILE);
    dir
}

/// Opens the module browser, which reads the library folder.
fn open_browser(ui: &mut Ui) {
    ui.send(Message::OpenBrowser);
    ui.redraw();
    assert!(ui.app.overlays.browser().is_some(), "the browser is open");
}

/// Inserts the library item called `name` from the browser, as a copy or
/// linked, and returns the new tile's id.
fn insert(ui: &mut Ui, name: &str, linked: bool) -> u32 {
    open_browser(ui);
    let row = ui
        .app
        .overlays
        .browser()
        .unwrap()
        .library_rows()
        .into_iter()
        .find(|(_, item)| item.name == name)
        .map(|(row, _)| row)
        .unwrap_or_else(|| panic!("{name} is in the library list"));
    if linked {
        ui.send(Message::Browser(browser::Message::Menu(row)));
        ui.send(Message::Browser(browser::Message::InsertLinked(row)));
    } else {
        ui.send(Message::Browser(browser::Message::Pick(row)));
    }
    ui.redraw();
    assert!(ui.app.overlays.browser().is_none(), "the browser closed");
    let mut selected = ui.app.session.selection.iter();
    let (Some(&id), None) = (selected.next(), selected.next()) else {
        panic!("the new tile is selected: {}", ui.app.ui_snapshot().status);
    };
    id
}

/// The live gain of the multiply inside the sub-patch tile `instance`.
fn inner_gain(app: &App, instance: u32) -> f32 {
    let inside = app.session.hierarchy.path_of(ModuleId(instance));
    let mul = app.session.hierarchy.id_at(&inside, 4).expect("the multiply");
    let node = app.session.controller.graph().node(mul).unwrap();
    let gain = node.parameter_index("gain").unwrap();
    node.descriptor.saved_values(&node.params)[gain]
}

/// The live tune of the phase generator inside the sub-patch tile
/// `instance`: a parameter the surface does not expose.
fn inner_tune(app: &App, instance: u32) -> f32 {
    let inside = app.session.hierarchy.path_of(ModuleId(instance));
    let generator = app.session.hierarchy.id_at(&inside, 2).expect("the phase generator");
    let node = app.session.controller.graph().node(generator).unwrap();
    let tune = node.parameter_index("tune").unwrap();
    node.descriptor.saved_values(&node.params)[tune]
}

/// Sets the tune of the phase generator inside `instance` from inside, as
/// an edit: an edit inside the sub-patch, since the tune is not exposed.
fn set_inner_tune(app: &mut App, instance: u32, value: f32) {
    let inside = app.session.hierarchy.path_of(ModuleId(instance));
    let generator = app.session.hierarchy.id_at(&inside, 2).expect("the phase generator");
    let tune = app
        .session
        .controller
        .graph()
        .node(generator)
        .unwrap()
        .parameter_index("tune")
        .unwrap();
    app.show_level(&inside);
    app.edit(|app| {
        app.set_param(generator.0, tune, value);
        app.commit();
    });
}

/// The module browser lists the library folder's patch files next to the
/// modules, marked as sub-patches; a right-click on one offers Insert
/// linked (sub-patches.md, Library).
#[test]
fn the_browser_lists_library_items_marked_as_sub_patches() {
    let dir = library();
    write(&dir.0, "Arp.ron", ARP_FILE);
    let mut ui = Ui::with_app(app_with(HOST, &dir.0));
    open_browser(&mut ui);
    let rows: Vec<(usize, String)> = ui
        .app
        .overlays
        .browser()
        .unwrap()
        .library_rows()
        .into_iter()
        .map(|(row, item)| (row, item.library.clone()))
        .collect();
    let modules = ui.app.overlays.browser().unwrap().results().len();
    assert_eq!(
        rows,
        [
            (modules, "Arp.ron".to_string()),
            (modules + 1, "Voices/Voice.ron".to_string())
        ],
        "after the modules, by path"
    );
    // A search finds a sub-patch by name, and the mark names what it is.
    ui.send(Message::Browser(browser::Message::Query("voice".into())));
    ui.redraw();
    let browser = ui.app.overlays.browser().unwrap();
    assert_eq!(browser.library_rows().len(), 1);
    assert!(ui.text("Voice").is_some(), "the item's name");
    assert!(ui.text(browser::LIBRARY_MARK).is_some(), "marked as a sub-patch");
    assert!(ui.text("Voices/Voice.ron").is_some(), "the highlighted item's path");
    // The item's menu holds Insert linked.
    let row = ui.app.overlays.browser().unwrap().library_rows()[0].0;
    assert!(ui.text(browser::INSERT_LINKED).is_none());
    ui.send(Message::Browser(browser::Message::Menu(row)));
    ui.redraw();
    assert!(ui.text(browser::INSERT_LINKED).is_some(), "the menu opened");
}

/// Adding a library item inserts it as a copy: a tile with the file's
/// ports and surface, its inside placed as the file had it, no link, and
/// the patch plays through it. Undo takes it away whole.
#[test]
fn a_library_item_is_inserted_as_a_copy() {
    let dir = library();
    let mut ui = Ui::with_app(app_with(HOST, &dir.0));
    let voice = insert(&mut ui, "Voice", false);
    let app = &ui.app;
    assert_eq!(app.ui_snapshot().status, "Inserted Voice");
    assert_eq!(app.ui_snapshot().undo.as_deref(), Some("insert Voice"));
    assert_eq!(
        app.session.hierarchy.tile_name(ModuleId(voice)).as_deref(),
        Some("Voice")
    );
    assert_eq!(app.session.hierarchy.library_of(ModuleId(voice)), None);
    let tile = app
        .ui_snapshot()
        .tiles
        .into_iter()
        .find(|t| t.id == voice)
        .expect("the tile is on the canvas");
    assert_eq!(
        tile.inputs.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(),
        ["pitch"]
    );
    assert_eq!(
        tile.outputs.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(),
        ["out"]
    );
    assert_eq!(tile.widgets.len(), 1, "the exposed gain on the surface");
    assert_eq!(app.session.controller.graph().nodes().count(), 6);
    let inside = app.session.hierarchy.path_of(ModuleId(voice));
    let mul = app.session.hierarchy.id_at(&inside, 4).unwrap();
    assert_eq!(
        app.session.positions.get(&mul.0),
        Some(&(600.0, 0.0)),
        "placed as the file"
    );
    assert_eq!(inner_gain(app, voice), 0.5);
    let saved = app.current_patch();
    assert_eq!(saved.subpatches.len(), 1);
    assert_eq!(saved.subpatches["voice"].metadata.authors, ["Ada"], "credit travels");
    assert_eq!(saved.subpatches["voice"].library, None);

    ui.app.undo();
    assert!(ui.app.session.hierarchy.is_flat());
    assert_eq!(ui.app.session.controller.graph().nodes().count(), 3);
}

/// Inserting a file that has a Transport and a Scale and tuning module
/// drops both, turns their cables into ports on the tile, and says so in
/// the status bar; the patch still has its one Transport (sub-patches.md,
/// Library; rule 8).
#[test]
fn inserting_a_file_with_patch_wide_modules_leaves_them_out_with_a_note() {
    let dir = library();
    write(&dir.0, "Arp.ron", ARP_FILE);
    let mut ui = Ui::with_app(app_with(HOST, &dir.0));
    let arp = insert(&mut ui, "Arp", false);
    let app = &ui.app;
    assert_eq!(
        app.ui_snapshot().status,
        "Inserted Arp. Arp: its Transport was left out; connect its clock ports.; Arp: its Scale and tuning was left out; connect its pitch ports."
    );
    let tile = app.ui_snapshot().tiles.into_iter().find(|t| t.id == arp).unwrap();
    assert_eq!(
        tile.inputs.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(),
        ["pitch", "Phase", "Reset"],
        "the file's port, then one per dropped cable"
    );
    let graph = app.session.controller.graph();
    assert_eq!(
        graph
            .nodes()
            .filter(|(_, n)| n.descriptor.id == loom_core::TRANSPORT_MODULE)
            .count(),
        1
    );
    assert!(
        graph.nodes().all(|(_, n)| n.unknown.is_none()),
        "no placeholder for the scale module inside"
    );
    assert!(app.current_patch().subpatches["arp"].library.is_none());
}

/// Two linked instances of one library file have separate definitions in
/// the saved patch, each following the file (sub-patches.md, Copies and
/// editing).
#[test]
fn two_linked_instances_have_separate_definitions() {
    let dir = library();
    let mut ui = Ui::with_app(app_with(HOST, &dir.0));
    let first = insert(&mut ui, "Voice", true);
    let second = insert(&mut ui, "Voice", true);
    assert_ne!(first, second);
    let app = &ui.app;
    assert_eq!(
        app.session.hierarchy.library_of(ModuleId(first)),
        Some("Voices/Voice.ron")
    );
    assert_eq!(
        app.session.hierarchy.library_of(ModuleId(second)),
        Some("Voices/Voice.ron")
    );
    let saved = app.current_patch();
    assert_eq!(saved.subpatches.len(), 2);
    assert!(
        saved
            .subpatches
            .values()
            .all(|d| d.library.as_deref() == Some("Voices/Voice.ron"))
    );
    let keys: Vec<Option<&str>> = saved
        .modules
        .iter()
        .filter(|m| m.is_instance())
        .map(|m| m.subpatch.as_deref())
        .collect();
    assert_eq!(keys, [Some("voice"), Some("voice 2")]);
    // The tile menu offers Unlink on a linked tile.
    let labels: Vec<&str> = app.tile_menu_items(first).into_iter().map(|(l, _)| l).collect();
    assert_eq!(labels, ["Open", "Dissolve sub-patch", "Save to library", "Unlink"]);
}

/// Editing inside one linked instance unlinks it, as part of the edit's
/// step, and leaves the other unchanged and linked; changing an exposed
/// value keeps the link; Unlink on the tile menu is a step of its own
/// (sub-patches.md, Copies and editing).
#[test]
fn editing_inside_a_linked_instance_unlinks_it_and_leaves_the_other() {
    let dir = library();
    let mut ui = Ui::with_app(app_with(HOST, &dir.0));
    let first = insert(&mut ui, "Voice", true);
    let second = insert(&mut ui, "Voice", true);
    let app = &mut ui.app;
    // An exposed value is the instance's: still linked.
    let target = app.session.hierarchy.exposed_target(ModuleId(first), 0).unwrap();
    let gain = app
        .session
        .controller
        .graph()
        .node(target.module)
        .unwrap()
        .parameter_index(&target.param)
        .unwrap();
    app.edit(|app| {
        app.set_param(target.module.0, gain, 0.25);
        app.commit();
    });
    assert_eq!(inner_gain(app, first), 0.25);
    assert_eq!(
        app.session.hierarchy.library_of(ModuleId(first)),
        Some("Voices/Voice.ron"),
        "an exposed value is not an edit inside"
    );
    // A value that is not exposed is an edit inside.
    let inside = app.session.hierarchy.path_of(ModuleId(first));
    let generator = app.session.hierarchy.id_at(&inside, 2).unwrap();
    app.show_level(&inside);
    app.edit(|app| {
        app.set_param(generator.0, 0, 0.1);
        app.commit();
    });
    assert_eq!(
        app.ui_snapshot().status,
        "Voice is now a copy. It no longer follows the library."
    );
    assert_eq!(app.session.hierarchy.library_of(ModuleId(first)), None);
    assert_eq!(
        app.session.hierarchy.library_of(ModuleId(second)),
        Some("Voices/Voice.ron"),
        "the other tile is untouched"
    );
    assert_eq!(inner_gain(app, second), 0.5);
    // One undo step brings the value and the link back.
    app.undo();
    assert_eq!(
        app.session.hierarchy.library_of(ModuleId(first)),
        Some("Voices/Voice.ron")
    );
    app.redo();
    assert_eq!(app.session.hierarchy.library_of(ModuleId(first)), None);

    // Unlink, from the tile menu, on the one still linked.
    app.show_level(&ModulePath::root());
    let _ = app.update(Message::Unlink(second));
    assert_eq!(
        app.ui_snapshot().status,
        "Voice is now a copy. It no longer follows the library."
    );
    assert_eq!(app.ui_snapshot().undo.as_deref(), Some("unlink Voice"));
    assert_eq!(app.session.hierarchy.library_of(ModuleId(second)), None);
    app.undo();
    assert_eq!(
        app.session.hierarchy.library_of(ModuleId(second)),
        Some("Voices/Voice.ron")
    );
    let saved = app.current_patch();
    assert_eq!(saved.subpatches["voice"].library, None);
    assert_eq!(saved.subpatches["voice 2"].library.as_deref(), Some("Voices/Voice.ron"));
}

/// Save to library from one instance writes the file and changes no other
/// tile in the open patch; reloading the patch updates the linked one,
/// keeps its exposed value by name, and the status bar names it
/// (sub-patches.md, Linked library items).
#[test]
fn save_to_library_leaves_the_other_instance_until_the_patch_reloads() {
    let dir = library();
    let mut ui = Ui::with_app(app_with(HOST, &dir.0));
    let first = insert(&mut ui, "Voice", true);
    let second = insert(&mut ui, "Voice", true);
    let app = &mut ui.app;
    set_inner_tune(app, first, 0.1);
    assert_eq!(app.session.hierarchy.library_of(ModuleId(first)), None);
    // The second keeps an exposed value of its own.
    let target = app.session.hierarchy.exposed_target(ModuleId(second), 0).unwrap();
    app.edit(|app| {
        app.set_param(target.module.0, 0, 0.25);
        app.commit();
    });
    let _ = fake::asked();
    // The copy asks where; the dialog starts in the library folder under
    // the sub-patch's name. Saving over the linked file is the point here.
    let voice_file = dir.0.join("Voices").join("Voice.ron");
    fake::answer_save(Some(voice_file.clone()));
    app.show_level(&ModulePath::root());
    let step = app.ui_snapshot().undo;
    let _ = app.update(Message::SaveToLibrary(first));
    assert_eq!(
        fake::asked(),
        [Asked::Save(Some(dir.0.join("Voice.ron")))],
        "a free name in the library folder"
    );
    assert_eq!(
        app.ui_snapshot().status,
        format!("saved Voice to {}", voice_file.display())
    );
    assert_eq!(app.ui_snapshot().undo, step, "writing a library file is not an edit");
    let written = Patch::from_ron(&std::fs::read_to_string(&voice_file).unwrap()).unwrap();
    assert_eq!(written.metadata.name, "Voice");
    assert_eq!(written.modules.iter().find(|m| m.id == 2).unwrap().params["tune"], 0.1);
    assert_eq!(written.exposed.len(), 1);
    assert_eq!(inner_gain(app, second), 0.25, "the other tile did not change");
    assert_ne!(inner_tune(app, second), 0.1);
    assert_eq!(
        app.session.hierarchy.library_of(ModuleId(second)),
        Some("Voices/Voice.ron")
    );

    // Saved and opened again, the linked tile takes the new file.
    let patch_file = dir.0.join("set.ron");
    crate::patch_file::save(&app.current_patch(), &patch_file).unwrap();
    assert!(app.open_path(&patch_file));
    assert_eq!(
        app.ui_snapshot().status,
        format!("opened {}: Updated from the library: Voice.", patch_file.display())
    );
    let reloaded = app.current_patch();
    let second_key = reloaded
        .modules
        .iter()
        .find(|m| m.id == second)
        .and_then(|m| m.subpatch.clone())
        .unwrap();
    assert_eq!(
        reloaded.subpatches[&second_key]
            .modules
            .iter()
            .find(|m| m.id == 2)
            .unwrap()
            .params["tune"],
        0.1,
        "the stored copy is the file's now"
    );
    assert_eq!(inner_tune(app, second), 0.1, "the linked tile plays the new file");
    assert_eq!(inner_gain(app, second), 0.25, "the exposed value is kept by name");
    assert_eq!(inner_tune(app, first), 0.1, "the unlinked copy keeps its own");
}

/// A missing library file loads the stored copy, with a status message
/// (sub-patches.md, rule 5); the linked instance still plays.
#[test]
fn a_missing_library_file_loads_the_stored_copy_with_a_status() {
    let dir = library();
    let mut ui = Ui::with_app(app_with(HOST, &dir.0));
    let voice = insert(&mut ui, "Voice", true);
    let app = &mut ui.app;
    let patch_file = dir.0.join("set.ron");
    crate::patch_file::save(&app.current_patch(), &patch_file).unwrap();
    std::fs::remove_file(dir.0.join("Voices").join("Voice.ron")).unwrap();
    assert!(app.open_path(&patch_file));
    assert_eq!(
        app.ui_snapshot().status,
        format!(
            "opened {}: Voice: the library file \"Voices/Voice.ron\" is missing, so its stored copy is used.",
            patch_file.display()
        )
    );
    assert_eq!(
        app.session.hierarchy.library_of(ModuleId(voice)),
        Some("Voices/Voice.ron"),
        "still linked, for when the file comes back"
    );
    assert_eq!(inner_gain(app, voice), 0.5);
    assert_eq!(app.session.controller.graph().nodes().count(), 6);
}

/// A chain of library links that comes back on itself leaves the stored
/// copy when the patch loads, with a note naming the loop, and a link that
/// would make a file contain itself is refused when inserted
/// (sub-patches.md, Patch file; rule 5).
#[test]
fn a_link_loop_keeps_the_stored_copy_and_a_self_link_is_refused_on_insert() {
    let dir = library();
    // A links to B, and B links to A.
    let mut a = Patch::from_ron(VOICE_FILE).unwrap();
    a.metadata.name = "A".to_string();
    let mut b = Patch::new("B");
    let mut a_in_b = loom_patch::SubPatch::new("A");
    a_in_b.library = Some("A.ron".to_string());
    b.subpatches.insert("a".to_string(), a_in_b);
    b.modules.push(instance(1, "a"));
    let mut b_in_a = loom_patch::SubPatch::new("B");
    b_in_a.library = Some("B.ron".to_string());
    a.subpatches.insert("b".to_string(), b_in_a);
    a.modules.push(instance(9, "b"));
    write(&dir.0, "A.ron", &a.to_ron().unwrap());
    write(&dir.0, "B.ron", &b.to_ron().unwrap());
    let mut host = Patch::from_ron(HOST).unwrap();
    let mut linked = loom_patch::SubPatch::new("A");
    linked.library = Some("A.ron".to_string());
    host.subpatches.insert("a".to_string(), linked);
    host.modules.push(instance(4, "a"));
    let patch_file = write(&dir.0, "set.ron", &host.to_ron().unwrap());

    let mut app = app_with(HOST, &dir.0);
    assert!(app.open_path(&patch_file), "a patch always opens");
    assert!(
        app.ui_snapshot()
            .status
            .contains("A: the library links loop (A › B › A), so its stored copy is used."),
        "{}",
        app.ui_snapshot().status
    );
    assert!(!app.session.hierarchy.is_flat());

    // Inserting a file linked inside a sub-patch that follows that file.
    let mut ui = Ui::with_app(app_with(HOST, &dir.0));
    let voice = insert(&mut ui, "Voice", true);
    let inside = ui.app.session.hierarchy.path_of(ModuleId(voice));
    ui.app.show_level(&inside);
    ui.redraw();
    open_browser(&mut ui);
    let row = ui
        .app
        .overlays
        .browser()
        .unwrap()
        .library_rows()
        .into_iter()
        .find(|(_, item)| item.name == "Voice")
        .unwrap()
        .0;
    ui.send(Message::Browser(browser::Message::InsertLinked(row)));
    assert_eq!(
        ui.app.ui_snapshot().status,
        "Cannot insert Voice: sub-patch Voice › Voice contains itself"
    );
    assert_eq!(
        ui.app.session.controller.graph().nodes().count(),
        6,
        "nothing was added"
    );
}

/// An instance module of `key` with id `id`, for a hand-made patch.
fn instance(id: u32, key: &str) -> loom_patch::PatchModule {
    loom_patch::PatchModule {
        id,
        kind: loom_patch::SUBPATCH_KIND.to_string(),
        subpatch: Some(key.to_string()),
        label: None,
        params: Default::default(),
        position: None,
        size: None,
        ranges: Default::default(),
    }
}

/// Inside a sub-patch, the toolbar offers Save to library for it; the
/// dialog starts in the library folder under the sub-patch's name, and
/// the file written opens on its own as a patch, with its ports as modules
/// and its surface kept (sub-patches.md, Library).
#[test]
fn save_to_library_inside_a_sub_patch_writes_a_file_that_opens_on_its_own() {
    let dir = library();
    let mut ui = Ui::with_app(app_with(HOST, &dir.0));
    let voice = insert(&mut ui, "Voice", false);
    assert!(ui.text("Save to library").is_none(), "not at the top level");
    let inside = ui.app.session.hierarchy.path_of(ModuleId(voice));
    ui.app.show_level(&inside);
    ui.redraw();
    let _ = fake::asked();
    let target = dir.0.join("Mine").join("Lead.ron");
    fake::answer_save(Some(target.clone()));
    ui.click_text("Save to library");
    assert_eq!(
        fake::asked(),
        [Asked::Save(Some(dir.0.join("Voice.ron")))],
        "the library folder, under the sub-patch's name"
    );
    assert!(target.is_file());
    // Opened on its own, it is a flat patch with its ports as modules.
    assert!(ui.app.open_path(&target));
    let app = &ui.app;
    assert!(app.session.hierarchy.is_flat());
    assert_eq!(app.patch_name(), "Lead.ron");
    assert_eq!(app.session.document.info().name, "Voice");
    let mut titles: Vec<String> = app.ui_snapshot().tiles.iter().map(|t| t.title.clone()).collect();
    titles.sort();
    assert!(titles.iter().any(|t| t == "Patch Input"), "{titles:?}");
    assert!(titles.iter().any(|t| t == "Patch Output"), "{titles:?}");
    assert_eq!(
        app.current_patch().exposed.len(),
        1,
        "the surface is kept for the next insert"
    );
    assert_eq!(app.current_patch().metadata.authors, ["Ada"]);
}

/// Save to library at the top level, or by a script, is refused with a
/// reason.
#[test]
fn save_to_library_needs_a_sub_patch_and_a_player() {
    let dir = library();
    let mut app = app_with(HOST, &dir.0);
    let _ = app.update(Message::File(crate::FileAction::SaveToLibrary));
    assert_eq!(app.ui_snapshot().status, crate::file_commands::SAVE_TO_LIBRARY_INSIDE);
    let _ = app.update(Message::SaveToLibrary(1));
    assert_eq!(app.ui_snapshot().status, crate::file_commands::SAVE_TO_LIBRARY_INSIDE);
}

/// Copy and paste of a sub-patch tile make an independent copy with its
/// own definition: editing inside the copy does not change the original,
/// and a copy of a linked tile is linked to the same file (sub-patches.md,
/// Copies and editing).
#[test]
fn pasting_a_sub_patch_tile_and_editing_inside_the_copy_leaves_the_original() {
    let dir = library();
    let mut ui = Ui::with_app(app_with(HOST, &dir.0));
    let voice = insert(&mut ui, "Voice", true);
    let app = &mut ui.app;
    app.session.selection = [voice].into_iter().collect();
    let text = app.copy().expect("a sub-patch tile copies");
    assert_eq!(app.ui_snapshot().status, "Copied Voice");
    let copied = Patch::from_ron(&text).unwrap();
    assert_eq!(copied.subpatches.len(), 1, "the definition travels with the tile");
    assert!(copied.modules.iter().any(|m| m.is_instance()));
    app.paste(iced::Point::new(300.0, 300.0), Some(text.clone()));
    assert_eq!(app.ui_snapshot().status, "Pasted Voice");
    assert_eq!(app.ui_snapshot().undo.as_deref(), Some("paste Voice"));
    let copy = *app.session.selection.iter().next().unwrap();
    assert_ne!(copy, voice);
    assert_eq!(
        app.session.hierarchy.library_of(ModuleId(copy)),
        Some("Voices/Voice.ron"),
        "a copy of a linked tile is linked to the same file"
    );
    assert_eq!(app.current_patch().subpatches.len(), 2, "its own definition");

    set_inner_tune(app, copy, 0.1);
    assert_eq!(inner_tune(app, copy), 0.1);
    assert_ne!(inner_tune(app, voice), 0.1, "the original is unchanged");
    assert_eq!(inner_gain(app, voice), 0.5);
    assert_eq!(app.session.hierarchy.library_of(ModuleId(copy)), None);
    assert_eq!(
        app.session.hierarchy.library_of(ModuleId(voice)),
        Some("Voices/Voice.ron")
    );
    // Undo takes the whole paste away; redo brings it back.
    app.undo();
    app.undo();
    assert_eq!(app.current_patch().subpatches.len(), 1);
    app.redo();
    assert_eq!(app.current_patch().subpatches.len(), 2);
}

/// Duplicate of a sub-patch tile keeps the cables that came into it, and
/// cut takes it out whole; a text with a Transport pastes without it,
/// with a note (sub-patches.md, rule 8).
#[test]
fn duplicate_and_cut_carry_a_sub_patch_tile_and_paste_leaves_the_transport_out() {
    let dir = library();
    let mut ui = Ui::with_app(app_with(HOST, &dir.0));
    let voice = insert(&mut ui, "Voice", false);
    let app = &mut ui.app;
    // Cable the note into the tile and the tile into the output.
    let lid = app.session.hierarchy.path_of(ModuleId(voice)).last();
    let root = ModulePath::root();
    assert!(app.change_hierarchy("connect", |h, _| {
        h.connect(&root, (1, "pitch".to_string()), (lid, "pitch".to_string()));
        h.connect(&root, (lid, "out".to_string()), (2, "in".to_string()));
        Ok(())
    }));
    app.commit();
    app.session.selection = [voice].into_iter().collect();
    app.duplicate();
    assert_eq!(app.ui_snapshot().status, "Duplicated Voice");
    let copy = *app.session.selection.iter().next().unwrap();
    assert_ne!(copy, voice);
    let saved = app.current_patch();
    let copy_lid = app.session.hierarchy.path_of(ModuleId(copy)).last();
    assert!(
        saved
            .cables
            .iter()
            .any(|c| c.from == (1, "pitch".to_string()) && c.to == (copy_lid, "pitch".to_string())),
        "the copy is fed by the same note"
    );
    assert!(
        !saved.cables.iter().any(|c| c.from.0 == copy_lid),
        "the outgoing cable is not copied"
    );
    assert_eq!(saved.subpatches.len(), 2);

    // Cut takes the copy out, inside and all, as one step.
    let before = app.session.controller.graph().nodes().count();
    app.session.selection = [copy].into_iter().collect();
    let text = app.cut().expect("the copy cuts");
    assert_eq!(app.session.controller.graph().nodes().count(), before - 3);
    assert_eq!(app.ui_snapshot().undo.as_deref(), Some("cut Voice"));
    assert_eq!(app.current_patch().subpatches.len(), 1);
    app.undo();
    assert_eq!(app.current_patch().subpatches.len(), 2);

    // Pasted text that holds a Transport leaves it out, and says so.
    let mut with_transport = Patch::from_ron(&text).unwrap();
    with_transport.modules.push(loom_patch::PatchModule {
        id: 99,
        kind: "loom.clock.transport@1".to_string(),
        subpatch: None,
        label: None,
        params: Default::default(),
        position: None,
        size: None,
        ranges: Default::default(),
    });
    let inside = app.session.hierarchy.path_of(ModuleId(voice));
    app.show_level(&inside);
    app.paste(iced::Point::ORIGIN, Some(with_transport.to_ron().unwrap()));
    assert_eq!(
        app.ui_snapshot().status,
        "Pasted Voice. The Transport was left out: a patch has one, at the top level."
    );
    assert_eq!(
        app.session
            .controller
            .graph()
            .nodes()
            .filter(|(_, n)| n.descriptor.id == loom_core::TRANSPORT_MODULE)
            .count(),
        1
    );
    assert_eq!(
        app.current_patch().subpatches.len(),
        3,
        "a sub-patch inside a sub-patch"
    );
}

/// A placeholder for the Scale and tuning module, which this build does
/// not have yet, is patch-wide too: copy leaves it out, duplicate makes no
/// second one, and cut leaves it where it is, since the clipboard did not
/// take it (sub-patches.md, rule 8; clipboard.md, rule 6).
#[test]
fn a_scale_placeholder_stays_out_of_copies_and_stays_put_on_cut() {
    let dir = library();
    let text = r#"Patch(
        version: 1,
        metadata: Metadata(name: "Host"),
        modules: [
            PatchModule(id: 1, kind: "loom.osc.sine@1", position: Some((0.0, 0.0))),
            PatchModule(id: 2, kind: "loom.pitch.scale@1", position: Some((200.0, 0.0))),
            PatchModule(id: 3, kind: "loom.clock.transport@1", position: Some((0.0, 300.0))),
        ],
    )"#;
    let mut app = app_with(text, &dir.0);
    assert!(
        app.session
            .controller
            .graph()
            .node(ModuleId(2))
            .unwrap()
            .unknown
            .is_some(),
        "the scale module loads as a placeholder"
    );
    app.session.selection = [1, 2, 3].into_iter().collect();
    let text = app.copy().expect("the sine copies");
    assert_eq!(app.ui_snapshot().status, "Copied Sine");
    let copied = Patch::from_ron(&text).unwrap();
    assert_eq!(copied.modules.len(), 1);
    assert_eq!(copied.modules[0].kind, "loom.osc.sine@1");
    app.duplicate();
    assert_eq!(app.ui_snapshot().status, "Duplicated Sine");
    assert_eq!(
        app.session.controller.graph().nodes().count(),
        4,
        "one sine more, nothing else"
    );
    app.session.selection = [1, 2, 3].into_iter().collect();
    app.cut();
    assert_eq!(app.ui_snapshot().status, "Cut Sine");
    assert!(app.session.controller.graph().node(ModuleId(1)).is_none());
    assert!(
        app.session.controller.graph().node(ModuleId(2)).is_some(),
        "the placeholder stays"
    );
    assert!(
        app.session.controller.graph().node(ModuleId(3)).is_some(),
        "the Transport stays"
    );
}

/// A library file that renamed a port still lets the patch open: the
/// cable out of the old port goes, the status bar says what went, and the
/// tile plays from the new file. A file whose fresh copy cannot play at
/// all leaves the stored copy, with a note (sub-patches.md, rule 5).
#[test]
fn a_changed_library_file_never_stops_the_patch_from_opening() {
    let dir = library();
    let mut ui = Ui::with_app(app_with(HOST, &dir.0));
    let voice = insert(&mut ui, "Voice", true);
    let app = &mut ui.app;
    let lid = app.session.hierarchy.path_of(ModuleId(voice)).last();
    let root = ModulePath::root();
    assert!(app.change_hierarchy("connect", |h, _| {
        h.connect(&root, (1, "pitch".to_string()), (lid, "pitch".to_string()));
        h.connect(&root, (lid, "out".to_string()), (2, "in".to_string()));
        Ok(())
    }));
    app.commit();
    let patch_file = dir.0.join("set.ron");
    crate::patch_file::save(&app.current_patch(), &patch_file).unwrap();

    // The file renames its output port.
    write(
        &dir.0,
        "Voices/Voice.ron",
        &VOICE_FILE.replace("label: Some(\"out\")", "label: Some(\"audio\")"),
    );
    assert!(app.open_path(&patch_file));
    assert_eq!(
        app.ui_snapshot().status,
        format!(
            "opened {}: Voice: the library file no longer has \"out\"; its cable was removed.; Updated from the library: Voice.",
            patch_file.display()
        )
    );
    let tile = app.ui_snapshot().tiles.into_iter().find(|t| t.id == voice).unwrap();
    assert_eq!(tile.outputs[0].label, "audio");
    assert_eq!(app.current_patch().cables.len(), 1, "the note into the tile stays");

    // The file's fresh copy cannot play: it exposes a module it does not
    // have. The stored copy plays instead.
    write(
        &dir.0,
        "Voices/Voice.ron",
        &VOICE_FILE.replace("module: 4, param: \"gain\"", "module: 99, param: \"gain\""),
    );
    assert!(app.open_path(&patch_file));
    let status = app.ui_snapshot().status;
    assert!(status.contains("The library's changes could not be used ("), "{status}");
    assert!(
        !status.contains("Updated from the library") && !status.contains("was removed"),
        "no refresh is claimed: {status}"
    );
    assert_eq!(inner_gain(app, voice), 0.5, "the stored copy plays");
    assert_eq!(
        app.session.controller.graph().cables().count(),
        4,
        "the note in, the two inside, and the stored copy's out to the output"
    );
}

/// A library file opened on its own cannot take a link to itself, or to a
/// file whose links reach it: the file would contain itself
/// (sub-patches.md, Patch file). A copy of itself is fine.
#[test]
fn inserting_the_open_library_file_into_itself_is_refused() {
    let dir = library();
    let mut ui = Ui::with_app(app_with(HOST, &dir.0));
    assert!(ui.app.open_path(&dir.0.join("Voices").join("Voice.ron")));
    ui.redraw();
    open_browser(&mut ui);
    let row = ui
        .app
        .overlays
        .browser()
        .unwrap()
        .library_rows()
        .into_iter()
        .find(|(_, item)| item.name == "Voice")
        .unwrap()
        .0;
    ui.send(Message::Browser(browser::Message::InsertLinked(row)));
    assert_eq!(
        ui.app.ui_snapshot().status,
        "Cannot insert Voice: sub-patch Voice › Voice contains itself"
    );
    assert!(ui.app.session.hierarchy.is_flat(), "nothing was added");
    let copy = insert(&mut ui, "Voice", false);
    assert_eq!(ui.app.session.hierarchy.library_of(ModuleId(copy)), None);
}

/// Save to library with a name typed without its extension, when a file
/// of that name exists, asks before replacing it, as Save As does.
#[test]
fn save_to_library_asks_before_replacing_a_file_named_without_its_extension() {
    let dir = library();
    let mut ui = Ui::with_app(app_with(HOST, &dir.0));
    let voice = insert(&mut ui, "Voice", false);
    let app = &mut ui.app;
    let _ = fake::asked();
    let target = dir.0.join("Voices").join("Voice.ron");
    let before = std::fs::read_to_string(&target).unwrap();
    // A fresh name the dialog did not check, with Voice.ron already there.
    fake::answer_save(Some(dir.0.join("Voices").join("Voice")));
    let _ = app.update(Message::SaveToLibrary(voice));
    let question = app.ui_snapshot().question.expect("the replace question");
    assert_eq!(question.title, "Replace \"Voice.ron\"?");
    assert_eq!(question.primary, "cancel");
    assert!(app.question_message(crate::question::Message::Answer(crate::question::Answer::Replace)));
    assert_eq!(app.ui_snapshot().status, format!("saved Voice to {}", target.display()));
    assert_ne!(std::fs::read_to_string(&target).unwrap(), before, "positions differ");
    assert!(app.ui_snapshot().question.is_none());
}

/// A MIDI control that moves a value inside a linked tile which is not
/// exposed changes the stored copy, so it unlinks the tile as an edit
/// would; one on an exposed value keeps the link (Notes from building).
#[test]
fn a_midi_control_moving_an_unexposed_inner_value_unlinks_the_tile() {
    let dir = library();
    let mut ui = Ui::with_app(app_with(HOST, &dir.0));
    let first = insert(&mut ui, "Voice", true);
    let second = insert(&mut ui, "Voice", true);
    let app = &mut ui.app;
    let inside = app.session.hierarchy.path_of(ModuleId(first));
    let generator = app.session.hierarchy.id_at(&inside, 2).unwrap();
    let exposed = app.session.hierarchy.exposed_target(ModuleId(second), 0).unwrap();
    let gain = app
        .session
        .controller
        .graph()
        .node(exposed.module)
        .unwrap()
        .parameter_index(&exposed.param)
        .unwrap();
    // Mapped straight on the controller, so the mapping itself is not an
    // edit here: controller 7 moves the first tile's tune, controller 8
    // the second tile's exposed gain.
    let cc = |controller: u8| loom_core::MidiControl { channel: 0, controller };
    app.session.controller.map_midi(cc(7), generator, 0).unwrap();
    app.session.controller.map_midi(cc(8), exposed.module, gain).unwrap();
    app.commit();
    assert_eq!(
        app.session.hierarchy.library_of(ModuleId(first)),
        Some("Voices/Voice.ron")
    );
    // The check that costs an unflatten runs only while a mapping can
    // change a stored copy: not for the exposed gain alone.
    assert!(app.midi_may_edit_a_linked_tile());
    app.session.controller.unmap_midi(generator, 0).unwrap();
    assert!(!app.midi_may_edit_a_linked_tile(), "an exposed value is the instance's");
    app.session.controller.map_midi(cc(7), generator, 0).unwrap();
    app.commit();
    let midi = app.rig.midi_in.clone().expect("the idle engine's MIDI input");
    assert!(loom_host_desktop::send_midi(
        &midi,
        loom_core::MidiEvent::from_bytes(0, &[0xB0, 8, 100])
    ));
    app.process(128);
    let _ = app.update(Message::Midi);
    assert_eq!(
        app.session.hierarchy.library_of(ModuleId(second)),
        Some("Voices/Voice.ron"),
        "an exposed value moved by MIDI keeps the link"
    );
    assert!(loom_host_desktop::send_midi(
        &midi,
        loom_core::MidiEvent::from_bytes(0, &[0xB0, 7, 100])
    ));
    app.process(128);
    let _ = app.update(Message::Midi);
    assert_eq!(
        app.ui_snapshot().status,
        "Voice is now a copy. It no longer follows the library."
    );
    assert_eq!(app.session.hierarchy.library_of(ModuleId(first)), None);
    assert_eq!(
        app.session.hierarchy.library_of(ModuleId(second)),
        Some("Voices/Voice.ron")
    );
}

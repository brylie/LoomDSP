//! The recorders for the parity gates of a change that moves who draws or
//! routes the window, such as plan step P4 of leaving Iced
//! (docs/dev/leaving-iced-plan.md, Parity gates). Each writes what the
//! window shows in one view, through the `Ui` harness, so the same code
//! records the tree before a change and after it, and the two can be
//! compared file by file.
//!
//! They are `#[ignore]`d and write files, so they run only by hand:
//!
//! ```sh
//! LOOM_PARITY_DIR=/tmp/before cargo test -p loom-app --lib parity_tests -- --ignored
//! ```
//!
//! For each view it writes `NN-name.png`, the window as the capture effect
//! draws it (the control channel's `screenshot`), when a GPU adapter is
//! found, and `NN-name.tree.txt`, the accessibility tree as text: one line
//! per node, indented by depth, with its role, name, value, description,
//! state, bounds, and UI ID, and never its node ID, which a change may
//! renumber. The seven views the command line opens are recorded with
//! `--screenshot-window` instead (the P3 pull request lists them).
//!
//! The views use only messages, names, and core types, so a change to the
//! harness's own types does not change what they record.

use crate::ui_test::{A11yNode, Ui};
use crate::{FileAction, Message};
use loom_ui_core::{Key, Modifiers, Point, UiId};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where the records go: `LOOM_PARITY_DIR`, which must be set.
fn out_dir() -> PathBuf {
    let dir = std::env::var_os("LOOM_PARITY_DIR").expect("set LOOM_PARITY_DIR to the folder for the records");
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("the records folder can be made");
    dir
}

/// Writes the window's capture as `name.png`, if a GPU draws it.
fn write_capture(ui: &mut Ui, dir: &Path, name: &str) {
    use std::sync::{Arc, Mutex};
    if !ui.draws_the_canvas() {
        eprintln!("{name}: no GPU adapter, no capture");
        return;
    }
    let seen = Arc::new(Mutex::new(None));
    let got = seen.clone();
    ui.app_mut().effect(loom_ui::Effects::capture(move |image| {
        *got.lock().unwrap() = Some(image);
        Message::Tick
    }));
    let _ = ui.deliver(Message::Tick);
    let image = seen
        .lock()
        .unwrap()
        .take()
        .expect("the capture was answered")
        .expect("a GPU draws it");
    let file = std::fs::File::create(dir.join(format!("{name}.png"))).expect("the capture file can be made");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(image.rgba()))
        .expect("the capture is written");
}

/// The accessibility tree as text, one node a line, indented by depth.
fn tree_text(ui: &mut Ui) -> String {
    let tree = ui.a11y_tree();
    let mut text = String::new();
    write_node(&mut text, tree.root(), 0);
    text
}

/// One node and, below it, every node inside it.
fn write_node(text: &mut String, node: A11yNode<'_>, depth: usize) {
    let _ = write!(text, "{:indent$}{:?}", "", node.role(), indent = depth * 2);
    if let Some(name) = node.name() {
        let _ = write!(text, " name={name:?}");
    }
    if let Some(value) = node.value() {
        let _ = write!(text, " value={value:?}");
    }
    if let Some(number) = node.number() {
        let _ = write!(text, " number={number}");
    }
    if let Some(description) = node.description() {
        let _ = write!(text, " description={description:?}");
    }
    if let Some(toggled) = node.toggled() {
        let _ = write!(text, " toggled={toggled:?}");
    }
    for (flag, on) in [
        ("focused", node.is_focused()),
        ("selected", node.is_selected()),
        ("disabled", node.is_disabled()),
        ("current", node.is_current()),
    ] {
        if on {
            let _ = write!(text, " {flag}");
        }
    }
    if let Some(expanded) = node.expanded() {
        let _ = write!(text, " expanded={expanded}");
    }
    if let Some(bounds) = node.bounds() {
        let _ = write!(
            text,
            " bounds=({:.1},{:.1})-({:.1},{:.1})",
            bounds.x0, bounds.y0, bounds.x1, bounds.y1
        );
    }
    if let Some(id) = node.ui_id() {
        let _ = write!(text, " id={id}");
    }
    text.push('\n');
    for child in node.children() {
        write_node(text, child, depth + 1);
    }
}

/// Writes both records of the view `ui` shows now.
fn record(ui: &mut Ui, dir: &Path, name: &str) {
    ui.redraw();
    let tree = tree_text(ui);
    std::fs::write(dir.join(format!("{name}.tree.txt")), tree).expect("the tree file is written");
    write_capture(ui, dir, name);
}

/// Module 4 of `subtractive.ron`, the filter, selected, so the inspector
/// shows its rows.
fn with_filter_selected() -> Ui {
    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::Select(vec![4])));
    ui
}

/// The views the command line cannot open, each as one record: the same
/// sixteen views the P3 pull request captured by hand.
#[test]
#[ignore = "writes the parity records to LOOM_PARITY_DIR; run by hand"]
fn record_the_views() {
    let dir = out_dir();

    let mut ui = Ui::open("subtractive.ron");
    record(&mut ui, &dir, "08-window");

    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::OpenBrowser);
    record(&mut ui, &dir, "09-browser");

    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::Preferences(crate::settings::Message::Open));
    record(&mut ui, &dir, "10-preferences");

    let mut ui = with_filter_selected();
    ui.send(Message::Matrix(crate::MatrixEvent::Tab(crate::RightTab::Matrix)));
    record(&mut ui, &dir, "11-matrix");

    let mut ui = with_filter_selected();
    ui.send(Message::Matrix(crate::MatrixEvent::Tab(crate::RightTab::Outline)));
    record(&mut ui, &dir, "12-outline");

    let mut ui = with_filter_selected();
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::TileMenu {
        module: 4,
        at: Point::new(100.0, 100.0),
        area: loom_ui_core::Rect::new(Point::ORIGIN, loom_ui_core::Size::new(800.0, 600.0)),
        by_key: false,
    }));
    record(&mut ui, &dir, "13-tile-menu");

    let mut ui = with_filter_selected();
    record(&mut ui, &dir, "14-inspector");

    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut().edit(|app| {
        app.session.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
    });
    ui.send(Message::File(FileAction::Close));
    record(&mut ui, &dir, "15-question");

    let mut ui = Ui::open("modulation.ron");
    ui.send(Message::Canvas(loom_canvas::CanvasEvent::Select(vec![4])));
    record(&mut ui, &dir, "16-modulation-inspector");
    ui.send(Message::Matrix(crate::MatrixEvent::Tab(crate::RightTab::Matrix)));
    record(&mut ui, &dir, "17-modulation-matrix");

    let mut ui = Ui::open("nested-subtractive.ron");
    ui.send(Message::OpenLevel(loom_patch::ModulePath::top(2)));
    record(&mut ui, &dir, "18-inside-a-sub-patch");

    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::Preferences(crate::settings::Message::Open));
    ui.app_mut().overlays.chooser = Some(crate::licence_chooser::Chooser::open(
        crate::licence_chooser::Target::Default,
        None,
    ));
    record(&mut ui, &dir, "19-licence-chooser");

    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::File(FileAction::Browse));
    record(&mut ui, &dir, "20-your-patches");

    let mut ui = with_filter_selected();
    let at = ui.value_field(4, 0);
    ui.move_to(at);
    ui.press();
    ui.release();
    ui.tap(Key::Character('a'), Modifiers::platform_shortcut());
    record(&mut ui, &dir, "21-value-field-selected");
    ui.typewrite("zzz");
    ui.tap(Key::Enter, Modifiers::NONE);
    ui.wake_after(Duration::from_secs(1));
    record(&mut ui, &dir, "22-value-field-refused");

    let mut ui = Ui::open("subtractive.ron");
    ui.lay_out_panels();
    let open: UiId = "toolbar/open".parse().unwrap();
    let bounds = ui.runtime().bounds(&open).expect("the toolbar shows Open");
    ui.move_to(Point::new(
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    ));
    ui.wake_after(Duration::from_secs(2));
    record(&mut ui, &dir, "23-toolbar-hover");
}

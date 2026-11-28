//! The app's accessibility tree, for screen readers, through AccessKit
//! (ADR-0041, `docs/design/ui-layer.md`, section 7).
//!
//! The tree comes from the same plain-data models the window draws from
//! (`App::ui_snapshot`, ADR-0021), so it cannot drift from the screen. A
//! screen reader's actions go through the same messages as the pointer.
//!
//! The parts:
//!
//! - [`tree`] builds an AccessKit `TreeUpdate` from the view models. No
//!   platform code.
//! - [`actions`] turns an AccessKit `ActionRequest` into messages. No
//!   platform code.
//! - The platform's adapter is `loom_ui`'s window shell's, on every
//!   platform: the app hands it the tree as an effect
//!   (`Effects::accessibility`), and hears the screen reader through
//!   `App::accessibility_event` (`runtime_app.rs`).
//! - This file holds what joins them to the app: the [`Event`]s and the
//!   state kept between events ([`Access`]).
//!
//! The tree is built only while a screen reader listens, after a message
//! that may have changed what it reports, and only the nodes that changed
//! are sent. Nodes that show a live value are sent at most every 250 ms.
//! Nothing runs on a timer at rest (ui-layer.md, section 7, rule 7).

use crate::places::Places;
use accesskit::{ActionRequest, TreeUpdate};
use loom_core::ModuleId;
use loom_ui_core::IdTable;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub(crate) mod actions;
pub(crate) mod items;
pub(crate) mod tree;

#[cfg(test)]
mod tests;

/// The least time between two updates of a node that shows a live value:
/// a meter, a light, a readout, a scope.
pub(crate) const LIVE_INTERVAL: Duration = Duration::from_millis(250);

/// What a message means for the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Touch {
    /// The accessibility layer's own message: it rebuilds for itself.
    Own,
    /// Only live values moved: meters, lights, traces.
    Live,
    /// Anything else: a view model may have changed.
    Changed,
    /// A timer's poll, with the key from before it ran, or `None` while no
    /// screen reader listens. Most polls change nothing, so the tree is
    /// built only when the key differs afterwards.
    Poll(Option<PollKey>),
}

/// What the tree reports that a timer's poll can change, as a key that is
/// cheap to make and to compare. A poll (the audio device check, the MIDI
/// port check, autosave, the pickup check, the audio watchers' readings of
/// load and dropouts) runs several times a second for the whole session,
/// and building the tree for each would be idle work (NFR-PERF-04).
///
/// Every field stands for something the tree reads. When the tree starts
/// to report something new that a poll can change, add it here, or the
/// tree goes stale until the next ordinary message.
///
/// The inspector's and the matrix's items need no field of their own. They
/// are built from the selection, the graph, the panel's own state (the
/// inspector's drafts, the matrix's sort, filter, chosen cell, and Add
/// form), and the panel values. No poll changes the first three:
/// selecting, editing, and typing are ordinary messages, classed as a
/// change. A poll changes the panel values only through their generation,
/// which `values` holds. So before adding a poll that edits the graph or
/// the selection, or a panel whose items read something a poll changes,
/// add that here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PollKey {
    /// The scene's generation: the tiles, their names and places, and the
    /// widgets on them. A device change that rebuilds tiles changes it.
    scene: u64,
    /// The panel values' generation: every control's value and text, and a
    /// meter's reading. It also changes for things the tree does not say,
    /// such as a pickup mark, which costs one build that sends nothing.
    values: u64,
    /// The toolbar, the right panel's open tab, and the bar at the bottom
    /// without its live readings, hashed. A poll that finds a device gone
    /// says so in the status line, turns Hold note off, and changes the
    /// audio label; a MIDI port that comes or goes changes the MIDI count.
    frame: u64,
    /// True while a panel owns the input, which hides the tiles.
    covered: bool,
    /// What the Preferences window shows, hashed, while it is open. Its
    /// playing line and input notes read the audio stream, and its device
    /// lists change when a poll finds a device gone, with no message of
    /// their own. None of it is a live value: the buffer size it shows only
    /// grows, so it changes once or twice a run.
    preferences: u64,
    // The audio load and the dropout count are not here. They are live
    // values (ui-layer.md, section 3, rule 7): the message that brings
    // them is classed as live, and the live timer sends them.
    /// The control the tree reports focus on.
    focus: Option<(u32, usize)>,
}

/// What the accessibility layer tells the app.
#[derive(Debug, Clone)]
pub(crate) enum Event {
    /// A screen reader started listening. `App::update` takes the window's
    /// scale first, so the tree's bounds are right from the start.
    Activated,
    /// The screen reader left. Windows and Linux say so; macOS does not
    /// (`accesskit_winit` 0.34.2 drops the event there), so on macOS the
    /// tree is kept up to date until the app quits.
    Deactivated,
    /// Live values changed a while ago: time to send them.
    LiveTick,
    /// The messages an action sent have run: the step's release, after
    /// them in the queue (`App::post`).
    Applied,
    /// Where the canvas and the window's named parts are, for a rebuild.
    Located(Places),
    /// Asks for fresh places after a layout invalidated an answer.
    LocateAfterLayout,
    /// A screen reader asked for an action.
    Action(ActionRequest),
    /// What one of the runtime's text fields reports changed: its text,
    /// caret, selection, or scroll (ui-layer.md, section 7, rule 7).
    FieldChanged,
}

/// What the app keeps for the accessibility tree between events. It
/// outlives the open patch: the window stays when a patch is replaced.
#[derive(Debug)]
pub(crate) struct Access {
    /// The node ID of every element in the tree.
    ids: IdTable,
    /// The nodes of `loom-ui`'s own elements (the stats bar), as the
    /// runtime last laid them out, spliced into the tree (ui-layer.md,
    /// section 7, until phase 4).
    pub(crate) ours: loom_ui::AccessNodes,
    /// True once a screen reader asked for the tree.
    active: bool,
    /// What the screen reader was last sent.
    sent: tree::Sent,
    /// True while a rebuild waits for the window to say where the canvas
    /// is. Messages that arrive meanwhile start no second one, but ask for
    /// one more once this one is built ([`Access::again`]).
    locating: bool,
    /// A rebuild was asked for while one was in flight: once that one is
    /// built, one more runs, so a change the in-flight build could not see
    /// yet (the runtime's layout of a bar an autosave just changed) is
    /// sent too.
    again: bool,
    /// A rebuild was asked for before there was a window to read: the
    /// first update that has one reads it ([`App::a11y_window_held`]).
    waiting_window: bool,
    /// True while live values have changed and were not sent yet. The
    /// timer that sends them runs only then.
    live_dirty: bool,
    /// True when the next rebuild sends live values too.
    flush_live: bool,
    /// The control with keyboard focus on the canvas, as the canvas last
    /// reported it: module ID and panel index.
    focus: Option<(u32, usize)>,
    /// A screen reader's request to focus a control: a key that changes
    /// with each request, and the control. The canvas acts when the key
    /// changes.
    focus_request: (u64, Option<(u32, usize)>),
    /// True with `--measure-tree`: each build is timed.
    timed: bool,
    /// Where the canvas was, with its camera, when the tree was last built:
    /// where a tile's menu opens when a screen reader asks for it.
    canvas: Option<loom_canvas::CanvasView>,
    /// The typed editor most recently reported to the screen reader.
    typed: Option<loom_canvas::CanvasTyped>,
    /// An edit requested for the canvas's open value field.
    typed_request: (u64, Option<loom_canvas::TypedRequest>),
}

/// How many times this process built the tree while `--measure-tree` timed
/// it, and how long that took. Process-wide, as the canvas counters are,
/// because the report runs on its own thread.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BuildTimes {
    /// The builds so far.
    pub count: u64,
    /// The time of all of them.
    pub total: Duration,
    /// The longest one.
    pub worst: Duration,
}

static BUILD_COUNT: AtomicU64 = AtomicU64::new(0);
static BUILD_NANOS: AtomicU64 = AtomicU64::new(0);
static BUILD_WORST_NANOS: AtomicU64 = AtomicU64::new(0);

impl BuildTimes {
    /// The counts of this process so far.
    pub(crate) fn process() -> Self {
        Self {
            count: BUILD_COUNT.load(Ordering::Relaxed),
            total: Duration::from_nanos(BUILD_NANOS.load(Ordering::Relaxed)),
            worst: Duration::from_nanos(BUILD_WORST_NANOS.load(Ordering::Relaxed)),
        }
    }

    /// The time of one build, on average. Zero before the first.
    pub(crate) fn average(self) -> Duration {
        match u32::try_from(self.count) {
            Ok(count) if count > 0 => self.total / count,
            _ => Duration::ZERO,
        }
    }

    /// Adds one build that took `time`.
    fn record(time: Duration) {
        let nanos = u64::try_from(time.as_nanos()).unwrap_or(u64::MAX);
        BUILD_COUNT.fetch_add(1, Ordering::Relaxed);
        BUILD_NANOS.fetch_add(nanos, Ordering::Relaxed);
        BUILD_WORST_NANOS.fetch_max(nanos, Ordering::Relaxed);
    }
}

impl Default for Access {
    fn default() -> Self {
        Self {
            ids: IdTable::new(),
            ours: loom_ui::AccessNodes::default(),
            active: false,
            sent: tree::Sent::default(),
            locating: false,
            waiting_window: false,
            again: false,
            live_dirty: false,
            flush_live: false,
            focus: None,
            focus_request: (0, None),
            canvas: None,
            typed: None,
            typed_request: (0, None),
            timed: false,
        }
    }
}

impl Access {
    /// The open patch was replaced. Its tiles are new elements, even where a
    /// module ID repeats, so they must get new node IDs: a screen reader
    /// resting on the old module 4 must not read the new one in its place
    /// (ui-layer.md, section 4, rule 4).
    pub(crate) fn patch_replaced(&mut self) {
        self.ids.forget(&tree::canvas_id());
        self.ids.forget(&crate::outline::tree_id());
        // The old patch's module IDs mean nothing in the new one.
        self.focus = None;
        self.focus_request.1 = None;
        self.typed = None;
        self.typed_request.1 = None;
    }

    /// Notes which control has keyboard focus on the canvas.
    pub(crate) fn set_focus(&mut self, control: Option<(u32, usize)>) {
        self.focus = control;
    }

    /// Asks the canvas to let go of the keyboard, with a focus request for
    /// no control, and notes it as let go.
    pub(crate) fn release_canvas_focus(&mut self) {
        self.focus_request = (self.focus_request.0 + 1, None);
        self.focus = None;
    }

    /// Requests focus on a control, or clears the control focus.
    pub(crate) fn request_control(&mut self, control: Option<(u32, usize)>) {
        self.focus_request = (self.focus_request.0 + 1, control);
        self.focus = control;
    }

    /// The canvas control with keyboard focus, as the canvas last said.
    pub(crate) fn focus(&self) -> Option<(u32, usize)> {
        self.focus
    }

    /// The focus request the canvas is shown: its key and its control.
    pub(crate) fn focus_request(&self) -> (u64, Option<(u32, usize)>) {
        self.focus_request
    }

    /// Latest text action on the open value editor.
    pub(crate) fn typed_request(&self) -> (u64, Option<&loom_canvas::TypedRequest>) {
        (self.typed_request.0, self.typed_request.1.as_ref())
    }

    /// True while a screen reader listens.
    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    /// The layer as `--measure-tree` runs it: building the tree as if a
    /// screen reader were listening from the start, and timing each build.
    /// For measuring only. A normal run starts with [`Access::default`].
    pub(crate) fn measured() -> Self {
        Self {
            active: true,
            timed: true,
            ..Self::default()
        }
    }

    /// True while live values wait to be sent.
    pub(crate) fn live_waits(&self) -> bool {
        self.active && self.live_dirty
    }
}

impl crate::App {
    /// Notes a message the app just handled. Live values wait for the
    /// timer. Anything else asks for a rebuild now, unless one is on its
    /// way. With no screen reader this does nothing.
    pub(crate) fn a11y_after(&mut self, touch: Touch) {
        if !self.a11y.active {
            return;
        }
        match touch {
            Touch::Own => {}
            Touch::Live => self.a11y.live_dirty = true,
            Touch::Changed => self.a11y_rebuild(),
            Touch::Poll(before) if before == Some(self.a11y_poll_key()) => {}
            Touch::Poll(_) => self.a11y_rebuild(),
        }
    }

    /// The key a poll is compared by, before and after it runs.
    pub(crate) fn a11y_poll_key(&self) -> PollKey {
        use std::hash::{Hash, Hasher};
        let mut frame = std::collections::hash_map::DefaultHasher::new();
        let bars = self.bars_model();
        for button in self.toolbar_model().buttons {
            (button.name, button.label, button.enabled, button.hover, button.on).hash(&mut frame);
        }
        (self.right_tab == crate::matrix::Tab::Matrix).hash(&mut frame);
        bars.dev.map(|dev| (dev.measuring, dev.animating)).hash(&mut frame);
        for item in bars.items.iter().filter(|item| item.kind != crate::bars::Kind::Live) {
            (item.name, &item.text, &item.hover).hash(&mut frame);
        }
        for field in bars.kept.iter().chain(std::iter::once(&bars.status)) {
            (
                &field.shown,
                &field.hover,
                field.severity as u8,
                field.dismiss.is_some(),
            )
                .hash(&mut frame);
        }
        PollKey {
            scene: self.session.scene.generation(),
            values: self.session.values.generation(),
            frame: frame.finish(),
            covered: !self.pointer_reaches_window(),
            // Its debug text, which cannot fail. The model holds messages
            // and `loom-core`'s `OutputSafety`, which do not derive `Hash`,
            // and adding it across those crates for one key is not worth it.
            preferences: self.preferences_model().map_or(0, |model| {
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                format!("{model:?}").hash(&mut hash);
                hash.finish()
            }),
            focus: self.a11y.focus,
        }
    }

    /// Asks the window where the canvas is, to rebuild the tree when the
    /// answer comes. One request at a time.
    pub(crate) fn a11y_rebuild(&mut self) {
        if !self.a11y.active {
            return;
        }
        if self.a11y.locating {
            self.a11y.again = true;
            return;
        }
        self.a11y.locating = true;
        self.a11y_read_places();
    }

    /// Starts the rebuild a screen reader asked for before the window was
    /// held, once it is (`window_now.rs`). One read at a time, as ever.
    pub(crate) fn a11y_window_held(&mut self) {
        if self.window_now.window && std::mem::take(&mut self.a11y.waiting_window) {
            self.a11y.locating = false;
            self.a11y_rebuild();
        }
    }

    /// Reads the window after its latest layout.
    fn a11y_read_places(&mut self) {
        // With no window there is nothing to read yet. The read stays in
        // flight, so later asks wait behind it, and the first update that
        // has a window starts it again, so the screen reader is not left
        // with no tree.
        if !self.window_now.window {
            self.a11y.waiting_window = true;
            return;
        }
        let places = self.places_now();
        self.post(crate::Message::A11y(Event::Located(places)));
    }

    /// Builds the tree and gives what the screen reader must be sent: the
    /// whole tree, and the nodes that changed since the last send, if any.
    /// Live values go only when `flush_live` was set.
    pub(crate) fn a11y_changes(&mut self, places: &Places) -> (TreeUpdate, Option<TreeUpdate>) {
        let view = places.canvas.as_ref();
        self.a11y.canvas = places.canvas;
        self.a11y.typed = places.typed.clone();
        let snapshot = self.ui_snapshot();
        let frames = self.a11y_frames();
        let mut ids = self.a11y_widget_ids(&snapshot);
        ids.map = self
            .map_view()
            .zip(places.canvas)
            .and_then(|(map, view)| map.bounds(&self.session.scene, view.camera, view.bounds));
        let whole = tree::tree(
            &snapshot,
            &frames,
            &ids,
            places,
            &self.a11y.ours,
            self.window_scale,
            &mut self.a11y.ids,
        );
        let flush = std::mem::take(&mut self.a11y.flush_live);
        if flush {
            self.a11y.live_dirty = false;
        }
        let table = &self.a11y.ids;
        // The canvas in the window, in physical pixels, as node bounds are.
        let screen = view.map(|view| {
            let s = f64::from(self.window_scale);
            let b = view.bounds;
            accesskit::Rect {
                x0: f64::from(b.x) * s,
                y0: f64::from(b.y) * s,
                x1: f64::from(b.x + b.width) * s,
                y1: f64::from(b.y + b.height) * s,
            }
        });
        let canvas = tree::canvas_id();
        let hold = |node: accesskit::NodeId, built: &accesskit::Node| {
            let Some(id) = table.id(node.0) else {
                return tree::Hold::Send;
            };
            // Below a panel that owns the input, what is inside a tile
            // keeps the value last sent. The tile itself is sent, hidden.
            if ids.is_covered() && id.is_under(&canvas) && tree::tile_of(id).is_none() && *id != canvas {
                return tree::Hold::Skip;
            }
            // The bar's readings are always on screen.
            if tree::is_live_bar_item(id) {
                return if flush { tree::Hold::Send } else { tree::Hold::Wait };
            }
            if !ids.is_live(id) {
                return tree::Hold::Send;
            }
            // A live value off screen keeps the value last sent.
            let off_screen = match (screen, built.bounds()) {
                (Some(screen), Some(bounds)) => {
                    bounds.x1 <= screen.x0 || bounds.x0 >= screen.x1 || bounds.y1 <= screen.y0 || bounds.y0 >= screen.y1
                }
                _ => false,
            };
            if off_screen {
                tree::Hold::Skip
            } else if flush {
                tree::Hold::Send
            } else {
                tree::Hold::Wait
            }
        };
        let (changes, held) = self.a11y.sent.changes(&whole, hold);
        if held {
            self.a11y.live_dirty = true;
        }
        (whole, changes)
    }

    /// Where the accessibility tree says the element named `id` is, in the
    /// window, in logical pixels. `None` when the tree has no such node, or
    /// the node has no bounds. The control channel's `locate` reads it, so
    /// a script and a screen reader find one element in one place.
    ///
    /// The tree is built at scale 1 with a table of its own, so asking
    /// changes nothing a screen reader was told.
    pub(crate) fn a11y_bounds(&self, id: &loom_ui_core::UiId, places: &Places) -> Option<loom_ui_core::Rect> {
        let snapshot = self.ui_snapshot();
        let frames = self.a11y_frames();
        let ids = self.a11y_widget_ids(&snapshot);
        let mut table = IdTable::new();
        // The runtime's own parts are found in its layout, not here.
        let ours = loom_ui::AccessNodes::default();
        let whole = tree::tree(&snapshot, &frames, &ids, places, &ours, 1.0, &mut table);
        let node = accesskit::NodeId(table.number(id));
        let bounds = whole.nodes.iter().find(|(found, _)| *found == node)?.1.bounds()?;
        Some(loom_ui_core::Rect::new(
            loom_ui_core::Point::new(bounds.x0 as f32, bounds.y0 as f32),
            loom_ui_core::Size::new((bounds.x1 - bounds.x0) as f32, (bounds.y1 - bounds.y0) as f32),
        ))
    }

    /// The UI IDs of every tile's panel widgets, from the names that find
    /// them again (`targets.rs`).
    pub(crate) fn a11y_widget_ids(&self, snapshot: &crate::snapshot::UiSnapshot) -> tree::WidgetIds {
        let graph = self.session.controller.graph();
        let hierarchy = &self.session.hierarchy;
        let mut ids = tree::WidgetIds::default();
        if !self.pointer_reaches_window() {
            ids.set_covered();
        }
        ids.focused_port = self.session.outline.focus_port;
        let structure = &self.session.patch_map.structure;
        for tile in &snapshot.tiles {
            ids.neighbours
                .insert(tile.id, crate::outline::neighbours(structure, tile.id));
            if let Some(view) = self.session.scene.module(tile.id) {
                for (output, ports) in [(false, &view.inputs), (true, &view.outputs)] {
                    for (index, port) in ports.iter().enumerate() {
                        let reference = loom_canvas::PortRef {
                            module: tile.id,
                            output,
                            index,
                        };
                        let Some(name) = crate::targets::port_name(graph, hierarchy, reference) else {
                            continue;
                        };
                        let Some(centre) = self.session.scene.port_position(reference) else {
                            continue;
                        };
                        let id = tree::tile_id(tile.id)
                            .with(if output { "output" } else { "input" })
                            .with(name);
                        let mut description = crate::outline::port_neighbours(structure, reference, true);
                        if !output
                            && description == "Not connected"
                            && let Some(spec) = graph
                                .node(ModuleId(tile.id))
                                .and_then(|n| n.descriptor.inputs.get(index))
                        {
                            description.push_str(&format!(", {}", spec.default.text()));
                        }
                        let controls = structure
                            .cables
                            .iter()
                            .filter(|wire| wire.from == reference)
                            .map(|wire| wire.to)
                            .collect();
                        ids.ports.insert(
                            (tile.id, output, index),
                            tree::PortNode {
                                id,
                                label: port.label.clone(),
                                description,
                                centre,
                                controls,
                            },
                        );
                    }
                }
            }
            let part = crate::surface::part_tile(hierarchy, tile.id);
            let node = graph.node(ModuleId(part));
            let on_module = node.is_some();
            // A slot's controls are its part's: keyed under the part, so a
            // swap makes new elements.
            let under = if part == tile.id {
                tree::tile_id(tile.id)
            } else if let Some(node) = node {
                tree::part_id(tile.id, tree::SlotPart::Module(node.descriptor.id))
            } else if let Some(key) = hierarchy.key_at(&hierarchy.path_of(ModuleId(part))) {
                tree::part_id(tile.id, tree::SlotPart::SubPatch(key))
            } else {
                // A part with no key cannot be told from another: its
                // controls get no IDs, so no nodes, rather than shared ones.
                continue;
            };
            let widgets = (0..tile.widgets.len()).map(|index| {
                // From the panel's description, not from how the widget is
                // drawn, so the kind agrees with the names `locate` takes.
                let control = crate::surface::is_control(graph, hierarchy, tile.id, index)
                    || crate::surface::is_inert(hierarchy, tile.id, index);
                let kind = if !control {
                    tree::WidgetKey::Display
                } else if on_module {
                    tree::WidgetKey::Param
                } else {
                    tree::WidgetKey::Exposed
                };
                (kind, crate::surface::widget_name(graph, hierarchy, tile.id, index))
            });
            ids.add_tile(tile.id, &under, widgets);
            if let Some((module, widget)) = self.a11y.focus.filter(|(module, _)| *module == tile.id) {
                ids.set_focus(module, widget);
            }
            // What each widget is: its kind from the scene the canvas
            // draws, and its range from the parameter the control moves.
            let views = self
                .session
                .scene
                .module(tile.id)
                .map(|view| view.panel.widgets.as_slice());
            for (index, view) in views.unwrap_or_default().iter().enumerate() {
                let name = if view.label.is_empty() {
                    crate::surface::widget_name(graph, hierarchy, tile.id, index).unwrap_or_default()
                } else {
                    view.label.clone()
                };
                let spec =
                    |part| crate::surface::control_target(graph, hierarchy, tile.id, index, part).map(|t| t.spec);
                ids.set_detail(
                    tile.id,
                    index,
                    tree::Detail {
                        kind: view.kind.clone(),
                        name,
                        main: spec(loom_canvas::ControlPart::Main),
                        second: spec(loom_canvas::ControlPart::Second),
                        reading: self.session.values.get(tile.id, index).and_then(|value| value.reading),
                    },
                );
            }
        }
        ids
    }

    /// The UI ID the tree gave node `node`: for the runtime's text actions
    /// on its fields (`App::access_target`), and for the harness
    /// (`ui_test/a11y.rs`).
    pub(crate) fn a11y_ui_id(&self, node: accesskit::NodeId) -> Option<loom_ui_core::UiId> {
        self.a11y.ids.id(node.0).cloned()
    }

    /// The node ID the tree gives the part `id`, for a test that acts on it
    /// as a screen reader would.
    #[cfg(test)]
    pub(crate) fn a11y_node_of(&mut self, id: &loom_ui_core::UiId) -> accesskit::NodeId {
        accesskit::NodeId(self.a11y.ids.number(id))
    }

    /// The tree as it is now, with the canvas placed by `view`.
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) fn a11y_tree(&mut self, places: &Places) -> TreeUpdate {
        // Actions that place something, such as a tile's menu, use the
        // canvas the tree was built with, as after `a11y_changes`.
        self.a11y.canvas = places.canvas;
        self.a11y.typed = places.typed.clone();
        let snapshot = self.ui_snapshot();
        let frames = self.a11y_frames();
        let mut ids = self.a11y_widget_ids(&snapshot);
        ids.map = self
            .map_view()
            .zip(places.canvas)
            .and_then(|(map, view)| map.bounds(&self.session.scene, view.camera, view.bounds));
        tree::tree(
            &snapshot,
            &frames,
            &ids,
            places,
            &self.a11y.ours,
            self.window_scale,
            &mut self.a11y.ids,
        )
    }

    /// Lends the table of node IDs out, for the harness to read the
    /// runtime's nodes with it while the runtime holds the app.
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) fn a11y_lend_ids(&mut self) -> IdTable {
        std::mem::replace(&mut self.a11y.ids, IdTable::new())
    }

    /// Takes the table back, with the runtime's nodes read with it.
    #[cfg(any(test, feature = "test-ui"))]
    pub(crate) fn a11y_return_ids(&mut self, ids: IdTable, ours: loom_ui::AccessNodes) {
        self.a11y.ids = ids;
        self.a11y.ours = ours;
    }

    /// Takes the runtime's nodes from `nodes`, which reads them with the
    /// app's own table of node IDs.
    pub(crate) fn a11y_take_ours(&mut self, nodes: impl FnOnce(&mut IdTable) -> loom_ui::AccessNodes) {
        self.a11y.ours = nodes(&mut self.a11y.ids);
    }

    /// The runtime's overlay that a screen reader is given, with its role,
    /// its name, and its UI ID: the one that owns the input, by the one
    /// answer the pointer reads too (`Overlays::owner`; section 7, rule
    /// 27). The question is over everything, help over the dialogs it opens
    /// from, and Preferences and the menus below them. `None` while the
    /// owner is not the runtime's.
    pub(crate) fn ours_overlay(&self) -> Option<(accesskit::Role, String, loom_ui_core::UiId)> {
        use crate::input_owner::InputOwner;
        match self.input_owner() {
            InputOwner::Question => self.overlays.question.as_ref().map(|question| {
                (
                    accesskit::Role::AlertDialog,
                    question.model().title,
                    crate::overlays::access::question_id(),
                )
            }),
            InputOwner::Help => self.overlays.help.as_ref().map(|help| {
                (
                    accesskit::Role::Dialog,
                    help.model.name.to_string(),
                    crate::overlays::access::help_id(),
                )
            }),
            // The licence chooser, a dialog named by its title with no
            // description (row 3c), over Preferences, which opens it.
            InputOwner::Chooser => self.overlays.chooser.as_ref().map(|chooser| {
                (
                    accesskit::Role::Dialog,
                    chooser.model().title.to_string(),
                    crate::licence_chooser::panel::licence_id(),
                )
            }),
            InputOwner::Preferences => Some((
                accesskit::Role::Dialog,
                "Preferences".to_string(),
                crate::settings::preferences_id(),
            )),
            // The module browser (row 5) and Your patches (row 5b): dialogs
            // around their panels' nodes.
            InputOwner::Browser => (!self.browser_stops().is_empty()).then(|| {
                (
                    accesskit::Role::Dialog,
                    "Add module".to_string(),
                    crate::browser::browser_id(),
                )
            }),
            InputOwner::Patches => (!self.patches_stops().is_empty()).then(|| {
                (
                    accesskit::Role::Dialog,
                    "Your patches".to_string(),
                    crate::patches::patches_id(),
                )
            }),
            InputOwner::TileMenu | InputOwner::ProbeMenu | InputOwner::RecentList => self
                .runtime_menu()
                .map(|menu| (accesskit::Role::Menu, menu.label, menu.id)),
            _ => None,
        }
    }

    /// What the tree reports of the parts around the canvas: the bar, the
    /// open panel's parts, and what has keyboard focus. The inspector's and
    /// the matrix's parts come from the same models their views draw.
    pub(crate) fn a11y_frames(&self) -> tree::Frames {
        let panel = match self.right_tab {
            // The runtime's inspector gives its own nodes (`inspector/panel.rs`).
            // While a cable jump list covers it, the column shows the list,
            // whose nodes the runtime gives too, and the inspector is not
            // drawn, so it is not reported.
            crate::matrix::Tab::Inspector => Vec::new(),
            // The runtime's matrix and Outline give their own nodes
            // (`matrix/panel.rs`, `outline/panel.rs`).
            crate::matrix::Tab::Matrix | crate::matrix::Tab::Outline => Vec::new(),
        };
        let (overlay, overlay_focus) = self.overlay_items();
        tree::Frames {
            panel,
            focused_field: self.focus.clone(),
            focused_item: self
                .jump_focus()
                .or_else(|| {
                    (self.right_tab == crate::matrix::Tab::Outline && self.session.outline.active)
                        .then(|| self.outline_focus(&self.outline_rows()))
                        .flatten()
                })
                .or_else(|| self.session.outline.inspector_action.clone()),
            overlay,
            overlay_focus,
            ours_overlay: self.ours_overlay(),
            ours_overlay_items: if self.overlays.question.is_some() {
                None
            } else {
                self.runtime_menu().map(|menu| menu.items.len())
            },
            ours_menu: self.browser_menu_group(),
        }
    }

    /// A screen reader's action on a part of the open panel: the messages
    /// its item says that action sends, or a focus request for a field.
    /// `None` for a node that is not one of the panel's parts.
    ///
    /// SetValue, Increment, and Decrement on a slider join the screen
    /// reader's undo group, as on the canvas. Any other action ends the
    /// group first, also when it changes nothing (section 7, rule 6).
    fn a11y_panel_action(&mut self, request: &ActionRequest, at: std::time::Instant) -> Option<Vec<crate::Message>> {
        use crate::edits::ReaderEvent;
        let id = self.a11y.ids.id(request.target_node.0)?.clone();
        let frames = self.a11y_frames();
        // A part of the overlay that owns the input acts; nothing below it
        // does (section 7, rules 24 and 27).
        let in_overlay = items::find(&frames.overlay, &id).cloned();
        if in_overlay.is_none() && !self.pointer_reaches_window() {
            return None;
        }
        let inspector = id.is_under(&loom_ui_core::UiId::panel("inspector"));
        // The runtime's matrix carries out its own Click, Focus, Expand,
        // Increment, and Decrement; a SetValue on a Depth slider is the
        // app's, in percent, as the slider reports it.
        if id.is_under(&crate::matrix::panel::matrix_id()) {
            if request.action != accesskit::Action::SetValue || in_overlay.is_some() || !self.matrix_owns() {
                return None;
            }
            let item = self.matrix_depth_item(&id)?;
            return Some(self.a11y_item_action(&item, request, at));
        }
        // The runtime's inspector carries out its own Click, Focus,
        // Increment, and Decrement; a SetValue on one of its sliders is the
        // app's, with the number its item reads (section 7, rule 30).
        if inspector && self.inspector_ours() {
            if request.action != accesskit::Action::SetValue || in_overlay.is_some() {
                return None;
            }
            let parts = crate::inspector::access::items(&self.inspector_model());
            let item = items::find(&parts, &id)?.clone();
            return Some(self.a11y_item_action(&item, request, at));
        }
        let item = match in_overlay {
            Some(item) => item,
            None if inspector => items::find(&frames.panel, &id)?.clone(),
            None => return None,
        };
        if request.action == accesskit::Action::Focus {
            let Some(field) = item.focus else {
                return Some(Vec::new());
            };
            self.reader_event(ReaderEvent::Close);
            self.effect(loom_ui::Effects::focus(field));
            return Some(Vec::new());
        }
        Some(self.a11y_item_action(&item, request, at))
    }

    /// The messages `item` sends for `request`, after the screen reader's
    /// undo group: SetValue, Increment, and Decrement on a slider join it,
    /// and any other action ends it first, also when it changes nothing
    /// (section 7, rule 6).
    fn a11y_item_action(
        &mut self,
        item: &items::Item,
        request: &ActionRequest,
        at: std::time::Instant,
    ) -> Vec<crate::Message> {
        use crate::edits::ReaderEvent;
        let messages = items::messages(item, request.action, request.data.as_ref());
        let joins = item.groups && actions::joins_group(request.action);
        if messages.is_empty() {
            if !joins {
                self.reader_event(ReaderEvent::Close);
            }
            return Vec::new();
        }
        self.reader_event(if joins {
            ReaderEvent::Action {
                node: request.target_node.0,
                at,
            }
        } else {
            ReaderEvent::Close
        });
        messages
    }

    /// The message for a Click on a part of the window outside the canvas:
    /// a toolbar button that is enabled, one of the right panel's tabs, a
    /// × in the status line, or a developer switch. `None` for any other
    /// node or action, and for a button that is disabled now.
    pub(crate) fn a11y_frame_message(&self, request: &ActionRequest) -> Option<crate::Message> {
        // What a pointer press could not reach, a screen reader's Click
        // does not reach either.
        if request.action != accesskit::Action::Click || !self.pointer_reaches_window() {
            return None;
        }
        let id = self.a11y.ids.id(request.target_node.0)?;
        // The toolbar, the breadcrumb, and the bar are `loom-ui`'s: their
        // Click is the same click a pointer or a key gives (section 3,
        // rule 4).
        if id.is_under(&crate::toolbar::toolbar_id()) {
            return self.toolbar_click(id);
        }
        if id.is_under(&crate::breadcrumb::breadcrumb_id()) {
            return self.breadcrumb_click(id);
        }
        if id.is_under(&crate::stats_bar::stats_id()) {
            return self.bar_click(id);
        }
        self.tabs_click(id)
    }

    /// The messages a screen reader's request that arrived at `at` sends,
    /// for `a11y_act` to post: each is its own update, on the next turn, and
    /// a step's release follows them ([`Event::Applied`]), so its edit joins
    /// the open group. It applies none of them itself.
    ///
    /// A SetValue, Increment, or Decrement joins the screen reader's undo
    /// group, or opens one. Any other action that changes something ends
    /// the group first and is a step of its own.
    pub(crate) fn a11y_action(&mut self, request: &ActionRequest, at: std::time::Instant) -> Vec<crate::Message> {
        use crate::edits::ReaderEvent;
        // A held arrow key owns the edit in progress, and may step again.
        // A second owner would share its undo step, so the action waits:
        // it does nothing, as Undo does nothing while a key is held.
        if self.session.edits.arrow_group() == crate::edits::ArrowGroup::Held {
            return Vec::new();
        }
        // A part of the open panel, or of the overlay that owns the input,
        // acts as its item says.
        if let Some(messages) = self.a11y_panel_action(request, at) {
            return messages;
        }
        // The runtime's question owns the input over everything (row 3b):
        // a Click on an answer gives it, and Focus moves the runtime's
        // focus there, as Tab would.
        if let Some(id) = self
            .a11y
            .ids
            .id(request.target_node.0)
            .filter(|id| {
                id.is_under(&crate::overlays::access::question_id())
                    || id.is_under(&crate::overlays::access::help_id())
                    || id.is_under(&crate::licence_chooser::panel::licence_id())
                    || id.is_under(&crate::settings::preferences_id())
                    || self.runtime_menu().is_some_and(|menu| id.is_under(&menu.id))
                    || (!self.browser_stops().is_empty() && id.is_under(&crate::browser::browser_id()))
                    || (!self.patches_stops().is_empty() && id.is_under(&crate::patches::patches_id()))
                    || (self.inspector_owns() && id.is_under(&crate::inspector::panel::inspector_id()))
                    || (self.matrix_owns() && id.is_under(&crate::matrix::panel::matrix_id()))
            })
            .cloned()
        {
            // A step the runtime carried out on its own slider or select
            // joins the screen reader's group there; only a Click or a
            // Focus ends it (section 7, rule 6).
            if matches!(request.action, accesskit::Action::Click | accesskit::Action::Focus) {
                self.reader_event(ReaderEvent::Close);
            }
            return match request.action {
                accesskit::Action::Click => self
                    .question_click(&id)
                    .or_else(|| self.help_click(&id))
                    .or_else(|| self.chooser_click(&id))
                    .or_else(|| self.preferences_click(&id))
                    .or_else(|| self.menu_click(&id))
                    .or_else(|| self.browser_click(&id))
                    .or_else(|| self.patches_click(&id))
                    .or_else(|| self.inspector_click(&id))
                    .or_else(|| self.matrix_click(&id))
                    .into_iter()
                    .collect(),
                accesskit::Action::Focus
                    if self.our_stops().contains(&id)
                        || (self.inspector_owns() && id.is_under(&crate::inspector::panel::inspector_id()))
                        || (self.matrix_owns() && id.is_under(&crate::matrix::panel::matrix_id())) =>
                {
                    // The inspector sits beside the canvas, not over it: a
                    // knob that had the keys lets go, as on the canvas's
                    // own Focus path, so only the field has them.
                    if id.is_under(&crate::inspector::panel::inspector_id()) {
                        self.a11y.release_canvas_focus();
                    }
                    self.effect(loom_ui::Effects::focus(id));
                    Vec::new()
                }
                _ => Vec::new(),
            };
        }
        // An open panel owns input, as it does for the pointer: the
        // canvas behind it does not change.
        if !self.pointer_reaches_window() {
            return Vec::new();
        }
        let snapshot = self.ui_snapshot();
        let ids = self.a11y_widget_ids(&snapshot);
        if self.typed_field_action(&ids, request) {
            return Vec::new();
        }
        if let Some(id) = self.a11y.ids.id(request.target_node.0).cloned() {
            if matches!(request.action, accesskit::Action::Focus | accesskit::Action::Click)
                && self.right_tab == crate::matrix::Tab::Outline
                && snapshot.outline.iter().any(|row| row.id == id)
            {
                return vec![crate::Message::OutlineRow(id)];
            }
            if request.action == accesskit::Action::Focus
                && let Some((&(module, output, index), _)) = ids.ports.iter().find(|(_, port)| port.id == id)
            {
                self.focus_canvas_port(loom_canvas::PortRef { module, output, index });
                return Vec::new();
            }
            if request.action == accesskit::Action::Click
                && let Some(message) = self.jump_action(&id)
            {
                return vec![message];
            }
        }
        // ShowContextMenu on a tile opens its menu at the tile's top-left
        // corner, as the menu key does (section 6, rule 8).
        if request.action == accesskit::Action::ShowContextMenu {
            let Some(module) = self.a11y.ids.id(request.target_node.0).and_then(tree::tile_of) else {
                return Vec::new();
            };
            let Some((tile, view)) = snapshot.tiles.iter().find(|t| t.id == module).zip(self.a11y.canvas) else {
                return Vec::new();
            };
            let corner = view
                .camera
                .to_screen(loom_ui_core::Point::new(tile.bounds.x, tile.bounds.y));
            let event = loom_canvas::CanvasEvent::TileMenu {
                module,
                at: loom_ui_core::Point::new(view.bounds.x + corner.x, view.bounds.y + corner.y),
                area: view.bounds,
                by_key: true,
            };
            self.reader_event(ReaderEvent::Close);
            return vec![crate::Message::Canvas(event)];
        }
        // Focus on one of the runtime's stops (a toolbar button, a
        // breadcrumb entry, a ×, a developer switch) moves the runtime's
        // focus there, as Tab would, through a focus effect (ui-layer.md,
        // section 7, rule 6). Only a stop that shows now.
        if request.action == accesskit::Action::Focus
            && let Some(id) = self.a11y.ids.id(request.target_node.0)
            && self.our_stops().contains(id)
        {
            let id = id.clone();
            // A focus move ends the screen reader's undo group, as it does
            // on the canvas, so a later step starts its own.
            self.reader_event(crate::edits::ReaderEvent::Close);
            // The canvas lets go of the keyboard, selecting nothing, so the
            // bar's keys are the bar's (section 6, rule 1). Always, even
            // when the canvas has reported no focus: a request still on its
            // way would otherwise land on the next frame and take the keys
            // back. With no focus, a request for no control does nothing.
            // The canvas's answer, `FocusChanged(None)`, clears the app's
            // copy on its frame; it is cleared here too, so the next key
            // already sees it.
            self.a11y.release_canvas_focus();
            // The right column's own keyboard parts let go too: the tree
            // reports their focus before the runtime's, so an outline row
            // or an inspector action would keep the screen reader's.
            self.session.outline.active = false;
            self.session.outline.inspector_action = None;
            self.session.outline.close_jump();
            self.effect(loom_ui::Effects::focus(id));
            return Vec::new();
        }
        if request.action == accesskit::Action::Focus {
            // Focus on a control moves the canvas's keyboard focus there.
            // The canvas selects the control's tile as a press on it does.
            // It changes nothing in the patch, and Focus on a tile does
            // nothing (ui-layer.md, section 7, rules 9 and 20).
            if let Some(control) = actions::focus_target(&snapshot, &ids, &self.a11y.ids, request) {
                self.a11y.focus_request = (self.a11y.focus_request.0 + 1, Some(control));
                self.effect(loom_ui::Effects::redraw());
                // The keys go with the focus, by the path a press on the
                // canvas takes: the matrix lets go of them, and a text
                // field that has them closes, leaving its draft as a press
                // leaves it (dropped, unsubmitted). A press closes the
                // field itself; with no press, the app closes it, then
                // reads focus back as after a press.
                self.canvas_event(loom_canvas::CanvasEvent::PointerDown);
                if self.focus.is_some() {
                    self.effect(loom_ui::Effects::unfocus());
                }
            }
            return Vec::new();
        }
        // A button, a tab, or a switch outside the canvas sends the message
        // its press sends. Anything else is on the canvas.
        let messages = match self.a11y_frame_message(request) {
            Some(message) => vec![message],
            None => actions::messages(&snapshot, &ids, &self.a11y.ids, request),
        };
        if messages.is_empty() {
            // A Click that changes nothing, such as one on the option
            // already chosen, still ends an open group, as a pointer
            // press does. A node that is gone ends nothing.
            if !actions::joins_group(request.action) && actions::names_a_part(&ids, &self.a11y.ids, request) {
                self.reader_event(ReaderEvent::Close);
            }
            return Vec::new();
        }
        self.reader_event(if actions::joins_group(request.action) {
            ReaderEvent::Action {
                node: request.target_node.0,
                at,
            }
        } else {
            ReaderEvent::Close
        });
        messages
    }

    /// Applies text-range actions to the open value editor.
    fn typed_field_action(&mut self, ids: &tree::WidgetIds, request: &ActionRequest) -> bool {
        use accesskit::{Action, ActionData};
        let Some(typed) = &self.a11y.typed else { return false };
        let Some(control) = ids.get(typed.module, typed.widget) else {
            return false;
        };
        let field = control.clone().with("field");
        if self.a11y.ids.id(request.target_node.0) != Some(&field) {
            return false;
        }
        let byte = |position: accesskit::TextPosition| {
            (self.a11y.ids.id(position.node.0) == Some(&field.clone().with("text")))
                .then(|| {
                    typed
                        .text
                        .char_indices()
                        .map(|(index, _)| index)
                        .chain(std::iter::once(typed.text.len()))
                        .nth(position.character_index)
                })
                .flatten()
        };
        let edit = match (request.action, &request.data) {
            (Action::SetTextSelection, Some(ActionData::SetTextSelection(selection))) => {
                let (Some(anchor), Some(caret)) = (byte(selection.anchor), byte(selection.focus)) else {
                    return true;
                };
                loom_canvas::TypedEdit::Select { anchor, caret }
            }
            (Action::ReplaceSelectedText, Some(ActionData::Value(text))) => {
                loom_canvas::TypedEdit::Replace(text.to_string())
            }
            _ => return true,
        };
        self.a11y.typed_request.0 += 1;
        self.a11y.typed_request.1 = Some(loom_canvas::TypedRequest {
            control: (typed.module, typed.widget),
            text: typed.text.clone(),
            edit,
        });
        self.effect(loom_ui::Effects::redraw());
        true
    }

    /// Carries out a screen reader's action that arrived at `at`: its
    /// messages are posted, each its own update on the next turn, and the
    /// step's release follows them ([`Event::Applied`]), so its edit joins
    /// the open group. With no message, the tree shows what changed at once.
    /// True when the action sent a message.
    pub(crate) fn a11y_act(&mut self, request: &ActionRequest, at: std::time::Instant) -> bool {
        let messages = self.a11y_action(request, at);
        if messages.is_empty() {
            self.a11y_rebuild();
            return false;
        }
        for message in messages {
            self.post(message);
        }
        self.post(crate::Message::A11y(Event::Applied));
        true
    }

    /// Handles the accessibility layer's events. Actions go through the same
    /// messages as the pointer, so undo, MIDI feedback, and autosave see
    /// them as ordinary edits.
    pub(crate) fn a11y_event(&mut self, event: Event) {
        match event {
            Event::Activated => {
                self.a11y.active = true;
                // The screen reader has nothing yet: send the whole tree.
                self.a11y.sent.clear();
                self.a11y_rebuild();
            }
            Event::Deactivated => {
                self.a11y.active = false;
                self.a11y.live_dirty = false;
                self.a11y.sent.clear();
            }
            Event::LiveTick => {
                self.a11y.flush_live = true;
                self.a11y_rebuild();
            }
            Event::FieldChanged => self.a11y_rebuild(),
            Event::LocateAfterLayout => {
                if self.a11y.active {
                    self.a11y_read_places();
                } else {
                    self.a11y.locating = false;
                }
            }
            Event::Located(places) => {
                self.a11y.locating = false;
                if self.a11y.active {
                    let start = self.a11y.timed.then(std::time::Instant::now);
                    let (whole, changes) = self.a11y_changes(&places);
                    if let Some(start) = start {
                        BuildTimes::record(start.elapsed());
                    }
                    self.effect(loom_ui::Effects::accessibility(whole, changes));
                }
                if std::mem::take(&mut self.a11y.again) {
                    self.a11y_rebuild();
                }
            }
            Event::Action(request) => {
                self.a11y_act(&request, std::time::Instant::now());
            }
            Event::Applied => {
                // The step's release, at once: the group now waits for the
                // next. Then the new value is shown.
                self.reader_event(crate::edits::ReaderEvent::Applied);
                self.a11y_rebuild();
            }
        }
    }
}

//! Signal-flow rows, keyboard navigation and cable following for the canvas.

use crate::patch_map::{PatchMap, Structure, Unit, names};
use crate::style::button;
use crate::{App, Element, Message, style};
use iced::keyboard::{
    self, Key,
    key::{Code, Named, Physical},
};
use iced::widget::{column, container, scrollable, text};
use loom_canvas::PortRef;
use loom_ui_core::{Point, UiId};
use serde::{Serialize, Serializer};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The outline's scrollable tree.
pub(crate) fn tree_id() -> UiId {
    UiId::panel("outline")
}

/// The stable row ID of a tile.
fn module_id(module: u32) -> UiId {
    tree_id().with("module").with(module)
}

/// Serializes a UI ID in the form accepted by the control channel.
fn serialize_id<S: Serializer>(id: &UiId, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(id)
}

/// One row in the visible outline, in tree order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Row {
    #[serde(serialize_with = "serialize_id")]
    pub(crate) id: UiId,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) level: usize,
    pub(crate) expanded: Option<bool>,
    pub(crate) position: usize,
    pub(crate) size: usize,
    #[serde(skip)]
    parent: Option<UiId>,
    #[serde(skip)]
    target: Option<Target>,
}

/// The part of the level where a jump starts or ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Tile(u32),
    Port(PortRef),
    Assignment { id: u32, source: bool },
}

impl Target {
    /// The tile that owns this place.
    fn module(self, structure: &Structure) -> Option<u32> {
        match self {
            Self::Tile(module) => Some(module),
            Self::Port(port) => Some(port.module),
            Self::Assignment { id, source } => {
                let assignment = structure.modulations.iter().find(|assignment| assignment.id == id)?;
                Some(if source { assignment.from.module } else { assignment.to })
            }
        }
    }
}

/// One possible jump with its connection's spoken label.
#[derive(Debug)]
struct Choice {
    target: Target,
    name: String,
}

/// A choice list opened while following a branch in the signal.
#[derive(Debug)]
struct Jump {
    from: Target,
    choices: Vec<Choice>,
    selected: usize,
}

/// Rows shared by the view, keyboard and accessibility updates.
#[derive(Debug)]
struct CachedRows {
    revision: (u64, u64),
    patch: String,
    rows: Rc<[Row]>,
}

/// What the outline and cable navigation remember in the current level.
#[derive(Debug, Default)]
pub(crate) struct State {
    pub(crate) active: bool,
    pub(crate) focus_port: Option<PortRef>,
    pub(crate) centre: (u64, Option<Point>),
    /// The toolbar button last focused in this level, where Ctrl+Tab
    /// returns to the toolbar (ui-layer.md, section 6, rule 7). The
    /// toolbar's focus itself is the runtime's, on one of its stops.
    pub(crate) last_toolbar: Option<UiId>,
    /// A focused inspector action when the panel has no editable field.
    pub(crate) inspector_action: Option<UiId>,
    /// The last real Inspector field or action focused in this level.
    pub(crate) last_inspector: Option<UiId>,
    /// The canvas control most recently left by Ctrl+Tab.
    last_canvas: Option<(u32, usize)>,
    focus: Option<UiId>,
    index: usize,
    expanded: BTreeMap<UiId, bool>,
    rows: RefCell<Option<CachedRows>>,
    search: Option<(Instant, String)>,
    jump: Option<Jump>,
    back: VecDeque<Target>,
}

impl State {
    /// Invalidates the visible rows when an expanded branch changes.
    fn set_expanded(&mut self, id: UiId, expanded: bool) {
        if self.expanded.insert(id, expanded) != Some(expanded) {
            *self.rows.get_mut() = None;
        }
    }

    /// True while a cable branch has focus.
    pub(crate) fn has_jump(&self) -> bool {
        self.jump.is_some()
    }

    /// Closes the jump list without changing its starting place.
    pub(crate) fn close_jump(&mut self) {
        self.jump = None;
    }

    /// Closes a jump whose starting point disappeared in a graph edit.
    pub(crate) fn reconcile_jump(&mut self, structure: &Structure) {
        if self.jump.as_ref().is_some_and(|jump| {
            jump.from
                .module(structure)
                .is_none_or(|module| !structure.tiles.contains_key(&module))
        }) {
            self.close_jump();
        }
    }

    /// Drops navigation state referring to the previous level.
    pub(crate) fn level_changed(&mut self) {
        self.focus = None;
        self.index = 0;
        self.expanded.clear();
        *self.rows.get_mut() = None;
        self.search = None;
        self.jump = None;
        self.back.clear();
        self.focus_port = None;
        self.centre.1 = None;
        self.inspector_action = None;
        self.last_inspector = None;
        self.last_canvas = None;
    }

    /// Remembers the row that now receives the tree's keys.
    fn select(&mut self, rows: &[Row], index: usize) {
        if let Some(row) = rows.get(index) {
            self.focus = Some(row.id.clone());
            self.index = index;
        }
    }

    /// Finds the focused row, falling forwards or backwards if it disappeared.
    fn focused_index(&self, rows: &[Row]) -> usize {
        self.focus
            .as_ref()
            .and_then(|id| rows.iter().position(|row| &row.id == id))
            .unwrap_or_else(|| self.index.min(rows.len().saturating_sub(1)))
    }

    /// Reveals a module row and the section or loop containing it.
    fn reveal(&mut self, map: &PatchMap, module: u32) {
        if !map.structure.heard.contains(&module) {
            self.set_expanded(tree_id().with("not-heard"), true);
        }
        if let Some(group) = map.structure.loops.iter().find(|group| group.members.contains(&module)) {
            self.set_expanded(loop_id(group), true);
        }
        self.focus = Some(module_id(module));
    }
}

/// Enabled Inspector fields and controls in the model's reading order.
fn inspector_stops(items: &[crate::a11y::items::Item]) -> Vec<&crate::a11y::items::Item> {
    let mut stops = Vec::new();
    for item in items {
        if item.enabled
            && (item.focus.is_some() || item.click.is_some() || item.step.0.is_some() || item.step.1.is_some())
        {
            stops.push(item);
        }
        stops.extend(inspector_stops(&item.children));
    }
    stops
}

/// A feedback group keeps its ID when its displayed number changes.
fn loop_id(group: &crate::patch_map::Loop) -> UiId {
    tree_id()
        .with("loop")
        .with(group.members.iter().copied().min().unwrap_or_default())
}

/// Builds the name shared between an outline row and its tile description.
pub(crate) fn neighbours(structure: &Structure, module: u32) -> String {
    let incoming: BTreeSet<_> = structure
        .cables
        .iter()
        .filter(|wire| wire.to.module == module)
        .map(|wire| wire.from.module)
        .collect();
    let outgoing: BTreeSet<_> = structure
        .cables
        .iter()
        .filter(|wire| wire.from.module == module)
        .map(|wire| wire.to.module)
        .collect();
    let list = |ids: BTreeSet<_>| names(ids.into_iter().map(|id| structure.name(id).to_string()), 3);
    let mut fragments = Vec::new();
    if incoming.is_empty() {
        if !outgoing.is_empty() {
            fragments.push("Source.".into());
        }
    } else {
        fragments.push(format!("From {}.", list(incoming)));
    }
    if structure.tiles.get(&module).is_some_and(|tile| tile.output) {
        fragments.push("The output.".into());
    } else if !outgoing.is_empty() {
        fragments.push(format!("To {}.", list(outgoing)));
    }
    fragments.join(" ")
}

/// The other ends of a connected port.
pub(crate) fn port_neighbours(structure: &Structure, port: PortRef, sentence: bool) -> String {
    let peers = names(
        structure
            .cables
            .iter()
            .filter_map(|wire| {
                if port.output {
                    (wire.from == port).then_some(wire.to)
                } else {
                    (wire.to == port).then_some(wire.from)
                }
            })
            .map(|peer| format!("{} {}", structure.name(peer.module), structure.port_label(peer))),
        usize::MAX,
    );
    if peers.is_empty() {
        return if sentence { "Not connected" } else { "not connected" }.into();
    }
    let direction = match (sentence, port.output) {
        (true, true) => "To",
        (true, false) => "From",
        (false, true) => "to",
        (false, false) => "from",
    };
    format!("{direction} {peers}")
}

impl Row {
    /// Creates a leaf. Its place in the sibling set is filled after construction.
    fn leaf(
        id: UiId,
        name: String,
        description: String,
        level: usize,
        parent: Option<UiId>,
        target: Option<Target>,
    ) -> Self {
        Self {
            id,
            name,
            description,
            level,
            parent,
            target,
            expanded: None,
            position: 0,
            size: 0,
        }
    }
}

/// The visible rows of a module and its expanded connections.
fn module_rows(rows: &mut Vec<Row>, map: &PatchMap, state: &State, module: u32, level: usize, parent: Option<UiId>) {
    let structure = &map.structure;
    let Some(tile) = structure.tiles.get(&module) else {
        return;
    };
    let id = module_id(module);
    let about = neighbours(structure, module);
    let name = if about.is_empty() {
        tile.name.clone()
    } else {
        format!("{}. {about}", tile.name)
    };
    let mut description = vec![tile.kind.clone()];
    let mut connections = Vec::new();
    for (output, ports) in [(false, &tile.inputs), (true, &tile.outputs)] {
        for (index, info) in ports.iter().enumerate() {
            let label = &info.label;
            let port = PortRef { module, output, index };
            let edges: Vec<_> = structure
                .cables
                .iter()
                .filter(|wire| if output { wire.from == port } else { wire.to == port })
                .collect();
            if edges.is_empty() {
                continue;
            }
            let key = if output { "output" } else { "input" };
            let about = port_neighbours(structure, port, false);
            let width = if edges.iter().any(|wire| wire.stereo) {
                "Stereo"
            } else {
                "Mono"
            };
            connections.push(Row::leaf(
                id.clone().with(key).with(info.key.as_str()),
                format!("{label}, {about}"),
                format!("{}, {width}", edges[0].signal.name()),
                level + 1,
                Some(id.clone()),
                Some(Target::Port(port)),
            ));
        }
    }
    for assignment in structure
        .modulations
        .iter()
        .filter(|line| line.to == module || line.from.module == module)
    {
        let off = if assignment.enabled { "" } else { ", off" };
        let rate = assignment.depth * 100.0;
        if assignment.to == module {
            let from = structure.name(assignment.from.module);
            description.push(format!("{} modulated by {from}{off}.", assignment.parameter));
            connections.push(Row::leaf(
                id.clone().with("modulation-in").with(assignment.id),
                format!("{}, modulated by {from}, {rate:.0}%{off}", assignment.parameter),
                String::new(),
                level + 1,
                Some(id.clone()),
                Some(Target::Assignment {
                    id: assignment.id,
                    source: false,
                }),
            ));
        }
        if assignment.from.module == module {
            let to = structure.name(assignment.to);
            connections.push(Row::leaf(
                id.clone().with("modulation-out").with(assignment.id),
                format!("Modulates {to} {}, {rate:.0}%{off}", assignment.parameter),
                String::new(),
                level + 1,
                Some(id.clone()),
                Some(Target::Assignment {
                    id: assignment.id,
                    source: true,
                }),
            ));
        }
    }
    let expanded = !connections.is_empty() && state.expanded.get(&id).copied().unwrap_or(false);
    let mut row = Row::leaf(
        id,
        name,
        description.join(" "),
        level,
        parent,
        Some(Target::Tile(module)),
    );
    row.expanded = (!connections.is_empty()).then_some(expanded);
    rows.push(row);
    if expanded {
        rows.extend(connections);
    }
}

/// Appends one ordered module or loop with its visible descendants.
fn unit_rows(rows: &mut Vec<Row>, map: &PatchMap, state: &State, unit: Unit, level: usize, parent: Option<UiId>) {
    match unit {
        Unit::Tile(module) => module_rows(rows, map, state, module, level, parent),
        Unit::Loop(index) => {
            let group = &map.structure.loops[index];
            let id = loop_id(group);
            let number = map
                .order
                .iter()
                .filter(|unit| matches!(unit, Unit::Loop(_)))
                .position(|unit| *unit == Unit::Loop(index))
                .unwrap_or(0)
                + 1;
            let members = group
                .members
                .iter()
                .map(|id| map.structure.name(*id))
                .collect::<Vec<_>>()
                .join(", ");
            let expanded = state.expanded.get(&id).copied().unwrap_or(true);
            let mut row = Row::leaf(
                id.clone(),
                format!("Feedback loop {number}: {members}"),
                group.delays.clone(),
                level,
                parent,
                None,
            );
            row.expanded = Some(expanded);
            rows.push(row);
            if expanded {
                for module in &group.members {
                    module_rows(rows, map, state, *module, level + 1, Some(id.clone()));
                }
            }
        }
    }
}

impl App {
    /// Builds the visible rows used by the view, snapshot, and accessibility tree.
    pub(crate) fn outline_rows(&self) -> Rc<[Row]> {
        let session = &self.session;
        let map = &session.patch_map;
        let revision = (map.generation, map.row_revision);
        let patch = session.document.name();
        let mut cache = session.outline.rows.borrow_mut();
        let rows = cache
            .as_mut()
            .filter(|cached| cached.revision == revision && cached.patch == patch);
        if let Some(cached) = rows {
            return Rc::clone(&cached.rows);
        }
        let rows = Rc::from(self.build_outline_rows());
        *cache = Some(CachedRows {
            revision,
            patch: patch.to_string(),
            rows: Rc::clone(&rows),
        });
        rows
    }

    /// Computes rows after a change to the structure, order or expanded branches.
    fn build_outline_rows(&self) -> Vec<Row> {
        let session = &self.session;
        let map = &session.patch_map;
        let state = &session.outline;
        let structure = &map.structure;
        let level_path = session.hierarchy.path_names(&session.level);
        let patch = session.document.name();
        let label = if level_path.is_empty() {
            patch.to_string()
        } else {
            format!("Inside {}, in {patch}", level_path.join(" / "))
        };
        let unheard = structure.tiles.len().saturating_sub(structure.heard.len());
        let loops = if structure.loops.is_empty() {
            "no feedback loops".into()
        } else {
            count(structure.loops.len(), "feedback loop")
        };
        let summary = format!(
            "{label}. {}, {}, {loops}. {} not heard.",
            count(structure.tiles.len(), "module"),
            count(structure.cables.len(), "cable"),
            count(unheard, "module")
        );
        let mut rows = vec![Row::leaf(
            tree_id().with("summary"),
            summary,
            String::new(),
            1,
            None,
            None,
        )];
        let is_heard = |unit| match unit {
            Unit::Tile(id) => structure.heard.contains(&id),
            Unit::Loop(index) => structure.loops[index]
                .members
                .iter()
                .any(|id| structure.heard.contains(id)),
        };
        for unit in map.order.iter().copied().filter(|unit| is_heard(*unit)) {
            unit_rows(&mut rows, map, state, unit, 1, None);
        }
        if unheard > 0 {
            let id = tree_id().with("not-heard");
            let expanded = state.expanded.get(&id).copied().unwrap_or(true);
            let mut row = Row::leaf(
                id.clone(),
                format!("Not heard, {}", count(unheard, "module")),
                String::new(),
                1,
                None,
                None,
            );
            row.expanded = Some(expanded);
            rows.push(row);
            if expanded {
                for unit in map.order.iter().copied().filter(|unit| !is_heard(*unit)) {
                    unit_rows(&mut rows, map, state, unit, 2, Some(id.clone()));
                }
            }
        }
        let mut size = BTreeMap::new();
        for row in &rows {
            *size.entry(row.parent.clone()).or_insert(0) += 1;
        }
        let mut position = BTreeMap::new();
        for row in &mut rows {
            let at = position.entry(row.parent.clone()).or_insert(0);
            *at += 1;
            row.position = *at;
            row.size = size[&row.parent];
        }
        rows
    }

    /// Draws the outline as one focus stop with stable IDs for every row.
    pub(crate) fn outline_view(&self) -> Element<'_, Message> {
        let rows = self.outline_rows();
        let index = self.session.outline.focused_index(&rows);
        let active = self.session.outline.active;
        let content = column(rows.iter().enumerate().map(|(i, row)| {
            let mark = match row.expanded {
                Some(true) => "▾ ",
                Some(false) => "▸ ",
                None => "",
            };
            let label = text(format!("{mark}{}", row.name)).size(style::BODY);
            let id = row.id.to_string();
            container(
                button(label)
                    .width(iced::Length::Fill)
                    .padding([5, 8 + ((row.level - 1) * 12) as u16])
                    .style(style::tab(active && i == index))
                    .on_press(Message::OutlineRow(row.id.clone())),
            )
            .id(id)
            .into()
        }))
        .spacing(2);
        container(scrollable(content).height(iced::Length::Fill))
            .id(tree_id().to_string())
            .into()
    }

    /// Draws the short list of possible destinations while a jump is pending.
    pub(crate) fn jump_view(&self) -> Option<Element<'_, Message>> {
        let jump = self.session.outline.jump.as_ref()?;
        let choices = column(jump.choices.iter().enumerate().map(|(index, choice)| {
            container(
                button(text(choice.name.as_str()).size(style::BODY))
                    .width(iced::Length::Fill)
                    .style(style::tab(index == jump.selected))
                    .on_press(Message::JumpChoice(index)),
            )
            .id(jump_id().with(index as u64).to_string())
            .into()
        }));
        Some(
            container(
                scrollable(
                    column![
                        text(count(jump.choices.len(), "connection")).size(style::HEADING),
                        choices
                    ]
                    .spacing(6),
                )
                .height(iced::Length::Fill),
            )
            .id(jump_id().to_string())
            .into(),
        )
    }

    /// The row with keyboard focus while the outline has the keys.
    pub(crate) fn outline_focus(&self, rows: &[Row]) -> Option<UiId> {
        (self.session.outline.active && self.right_tab == crate::matrix::Tab::Outline)
            .then(|| {
                rows.get(self.session.outline.focused_index(rows))
                    .map(|row| row.id.clone())
            })
            .flatten()
    }

    /// Takes keyboard focus on behalf of a click or an accessibility action.
    pub(crate) fn outline_select(&mut self, id: &UiId) {
        let rows = self.outline_rows();
        if let Some(index) = rows.iter().position(|row| &row.id == id) {
            self.session.outline.active = true;
            self.session.outline.select(&rows, index);
            self.reveal_outline_focus();
        }
    }

    /// Handles the physical navigation keys and the outline's tree keys.
    pub(crate) fn navigation_key(&mut self, event: &keyboard::Event, now: Instant) -> bool {
        let keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            repeat,
            ..
        } = event
        else {
            return false;
        };
        if !self.pointer_reaches_window() {
            return false;
        }
        let key = key.as_ref();
        if key == Key::Named(Named::Tab) && modifiers.control() && !modifiers.alt() {
            self.cycle_region(modifiers.shift());
            return true;
        }
        if self.toolbar_has_focus() && self.toolbar_key(&key, modifiers.is_empty()) {
            return true;
        }
        let outline = self.session.outline.active && self.right_tab == crate::matrix::Tab::Outline;
        if self.session.outline.has_jump() && modifiers.is_empty() && self.jump_key(&key) {
            return true;
        }
        if self.session.outline.inspector_action.is_some() && modifiers.is_empty() {
            return self.inspector_action_key(&key);
        }
        if self.focus.is_some() || self.session.value_draft.is_some() || self.matrix_has_keys() {
            return false;
        }
        if !outline
            && !self.session.outline.has_jump()
            && !self.toolbar_has_focus()
            && modifiers.is_empty()
            && let Key::Character(letter) = &key
        {
            if loom_canvas::shortcuts::CABLE.matches_letter(letter) {
                return self.open_cable_picker();
            }
            // The canvas uses the pointer when it has one over the patch.
            // Elsewhere only a key the window's widgets left reaches here.
            if !repeat && loom_canvas::shortcuts::ADD.matches_letter(letter) {
                self.post(Message::OpenBrowser);
                return true;
            }
        }
        if !outline && matches!(key, Key::Character(c) if c.eq_ignore_ascii_case("o")) && modifiers.is_empty() {
            self.right_tab = crate::matrix::Tab::Outline;
            self.session.outline.active = true;
            return true;
        }
        if *physical_key == Physical::Code(Code::BracketLeft) && modifiers.command() && !modifiers.alt() {
            self.jump_back(outline);
            return true;
        }
        if modifiers.is_empty() && matches!(physical_key, Physical::Code(Code::BracketLeft | Code::BracketRight)) {
            if let Some(from) = self.navigation_target(outline) {
                self.follow(from, Some(*physical_key == Physical::Code(Code::BracketRight)), outline);
            }
            return true;
        }
        if outline {
            if !modifiers.is_empty() && !(modifiers.command() && key == Key::Named(Named::Enter)) {
                return false;
            }
            self.outline_key(key, modifiers.command(), now);
            self.reveal_outline_focus();
        }
        outline
    }

    /// Moves focus to the adjacent region, remembering the outline's row.
    fn cycle_region(&mut self, back: bool) {
        use crate::matrix::Tab;
        let toolbar = self.toolbar_has_focus();
        let state = &mut self.session.outline;
        let current = if toolbar {
            2
        } else if state.active
            || state.inspector_action.is_some()
            || self.session.matrix.focused
            || self.focus.is_some()
        {
            1
        } else {
            0
        };
        if current == 0 {
            state.last_canvas = self.a11y.focus();
        }
        state.close_jump();
        let next = (current + if back { 2 } else { 1 }) % 3;
        state.active = next == 1 && self.right_tab == Tab::Outline;
        state.inspector_action = None;
        self.session.matrix.focused = next == 1 && self.right_tab == Tab::Matrix;
        if next == 1 && self.right_tab == Tab::Inspector {
            self.a11y.request_control(None);
            self.focus_inspector_region();
            return;
        }
        let last_canvas = state
            .last_canvas
            .filter(|(module, _)| self.session.selection.contains(module));
        if next == 2 {
            // The toolbar is one stop: the button last focused here, or
            // the first the first time, as Tab enters it.
            if let Some(stop) = self.toolbar_entry() {
                self.effect(loom_ui::Effects::focus(stop));
            }
            self.effect(loom_ui::Effects::redraw());
            return;
        }
        if self.focus.is_some() {
            self.effect(loom_ui::Effects::unfocus());
        }
        if next == 0 {
            self.a11y.request_control(last_canvas);
            self.effect(loom_ui::Effects::redraw());
        }
    }

    /// Focuses the last live Inspector stop or the first editable field.
    fn focus_inspector_region(&mut self) {
        let panel = crate::inspector::access::items(&self.inspector_model());
        let stops = inspector_stops(&panel);
        let saved = self.session.outline.last_inspector.as_ref();
        let stop = saved
            .and_then(|id| stops.iter().copied().find(|item| &item.id == id))
            .or_else(|| stops.iter().copied().find(|item| item.focus.is_some()))
            .or_else(|| stops.first().copied());
        if let Some(stop) = stop {
            self.focus_inspector_stop(stop);
        } else if let Some(item) = panel.first() {
            self.session.outline.inspector_action = Some(item.id.clone());
        } else {
            self.session.outline.inspector_action = Some(crate::a11y::tree::tab_id(crate::matrix::Tab::Inspector));
        }
    }

    /// Takes a real text field through the focus bridge, or a keyboard-operable action.
    fn focus_inspector_stop(&mut self, item: &crate::a11y::items::Item) {
        self.session.outline.last_inspector = Some(item.id.clone());
        self.effect(loom_ui::Effects::scroll_into_view(item.id.clone()));
        if let Some(part) = item.focus.as_ref().and_then(crate::places::field_of) {
            self.session.outline.inspector_action = None;
            self.effect(loom_ui::Effects::focus(part));
        } else {
            self.session.outline.inspector_action = Some(item.id.clone());
        }
    }

    /// Activates an inspector stop that has no toolkit text field.
    fn inspector_action_key(&mut self, key: &Key<&str>) -> bool {
        use crate::a11y::items;
        use accesskit::Action;
        let panel = crate::inspector::access::items(&self.inspector_model());
        let id = self.session.outline.inspector_action.as_ref();
        let action = match key {
            Key::Named(Named::Escape) => {
                self.session.outline.inspector_action = None;
                self.a11y.request_control(self.session.outline.last_canvas);
                self.effect(loom_ui::Effects::redraw());
                return true;
            }
            Key::Named(Named::Enter | Named::Space) => Action::Click,
            Key::Named(Named::ArrowRight | Named::ArrowUp) => Action::Increment,
            Key::Named(Named::ArrowLeft | Named::ArrowDown) => Action::Decrement,
            _ => return false,
        };
        if let Some(item) = id.and_then(|id| items::find(&panel, id)) {
            for message in items::messages(item, action, None) {
                self.post(message);
            }
        }
        true
    }

    /// Gives the outline's newly focused row room in its scrolling view.
    fn reveal_outline_focus(&mut self) {
        if let Some(id) = self.outline_focus(&self.outline_rows()) {
            self.effect(loom_ui::Effects::scroll_into_view(id));
        }
    }

    /// True while one of the toolbar's buttons has the runtime's focus:
    /// the toolbar region of ui-layer.md, section 6, rule 7.
    pub(crate) fn toolbar_has_focus(&self) -> bool {
        self.focus
            .as_ref()
            .is_some_and(|id| id.is_under(&crate::toolbar::toolbar_id()))
    }

    /// Moves the runtime's focus among the turned-on toolbar buttons with
    /// the arrow keys, Home, and End, and leaves the toolbar for the canvas
    /// with Escape (the WAI-ARIA toolbar pattern; Brylie's decision,
    /// 2026-11-19). The arrows stop at the ends, as #849's did; turned-off
    /// buttons are no stops. Enter, Space, and Tab stay the runtime's,
    /// which clicks the focused button and leaves the toolbar. Other plain
    /// keys stay here, so they cannot edit the canvas. The move is posted
    /// for a turn of its own, so a message the same key sent elsewhere
    /// (Escape also closes help) runs while the focus is still here.
    fn toolbar_key(&mut self, key: &Key<&str>, plain: bool) -> bool {
        if !plain || matches!(key, Key::Named(Named::Enter | Named::Space | Named::Tab)) {
            return false;
        }
        let stops = self.toolbar_stops();
        let at = self
            .focus
            .as_ref()
            .and_then(|id| stops.iter().position(|stop| stop == id));
        let last = stops.len().saturating_sub(1);
        let to = match key {
            Key::Named(Named::ArrowLeft) => at.map(|at| at.saturating_sub(1)),
            Key::Named(Named::ArrowRight) => at.map(|at| (at + 1).min(last)),
            Key::Named(Named::Home) => Some(0),
            Key::Named(Named::End) => Some(last),
            Key::Named(Named::Escape) => {
                self.post(Message::ToolbarFocus(None));
                return true;
            }
            _ => None,
        };
        if let Some(stop) = to.filter(|to| Some(*to) != at).and_then(|to| stops.get(to)) {
            self.post(Message::ToolbarFocus(Some(stop.clone())));
        }
        true
    }

    /// Carries out [`Message::ToolbarFocus`]: focuses `stop`, or with `None`
    /// lets the toolbar's focus go and gives the canvas back the control it
    /// last had.
    pub(crate) fn toolbar_focus(&mut self, stop: Option<UiId>) {
        match stop {
            Some(stop) => self.effect(loom_ui::Effects::focus(stop)),
            None => {
                if self.toolbar_has_focus() {
                    self.effect(loom_ui::Effects::unfocus());
                }
                self.a11y.request_control(self.session.outline.last_canvas);
            }
        }
        self.effect(loom_ui::Effects::redraw());
    }

    /// Visits, expands, and activates rows using the standard tree keys.
    fn outline_key(&mut self, key: Key<&str>, command: bool, now: Instant) {
        let rows = self.outline_rows();
        let index = self.session.outline.focused_index(&rows);
        let row = &rows[index];
        let state = &mut self.session.outline;
        match key {
            Key::Named(Named::ArrowDown) => state.select(&rows, (index + 1).min(rows.len() - 1)),
            Key::Named(Named::ArrowUp) => state.select(&rows, index.saturating_sub(1)),
            Key::Named(Named::Home) => state.select(&rows, 0),
            Key::Named(Named::End) => state.select(&rows, rows.len() - 1),
            Key::Named(Named::ArrowRight) => match row.expanded {
                Some(false) => {
                    state.set_expanded(row.id.clone(), true);
                }
                Some(true) => state.select(&rows, index + 1),
                None => {}
            },
            Key::Named(Named::ArrowLeft) => {
                if row.expanded == Some(true) {
                    state.set_expanded(row.id.clone(), false);
                } else if let Some(parent) = &row.parent
                    && let Some(index) = rows.iter().position(|row| &row.id == parent)
                {
                    state.select(&rows, index);
                }
            }
            Key::Named(Named::Escape) => {
                let level = &self.session.level;
                if let Some(parent) = level.parent()
                    && let Some(instance) = self.session.hierarchy.id_at(&parent, level.last())
                {
                    self.go_up();
                    self.session.outline.reveal(&self.session.patch_map, instance.0);
                } else {
                    self.session.outline.active = false;
                }
            }
            Key::Named(Named::Enter) => match row.target {
                Some(Target::Tile(module)) if command && self.session.patch_map.structure.tiles[&module].instance => {
                    self.open_tile(module)
                }
                Some(Target::Tile(module)) if !command => self.focus_outline_tile(module),
                Some(target) if !command => self.follow(target, None, true),
                _ => {}
            },
            Key::Named(Named::Space) => {
                if let Some(module) = row
                    .target
                    .and_then(|target| target.module(&self.session.patch_map.structure))
                {
                    self.centre_tile(module);
                }
            }
            Key::Character(chars) if !command && chars.chars().all(char::is_alphanumeric) => {
                let (at, search) = state.search.get_or_insert_with(|| (now, String::new()));
                if now.saturating_duration_since(*at) >= Duration::from_secs(1) {
                    search.clear()
                }
                *at = now;
                search.push_str(&chars.to_lowercase());
                if let Some(found) = (1..=rows.len())
                    .map(|step| (index + step) % rows.len())
                    .find(|i| rows[*i].name.to_lowercase().starts_with(search.as_str()))
                {
                    state.select(&rows, found);
                }
            }
            _ => {}
        }
    }

    /// Selects and centres a tile without recording a patch edit.
    fn centre_tile(&mut self, module: u32) {
        if let Some(tile) = self.session.scene.module(module) {
            self.session.selection = [module].into();
            let request = &mut self.session.outline.centre;
            *request = (request.0 + 1, Some(tile.bounds().center()));
            self.effect(loom_ui::Effects::redraw());
        }
    }

    /// Hands focus to the canvas and the tile's first control, when present.
    fn focus_outline_tile(&mut self, module: u32) {
        self.session.outline.active = false;
        self.centre_tile(module);
        let widget = self.session.scene.module(module).and_then(|tile| {
            tile.panel
                .widgets
                .iter()
                .position(|widget| widget.is_control() && !widget.hidden)
        });
        self.a11y.request_control(widget.map(|widget| (module, widget)));
    }

    /// Finds the focused place in the view where the key was pressed.
    fn navigation_target(&self, outline: bool) -> Option<Target> {
        if outline {
            let rows = self.outline_rows();
            return rows.get(self.session.outline.focused_index(&rows))?.target;
        }
        if let Some(port) = self.session.outline.focus_port {
            return Some(Target::Port(port));
        }
        if let Some((module, _)) = self.a11y.focus() {
            return Some(Target::Tile(module));
        }
        let mut selected = self.session.selection.iter();
        match (selected.next(), selected.next()) {
            (Some(module), None) => Some(Target::Tile(*module)),
            _ => None,
        }
    }

    /// Takes a cable step or opens choices when the signal branches.
    fn follow(&mut self, from: Target, downstream: Option<bool>, outline: bool) {
        let map = &self.session.patch_map;
        let mut choices = jump_choices(map, from, downstream);
        match choices.len() {
            0 => self.alerts.say(if downstream == Some(true) {
                "Nothing downstream"
            } else {
                "Nothing upstream"
            }),
            1 => self.jump_to(from, choices.remove(0).target, outline),
            _ => {
                self.alerts.say(count(choices.len(), "connection"));
                self.session.outline.jump = Some(Jump {
                    from,
                    choices,
                    selected: 0,
                });
            }
        }
    }

    /// Moves through jump choices without changing the underlying focus.
    fn jump_key(&mut self, key: &Key<&str>) -> bool {
        let Some(jump) = self.session.outline.jump.as_mut() else {
            return false;
        };
        match key {
            Key::Named(Named::ArrowDown) => jump.selected = (jump.selected + 1).min(jump.choices.len() - 1),
            Key::Named(Named::ArrowUp) => jump.selected = jump.selected.saturating_sub(1),
            Key::Named(Named::Enter) => {
                let selected = jump.selected;
                self.choose_jump(selected);
            }
            Key::Named(Named::Escape) => self.session.outline.close_jump(),
            _ => return false,
        }
        true
    }

    /// Jumps to the chosen destination of the open list.
    pub(crate) fn choose_jump(&mut self, index: usize) {
        if let Some(jump) = self.session.outline.jump.take()
            && let Some(choice) = jump.choices.get(index)
        {
            self.jump_to(jump.from, choice.target, self.session.outline.active);
        }
    }

    /// Remembers this starting point and visits the destination in the same view.
    fn jump_to(&mut self, from: Target, target: Target, outline: bool) {
        let back = &mut self.session.outline.back;
        if back.len() == 50 {
            back.pop_front();
        }
        back.push_back(from);
        self.land(target, outline);
    }

    /// Returns to the last live starting point in this level.
    fn jump_back(&mut self, outline: bool) {
        while let Some(target) = self.session.outline.back.pop_back() {
            if target
                .module(&self.session.patch_map.structure)
                .is_some_and(|module| self.session.patch_map.structure.tiles.contains_key(&module))
            {
                self.land(target, outline);
                return;
            }
        }
        self.alerts.say("No earlier place");
    }

    /// Updates focus and camera at a destination.
    fn land(&mut self, target: Target, outline: bool) {
        let Some(module) = target.module(&self.session.patch_map.structure) else {
            return;
        };
        if outline {
            self.session.outline.reveal(&self.session.patch_map, module);
            self.reveal_outline_focus();
        } else {
            self.a11y.request_control(None);
            self.session.outline.focus_port = match target {
                Target::Port(port) => Some(port),
                _ => None,
            };
            self.centre_tile(module);
        }
    }
}

/// Writes a count and the correctly inflected noun.
fn count(size: usize, noun: &str) -> String {
    format!("{size} {noun}{}", if size == 1 { "" } else { "s" })
}

/// Collects one destination per neighbouring tile, in signal-flow order.
fn jump_choices(map: &PatchMap, from: Target, downstream: Option<bool>) -> Vec<Choice> {
    let structure = &map.structure;
    let Some(module) = from.module(structure) else {
        return Vec::new();
    };
    let mut choices = BTreeMap::new();
    let matches = |port| match from {
        Target::Port(start) => start == port,
        Target::Assignment { .. } => false,
        Target::Tile(_) => true,
    };
    for wire in &structure.cables {
        let peer = if downstream != Some(false) && wire.from.module == module && matches(wire.from) {
            wire.to
        } else if downstream != Some(true) && wire.to.module == module && matches(wire.to) {
            wire.from
        } else {
            continue;
        };
        let target = if matches!(from, Target::Port(_)) {
            Target::Port(peer)
        } else {
            Target::Tile(peer.module)
        };
        let from_label = structure.port_label(wire.from);
        let to_label = structure.port_label(wire.to);
        let far = structure.name(peer.module);
        let name = if peer.output {
            format!("{far} {from_label} to {to_label}")
        } else {
            format!("{from_label} to {far} {to_label}")
        };
        choices.entry(peer.module).or_insert(Choice { target, name });
    }
    for assignment in &structure.modulations {
        if let Target::Assignment { id, .. } = from
            && assignment.id != id
        {
            continue;
        }
        if let Target::Port(port) = from
            && port != assignment.from
        {
            continue;
        }
        let peer = if downstream != Some(false) && assignment.from.module == module {
            assignment.to
        } else if downstream != Some(true) && assignment.to == module {
            assignment.from.module
        } else {
            continue;
        };
        let from_label = structure.port_label(assignment.from);
        let to_label = &assignment.parameter;
        let far = structure.name(peer);
        let name = if assignment.from.module == module {
            format!("{from_label} to {far} {to_label} (modulation)")
        } else {
            format!("{far} {from_label} to {to_label} (modulation)")
        };
        choices.entry(peer).or_insert(Choice {
            target: Target::Tile(peer),
            name,
        });
    }
    map.order
        .iter()
        .flat_map(|unit| match unit {
            Unit::Tile(module) => std::slice::from_ref(module),
            Unit::Loop(index) => &structure.loops[*index].members,
        })
        .filter_map(|module| choices.remove(module))
        .collect()
}

/// The cable choices as one panel.
pub(crate) fn jump_id() -> UiId {
    UiId::panel("cable-jump")
}

impl App {
    /// Moves the canvas's navigation focus to a port.
    pub(crate) fn focus_canvas_port(&mut self, port: PortRef) {
        self.session.outline.active = false;
        self.session.outline.focus_port = Some(port);
        self.a11y.request_control(None);
        self.centre_tile(port.module);
    }

    /// The cable-choice panel as a list of accessible options.
    pub(crate) fn jump_items(&self) -> Vec<crate::a11y::items::Item> {
        use crate::a11y::items::{Item, Kind};
        let Some(jump) = &self.session.outline.jump else {
            return Vec::new();
        };
        let mut list = Item::new(jump_id(), Kind::ListBox);
        list.label = Some(count(jump.choices.len(), "connection"));
        list.children = jump
            .choices
            .iter()
            .enumerate()
            .map(|(index, choice)| {
                let mut item = Item::new(jump_id().with(index as u64), Kind::ListBoxOption);
                item.label = Some(choice.name.clone());
                item.selected = Some(index == jump.selected);
                item.click = Some(Message::JumpChoice(index));
                item
            })
            .collect();
        vec![list]
    }

    /// The item chosen with the arrow keys in the cable list.
    pub(crate) fn jump_focus(&self) -> Option<UiId> {
        Some(jump_id().with(self.session.outline.jump.as_ref()?.selected as u64))
    }

    /// The action of clicking one of the visible cable destinations.
    pub(crate) fn jump_action(&self, id: &UiId) -> Option<Message> {
        let jump = self.session.outline.jump.as_ref()?;
        (0..jump.choices.len())
            .find(|index| jump_id().with(*index as u64) == *id)
            .map(Message::JumpChoice)
    }
}

#[cfg(test)]
mod tests;

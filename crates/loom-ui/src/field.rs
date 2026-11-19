//! Single-line text fields: the editing state the runtime keeps for each
//! [`Element::field`](crate::Element::field) (ui-layer.md, section 8).
//!
//! - **The app owns the text.** A field element carries the text the app
//!   holds. The field edits in place and tells the app each change, as
//!   [`FieldEvent::Edited`]. Whenever a layout shows a text other than the
//!   field's, whatever the cause, the field takes it: its selection is
//!   clamped to it, and its undo steps go, since they no longer apply. An
//!   edit the app does not take is one case: the field goes back (rule 2).
//! - Typing over a selection, a delete by word, and a cut are each an undo
//!   step of their own (rule 3).
//! - **Editing comes from Parley's editor** (rule 1). Moving by word walks
//!   the shaped layout, and a key reaches a field in a turn, where no
//!   typesetter is at hand, so the fields keep a font context and a layout
//!   context of their own: the process's one scan of the system's fonts,
//!   made when the first field appears.
//! - **Undo is the field's own** (rule 3): snapshots of its text and
//!   selection. A typing run is one step; a pause of
//!   [`TYPING_PAUSE`], a caret move, or a space typed after a letter ends
//!   it. Every delete by word and every cut is a step of its own. Leaving
//!   the field or committing it clears both stacks.
//! - **The keys follow section 6, rule 6** (see [`FieldState::key`]).
//! - **A field scrolls** so its caret stays inside its box, with
//!   [`CARET_MARGIN`] to spare.

use crate::paint::{editing_contexts, properties};
use loom_ui_core::{IdKey, ImeEvent, Key, KeyEvent, Platform, Point, Rect, Size, TextStyle, UiId};
use parley::{FontContext, LayoutContext, PlainEditor, PlainEditorDriver, Selection};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A pause in typing this long ends the typing run, so the next letter
/// starts a new undo step.
pub(crate) const TYPING_PAUSE: Duration = Duration::from_millis(500);

/// How far inside the box the caret stays when the field scrolls, in
/// logical pixels.
pub(crate) const CARET_MARGIN: f32 = 2.0;

/// The most undo steps a field keeps. The oldest goes first.
const UNDO_STEPS: usize = 100;

/// The most characters one screen reader run holds. AccessKit keeps a
/// run's character lengths and word starts as bytes, so a longer text is
/// split into runs of this many characters.
pub(crate) const RUN_CHARACTERS: usize = 255;

/// What a text field tells the app, through
/// [`App::field_event`](crate::App::field_event).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldEvent {
    /// Its text changed, to this.
    Edited(String),
    /// Enter was pressed, with this text.
    Committed(String),
    /// Focus moved away from it. The window losing the keys is not this:
    /// the field still has focus when they come back.
    Left,
}

/// The fields of the window, by UI ID, with the contexts they edit with.
#[derive(Default)]
pub(crate) struct Fields {
    /// The fonts and the layout context, made with the first field.
    contexts: Option<(FontContext, LayoutContext<()>)>,
    /// Each laid-out field's state.
    states: HashMap<UiId, FieldState>,
    /// A layout changed what a field reports to a screen reader: its
    /// text, selection, or scroll. The runtime tells the app on its next
    /// turn ([`App::access_changed`](crate::App::access_changed)).
    changed: bool,
}

impl std::fmt::Debug for Fields {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fields")
            .field("states", &self.states.len())
            .finish_non_exhaustive()
    }
}

/// One field as a layout found it: its UI ID, the app's text, its style,
/// and the width of its content box.
pub(crate) struct Seen {
    /// Its UI ID.
    pub(crate) id: UiId,
    /// The text the app holds.
    pub(crate) text: String,
    /// How it is set.
    pub(crate) style: TextStyle,
    /// The width its text shows in, in logical pixels.
    pub(crate) width: f32,
}

impl Fields {
    /// After a layout: keeps a state for each field in `seen` and drops
    /// the rest, so a field's state lives as long as its element (section
    /// 3, rule 4). A field whose text the app changed takes it.
    pub(crate) fn sync(&mut self, seen: Vec<Seen>) {
        self.states.retain(|id, _| seen.iter().any(|field| field.id == *id));
        if seen.is_empty() {
            return;
        }
        let (fonts, layouts) = self.contexts.get_or_insert_with(editing_contexts);
        for field in seen {
            match self.states.get_mut(&field.id) {
                Some(state) => {
                    // The text is compared before it is taken, so nothing
                    // is copied while nothing changes.
                    let text = state.text() != field.text;
                    let before = (*state.editor.raw_selection(), state.scroll.to_bits());
                    state.take(&field.text, field.style, field.width, fonts, layouts);
                    self.changed |= text || (*state.editor.raw_selection(), state.scroll.to_bits()) != before;
                }
                None => {
                    let mut state = FieldState::new(&field.text, field.style, fonts, layouts);
                    state.take(&field.text, field.style, field.width, fonts, layouts);
                    self.states.insert(field.id, state);
                }
            }
        }
    }

    /// Whether a layout changed what a field reports, since the last call.
    pub(crate) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// The field whose UI ID is `id`, or whose run's is.
    pub(crate) fn owner_of(&self, id: &UiId) -> Option<&UiId> {
        self.states.get_key_value(id).map(|(key, _)| key).or_else(|| {
            self.states
                .keys()
                .find(|field| id.is_under(field) && run_index(field, id).is_some())
        })
    }

    /// Carries out a screen reader's `action` on the field `id`, at `now`.
    pub(crate) fn act(&mut self, id: &UiId, action: FieldAction, now: Instant) -> ActUse {
        let (Some(state), Some((fonts, layouts))) = (self.states.get_mut(id), self.contexts.as_mut()) else {
            return ActUse::default();
        };
        state.act(action, now, fonts, layouts)
    }

    /// The field with UI ID `id`.
    pub(crate) fn get(&self, id: &UiId) -> Option<&FieldState> {
        self.states.get(id)
    }

    /// True when `id` is a field's.
    pub(crate) fn has(&self, id: &UiId) -> bool {
        self.states.contains_key(id)
    }

    /// Gives `event` to the field `id`, at `now`, with `platform`'s keys.
    pub(crate) fn key(&mut self, id: &UiId, event: &KeyEvent, now: Instant, platform: Platform) -> KeyUse {
        let (Some(state), Some((fonts, layouts))) = (self.states.get_mut(id), self.contexts.as_mut()) else {
            return KeyUse::default();
        };
        state.key(event, now, platform, fonts, layouts)
    }

    /// Pastes the clipboard's `text` into the field `id`, at `now`.
    pub(crate) fn paste(&mut self, id: &UiId, text: &str, now: Instant) -> KeyUse {
        let (Some(state), Some((fonts, layouts))) = (self.states.get_mut(id), self.contexts.as_mut()) else {
            return KeyUse::default();
        };
        state.paste(text, now, fonts, layouts)
    }

    /// Gives the input method's `event` to the field `id`, at `now`.
    pub(crate) fn ime(&mut self, id: &UiId, event: &ImeEvent, now: Instant) -> KeyUse {
        let (Some(state), Some((fonts, layouts))) = (self.states.get_mut(id), self.contexts.as_mut()) else {
            return KeyUse::default();
        };
        state.ime(event, now, fonts, layouts)
    }

    /// Focus left the field `id`: a composition in progress is kept as
    /// text, as what the user saw stays, then its typing run ends and its
    /// undo steps go. Returns whether the text changed, so the app hears
    /// it as an edit.
    pub(crate) fn left(&mut self, id: &UiId, now: Instant) -> bool {
        let (Some(state), Some((fonts, layouts))) = (self.states.get_mut(id), self.contexts.as_mut()) else {
            return false;
        };
        let edited = state.finish_composing(now, fonts, layouts);
        state.forget_steps();
        edited
    }
}

/// The index of the run `id` names in the field `field`: the field's UI ID,
/// then `run`, then the index.
pub(crate) fn run_index(field: &UiId, id: &UiId) -> Option<usize> {
    let keys = id.keys();
    let inside = field.keys().len();
    if keys.len() != inside + 2 || !id.is_under(field) || keys[inside] != IdKey::from("run") {
        return None;
    }
    match &keys[inside + 1] {
        IdKey::Number(index) => usize::try_from(*index).ok(),
        IdKey::Name(_) => None,
    }
}

/// The UI ID of run `index` of the field `field`.
pub(crate) fn run_id(field: &UiId, index: usize) -> UiId {
    field.clone().with("run").with(index as u64)
}

/// A screen reader's request on a field (ui-layer.md, section 7, rule 10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FieldAction {
    /// Take this text, as one undo step, with the caret at its end.
    SetValue(String),
    /// Put this text in place of the selection, as one undo step.
    ReplaceSelection(String),
    /// Select from `anchor` to `focus`, each a run and a character in it.
    Select {
        /// Where the selection starts.
        anchor: (usize, usize),
        /// Where it ends, and the caret is.
        focus: (usize, usize),
    },
}

/// What a screen reader's request did in a field.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ActUse {
    /// Its text changed.
    pub(crate) edited: bool,
    /// What it reports changed: its text, selection, or scroll.
    pub(crate) changed: bool,
}

/// One run of a field's text, for a screen reader (section 7, rule 10).
/// A character is one of Parley's clusters, the smallest piece the editor
/// moves over and selects, so the screen reader's cursor stops where the
/// caret does.
///
/// Positions are in visual order along the line. AccessKit defines a run's
/// positions in one direction, so text that mixes in right-to-left gives a
/// screen reader positions that are off there; the field still edits it,
/// since Parley's editor handles both directions.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ReaderRun {
    /// Its text.
    pub(crate) text: String,
    /// Each character's length in UTF-8 bytes.
    pub(crate) lengths: Vec<u8>,
    /// Where each character starts, from the run's left edge, in logical
    /// pixels.
    pub(crate) positions: Vec<f32>,
    /// Each character's width, in logical pixels.
    pub(crate) widths: Vec<f32>,
    /// The characters that start a word.
    pub(crate) word_starts: Vec<u8>,
    /// Where the run starts, from the start of the text, in logical pixels.
    pub(crate) x: f32,
    /// How wide it is.
    pub(crate) width: f32,
}

/// One cluster of a field's layout: its bytes, where it starts, its width,
/// and whether a word starts there.
struct Character {
    bytes: std::ops::Range<usize>,
    /// The bytes of the whole cluster it is part of: the same as `bytes`
    /// unless the cluster was split into pieces.
    cluster: std::ops::Range<usize>,
    offset: f32,
    advance: f32,
    word: bool,
}

/// What a key did in a field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct KeyUse {
    /// The field used the key, so it goes no further. A panel step, when
    /// one comes, hears it still (section 6, rule 6, step 4).
    pub(crate) taken: bool,
    /// Its text changed.
    pub(crate) edited: bool,
    /// Enter: the app commits the text.
    pub(crate) committed: bool,
    /// Text for the clipboard, from copy or cut.
    pub(crate) copy: Option<String>,
    /// The paste key: the runtime asks the driver for the clipboard, and
    /// the text goes in on a later turn.
    pub(crate) paste: bool,
    /// Something it shows changed: its text, caret, selection, or scroll.
    pub(crate) redraw: bool,
}

/// What a typing run is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunKind {
    /// Letters typed.
    Typing,
    /// Letters deleted one at a time.
    Deleting,
}

/// The undo step that is open: what it is made of, when it last grew, and
/// whether the last letter typed was a space.
#[derive(Debug, Clone, Copy)]
struct Run {
    kind: RunKind,
    last: Instant,
    space: bool,
}

/// A field's text and selection, for undo.
#[derive(Debug, Clone)]
struct Snapshot {
    text: String,
    selection: Selection,
}

/// One field's editing state.
pub(crate) struct FieldState {
    editor: PlainEditor<()>,
    style: TextStyle,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    run: Option<Run>,
    /// The undo step an edit key opened, kept only if the key changes the
    /// text, so a delete with nothing to delete leaves no empty step.
    opened: Option<Snapshot>,
    /// The typing run as it was before an edit key, put back if the key
    /// changes nothing, so a no-op cannot carry a later edit into its run.
    run_before: Option<Option<Run>>,
    /// How far the text has slid left, in logical pixels.
    scroll: f32,
    /// The width its text shows in.
    width: f32,
}

impl std::fmt::Debug for FieldState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FieldState")
            .field("text", &self.text())
            .field("undo", &self.undo.len())
            .field("redo", &self.redo.len())
            .field("scroll", &self.scroll)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
thread_local! {
    /// How many clusters this thread's calls to `FieldState::characters`
    /// have visited, so a test can show a run is built in linear time.
    pub(crate) static VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `text` with no control characters, or `None` when nothing is left to
/// type.
fn printable(text: Option<&str>) -> Option<String> {
    let text: String = text?.chars().filter(|c| !c.is_control()).collect();
    (!text.is_empty()).then_some(text)
}

/// `text` as one line, for a field that shows one: each line break (CRLF,
/// LF, CR, or the line and paragraph separators U+2028 and U+2029, which
/// the text engine also breaks at) becomes one space, a break at the end
/// is dropped, a tab
/// becomes one space too, since it usually separates words, and other
/// control characters go, as when typing. An empty text stays empty.
/// (ui-layer.md, section 8, rule 2.)
pub(crate) fn one_line(text: &str) -> String {
    let text = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix(['\n', '\r', '\u{2028}', '\u{2029}']))
        .unwrap_or(text);
    text.replace("\r\n", " ")
        .chars()
        .filter_map(|c| match c {
            '\n' | '\r' | '\t' | '\u{2028}' | '\u{2029}' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

impl FieldState {
    /// A field showing `text` in `style`, with the caret at its end.
    pub(crate) fn new(text: &str, style: TextStyle, fonts: &mut FontContext, layouts: &mut LayoutContext<()>) -> Self {
        let mut editor = PlainEditor::new(style.size);
        for property in properties(style) {
            editor.edit_styles().insert(property);
        }
        editor.set_text(text);
        editor.driver(fonts, layouts).move_to_text_end();
        Self {
            editor,
            style,
            undo: Vec::new(),
            redo: Vec::new(),
            run: None,
            opened: None,
            run_before: None,
            scroll: 0.0,
            width: 0.0,
        }
    }

    /// Takes what a layout found: the app's `text`, the `style`, and the
    /// `width` the text shows in. A text that differs from the field's is
    /// the app's change, or the app not taking the field's: the field goes
    /// to it, with its selection clamped, and its undo steps go.
    pub(crate) fn take(
        &mut self,
        text: &str,
        style: TextStyle,
        width: f32,
        fonts: &mut FontContext,
        layouts: &mut LayoutContext<()>,
    ) {
        if style != self.style {
            self.style = style;
            self.editor.edit_styles().clear();
            for property in properties(style) {
                self.editor.edit_styles().insert(property);
            }
        }
        // The app holds the committed text, without a preedit, so an app
        // text that did not change never ends a composition.
        if self.committed() != text {
            self.editor.set_text(text);
            self.forget_steps();
        }
        self.width = width;
        self.editor.refresh_layout(fonts, layouts);
        self.keep_caret();
    }

    /// The text it shows, with any text the input method is composing.
    /// Drawing and a screen reader read this (section 7, rule 10).
    pub(crate) fn text(&self) -> &str {
        self.editor.raw_text()
    }

    /// The text without what the input method is composing: what the app
    /// holds, what an edit sends, and what undo keeps. The same as
    /// [`text`](Self::text) while nothing is composed.
    pub(crate) fn committed(&self) -> String {
        self.editor.text().chars().collect()
    }

    /// Whether the input method is composing text in it.
    pub(crate) fn composing(&self) -> bool {
        self.editor.is_composing()
    }

    /// The range of the text being composed, in bytes of [`text`](Self::text).
    pub(crate) fn preedit(&self) -> Option<std::ops::Range<usize>> {
        self.editor.raw_compose().clone()
    }

    /// Where the input method's candidate window should go: the text being
    /// composed, or the caret, in logical pixels from the left of its
    /// content box, moved by its scroll.
    pub(crate) fn ime_area(&self) -> Rect {
        let area = self.editor.ime_cursor_area();
        Rect::new(
            Point::new(area.x0 as f32 - self.scroll, area.y0 as f32),
            Size::new((area.x1 - area.x0) as f32, (area.y1 - area.y0) as f32),
        )
    }

    /// The input method's `event`, at `now` (section 8, rule 4). A preedit
    /// shows in the field but is not its text; a commit puts its text in
    /// as one undo step, made one line; an input method turned off in
    /// mid-composition keeps what it showed, as focus leaving does.
    pub(crate) fn ime(
        &mut self,
        event: &ImeEvent,
        now: Instant,
        fonts: &mut FontContext,
        layouts: &mut LayoutContext<()>,
    ) -> KeyUse {
        let before = self.reported();
        let committed = self.committed();
        match event {
            ImeEvent::Enabled => {}
            ImeEvent::Preedit { text, cursor } if !text.is_empty() => {
                // Composing ends the typing run, so the commit is a step
                // of its own.
                self.run = None;
                let cursor = cursor.filter(|(start, end)| {
                    start <= end && *end <= text.len() && text.is_char_boundary(*start) && text.is_char_boundary(*end)
                });
                self.editor.driver(fonts, layouts).set_compose(text, cursor);
            }
            ImeEvent::Preedit { .. } => self.editor.driver(fonts, layouts).clear_compose(),
            ImeEvent::Commit(text) => {
                self.editor.driver(fonts, layouts).clear_compose();
                self.commit_text(text, now, fonts, layouts);
            }
            ImeEvent::Disabled => {
                self.finish_composing(now, fonts, layouts);
            }
        }
        self.editor.refresh_layout(fonts, layouts);
        self.keep_caret();
        KeyUse {
            taken: true,
            edited: self.committed() != committed,
            redraw: self.reported() != before,
            ..KeyUse::default()
        }
    }

    /// Ends a composition in progress by keeping its text, as one undo
    /// step. Returns whether the committed text changed.
    fn finish_composing(&mut self, now: Instant, fonts: &mut FontContext, layouts: &mut LayoutContext<()>) -> bool {
        let Some(range) = self.preedit() else {
            return false;
        };
        let text = self.text()[range].to_owned();
        self.editor.driver(fonts, layouts).clear_compose();
        self.commit_text(&text, now, fonts, layouts);
        self.editor.refresh_layout(fonts, layouts);
        self.keep_caret();
        !text.is_empty()
    }

    /// Puts `text` in at the selection as one undo step of its own, made
    /// one line (section 8, rule 2): an input method's commit or a paste.
    fn commit_text(&mut self, text: &str, now: Instant, fonts: &mut FontContext, layouts: &mut LayoutContext<()>) {
        let text = one_line(text);
        if text.is_empty() {
            return;
        }
        self.run = None;
        self.step(None, now, false);
        self.editor.driver(fonts, layouts).insert_or_replace_selection(&text);
        if let Some(snapshot) = self.opened.take() {
            if self.undo.len() == UNDO_STEPS {
                self.undo.remove(0);
            }
            self.undo.push(snapshot);
            self.redo.clear();
        }
        self.run_before = None;
        self.run = None;
    }

    /// The clipboard's `text`, pasted at the selection as one undo step,
    /// at `now`. Returns what it did, as a key's use.
    pub(crate) fn paste(
        &mut self,
        text: &str,
        now: Instant,
        fonts: &mut FontContext,
        layouts: &mut LayoutContext<()>,
    ) -> KeyUse {
        let before = self.reported();
        let committed = self.committed();
        // A paste that comes during a composition keeps the preedit as
        // text first, as a screen reader's edit does, then goes in after
        // it: the user saw the preedit, and asked for the paste.
        self.finish_composing(now, fonts, layouts);
        self.commit_text(text, now, fonts, layouts);
        self.editor.refresh_layout(fonts, layouts);
        self.keep_caret();
        KeyUse {
            taken: true,
            edited: self.committed() != committed,
            redraw: self.reported() != before,
            ..KeyUse::default()
        }
    }

    /// How far its text has slid left to keep the caret in view.
    pub(crate) fn scroll(&self) -> f32 {
        self.scroll
    }

    /// The width of its text on one line.
    pub(crate) fn text_width(&self) -> f32 {
        self.editor.try_layout().map_or(0.0, parley::Layout::width)
    }

    /// The selected part's boxes, from the start of the text. None when the
    /// selection is a caret.
    pub(crate) fn selection(&self) -> Vec<Rect> {
        if self.editor.raw_selection().is_collapsed() {
            return Vec::new();
        }
        self.editor
            .selection_geometry()
            .into_iter()
            .map(|(bounds, _)| {
                Rect::new(
                    Point::new(bounds.x0 as f32, bounds.y0 as f32),
                    Size::new((bounds.x1 - bounds.x0) as f32, (bounds.y1 - bounds.y0) as f32),
                )
            })
            .collect()
    }

    /// Where the text being composed is, from the start of the text, one
    /// box per line it covers, for its underline. Empty while nothing is
    /// composed.
    pub(crate) fn preedit_boxes(&self) -> Vec<Rect> {
        let (Some(range), Some(layout)) = (self.preedit(), self.editor.try_layout()) else {
            return Vec::new();
        };
        let cursor = |at| parley::Cursor::from_byte_index(layout, at, parley::Affinity::Downstream);
        let mut out = Vec::new();
        Selection::new(cursor(range.start), cursor(range.end)).geometry_with(layout, |bounds, _| {
            out.push(Rect::new(
                Point::new(bounds.x0 as f32, bounds.y0 as f32),
                Size::new((bounds.x1 - bounds.x0) as f32, (bounds.y1 - bounds.y0) as f32),
            ));
        });
        out
    }

    /// Where the caret is, from the start of the text: its left edge and
    /// its line's top and height. None while something is selected.
    pub(crate) fn caret(&self) -> Option<Rect> {
        if !self.editor.raw_selection().is_collapsed() {
            return None;
        }
        let bounds = self.editor.cursor_geometry(1.0)?;
        Some(Rect::new(
            Point::new(bounds.x0 as f32, bounds.y0 as f32),
            Size::new(1.0, (bounds.y1 - bounds.y0) as f32),
        ))
    }

    /// The selection, as byte offsets into the text: where it started and
    /// where the caret is.
    #[cfg(test)]
    pub(crate) fn selection_bytes(&self) -> (usize, usize) {
        let selection = self.editor.raw_selection();
        (selection.anchor().index(), selection.focus().index())
    }

    /// How many undo and redo steps it holds.
    #[cfg(test)]
    pub(crate) fn steps(&self) -> (usize, usize) {
        (self.undo.len(), self.redo.len())
    }

    /// What it reports to a screen reader, to tell when that changes.
    fn reported(&self) -> (String, Selection, u32) {
        (
            self.text().to_owned(),
            *self.editor.raw_selection(),
            self.scroll.to_bits(),
        )
    }

    /// Its clusters, in text order, with their places and widths when
    /// `geometry` is set; without it, both are 0, for a caller that needs
    /// only bytes.
    fn characters(&self, geometry: bool) -> Vec<Character> {
        let Some(layout) = self.editor.try_layout() else {
            return Vec::new();
        };
        // One walk over each line's shaped runs, in visual order, takes
        // every cluster with its place; sorting by byte then gives text
        // order. Parley's own ways to step from cluster to cluster cost
        // more on a long line: `Cluster::visual_offset` walks the line up
        // to its cluster, and `next_logical` searches the line's runs from
        // the start at each run boundary, so text that changes script
        // often would cost its length squared. A field has no inline
        // boxes, so the runs are all a line holds.
        struct Found {
            bytes: std::ops::Range<usize>,
            offset: f32,
            advance: f32,
            boundary: bool,
        }
        let mut found = Vec::new();
        for line in layout.lines() {
            let mut offset = line.metrics().offset;
            for run in line.runs() {
                for cluster in run.visual_clusters() {
                    #[cfg(test)]
                    VISITS.with(|visits| visits.set(visits.get() + 1));
                    let bytes = cluster.text_range();
                    let advance = cluster.advance();
                    if !bytes.is_empty() {
                        found.push(Found {
                            bytes,
                            offset: if geometry { offset } else { 0.0 },
                            advance: if geometry { advance } else { 0.0 },
                            boundary: cluster.is_word_boundary() && !cluster.is_space_or_nbsp(),
                        });
                    }
                    offset += advance;
                }
            }
        }
        found.sort_by_key(|cluster| cluster.bytes.start);
        let mut out = Vec::with_capacity(found.len());
        for cluster in found {
            let bytes = cluster.bytes;
            // A word starts at a boundary that is not a space, since
            // AccessKit counts the space after a word as part of it; the
            // line's first character starts one too. These are where
            // moving left by word stops.
            let word = out.is_empty() || cluster.boundary;
            // AccessKit keeps a character's length in one byte, so a
            // cluster longer than that (a letter with hundreds of
            // combining marks) is split at char boundaries into pieces of
            // at most 255 bytes. The first piece has the cluster's place
            // and width; the rest sit at its trailing edge with no width,
            // since no caret stands inside a cluster, and a caret after
            // the last piece then reports the cluster's end. The lengths
            // still add up to the text and the positions stay in order.
            let text = &self.text()[bytes.clone()];
            let mut start = 0;
            let mut first = true;
            while start < text.len() {
                let mut end = (start + usize::from(u8::MAX)).min(text.len());
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                out.push(Character {
                    bytes: bytes.start + start..bytes.start + end,
                    cluster: bytes.clone(),
                    offset: if first {
                        cluster.offset
                    } else {
                        cluster.offset + cluster.advance
                    },
                    advance: if first { cluster.advance } else { 0.0 },
                    word: word && first,
                });
                first = false;
                start = end;
            }
        }
        out
    }

    /// Its text as runs for a screen reader, of at most [`RUN_CHARACTERS`]
    /// characters each. An empty field has one empty run.
    pub(crate) fn runs(&self) -> Vec<ReaderRun> {
        let characters = self.characters(true);
        if characters.is_empty() {
            return vec![ReaderRun::default()];
        }
        characters
            .chunks(RUN_CHARACTERS)
            .map(|chunk| {
                let x = chunk.iter().map(|character| character.offset).fold(f32::MAX, f32::min);
                let end = chunk
                    .iter()
                    .map(|character| character.offset + character.advance)
                    .fold(f32::MIN, f32::max);
                let start = chunk[0].bytes.start;
                let stop = chunk[chunk.len() - 1].bytes.end;
                ReaderRun {
                    text: self.text()[start..stop].to_owned(),
                    lengths: chunk
                        .iter()
                        .map(|character| u8::try_from(character.bytes.len()).expect("a piece is at most 255 bytes"))
                        .collect(),
                    positions: chunk.iter().map(|character| character.offset - x).collect(),
                    widths: chunk.iter().map(|character| character.advance).collect(),
                    word_starts: (0..chunk.len())
                        .filter(|&i| chunk[i].word)
                        .map(|i| u8::try_from(i).expect("a run is at most 255 characters"))
                        .collect(),
                    x,
                    width: end - x,
                }
            })
            .collect()
    }

    /// The selection's two ends as runs and characters in them: where it
    /// started, then where the caret is. An end past the last character
    /// is the last run's length.
    pub(crate) fn reader_selection(&self) -> ((usize, usize), (usize, usize)) {
        let characters = self.characters(false);
        let place = |byte: usize| {
            let index = characters
                .iter()
                .take_while(|character| character.bytes.start < byte)
                .count();
            let runs = characters.len().div_ceil(RUN_CHARACTERS).max(1);
            let run = (index / RUN_CHARACTERS).min(runs - 1);
            (run, index - run * RUN_CHARACTERS)
        };
        let selection = self.editor.raw_selection();
        (place(selection.anchor().index()), place(selection.focus().index()))
    }

    /// The byte where character `index` of run `run` starts, with the
    /// cluster it is in: at the end of a run, the next run's first
    /// character, and past the last character, the end of the text.
    /// `None` for a run the field does not have, or an index past that
    /// run's length, so a stale request does nothing.
    fn byte_at(
        characters: &[Character],
        text_len: usize,
        run: usize,
        index: usize,
    ) -> Option<(usize, std::ops::Range<usize>)> {
        let runs = characters.len().div_ceil(RUN_CHARACTERS).max(1);
        // The run is checked before it is multiplied, so a huge index from
        // a stale or made-up node cannot overflow.
        if run >= runs {
            return None;
        }
        let length = characters
            .len()
            .saturating_sub(run * RUN_CHARACTERS)
            .min(RUN_CHARACTERS);
        if index > length {
            return None;
        }
        let at = run * RUN_CHARACTERS + index;
        Some(match characters.get(at) {
            Some(character) => (character.bytes.start, character.cluster.clone()),
            None => (text_len, text_len..text_len),
        })
    }

    /// The byte range a screen reader's selection from `anchor` to `focus`
    /// (each a run and a character in it) names, or `None` if either end
    /// is stale. An end inside a cluster split into pieces moves out to the
    /// cluster's edge, the near end back to its start and the far end on to
    /// its end, since the editor selects whole clusters only.
    fn reader_range(&self, anchor: (usize, usize), focus: (usize, usize)) -> Option<(usize, usize)> {
        let characters = self.characters(false);
        let len = self.text().len();
        let (anchor_at, anchor_cluster) = Self::byte_at(&characters, len, anchor.0, anchor.1)?;
        let (focus_at, focus_cluster) = Self::byte_at(&characters, len, focus.0, focus.1)?;
        let inside = |at: usize, cluster: &std::ops::Range<usize>| at > cluster.start && at < cluster.end;
        let snap = |at: usize, cluster: &std::ops::Range<usize>, far: bool| match (inside(at, cluster), far) {
            (true, true) => cluster.end,
            (true, false) => cluster.start,
            (false, _) => at,
        };
        // A caret, both ends at one place inside a split cluster, stays a
        // caret: it goes to the cluster's start.
        if anchor_at == focus_at {
            let at = snap(anchor_at, &anchor_cluster, false);
            return Some((at, at));
        }
        let forward = anchor_at <= focus_at;
        Some((
            snap(anchor_at, &anchor_cluster, !forward),
            snap(focus_at, &focus_cluster, forward),
        ))
    }

    /// Carries out a screen reader's `action`, at `now`. Each edit ends the
    /// typing run first and is one undo step, so it never joins typing.
    fn act(
        &mut self,
        action: FieldAction,
        now: Instant,
        fonts: &mut FontContext,
        layouts: &mut LayoutContext<()>,
    ) -> ActUse {
        // A screen reader acting in mid-composition first keeps the
        // preedit as text, as focus leaving does, so the editor never edits
        // around a preedit range that no longer fits. The text it shows is
        // the same, so the request's positions still hold. Keeping it is an
        // edit the app must hear, as the committed text grew, even if the
        // action itself then changes nothing.
        let committed = self.committed();
        let kept = self.finish_composing(now, fonts, layouts);
        // A stale selection changes nothing, not even the typing run, so
        // typing after it still joins the run it was in.
        let range = match &action {
            FieldAction::Select { anchor, focus } => match self.reader_range(*anchor, *focus) {
                Some(range) => Some(range),
                None => {
                    return ActUse {
                        edited: kept,
                        changed: kept,
                    };
                }
            },
            _ => None,
        };
        let before = self.reported();
        self.run = None;
        match action {
            // A field shows one line, so a screen reader's text is made one
            // as a pasted text is (section 8, rule 2).
            FieldAction::SetValue(text) => {
                self.step(None, now, false);
                self.editor.set_text(&one_line(&text));
                self.editor.driver(fonts, layouts).move_to_text_end();
            }
            FieldAction::ReplaceSelection(text) => {
                self.step(None, now, false);
                self.editor
                    .driver(fonts, layouts)
                    .insert_or_replace_selection(&one_line(&text));
            }
            FieldAction::Select { .. } => {
                if let Some((anchor, focus)) = range {
                    self.editor.driver(fonts, layouts).select_byte_range(anchor, focus);
                }
            }
        }
        self.editor.refresh_layout(fonts, layouts);
        self.keep_caret();
        // The action's own undo step is kept only if the action changed the
        // text; what the app hears also counts a kept composition.
        let acted = self.text() != before.0;
        let edited = self.committed() != committed;
        if let Some(snapshot) = self.opened.take()
            && acted
        {
            if self.undo.len() == UNDO_STEPS {
                self.undo.remove(0);
            }
            self.undo.push(snapshot);
            self.redo.clear();
        }
        self.run_before = None;
        self.run = None;
        ActUse {
            edited,
            changed: kept || self.reported() != before,
        }
    }

    /// Ends the typing run and drops both undo stacks.
    pub(crate) fn forget_steps(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.run = None;
    }

    /// What the field does with `event`, at `now`, with `platform`'s keys.
    ///
    /// A focused field takes (section 6, rule 6):
    ///
    /// - printable keys and Space, as text;
    /// - Backspace and Delete, and by word with the word key
    ///   ([`Modifiers::word_on`](loom_ui_core::Modifiers::word_on)); on a
    ///   Mac, with Command, to the start and the end of the line, a step of
    ///   their own. With any other modifiers it takes them and does
    ///   nothing, so the app never deletes while a field has focus;
    /// - Left and Right, by word with the word key, and on a Mac to the
    ///   start and the end with Command; Shift extends the selection;
    /// - Home and End, extended by Shift;
    /// - Up and Down: on a Mac to the start and the end, as its one-line
    ///   fields do, with Command too; elsewhere taken, with no move;
    /// - with the shortcut key, select all (A), copy (C), cut (X), undo
    ///   (Z), and redo (Shift+Z, and Y off a Mac);
    /// - Enter, which commits.
    ///
    /// It never takes Tab, Escape, or any other shortcut, such as paste
    /// before the field's own paste comes (plan step 3.3.2) or Save: those
    /// go on to the app.
    pub(crate) fn key(
        &mut self,
        event: &KeyEvent,
        now: Instant,
        platform: Platform,
        fonts: &mut FontContext,
        layouts: &mut LayoutContext<()>,
    ) -> KeyUse {
        let KeyEvent::Pressed {
            key, modifiers, text, ..
        } = event
        else {
            return KeyUse::default();
        };
        // While the input method composes, the keys are its own: the field
        // takes every one and does nothing, so none reaches the app, and a
        // stray Backspace never edits the committed text.
        if self.composing() {
            return KeyUse {
                taken: true,
                ..KeyUse::default()
            };
        }
        let shortcut = modifiers.shortcut_on(platform);
        let word = modifiers.word_on(platform);
        let shift = modifiers.shift;
        let mac = platform == Platform::Mac;
        // AltGr is Control and Alt together off a Mac, and types letters;
        // Control alone on a Mac types nothing, and Control+Space there
        // switches the input source.
        let types = if mac {
            !shortcut && !modifiers.control
        } else {
            !shortcut || modifiers.alt
        };
        let before = self.editor.raw_text().to_owned();
        let selected = *self.editor.raw_selection();
        let mut out = KeyUse {
            taken: true,
            ..KeyUse::default()
        };
        match key {
            Key::Tab | Key::Escape => return KeyUse::default(),
            Key::Enter => {
                self.forget_steps();
                out.committed = true;
            }
            Key::Left | Key::Right if mac && shortcut => {
                self.moved(fonts, layouts, |driver| match (key, shift) {
                    (Key::Left, false) => driver.move_to_line_start(),
                    (Key::Left, true) => driver.select_to_line_start(),
                    (_, false) => driver.move_to_line_end(),
                    (_, true) => driver.select_to_line_end(),
                });
            }
            Key::Left | Key::Right if word => {
                self.moved(fonts, layouts, |driver| match (key, shift) {
                    (Key::Left, false) => driver.move_word_left(),
                    (Key::Left, true) => driver.select_word_left(),
                    (_, false) => driver.move_word_right(),
                    (_, true) => driver.select_word_right(),
                });
            }
            Key::Left | Key::Right if !shortcut => {
                self.moved(fonts, layouts, |driver| match (key, shift) {
                    (Key::Left, false) => driver.move_left(),
                    (Key::Left, true) => driver.select_left(),
                    (_, false) => driver.move_right(),
                    (_, true) => driver.select_right(),
                });
            }
            Key::Home | Key::End if !shortcut => {
                self.moved(fonts, layouts, |driver| match (key, shift) {
                    (Key::Home, false) => driver.move_to_line_start(),
                    (Key::Home, true) => driver.select_to_line_start(),
                    (_, false) => driver.move_to_line_end(),
                    (_, true) => driver.select_to_line_end(),
                });
            }
            Key::Up | Key::Down if !shortcut || mac => {
                if mac {
                    self.moved(fonts, layouts, |driver| match (key, shift) {
                        (Key::Up, false) => driver.move_to_text_start(),
                        (Key::Up, true) => driver.select_to_text_start(),
                        (_, false) => driver.move_to_text_end(),
                        (_, true) => driver.select_to_text_end(),
                    });
                }
            }
            Key::Backspace | Key::Delete if mac && shortcut => {
                self.step(None, now, false);
                let mut driver = self.editor.driver(fonts, layouts);
                if driver.editor.raw_selection().is_collapsed() {
                    if *key == Key::Backspace {
                        driver.select_to_line_start();
                    } else {
                        driver.select_to_line_end();
                    }
                }
                driver.delete_selection();
            }
            Key::Backspace | Key::Delete if word => {
                self.step(None, now, false);
                let mut driver = self.editor.driver(fonts, layouts);
                if *key == Key::Backspace {
                    driver.backdelete_word();
                } else {
                    driver.delete_word();
                }
            }
            // Plain, or with Shift as the platforms allow while typing
            // capitals: one character.
            Key::Backspace | Key::Delete if !(modifiers.control || modifiers.alt || modifiers.meta) => {
                self.step(Some(RunKind::Deleting), now, false);
                let mut driver = self.editor.driver(fonts, layouts);
                if *key == Key::Backspace {
                    driver.backdelete();
                } else {
                    driver.delete();
                }
            }
            // Backspace or Delete with any other modifiers is the field's,
            // and does nothing: the app must never take it as its own
            // delete while a field has focus.
            Key::Backspace | Key::Delete => {}
            Key::Character(letter) if shortcut && !modifiers.alt => match letter.to_ascii_lowercase() {
                'a' => self.moved(fonts, layouts, |driver| driver.select_all()),
                'c' => out.copy = self.editor.selected_text().map(str::to_owned),
                'x' => {
                    if let Some(cut) = self.editor.selected_text().map(str::to_owned) {
                        self.step(None, now, false);
                        self.editor.driver(fonts, layouts).delete_selection();
                        out.copy = Some(cut);
                    }
                }
                'z' if shift => self.redo(fonts, layouts),
                'z' => self.undo(fonts, layouts),
                'y' if !mac => self.redo(fonts, layouts),
                'v' => out.paste = true,
                _ => return KeyUse::default(),
            },
            // Space types what the platform says it types, such as the
            // no-break space of Option+Space on a Mac, or a plain space.
            Key::Space if types => {
                let typed = printable(text.as_deref()).unwrap_or_else(|| " ".to_owned());
                self.type_text(&typed, now, fonts, layouts);
            }
            _ => match printable(text.as_deref()).filter(|_| types) {
                Some(typed) => self.type_text(&typed, now, fonts, layouts),
                None => return KeyUse::default(),
            },
        }
        self.editor.refresh_layout(fonts, layouts);
        let scroll = self.scroll;
        self.keep_caret();
        out.edited = self.editor.raw_text() != before;
        if let Some(run) = self.run_before.take()
            && !out.edited
        {
            self.run = run;
        }
        if let Some(snapshot) = self.opened.take()
            && out.edited
        {
            if self.undo.len() == UNDO_STEPS {
                self.undo.remove(0);
            }
            self.undo.push(snapshot);
            self.redo.clear();
        }
        out.redraw = out.edited || *self.editor.raw_selection() != selected || self.scroll != scroll;
        out
    }

    /// Moves the caret or the selection with `act`, which ends the typing
    /// run.
    fn moved(
        &mut self,
        fonts: &mut FontContext,
        layouts: &mut LayoutContext<()>,
        act: impl FnOnce(&mut PlainEditorDriver<'_, ()>),
    ) {
        self.run = None;
        act(&mut self.editor.driver(fonts, layouts));
    }

    /// Types `text` at the selection, at `now`. Typing over a selection is
    /// an undo step of its own (ui-layer.md, section 8, rule 3), so the
    /// typing after it starts a new run.
    fn type_text(&mut self, text: &str, now: Instant, fonts: &mut FontContext, layouts: &mut LayoutContext<()>) {
        let space = text.chars().all(char::is_whitespace);
        let over = !self.editor.raw_selection().is_collapsed();
        self.step(Some(RunKind::Typing), now, space);
        self.editor.driver(fonts, layouts).insert_or_replace_selection(text);
        if over {
            self.run = None;
        }
    }

    /// Before an edit at `now`: opens a new undo step, which
    /// [`FieldState::key`] keeps if the edit changes the text, unless the edit
    /// carries on the open run of `kind`. A run carries on while it is the
    /// same kind, the pause since it last grew is shorter than
    /// [`TYPING_PAUSE`], nothing is selected, and, for typing, the edit is
    /// not a space after a letter. `kind` is `None` for an edit that is
    /// always a step of its own.
    fn step(&mut self, kind: Option<RunKind>, now: Instant, space: bool) {
        let carries_on = match (kind, self.run) {
            (Some(kind), Some(run)) => {
                run.kind == kind
                    && now.saturating_duration_since(run.last) < TYPING_PAUSE
                    && self.editor.raw_selection().is_collapsed()
                    && !(kind == RunKind::Typing && space && !run.space)
            }
            _ => false,
        };
        if !carries_on {
            self.opened = Some(self.snapshot());
        }
        self.run_before = Some(self.run);
        self.run = kind.map(|kind| Run { kind, last: now, space });
    }

    /// The text and the selection now.
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.editor.raw_text().to_owned(),
            selection: *self.editor.raw_selection(),
        }
    }

    /// Goes back to `snapshot`.
    fn restore(&mut self, snapshot: Snapshot, fonts: &mut FontContext, layouts: &mut LayoutContext<()>) {
        self.editor.set_text(&snapshot.text);
        self.editor.driver(fonts, layouts).set_selection(snapshot.selection);
    }

    /// Undoes the last step, if there is one.
    fn undo(&mut self, fonts: &mut FontContext, layouts: &mut LayoutContext<()>) {
        self.run = None;
        if let Some(snapshot) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(snapshot, fonts, layouts);
        }
    }

    /// Redoes the last undone step, if there is one.
    fn redo(&mut self, fonts: &mut FontContext, layouts: &mut LayoutContext<()>) {
        self.run = None;
        if let Some(snapshot) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(snapshot, fonts, layouts);
        }
    }

    /// Slides the text so the caret stays [`CARET_MARGIN`] inside the box,
    /// and no further than it must: a text that fits does not slide.
    fn keep_caret(&mut self) {
        let Some(caret) = self.editor.cursor_geometry(1.0) else {
            return;
        };
        let x = caret.x0 as f32;
        let width = self.width;
        if width <= 2.0 * CARET_MARGIN {
            self.scroll = 0.0;
            return;
        }
        if x - self.scroll > width - CARET_MARGIN {
            self.scroll = x - width + CARET_MARGIN;
        } else if x - self.scroll < CARET_MARGIN {
            self.scroll = (x - CARET_MARGIN).max(0.0);
        }
        let most = (self.text_width() + CARET_MARGIN - width).max(0.0);
        self.scroll = self.scroll.clamp(0.0, most);
    }
}

#[cfg(test)]
mod tests;

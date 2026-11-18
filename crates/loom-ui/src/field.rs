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
use loom_ui_core::{Key, KeyEvent, Platform, Point, Rect, Size, TextStyle, UiId};
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
            let state = self
                .states
                .entry(field.id)
                .or_insert_with(|| FieldState::new(&field.text, field.style, fonts, layouts));
            state.take(&field.text, field.style, field.width, fonts, layouts);
        }
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

    /// Focus left the field `id`: its typing run ends and its undo steps
    /// go.
    pub(crate) fn left(&mut self, id: &UiId) {
        if let Some(state) = self.states.get_mut(id) {
            state.forget_steps();
        }
    }
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

/// `text` with no control characters, or `None` when nothing is left to
/// type.
fn printable(text: Option<&str>) -> Option<String> {
    let text: String = text?.chars().filter(|c| !c.is_control()).collect();
    (!text.is_empty()).then_some(text)
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
        if self.editor.raw_text() != text {
            self.editor.set_text(text);
            self.forget_steps();
        }
        self.width = width;
        self.editor.refresh_layout(fonts, layouts);
        self.keep_caret();
    }

    /// The text it shows.
    pub(crate) fn text(&self) -> &str {
        self.editor.raw_text()
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

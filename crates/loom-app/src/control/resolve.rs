//! Turning names in commands into what the app shows: ports, controls on screen, keys, and buttons.

use super::*;
use crate::places::Places;
use loom_core::ModuleNames;
use loom_ui_core::{Code, IdKey, Key, KeyEvent, Modifiers, UiId};

/// The most moves a `drag` takes.
pub(super) const MAX_DRAG_STEPS: u32 = 1000;

/// Where a pointer command aims: `target`, or `x` and `y`, or neither.
pub(super) fn aim(target: Option<String>, x: Option<f32>, y: Option<f32>) -> Result<Option<Aim>, String> {
    match (target, x, y) {
        (Some(target), None, None) => Ok(Some(Aim::Target(target))),
        (None, Some(x), Some(y)) => Ok(Some(Aim::At(Point::new(x, y)))),
        (None, None, None) => Ok(None),
        _ => Err("give `target`, or `x` and `y`".to_string()),
    }
}

/// One end of a `drag`: a target, or `[x, y]`.
pub(super) fn drag_end(value: &Value, which: &str) -> Result<Aim, String> {
    match value {
        Value::String(target) => Ok(Aim::Target(target.clone())),
        Value::Array(xy) => match xy.as_slice() {
            [x, y] => match (x.as_f64(), y.as_f64()) {
                (Some(x), Some(y)) => Ok(Aim::At(Point::new(x as f32, y as f32))),
                _ => Err(format!("`{which}` is a target or [x, y]")),
            },
            _ => Err(format!("`{which}` is a target or [x, y]")),
        },
        _ => Err(format!("`{which}` is a target or [x, y]")),
    }
}

/// The steps of a pointer command, between setting its modifier keys and
/// letting go of them.
pub(super) fn with_modifiers(modifiers: Modifiers, steps: Vec<PointerStep>) -> Vec<PointerStep> {
    if modifiers.none() {
        return steps;
    }
    std::iter::once(PointerStep::Modifiers(modifiers))
        .chain(steps)
        .chain(std::iter::once(PointerStep::Modifiers(Modifiers::NONE)))
        .collect()
}

/// What a widget operation found in the live window.
#[derive(Debug, Clone, Default)]
pub struct Found {
    pub canvas: Option<CanvasView>,
    pub text: Option<Rect>,
    /// The window's named parts, for a target given as UI ID text.
    pub(crate) places: Places,
    /// The target in `loom-ui`'s own layout: by its UI ID, what of it
    /// shows, or by the text it shows. Filled in the update that answers,
    /// which can read that layout (`App::update`).
    pub(crate) ours: Option<Rect>,
}

impl Found {
    /// Fills [`Found::ours`] for `target` from the runtime's layout, by its
    /// UI ID when it reads as one, else by the text the window shows.
    pub(crate) fn find_ours<M>(&mut self, target: &str, cx: &loom_ui::Context<'_, M>) {
        self.place_ours(
            target,
            |id| cx.bounds(id),
            |id| cx.shown_bounds(id),
            |text| cx.text_bounds(text),
        );
    }

    /// [`Found::find_ours`] between turns, from the layout a driver holds,
    /// for a script's pointer.
    pub(crate) fn find_ours_in(&mut self, target: &str, panels: &loom_ui::Panels<'_, crate::App>) {
        self.place_ours(
            target,
            |id| panels.bounds(id),
            |id| panels.shown_bounds(id),
            |text| panels.locate(None, text),
        );
    }

    /// The one rule both read the layout by: a UI ID by what of it shows;
    /// text the window shows, which is cut to what shows too, only for a
    /// target that names no laid-out part, so a part scrolled out of view
    /// is never taken for other text that reads the same; and whether the
    /// UI ID is laid out but scrolled out of view.
    pub(super) fn place_ours(
        &mut self,
        target: &str,
        bounds: impl Fn(&UiId) -> Option<Rect>,
        shown: impl Fn(&UiId) -> Option<Rect>,
        text: impl Fn(&str) -> Option<Rect>,
    ) {
        let id = ui_id(target);
        let laid_out = id.as_ref().is_some_and(|id| bounds(id).is_some());
        self.ours = match &id {
            Some(id) if laid_out => shown(id),
            _ => text(target),
        };
        self.places.scrolled_out = laid_out && self.ours.is_none();
    }
}

/// A module and a name, from `"4:cutoff"`; or a module alone, from `"4"`.
pub(super) fn split_target(target: &str) -> Option<(u32, Option<&str>)> {
    match target.split_once(':') {
        Some((module, name)) => Some((module.parse().ok()?, Some(name))),
        None => Some((target.parse().ok()?, None)),
    }
}

/// A port by module and name, as in patch files: `"3:out"`.
pub(super) fn port(app: &App, text: &str, output: bool) -> Result<PortRef, String> {
    let (module, Some(name)) = split_target(text).ok_or_else(|| format!("`{text}` is not MODULE:PORT"))? else {
        return Err(format!("`{text}` is not MODULE:PORT"));
    };
    let node = app
        .session
        .controller
        .graph()
        .node(ModuleId(module))
        .ok_or_else(|| format!("no module {module}"))?;
    let count = if output {
        node.output_count()
    } else {
        node.input_count()
    };
    let index = (0..count).find(|&index| if output { node.output_name(index) } else { node.input_name(index) } == Some(name)).ok_or_else(|| {
        let kind = if output { "output" } else { "input" };
        format!("module {module} ({}) has no {kind} `{name}`", node.descriptor.name)
    })?;
    Ok(PortRef { module, output, index })
}

/// A world rectangle in window coordinates.
pub(super) fn to_window(view: &CanvasView, world: Rect) -> Rect {
    let a = view.to_window(Point::new(world.x, world.y));
    let b = view.to_window(Point::new(world.x + world.width, world.y + world.height));
    Rect::new(a, loom_ui_core::Size::new(b.x - a.x, b.y - a.y))
}

/// The first keys a UI ID's text can start with: the window's panels and
/// the overlays (`docs/design/ui-layer.md`, section 4, rule 5). A target that starts
/// with another word is not taken for a UI ID.
const PANELS: [&str; 17] = [
    "canvas",
    "toolbar",
    "breadcrumb",
    "panel",
    "inspector",
    "matrix",
    "outline",
    "cable-jump",
    "stats",
    "menu",
    "recent",
    "question",
    "help",
    "browser",
    "patches",
    "preferences",
    "licence",
];

/// `target` as a UI ID, when it is the text of one: its first key is a
/// panel's name.
pub(crate) fn ui_id(target: &str) -> Option<UiId> {
    let id: UiId = target.parse().ok()?;
    match id.keys().first()? {
        IdKey::Name(panel) if PANELS.contains(&&**panel) => Some(id),
        _ => None,
    }
}

/// Where the part a UI ID names is in the window, or `None` when the
/// window shows no such part. An error when it is there but scrolled out
/// of view.
///
/// A port or a badge on a tile is found through its short name, since the
/// accessibility tree has no port nodes yet. Anything else on the canvas
/// is where the accessibility tree says its node is, built here at scale
/// 1, so `locate` and a screen reader agree (section 4, rule 1). A part
/// outside the canvas is where the window drew its container.
fn locate_id(app: &App, id: &UiId, found: &Found) -> Result<Option<Rect>, String> {
    let on_canvas = id.is_under(&crate::a11y::tree::canvas_id());
    if let [_, IdKey::Number(module), IdKey::Name(kind), IdKey::Name(name)] = id.keys()
        && on_canvas
        && matches!(&**kind, "input" | "output" | "badge")
    {
        let short = format!("{module}:{kind}:{name}");
        return locate(app, &short, found).map(Some);
    }
    if on_canvas {
        return Ok(app.a11y_bounds(id, &found.places));
    }
    // Outside the canvas, the runtime's layout places it (`Found::ours`).
    Ok(None)
}

/// True when `locate` cannot answer `target` from Iced's search and the
/// canvas alone, and the answer could come from the runtime's own layout:
/// a UI ID inside one of its panels, or plain text it may show. Such a
/// `locate` waits for a current layout; any other, a missing module or a
/// part of Iced's scrolled out of view, answers at once with its error.
pub(crate) fn waits_for_ours(app: &App, target: &str, found: &Found) -> bool {
    locate(app, target, found).is_err() && may_be_ours(target) && found.text.is_none()
}

/// True when `target` may name a part only the runtime's layout places, so
/// a layout still to come could find it: a UI ID of a runtime panel, or
/// text, which the runtime's panels may show. A module, a control, or a
/// port by name never is: the canvas places those. `locate` and the
/// pointer commands both wait by this rule, so the two cannot drift.
pub(crate) fn may_be_ours(target: &str) -> bool {
    split_target(target).is_none()
}

/// Where the runtime's layout places `target` for a pointer command, or
/// `None` for a target it does not hold. A module, a control, or a port
/// by name is the canvas's, even when some text in the window reads the
/// same, as `locate` resolves it.
pub(crate) fn ours_for_pointer<M>(target: &str, cx: &loom_ui::Context<'_, M>) -> Option<Rect> {
    if split_target(target).is_some() {
        return None;
    }
    let mut found = Found::default();
    found.find_ours(target, cx);
    found.ours
}

/// Where a target is in the window: a UI ID as text (`toolbar/undo`,
/// `canvas/4/param/cutoff`), a module, a control, port, light, or meter by
/// name (rule 10), or any text the window shows.
pub fn locate(app: &App, target: &str, found: &Found) -> Result<Rect, String> {
    // A UI ID first. Text that reads as one but names nothing the window
    // shows may still be text in the window.
    if let Some(id) = ui_id(target) {
        return match locate_id(app, &id, found)? {
            Some(rect) => Ok(rect),
            None if found.places.scrolled_out => Err(format!("`{id}` is scrolled out of view")),
            None => found
                .ours
                .or(found.text)
                .ok_or_else(|| format!("nothing in the window has the UI ID `{target}`")),
        };
    }
    let Some((module, name)) = split_target(target) else {
        return found
            .text
            .or(found.ours)
            .ok_or_else(|| format!("no module, port, control, or text `{target}` in the window"));
    };
    let view = found.canvas.as_ref().ok_or("the window has no patch canvas")?;
    let tile = app
        .session
        .scene
        .module(module)
        .ok_or_else(|| format!("no module {module} on the canvas"))?;
    let Some(name) = name else {
        return Ok(to_window(view, tile.bounds()));
    };
    let part = crate::targets::find(app.session.controller.graph(), &app.session.hierarchy, module, name)?;
    let world = crate::targets::world_bounds(&app.session.scene, &app.session.values, part)?;
    Ok(to_window(view, world))
}

pub(super) fn rect_fields(r: Rect) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("x".to_string(), json!(r.center_x()));
    map.insert("y".to_string(), json!(r.center_y()));
    map.insert("width".to_string(), json!(r.width));
    map.insert("height".to_string(), json!(r.height));
    map
}

pub(super) fn fields(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// A key by name: one character, or a named key. A letter is the key with
/// no modifier applied, so `A` is the `a` key, as a keyboard gives it.
pub(super) fn key(name: &str) -> Result<Key, String> {
    let key = match name.to_ascii_lowercase().as_str() {
        "escape" => Key::Escape,
        "enter" => Key::Enter,
        "tab" => Key::Tab,
        "space" => Key::Space,
        "delete" => Key::Delete,
        "backspace" => Key::Backspace,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "f2" => Key::F2,
        "f6" => Key::F6,
        _ if name.chars().count() == 1 => {
            let mut lower = name.chars().flat_map(char::to_lowercase);
            match (lower.next(), lower.next()) {
                (Some(only), None) => Key::Character(only),
                _ => Key::Other,
            }
        }
        _ => return Err(format!("unknown key `{name}`")),
    };
    Ok(key)
}

/// Where a scripted key sits, as the US layout gives it, so a script does
/// the same on every machine (ui-layer.md, section 1, Keys in
/// `loom-ui-core`, rule 5): `[` and `]` are their places, which the
/// Outline's cable following reads.
pub(super) fn code(key: Key) -> Code {
    match key {
        Key::Character('[') => Code::BracketLeft,
        Key::Character(']') => Code::BracketRight,
        _ => Code::Other,
    }
}

/// The modifier keys by name. `cmd` is the platform's shortcut key:
/// Command on a Mac and Control elsewhere, as the app reads it.
pub(super) fn modifiers(names: &[String]) -> Result<Modifiers, String> {
    names.iter().try_fold(Modifiers::NONE, |m, name| {
        let add = match name.to_ascii_lowercase().as_str() {
            "cmd" | "command" => Modifiers::platform_shortcut(),
            "ctrl" | "control" => Modifiers::CONTROL,
            "shift" => Modifiers::SHIFT,
            "alt" | "option" => Modifiers::ALT,
            _ => return Err(format!("unknown modifier `{name}`")),
        };
        Ok(m | add)
    })
}

/// Refuses the shortcuts for the file pickers (Open, Save, Save As): they are
/// native, and would stop the app with nobody to answer (rule 9). The `open`
/// and `save` commands do the same without a dialog. New raises the in-app
/// question, which a script can answer.
pub(super) fn refuse_dialog_keys(key: &Key, modifiers: Modifiers) -> Result<(), String> {
    let Key::Character(c) = key else {
        return Ok(());
    };
    if modifiers.shortcut() && matches!(c, 'o' | 's') {
        Err(format!(
            "the {c} shortcut opens a dialog; use the `open` or `save` command instead"
        ))
    } else {
        Ok(())
    }
}

/// The toolbar buttons `toolbar` clicks, by stable name. Labels are not
/// used: they change while the app runs ("Hold note" becomes "Release
/// note") and may be reworded.
pub(super) const BUTTONS: [&str; 6] = ["new", "learn", "hold", "panic", "undo", "redo"];

/// The message a toolbar button sends, or why it cannot be pressed: it
/// opens a dialog (rule 9), it is disabled, or there is no such button.
/// Add module is left out: it opens the browser at the pointer, which a
/// script does not have, and the `add` command adds a module directly.
pub(super) fn press_message(app: &App, name: &str) -> Result<Message, String> {
    if let Some(question) = &app.overlays.question {
        return question_press(question, name);
    }
    let model_name = match name {
        "learn" => "midi_learn",
        "new" | "hold" | "panic" | "undo" | "redo" => name,
        "open" | "save" | "save-as" => {
            return Err(format!(
                "the {name} button opens a dialog; use the `open` or `save` command instead"
            ));
        }
        "add" => return Err("use the `add` command to add a module".to_string()),
        _ => return Err(format!("no toolbar button `{name}`; buttons: {}", BUTTONS.join(", "))),
    };
    let model = app.toolbar_model();
    let button = model
        .buttons
        .iter()
        .find(|button| button.name == model_name)
        .expect("scriptable toolbar buttons are always in the model");
    match button.disabled {
        Some(reason) => Err(reason.to_string()),
        None => Ok(button.press().expect("an enabled toolbar button has a message")),
    }
}

/// A button of the open question, or Panic, which still works. Anything else
/// would be blocked by the question, so it is refused and not lost in silence
/// (rule 9).
fn question_press(question: &crate::question::Question, name: &str) -> Result<Message, String> {
    if name == "panic" {
        return Ok(Message::Panic);
    }
    question
        .answers()
        .iter()
        .find(|answer| answer.name() == name)
        .map(|&answer| Message::Question(crate::question::Message::Answer(answer)))
        .ok_or_else(|| question_open_error(question))
}

/// The refusal for what an open question blocks. It says how to go on.
pub(super) fn question_open_error(question: &crate::question::Question) -> String {
    let model = question.model();
    format!(
        "a question is open (\"{}\"); answer it first with `toolbar`: {}",
        model.title,
        model.buttons.join(", ")
    )
}

/// Refuses a key an open question would block: anything but its own keys and
/// the Panic key, which works whatever is open (panic.md).
pub(super) fn refuse_while_asking(
    question: Option<&crate::question::Question>,
    key: &Key,
    modifiers: Modifiers,
) -> Result<(), String> {
    let Some(question) = question else {
        return Ok(());
    };
    // Space gives the focused answer when it comes up, as on any button
    // (ui-layer.md, section 3, rule 4).
    let its_own = matches!(
        key,
        Key::Tab | Key::Enter | Key::Space | Key::Escape | Key::Left | Key::Right | Key::Up | Key::Down
    );
    if its_own || crate::panic::is_panic_key(key, modifiers) {
        Ok(())
    } else {
        Err(question_open_error(question))
    }
}

/// The key press and release, as the input listener sends them.
pub(super) fn key_events(key: Key, modifiers: Modifiers) -> [KeyEvent; 2] {
    let code = code(key);
    [
        KeyEvent::Pressed {
            key,
            code,
            modifiers,
            // A real keyboard sends a character's text, unless Command or Ctrl
            // makes it a shortcut. The typed field and type-ahead read it.
            text: match key {
                Key::Character(c) if !modifiers.shortcut() && !modifiers.control => Some(c.to_string()),
                _ => None,
            },
            repeat: false,
        },
        KeyEvent::Released { key, code },
    ]
}

/// The most characters one `type` command takes. Each takes two frames, so
/// this stays well inside the time a listener waits for an answer.
const MAX_TYPED: usize = 500;

/// The key presses and releases that type `text`, one pair per character, or
/// why it cannot be typed. Enter and Tab act on the field or the window, so
/// a script sends those with `key`.
pub(super) fn type_events(text: &str) -> Result<Vec<KeyEvent>, String> {
    if text.is_empty() {
        return Err("no text to type".to_string());
    }
    if text.chars().count() > MAX_TYPED {
        return Err(format!("`type` takes at most {MAX_TYPED} characters"));
    }
    if text.chars().any(char::is_control) {
        return Err("`type` takes text only; use `key` for Enter, Tab, and other keys".to_string());
    }
    Ok(text
        .chars()
        .flat_map(|c| {
            let key = match c {
                ' ' => Key::Space,
                _ => Key::Character(c),
            };
            // The same places as `key` gives, so `type "["` and `key "["`
            // do the same.
            let code = code(key);
            [
                KeyEvent::Pressed {
                    key,
                    code,
                    modifiers: Modifiers::NONE,
                    text: Some(c.to_string()),
                    repeat: false,
                },
                KeyEvent::Released { key, code },
            ]
        })
        .collect())
}

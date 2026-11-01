//! Turning names in commands into what the app shows: ports, controls on screen, keys, and buttons.

use super::*;

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
pub(super) fn with_modifiers(modifiers: keyboard::Modifiers, steps: Vec<PointerStep>) -> Vec<PointerStep> {
    if modifiers.is_empty() {
        return steps;
    }
    std::iter::once(PointerStep::Modifiers(modifiers))
        .chain(steps)
        .chain(std::iter::once(PointerStep::Modifiers(keyboard::Modifiers::default())))
        .collect()
}

/// What a widget operation found in the live window.
#[derive(Debug, Clone, Default)]
pub struct Found {
    pub canvas: Option<CanvasView>,
    pub text: Option<Rectangle>,
}

/// Finds the canvas, and the first text widget that reads `text`.
pub(super) struct Find {
    pub(super) text: String,
    pub(super) found: Found,
}

impl Operation<Found> for Find {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Found>)) {
        operate(self);
    }

    fn custom(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Any) {
        if let Some(view) = state.downcast_ref::<CanvasView>() {
            self.found.canvas = Some(*view);
        }
    }

    fn text(&mut self, _id: Option<&Id>, bounds: Rectangle, text: &str) {
        if self.found.text.is_none() && text == self.text {
            self.found.text = Some(bounds);
        }
    }

    fn finish(&self) -> Outcome<Found> {
        Outcome::Some(self.found.clone())
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
pub(super) fn to_window(view: &CanvasView, world: Rectangle) -> Rectangle {
    let a = view.to_window(Point::new(world.x, world.y));
    let b = view.to_window(Point::new(world.x + world.width, world.y + world.height));
    Rectangle::new(a, iced::Size::new(b.x - a.x, b.y - a.y))
}

/// Where a target is in the window: a module, a control, port, light, or
/// meter by name (rule 10), or any text the window shows.
pub fn locate(app: &App, target: &str, found: &Found) -> Result<Rectangle, String> {
    let Some((module, name)) = split_target(target) else {
        return found
            .text
            .ok_or_else(|| format!("no module, port, control, or text `{target}` in the window"));
    };
    let view = found.canvas.as_ref().ok_or("the window has no patch canvas")?;
    let tile = app
        .scene
        .module(module)
        .ok_or_else(|| format!("no module {module} on the canvas"))?;
    let Some(name) = name else {
        return Ok(to_window(view, tile.bounds()));
    };
    let part = crate::targets::find(app.controller.graph(), module, name)?;
    let world = crate::targets::world_bounds(&app.scene, part)?;
    Ok(to_window(view, world))
}

pub(super) fn rect_fields(r: Rectangle) -> Map<String, Value> {
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

/// A key by name: one character, or a named key.
pub(super) fn key(name: &str) -> Result<keyboard::Key, String> {
    use keyboard::key::Named;
    let named = match name.to_ascii_lowercase().as_str() {
        "escape" => Named::Escape,
        "enter" => Named::Enter,
        "tab" => Named::Tab,
        "space" => Named::Space,
        "delete" => Named::Delete,
        "backspace" => Named::Backspace,
        "up" => Named::ArrowUp,
        "down" => Named::ArrowDown,
        "left" => Named::ArrowLeft,
        "right" => Named::ArrowRight,
        "home" => Named::Home,
        "end" => Named::End,
        "pageup" => Named::PageUp,
        "pagedown" => Named::PageDown,
        _ if name.chars().count() == 1 => return Ok(keyboard::Key::Character(name.to_lowercase().into())),
        _ => return Err(format!("unknown key `{name}`")),
    };
    Ok(keyboard::Key::Named(named))
}

pub(super) fn modifiers(names: &[String]) -> Result<keyboard::Modifiers, String> {
    names.iter().try_fold(keyboard::Modifiers::default(), |m, name| {
        let add = match name.to_ascii_lowercase().as_str() {
            "cmd" | "command" => keyboard::Modifiers::COMMAND,
            "ctrl" | "control" => keyboard::Modifiers::CTRL,
            "shift" => keyboard::Modifiers::SHIFT,
            "alt" | "option" => keyboard::Modifiers::ALT,
            _ => return Err(format!("unknown modifier `{name}`")),
        };
        Ok(m | add)
    })
}

/// Refuses the file shortcuts (New, Open, Save, Save As): they open a file
/// dialog or ask to save, which would stop the app with nobody to answer
/// (rule 9). The `open` and `save` commands do the same without a dialog.
pub(super) fn refuse_dialog_keys(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> Result<(), String> {
    let keyboard::Key::Character(c) = key else {
        return Ok(());
    };
    if modifiers.command() && matches!(c.as_str(), "n" | "o" | "s") {
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
pub(super) const BUTTONS: [&str; 5] = ["learn", "hold", "panic", "undo", "redo"];

/// The message a toolbar button sends, or why it cannot be pressed: it
/// opens a dialog (rule 9), it is disabled, or there is no such button.
/// Add module is left out: it opens the browser at the pointer, which a
/// script does not have, and the `add` command adds a module directly.
pub(super) fn press_message(app: &App, name: &str) -> Result<Message, String> {
    let (message, disabled) = match name {
        "learn" => (Message::Learn(crate::learn::Message::Toggle), None),
        "hold" => (
            Message::HoldNote,
            app.midi
                .is_none()
                .then_some("Hold note is disabled: no audio is playing the engine"),
        ),
        "panic" => (Message::Panic, None),
        "undo" => (
            Message::Undo,
            app.edits
                .history
                .undo_step()
                .is_none()
                .then_some("Undo is disabled: nothing to undo"),
        ),
        "redo" => (
            Message::Redo,
            app.edits
                .history
                .redo_step()
                .is_none()
                .then_some("Redo is disabled: nothing to redo"),
        ),
        "new" | "open" | "save" | "save-as" => {
            return Err(format!(
                "the {name} button opens a dialog; use the `new`, `open`, or `save` command instead"
            ));
        }
        "add" => return Err("use the `add` command to add a module".to_string()),
        _ => return Err(format!("no toolbar button `{name}`; buttons: {}", BUTTONS.join(", "))),
    };
    match disabled {
        Some(reason) => Err(reason.to_string()),
        None => Ok(message),
    }
}

/// The key press and release, as the keyboard listener sends them.
pub(super) fn key_events(key: keyboard::Key, modifiers: keyboard::Modifiers) -> [keyboard::Event; 2] {
    let physical = keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified);
    [
        keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key.clone(),
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        },
        keyboard::Event::KeyReleased {
            key: key.clone(),
            modified_key: key,
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers,
        },
    ]
}

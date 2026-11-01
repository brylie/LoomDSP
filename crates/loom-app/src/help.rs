//! Offline module help from the running registry and bundled notes.

use crate::style::{self, button};
use iced::widget::{column, container, markdown, row, scrollable, space, text};
use iced::{Element, Length, Theme};
use loom_core::ModuleDescriptor;
use serde::Serialize;

use crate::Message;

include!(concat!(env!("OUT_DIR"), "/help_pages.rs"));

const MARKER: &str = "<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->";

/// A descriptor entry shown in the help view and snapshot.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    pub name: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub details: String,
}

/// The content of the open help view.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    /// The module's family (ADR-0029).
    pub family: &'static str,
    pub description: &'static str,
    pub inputs: Vec<Entry>,
    pub outputs: Vec<Entry>,
    pub params: Vec<Entry>,
    pub notes: String,
    pub example: Option<String>,
}

/// The open overlay and its parsed Markdown: the notes before the
/// example, and the example section with what follows it.
pub struct Help {
    pub model: Model,
    lead: Vec<markdown::Item>,
    tail: Vec<markdown::Item>,
}

/// Where the hand-written notes' example section starts. The overlay shows
/// the reference sections between the notes and the example
/// (help-pages.md, In the app).
const EXAMPLE_HEADING: &str = "\n## Example";

impl Help {
    /// Builds the view from the actual registry descriptor.
    pub fn new(d: &'static ModuleDescriptor) -> Self {
        let notes = bundled_page(d.id)
            .and_then(|page| page.split_once(MARKER).map(|(_, notes)| notes.trim()))
            .unwrap_or("")
            .replace("\r\n", "\n");
        let example = notes
            .split_once("```ron\n")
            .and_then(|(_, rest)| rest.split_once("```"))
            .map(|(example, _)| example.trim().to_string());
        let port = |p: &loom_core::PortSpec, input: bool| {
            let stereo = if p.channels == loom_core::Channels::Stereo {
                ", stereo"
            } else {
                ""
            };
            let details = if input {
                format!("{}{stereo}; reads {} with no cable", p.signal.name(), p.default.text())
            } else {
                format!(
                    "{}{stereo}, {}",
                    p.signal.name(),
                    format!("{:?}", p.polarity()).to_lowercase()
                )
            };
            Entry {
                name: p.name,
                label: p.label,
                description: p.description,
                details,
            }
        };
        let model = Model {
            id: d.id,
            name: d.name,
            category: d.category,
            family: d.family.label(),
            description: d.description,
            inputs: d.inputs.iter().map(|p| port(p, true)).collect(),
            outputs: d.outputs.iter().map(|p| port(p, false)).collect(),
            params: d
                .params
                .iter()
                .map(|p| Entry {
                    name: p.name,
                    label: p.label,
                    description: p.description,
                    details: format!("{}. Default {}.", p.range_text(), p.default_text()),
                })
                .collect(),
            notes: notes.clone(),
            example,
        };
        Self::from_model(model)
    }

    /// The keyboard shortcuts page: every canvas gesture and shortcut,
    /// opened from the inspector or with F1 when nothing is selected.
    pub fn shortcuts() -> Self {
        let notes: String = crate::HELP.lines().map(|line| format!("- {}\n", line.trim())).collect();
        Self::from_model(Model {
            id: "",
            name: "Keyboard shortcuts",
            category: "",
            family: "",
            description: "",
            inputs: Vec::new(),
            outputs: Vec::new(),
            params: Vec::new(),
            notes,
            example: None,
        })
    }

    /// Parses the notes, split before the example section.
    fn from_model(model: Model) -> Self {
        let (lead, tail) = match model.notes.find(EXAMPLE_HEADING) {
            Some(at) => model.notes.split_at(at),
            None => (model.notes.as_str(), ""),
        };
        Self {
            lead: markdown::parse(lead).collect(),
            tail: markdown::parse(tail).collect(),
            model,
        }
    }

    /// Draws the overlay from the same model exposed to snapshots: a fixed
    /// title row, then one scrolling body in reading order (summary, notes,
    /// Parameters, Inputs, Outputs, example), over a dimmed backdrop that
    /// closes help when clicked.
    pub fn view(&self, theme: Theme) -> Element<'_, Message> {
        let m = &self.model;
        let header = row![
            text(m.name).size(style::TITLE),
            space::horizontal(),
            button(text("Close").size(style::CONTROL)).on_press(Message::CloseHelp)
        ]
        .align_y(iced::Alignment::Center);
        let mut content = column![].spacing(12).width(Length::Fill);
        if !m.id.is_empty() {
            content = content.push(
                text(format!("{} · {} · {}", m.family, m.category, m.id))
                    .size(style::SMALL)
                    .color(style::QUIET),
            );
            content = content.push(text(m.description).size(style::BODY));
        }
        content = content.push(markdown::view(&self.lead, style::markdown(&theme)).map(Message::HelpLink));
        for (title, entries) in [
            ("Parameters", &m.params),
            ("Inputs", &m.inputs),
            ("Outputs", &m.outputs),
        ] {
            if m.id.is_empty() {
                break;
            }
            content = content.push(text(title).size(style::HEADING));
            if entries.is_empty() {
                content = content.push(text("None.").size(style::BODY).color(style::QUIET));
            }
            for entry in entries {
                content = content.push(
                    column![
                        text(entry.label).size(style::CONTROL),
                        text(entry.details.clone()).size(style::SMALL).color(style::QUIET),
                        text(entry.description).size(style::BODY),
                    ]
                    .spacing(2),
                );
            }
        }
        if m.example.is_some() {
            content = content.push(button(text("Open example").size(style::CONTROL)).on_press(Message::HelpExample));
        }
        content = content.push(markdown::view(&self.tail, style::markdown(&theme)).map(Message::HelpLink));
        let panel = container(
            column![
                header,
                // The scrollbar takes its own space, so text wraps before it
                // instead of running under it.
                scrollable(content)
                    .id("module-help-content")
                    .spacing(10)
                    .height(Length::Fill)
            ]
            .spacing(12),
        )
        .padding(20)
        .width(Length::Fill)
        .max_width(720)
        .height(Length::Fill)
        .style(container::rounded_box);
        // The panel takes its own clicks; the backdrop around it closes help.
        let backdrop = container(iced::widget::opaque(panel))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(30)
            .style(|_: &Theme| container::Style {
                background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
                ..container::Style::default()
            });
        iced::widget::mouse_area(backdrop).on_press(Message::CloseHelp).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanvasEvent, Message, files, tests, ui_test::Ui};
    use iced::{
        Point,
        keyboard::{Key, Modifiers, key::Named},
        mouse,
    };

    /// Every built-in example parses and loads with the running registry.
    #[test]
    fn bundled_help_examples_load() {
        let app = tests::app();
        for descriptor in &app.modules {
            let help = Help::new(descriptor);
            assert!(!help.model.notes.is_empty(), "{} has no notes", descriptor.id);
            let example = help.model.example.as_ref().expect(descriptor.id);
            let patch = loom_patch::Patch::from_ron(example).expect(descriptor.id);
            let session = crate::App::session(
                loom_core::EngineConfig::default(),
                false,
                crate::Source::Recovered(Box::new(patch)),
            );
            assert!(
                session.is_ok(),
                "{}: {}",
                descriptor.id,
                session.err().unwrap_or_default()
            );
        }
    }

    /// A registry module with no bundled notes has no example action.
    #[test]
    fn modules_without_notes_have_no_example() {
        let app = crate::App::new(crate::Args {
            test_modules: true,
            ..tests::app_args()
        });
        let descriptor = app
            .modules
            .iter()
            .find(|d| d.id.starts_with("test."))
            .expect("test module");
        let help = Help::new(descriptor);
        assert!(help.model.notes.is_empty());
        assert!(help.model.example.is_none());
    }

    /// F1 reaches the focused browser, and Escape returns to the same search.
    #[test]
    fn help_from_focused_browser_preserves_search() {
        let mut ui = Ui::open("subtractive.ron");
        let canvas = ui.canvas();
        ui.move_to(Point::new(canvas.bounds.x + 8.0, canvas.bounds.y + 8.0));
        ui.tap(Key::Character("a".into()), Modifiers::default());
        ui.typewrite("svf");
        ui.tap(Key::Named(Named::F1), Modifiers::default());
        let help = ui.app.ui_snapshot().help.expect("help is open");
        assert_eq!(help.id, "loom.filter.svf");
        assert!(!help.inputs.is_empty());
        assert!(help.notes.contains("Example patch"));
        assert!(ui.text("Open example").is_some());
        ui.tap(Key::Named(Named::Escape), Modifiers::default());
        assert!(ui.app.help.is_none());
        assert_eq!(
            ui.app.browser.as_ref().unwrap().highlighted().unwrap().id,
            "loom.filter.svf"
        );
        ui.typewrite("x");
        assert!(
            ui.app.browser.as_ref().unwrap().highlighted().is_none(),
            "search retains focus"
        );
    }

    /// The selected tile, inspector action, and tile menu use the same model.
    #[test]
    fn help_from_selected_tile_inspector_and_menu() {
        let mut ui = Ui::open("subtractive.ron");
        let _ = ui.app.update(Message::Canvas(CanvasEvent::Select(vec![4])));
        ui.tap(Key::Named(Named::F1), Modifiers::default());
        let expected = ui.app.ui_snapshot().help.unwrap();
        // Help names the module's family first (ADR-0029).
        let line = format!("{} · {} · {}", expected.family, expected.category, expected.id);
        assert!(!expected.family.is_empty() && ui.text(&line).is_some(), "{line}");
        ui.tap(Key::Named(Named::Escape), Modifiers::default());
        let at = ui.text("Help").expect("inspector help action").center();
        ui.move_to(at);
        ui.press();
        ui.release();
        assert_eq!(ui.app.ui_snapshot().help, Some(expected.clone()));
        ui.tap(Key::Named(Named::Escape), Modifiers::default());
        let bounds = ui.app.scene.module(4).unwrap().bounds();
        let at = ui.canvas().to_window(Point::new(bounds.center().x, bounds.y + 4.0));
        ui.move_to(at);
        ui.step(&[iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))]);
        ui.step(&[iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right))]);
        assert!(ui.app.tile_menu.is_some());
        let (_, menu_at) = ui.app.tile_menu.unwrap();
        ui.move_to(Point::new(menu_at.x + 30.0, menu_at.y + 15.0));
        ui.press();
        ui.release();
        assert_eq!(ui.app.ui_snapshot().help, Some(expected));
    }

    /// Scrolling the notes keeps the Close button visible and clickable.
    #[test]
    fn help_close_stays_visible_while_scrolling() {
        let mut ui = Ui::open("subtractive.ron");
        let _ = ui.app.update(Message::Canvas(CanvasEvent::Select(vec![4])));
        ui.tap(Key::Named(Named::F1), Modifiers::default());
        let before = ui.text("Close").expect("close action");
        let scroll_before = ui.scroll_translation("module-help-content").expect("help scroll area");
        ui.move_to(Point::new(700.0, 600.0));
        ui.step(&[iced::Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: -12.0 },
        })]);
        let after = ui.text("Close").expect("close action remains visible");
        assert_eq!(before, after);
        let scroll_after = ui.scroll_translation("module-help-content").expect("help scroll area");
        assert_ne!(scroll_before, scroll_after, "the content scrolled");
        ui.move_to(after.center());
        ui.press();
        ui.release();
        assert!(ui.app.help.is_none());
    }

    /// The example button asks about unsaved changes and honors the answer.
    #[test]
    fn example_button_honors_unsaved_changes() {
        let mut ui = Ui::open("sine.ron");
        let _ = ui.app.update(Message::Canvas(CanvasEvent::Select(vec![3])));
        let _ = ui.app.update(Message::Param {
            module: 5,
            index: 0,
            position: 0.8,
        });
        let _ = ui.app.update(Message::SliderReleased);
        assert!(ui.app.unsaved());
        ui.tap(Key::Named(Named::F1), Modifiers::default());
        let _ = files::fake::asked();
        let at = ui.text("Open example").expect("example button").center();
        ui.move_to(at);
        ui.press();
        ui.release();
        assert!(ui.app.help.is_some(), "cancellation keeps the help open");
        assert!(ui.app.unsaved());
        assert!(matches!(
            files::fake::asked().as_slice(),
            [files::fake::Asked::Discard(_)]
        ));

        files::fake::answer_discard(files::Choice::DontSave);
        ui.press();
        ui.release();
        assert!(ui.app.help.is_none());
        assert!(!ui.app.unsaved());
        assert!(ui.app.file.is_none());
        assert_eq!(
            ui.app
                .controller
                .graph()
                .nodes()
                .filter(|(_, node)| node.descriptor.id == "loom.osc.sine")
                .count(),
            1
        );
        assert!(matches!(
            files::fake::asked().as_slice(),
            [files::fake::Asked::Discard(_)]
        ));
    }

    /// Saving from the unsaved-changes dialog writes the old patch first.
    #[test]
    fn example_button_saves_the_old_patch_when_requested() {
        let dir = crate::recovery::tests::TempDir::new("help-example-save");
        let destination = dir.0.join("saved.ron");
        let mut ui = Ui::open("sine.ron");
        // The test patch is an untitled copy, so saving asks for a path.
        ui.app.file = None;
        let _ = ui.app.update(Message::Canvas(CanvasEvent::Select(vec![3])));
        let _ = ui.app.update(Message::Param {
            module: 5,
            index: 0,
            position: 0.8,
        });
        let _ = ui.app.update(Message::SliderReleased);
        let value = ui.app.controller.graph().node(loom_core::ModuleId(5)).unwrap().params[0];
        ui.tap(Key::Named(Named::F1), Modifiers::default());
        let _ = files::fake::asked();
        files::fake::answer_save(Some(destination.clone()));
        files::fake::answer_discard(files::Choice::Save);
        let at = ui.text("Open example").expect("example button").center();
        ui.move_to(at);
        ui.press();
        ui.release();
        let saved = loom_patch::Patch::load(&destination).expect("old patch was saved");
        let gain = saved.modules.iter().find(|module| module.id == 5).unwrap().params["gain"];
        assert_eq!(gain, value);
        assert!(ui.app.file.is_none(), "the example is an untitled copy");
        assert!(!ui.app.unsaved());
        assert!(matches!(
            files::fake::asked().as_slice(),
            [files::fake::Asked::Discard(_), files::fake::Asked::Save(_)]
        ));
        files::fake::answer_save(None);
    }

    /// Opening a bundled example respects cancellation of unsaved changes.
    #[test]
    fn example_uses_existing_unsaved_changes_flow() {
        let mut ui = Ui::open("subtractive.ron");
        let _ = ui.app.update(Message::Canvas(CanvasEvent::Select(vec![4])));
        let _ = ui.app.update(Message::ShowHelp);
        let _ = ui.app.update(Message::Param {
            module: 4,
            index: 0,
            position: 0.8,
        });
        let _ = ui.app.update(Message::SliderReleased);
        assert!(ui.app.unsaved());
        let _ = files::fake::asked();
        let _ = ui.app.update(Message::HelpExample);
        assert!(ui.app.help.is_some());
        assert!(matches!(
            files::fake::asked().as_slice(),
            [files::fake::Asked::Discard(_)]
        ));
        let mut clean = Ui::open("subtractive.ron");
        let _ = clean.app.update(Message::Canvas(CanvasEvent::Select(vec![4])));
        let _ = clean.app.update(Message::ShowHelp);
        let _ = clean.app.update(Message::HelpExample);
        assert!(clean.app.help.is_none());
        assert!(clean.app.file.is_none());
    }

    /// Clicks the middle of a window rectangle.
    fn click(ui: &mut Ui, at: Point) {
        ui.move_to(at);
        ui.press();
        ui.release();
    }

    /// With nothing selected the inspector is short, and its button and F1
    /// open the shortcuts page (#457).
    #[test]
    fn empty_inspector_and_f1_open_the_shortcuts() {
        let mut ui = Ui::open("subtractive.ron");
        assert!(ui.text("Select a module to edit it.").is_some());
        assert!(
            ui.text(crate::HELP.lines().next().unwrap()).is_none(),
            "no shortcut list in the panel"
        );
        let button = ui.text("Keyboard shortcuts").expect("the shortcuts button").center();
        click(&mut ui, button);
        assert_eq!(ui.app.help.as_ref().map(|h| h.model.name), Some("Keyboard shortcuts"));
        ui.tap(Key::Named(Named::Escape), Modifiers::default());
        assert!(ui.app.help.is_none());
        ui.tap(Key::Named(Named::F1), Modifiers::default());
        assert_eq!(ui.app.help.as_ref().map(|h| h.model.name), Some("Keyboard shortcuts"));
    }

    /// Help details read as words with units: no "unconnected:", no stray
    /// spaces, and lower-case signal kinds (#457).
    #[test]
    fn help_details_read_as_words_with_units() {
        let svf = loom_modules::registry().resolve("loom.filter.svf").unwrap();
        let help = Help::new(svf);
        let m = &help.model;
        assert_eq!(m.params[0].details, "20 to 20000 Hz, logarithmic. Default 1000 Hz.");
        assert_eq!(m.inputs[0].details, "audio; reads 0 with no cable");
        assert_eq!(m.outputs[0].details, "audio, bipolar");
        for entry in m.params.iter().chain(&m.inputs).chain(&m.outputs) {
            assert!(!entry.details.contains("unconnected"), "{}", entry.details);
            assert!(!entry.details.contains(" ,"), "{}", entry.details);
        }
    }

    /// Help text wraps inside the overlay, and a click on the dimmed
    /// backdrop closes help while a click inside it does not (#457).
    #[test]
    fn help_wraps_inside_and_closes_from_the_backdrop() {
        let mut ui = Ui::open("subtractive.ron");
        let _ = ui.app.update(Message::Canvas(CanvasEvent::Select(vec![4])));
        ui.tap(Key::Named(Named::F1), Modifiers::default());
        let close = ui.text("Close").expect("the Close button");
        // The panel ends 20 px right of its Close button.
        let right = close.x + close.width + 20.0;
        let help = ui.app.help.as_ref().unwrap().model.clone();
        for entry in help.params.iter().chain(&help.inputs).chain(&help.outputs) {
            let text = ui.text(entry.description).expect("an entry's description");
            assert!(
                text.x + text.width <= right,
                "{} runs past the panel",
                entry.description
            );
        }
        // "Parameters" shows only in help; the summary is also in the
        // inspector, outside the overlay.
        let heading = ui.text("Parameters").expect("a help heading");
        click(&mut ui, heading.center());
        assert!(ui.app.help.is_some(), "a click inside keeps help open");
        click(&mut ui, Point::new(10.0, crate::WINDOW.height / 2.0));
        assert!(ui.app.help.is_none(), "a click on the backdrop closes help");
    }

    /// The inspector lists each port on one line with its signal and range,
    /// and keeps its description for hover text (#457).
    #[test]
    fn inspector_ports_are_one_line_each() {
        let mut ui = Ui::open("subtractive.ron");
        let _ = ui.app.update(Message::Canvas(CanvasEvent::Select(vec![4])));
        let crate::inspector::Inspector::Module(rows) = ui.app.inspector_model() else {
            panic!("the SVF's rows");
        };
        let input = &rows.ports[0];
        assert_eq!((input.label, input.details.as_str()), ("In", "audio, -1 to 1"));
        assert!(input.description.starts_with("The audio to filter."));
        assert!(ui.text("audio, -1 to 1").is_some());
        assert!(ui.text(&rows.kind).is_none(), "no raw type line");
    }

    /// Browser rows show no ID; the highlighted module's ID shows under
    /// its description (#457).
    #[test]
    fn the_browser_shows_the_id_under_the_description() {
        let mut ui = Ui::open("subtractive.ron");
        // As a user opens it: A over the canvas.
        let canvas = ui.canvas();
        ui.move_to(Point::new(canvas.bounds.x + 8.0, canvas.bounds.y + 8.0));
        ui.tap(Key::Character("a".into()), Modifiers::default());
        let first = ui.app.browser.as_ref().unwrap().highlighted().unwrap();
        assert!(ui.text(first.id).is_some(), "the highlighted ID");
        assert!(ui.text(first.description).is_some());
        let second = ui.app.browser.as_ref().unwrap().results()[1];
        assert!(ui.text(second.id).is_none(), "no ID on other rows");
        // Each row shows its family (ADR-0029).
        assert!(ui.text(first.family.label()).is_some(), "the row's family");
    }
}

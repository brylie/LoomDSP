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

/// The open overlay and its parsed Markdown.
pub struct Help {
    pub model: Model,
    markdown: Vec<markdown::Item>,
}

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
            let channels = if p.channels == loom_core::Channels::Stereo {
                ", stereo"
            } else {
                ""
            };
            let extra = if input {
                let default = match p.default {
                    loom_core::PortDefault::Value(value) => value.to_string(),
                    loom_core::PortDefault::BarPhase => "bar phase".into(),
                    loom_core::PortDefault::BeatPhase => "beat phase".into(),
                    loom_core::PortDefault::Playing => "play gate".into(),
                };
                format!("unconnected: {default}")
            } else {
                format!("{:?}", p.polarity())
            };
            Entry {
                name: p.name,
                label: p.label,
                description: p.description,
                details: format!("{}{channels}, {extra}", p.signal.name()),
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
                    details: format!(
                        "{} to {} {}, default {}{}",
                        p.min,
                        p.max,
                        p.unit,
                        p.default,
                        if p.options.choices.is_empty() {
                            String::new()
                        } else {
                            format!(", choices {}", p.options.choices.join(", "))
                        }
                    ),
                })
                .collect(),
            notes: notes.clone(),
            example,
        };
        Self {
            markdown: markdown::parse(&notes).collect(),
            model,
        }
    }

    /// Draws the overlay from the same model exposed to snapshots.
    pub fn view(&self, theme: Theme) -> Element<'_, Message> {
        let m = &self.model;
        let header = row![
            text(m.name).size(style::TITLE),
            space::horizontal(),
            button("Close").on_press(Message::CloseHelp)
        ]
        .align_y(iced::Alignment::Center);
        let mut content = column![
            text(format!("{} · {} · {}", m.family, m.category, m.id)).size(style::SMALL),
            text(m.description).size(style::BODY)
        ]
        .spacing(10);
        for (title, entries) in [
            ("Inputs", &m.inputs),
            ("Outputs", &m.outputs),
            ("Parameters", &m.params),
        ] {
            content = content.push(text(title).size(style::HEADING));
            if entries.is_empty() {
                content = content.push(text("None.").size(style::BODY));
            }
            for entry in entries {
                content = content.push(
                    text(format!(
                        "{} ({}) · {}\n{}",
                        entry.label, entry.name, entry.details, entry.description
                    ))
                    .size(style::BODY),
                );
            }
        }
        if m.example.is_some() {
            content = content.push(button("Open example").on_press(Message::HelpExample));
        }
        content = content.push(markdown::view(&self.markdown, style::markdown(&theme)).map(Message::HelpLink));
        container(
            container(
                column![
                    header,
                    scrollable(content).id("module-help-content").height(Length::Fill)
                ]
                .spacing(10),
            )
            .padding(20)
            .width(720)
            .height(Length::Fill)
            .style(container::rounded_box),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(30)
        .into()
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
}

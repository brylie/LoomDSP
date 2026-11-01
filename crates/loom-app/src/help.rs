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
mod tests;

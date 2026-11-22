//! Offline module help from the running registry and bundled notes.

use crate::Element;
use crate::style::{self, button};
use iced::widget::markdown as iced_markdown;
use iced::widget::{column, container, row, scrollable, space, text};
use iced::{Length, Theme};
use loom_core::{ModuleDescriptor, Registry, RoleDescriptor, RolePort};
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
    /// The notes before the example section, parsed: what the view shows
    /// and the accessibility tree reads. Left out of the snapshot, which
    /// has the notes as written.
    #[serde(skip)]
    pub(crate) lead: Vec<markdown::Block>,
    /// The example section and what follows it, parsed.
    #[serde(skip)]
    pub(crate) tail: Vec<markdown::Block>,
}

impl Model {
    /// The heading details, shared by the overlay and its accessibility tree.
    pub(crate) fn summary(&self) -> String {
        [self.family, self.category, self.id]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// The open overlay and its parsed Markdown: the notes before the
/// example, and the example section with what follows it.
pub struct Help {
    pub model: Model,
    lead: Vec<iced_markdown::Item>,
    tail: Vec<iced_markdown::Item>,
}

/// Where the hand-written notes' example section starts. The overlay shows
/// the reference sections between the notes and the example
/// (help-pages.md, In the app).
pub(crate) const EXAMPLE_HEADING: &str = "\n## Example";

/// The notes before the example section, and the section with what follows
/// it.
fn split_example(notes: &str) -> (&str, &str) {
    match notes.find(EXAMPLE_HEADING) {
        Some(at) => notes.split_at(at),
        None => (notes, ""),
    }
}

impl Help {
    /// Builds the view from the actual registry descriptor.
    pub fn new(d: &'static ModuleDescriptor) -> Self {
        let mut notes = bundled_notes(d.id);
        if !d.roles.is_empty() {
            let roles: Vec<_> = d
                .roles
                .iter()
                .map(|fills| format!("[{}]({}.md)", fills.role.name, fills.role.id))
                .collect();
            notes = format!("Fills roles: {}.\n\n{notes}", roles.join(", "));
        }
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
            lead: Vec::new(),
            tail: Vec::new(),
        };
        Self::from_model(model)
    }

    /// Builds offline help for the contract a slot's parts fill.
    pub fn role(role: &'static RoleDescriptor, registry: &Registry) -> Self {
        let requirement = |required| if required { "required" } else { "optional" };
        let port = |p: &RolePort| Entry {
            name: p.name,
            label: p.label,
            description: p.description,
            details: format!("{}, {}", p.signal.name(), requirement(p.required)),
        };
        let parts = registry.parts_by_name(role.id);
        let links: Vec<_> = parts.iter().map(|d| format!("[{}]({}.md)", d.name, d.id)).collect();
        let parts = if links.is_empty() {
            "No registered modules fill this role yet.".to_string()
        } else {
            format!("Modules that fill this role: {}.", links.join(", "))
        };
        let notes = format!("{parts}\n\n{}", bundled_notes(role.id));
        Self::from_model(Model {
            id: role.id,
            name: role.name,
            category: "Slot role",
            family: "",
            description: role.description,
            inputs: role.inputs.iter().map(port).collect(),
            outputs: role.outputs.iter().map(port).collect(),
            params: role
                .params
                .iter()
                .map(|p| {
                    let values = if p.whole { "whole numbers" } else { "continuous" };
                    let details = [p.unit, values, requirement(p.required)]
                        .into_iter()
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>()
                        .join(", ");
                    Entry {
                        name: p.name,
                        label: p.label,
                        description: p.description,
                        details,
                    }
                })
                .collect(),
            notes,
            example: None,
            lead: Vec::new(),
            tail: Vec::new(),
        })
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
            lead: Vec::new(),
            tail: Vec::new(),
        })
    }

    /// Parses the notes, split before the example section: once into our
    /// own blocks, which the accessibility tree reads, and, until the view
    /// moves to `loom-ui` (plan 3.2, row 6), once more for Iced's view.
    fn from_model(mut model: Model) -> Self {
        let (lead, tail) = split_example(&model.notes);
        let (iced_lead, iced_tail) = (
            iced_markdown::parse(lead).collect(),
            iced_markdown::parse(tail).collect(),
        );
        model.lead = markdown::parse(lead).blocks;
        model.tail = markdown::parse(tail).blocks;
        Self {
            lead: iced_lead,
            tail: iced_tail,
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
            crate::inspector::ids::named(
                &crate::overlays::access::help_id().with("close"),
                button(text("Close").size(style::CONTROL)).on_press(Message::CloseHelp),
            )
        ]
        .align_y(iced::Alignment::Center);
        let mut content = column![].spacing(12).width(Length::Fill);
        if !m.id.is_empty() {
            content = content.push(text(m.summary()).size(style::SMALL).color(style::QUIET));
            content = content.push(text(m.description).size(style::BODY));
        }
        content = content.push(iced_markdown::view(&self.lead, style::markdown(&theme)).map(Message::HelpLink));
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
            content = content.push(crate::inspector::ids::named(
                &crate::overlays::access::help_id().with("example"),
                button(text("Open example").size(style::CONTROL)).on_press(Message::HelpExample),
            ));
        }
        content = content.push(iced_markdown::view(&self.tail, style::markdown(&theme)).map(Message::HelpLink));
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
        // The panel is named by its UI ID, so the window says where it is.
        let panel = crate::inspector::ids::named(&crate::overlays::access::help_id(), panel);
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

/// The hand-written part of a bundled help page.
fn bundled_notes(id: &str) -> String {
    bundled_page(id)
        .and_then(|page| page.split_once(MARKER).map(|(_, notes)| notes.trim()))
        .unwrap_or("")
        .replace("\r\n", "\n")
}

impl crate::App {
    /// The help page for a canvas tile, including a slot's role contract.
    pub(crate) fn tile_help(&self, id: u32) -> Option<Help> {
        let id = loom_core::ModuleId(id);
        let controller = &self.session.controller;
        if let Some(slot) = self.session.hierarchy.slot(id) {
            return Some(Help::role(slot.role, controller.registry()));
        }
        controller.graph().node(id).map(|node| Help::new(node.descriptor))
    }
}

pub(crate) mod markdown;

#[cfg(test)]
mod tests;

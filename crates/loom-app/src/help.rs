//! Offline module help from the running registry and bundled notes.

use crate::style;
use iced::Theme;
use iced::widget::container;
use loom_core::{ModuleDescriptor, Registry, RoleDescriptor, RolePort};
use serde::Serialize;

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

/// The open overlay: its model, with the notes parsed into blocks, which
/// its panel draws and the accessibility tree reads (`help/panel.rs`).
pub struct Help {
    pub model: Model,
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

    /// Parses the notes once, split before the example section.
    fn from_model(mut model: Model) -> Self {
        let (lead, tail) = split_example(&model.notes);
        model.lead = markdown::parse(lead).blocks;
        model.tail = markdown::parse(tail).blocks;
        Self { model }
    }
}

/// The dialog's panel, from the style token (`style::dialog`), which
/// `loom-ui`'s panels take too. Your patches uses it as well.
pub(crate) fn dialog_style(theme: &Theme) -> container::Style {
    use loom_canvas::iced_convert::ToIced as _;
    let look = style::dialog(theme);
    container::Style {
        background: Some(look.fill.to_iced().into()),
        text_color: Some(look.text.to_iced()),
        border: iced::border::rounded(look.radius),
        ..container::Style::default()
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
mod panel;

#[cfg(test)]
mod tests;

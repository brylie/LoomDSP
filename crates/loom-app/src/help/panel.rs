//! Help as a panel of `loom-ui`'s Overlays layer (help-pages.md, Keys,
//! links, and focus; plan 3.2, row 6): a backdrop over the window, a box
//! with the title and Close, and one scrolling body in reading order. Its
//! notes are the blocks `help/markdown.rs` parsed, as rich text, so the
//! view and the accessibility tree read one source (ui-layer.md, section
//! 7, rule 1). Each link to a bundled page is a link of its own; a link to
//! a page the app cannot open is plain text.

use super::markdown::{Block, Kind};
use super::{Help, Model};
use crate::App;
use crate::overlays::access::help_id;
use crate::style::{self, Role as ButtonRole};
use loom_canvas::iced_convert::ToCore as _;
use loom_ui::access::{Access, Action, Role};
use loom_ui::{Align, Element, Inline, Layer, Length, View};
use loom_ui_core::{Color, UiId};

/// The UI ID of help's Close button.
pub(crate) fn close_id() -> UiId {
    help_id().with("close")
}

/// The key of the backdrop's own panel, under the box's, so its press
/// is told apart from the box's.
fn backdrop_panel() -> UiId {
    help_id().with("backdrop")
}

/// The UI ID of the backdrop around the box, a press on which closes help.
fn backdrop_id() -> UiId {
    backdrop_panel().with("scrim")
}

/// The UI ID of help's scrolling body. Every part of the page inside it
/// is keyed under it, since a scroll container keeps its offset by its UI
/// ID: `help/body/example`, `help/body/notes/0`, and so on.
pub(crate) fn body_id() -> UiId {
    help_id().with("body")
}

/// The UI ID of help's Open example button.
fn example_id() -> UiId {
    body_id().with("example")
}

/// The UI ID of the first part of the page, at the top of the body: the
/// summary on a module's page, the notes on the shortcuts page. A new page
/// is scrolled to it, so it starts at its top.
pub(crate) fn top_id(model: &Model) -> UiId {
    if model.id.is_empty() {
        body_id().with("notes")
    } else {
        body_id().with("summary")
    }
}

/// The key of link `n` in a block.
fn link_key(n: usize) -> String {
    format!("link-{n}")
}

/// What one block of the notes shows: its block, and which of its links
/// the app can follow.
#[derive(Debug, Clone, PartialEq)]
struct Notes {
    /// The blocks, in reading order.
    blocks: Vec<Block>,
    /// For each block, for each of its links, whether its page is bundled.
    bundled: Vec<Vec<bool>>,
}

/// One entry of Parameters, Inputs, or Outputs: its name, its label, its
/// details in words, and its description.
type EntryLines = (&'static str, String, String, &'static str);

/// Parameters, Inputs, or Outputs: its key, its title, and its entries.
type Section = (&'static str, &'static str, Vec<EntryLines>);

/// Everything help's panel shows, so the runtime builds it again only
/// when this changes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Panel {
    /// The title.
    title: &'static str,
    /// The family, category, and ID, or nothing for the shortcuts page.
    summary: Option<String>,
    /// The one-sentence description.
    description: &'static str,
    /// The notes before the example.
    lead: Notes,
    /// Parameters, Inputs, and Outputs: the key, the title, and each
    /// entry's name and three lines. Empty for the shortcuts page.
    sections: Vec<Section>,
    /// The page has an example to open.
    example: bool,
    /// The example section and what follows.
    tail: Notes,
    /// The look of the backdrop, the box, its text, and its links.
    looks: Looks,
}

/// The colours help is painted in, from the style tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Looks {
    scrim: Color,
    fill: Color,
    text: Color,
    quiet: Color,
    link: Color,
    radius: f32,
}

impl App {
    /// Adds help's panel to the runtime's view, while help is open.
    pub(crate) fn help_panel(&self, view: &mut View) {
        let Some(help) = &self.overlays.help else {
            return;
        };
        let theme = self.theme();
        let dialog = style::dialog(&theme);
        let looks = Looks {
            scrim: style::HELP_SCRIM,
            fill: dialog.fill,
            text: dialog.text,
            quiet: style::QUIET.to_core(),
            link: style::link(&theme).colour,
            radius: dialog.radius,
        };
        let panel = self.help_model(&help.model, looks);
        // Two panels: the backdrop, a press on which closes help, then the
        // box over it, so the box's parts keep IDs under `help` alone.
        view.panel_with(backdrop_panel(), Layer::Overlays, &looks.scrim, backdrop);
        view.panel_with(help_id(), Layer::Overlays, &panel, build);
    }

    /// Help's panel model for `model`.
    fn help_model(&self, model: &Model, looks: Looks) -> Panel {
        let notes = |blocks: &[Block]| Notes {
            blocks: blocks.to_vec(),
            bundled: blocks
                .iter()
                .map(|block| block.links().iter().map(|link| self.bundled(&link.target)).collect())
                .collect(),
        };
        let module = !model.id.is_empty();
        let entries = |entries: &[super::Entry]| {
            entries
                .iter()
                .map(|entry| {
                    (
                        entry.name,
                        entry.label.to_string(),
                        entry.details.clone(),
                        entry.description,
                    )
                })
                .collect()
        };
        Panel {
            title: model.name,
            summary: module.then(|| model.summary()),
            description: model.description,
            lead: notes(&model.lead),
            sections: if module {
                vec![
                    ("parameters", "Parameters", entries(&model.params)),
                    ("inputs", "Inputs", entries(&model.inputs)),
                    ("outputs", "Outputs", entries(&model.outputs)),
                ]
            } else {
                Vec::new()
            },
            example: model.example.is_some(),
            tail: notes(&model.tail),
            looks,
        }
    }

    /// True when help can open the page `target` names: a module's or a
    /// role's bundled page, as [`crate::Message::HelpLink`] follows it.
    pub(crate) fn bundled(&self, target: &str) -> bool {
        target.strip_suffix(".md").is_some_and(|id| {
            !id.contains('/')
                && (self.modules.iter().any(|d| d.id == id) || self.session.controller.registry().role(id).is_some())
        })
    }

    /// Shows a page a link opened from its top, with the focus on Close
    /// again, so Tab starts over, as on a new page in a browser: the old
    /// link's UI ID may name nothing on the new page, or another link part
    /// way down it (help-pages.md, Keys, links, and focus).
    pub(crate) fn help_to_top(&mut self) {
        if let Some(help) = &self.overlays.help {
            let top = top_id(&help.model);
            self.effect(loom_ui::Effects::scroll_into_view(top));
            self.effect(loom_ui::Effects::focus_as_input(close_id()));
        }
    }

    /// The message for a click on a part of help: Close and the backdrop
    /// close it, Open example opens the example, and a link follows its
    /// page. From the pointer, Enter, or a screen reader.
    pub(crate) fn help_click(&self, id: &UiId) -> Option<crate::Message> {
        let help = self.overlays.help.as_ref()?;
        if *id == close_id() || *id == backdrop_id() {
            return Some(crate::Message::CloseHelp);
        }
        if *id == example_id() {
            return Some(crate::Message::HelpExample);
        }
        for (key, blocks) in [("notes", &help.model.lead), ("example-notes", &help.model.tail)] {
            for (at, block) in blocks.iter().enumerate() {
                for (n, link) in block.links().iter().enumerate() {
                    if *id == body_id().with(key).with(at as u32).with(link_key(n)) {
                        return Some(crate::Message::HelpLink(link.target.clone()));
                    }
                }
            }
        }
        None
    }

    /// Help's focus stops, in reading order: Close, the links of the
    /// notes, Open example, then the example section's links.
    pub(crate) fn help_stops(&self) -> Vec<UiId> {
        let Some(help) = &self.overlays.help else {
            return Vec::new();
        };
        let links = |key: &str, blocks: &[Block]| {
            blocks
                .iter()
                .enumerate()
                .flat_map(|(at, block)| {
                    block
                        .links()
                        .iter()
                        .enumerate()
                        .filter(|(_, link)| self.bundled(&link.target))
                        .map(move |(n, _)| body_id().with(key).with(at as u32).with(link_key(n)))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        let mut stops = vec![close_id()];
        stops.extend(links("notes", &help.model.lead));
        if help.model.example.is_some() {
            stops.push(example_id());
        }
        stops.extend(links("example-notes", &help.model.tail));
        stops
    }
}

impl App {
    /// The stop Tab goes to inside help from `from`, or the one before when
    /// `back`, wrapping at both ends; from no stop of help's, the first (or
    /// the last). The runtime's Tab and a script's both come here.
    pub(crate) fn help_next_stop(&self, from: Option<&UiId>, back: bool) -> Option<UiId> {
        let stops = self.help_stops();
        let last = stops.len().checked_sub(1)?;
        let at = from.and_then(|focus| stops.iter().position(|stop| stop == focus));
        let to = match (at, back) {
            (Some(at), false) => {
                if at == last {
                    0
                } else {
                    at + 1
                }
            }
            (Some(at), true) => at.checked_sub(1).unwrap_or(last),
            (None, false) => 0,
            (None, true) => last,
        };
        stops.get(to).cloned()
    }
}

impl Help {
    /// Where the focus goes when help opens: its Close button, the first
    /// thing a keyboard user can act on, at the top of the box, so nothing
    /// scrolls away (help-pages.md, Keys, links, and focus).
    pub(crate) fn first_stop() -> UiId {
        close_id()
    }
}

/// The backdrop over the whole window, which takes every press under it.
fn backdrop(scrim: &Color) -> Element {
    Element::column([])
        .width(Length::FILL)
        .height(Length::FILL)
        .background(*scrim)
        .key("scrim")
        .opaque()
}

/// The box, centred over the backdrop. Only the box takes presses, so one
/// beside it reaches the backdrop.
fn build(panel: &Panel) -> Element {
    let looks = panel.looks;
    let title = Element::row([
        Element::text(panel.title, style::TITLE_TEXT).colour(looks.text),
        Element::space(Length::FILL, Length::Shrink),
        button("close", "Close"),
    ])
    .align_y(Align::Center)
    .width(Length::FILL);
    let mut body = Vec::new();
    if let Some(summary) = &panel.summary {
        body.push(
            Element::text(summary.clone(), style::SMALL_TEXT)
                .colour(looks.quiet)
                .wrap()
                .key("summary")
                .access(Access::label(summary.clone())),
        );
        body.push(
            Element::text(panel.description, style::BODY_TEXT)
                .colour(looks.text)
                .wrap()
                .key("description")
                .access(Access::label(panel.description)),
        );
    }
    body.push(notes("notes", &panel.lead, looks));
    for (key, title, entries) in &panel.sections {
        body.push(section(key, title, entries, looks));
    }
    if panel.example {
        body.push(button("example", "Open example"));
    }
    body.push(notes("example-notes", &panel.tail, looks));
    let content = Element::column(body).spacing(12.0).width(Length::FILL);
    // The scroll keys reach it from Close too, which is outside it. A
    // filling column takes its content's height at least, as flexbox's
    // minimum does, so the body and the box may shrink to the room the
    // window gives, and the body scrolls inside it.
    let scroll = Element::scroll(content, style::scroll())
        .key("body")
        .scrolls_from(close_id())
        .height(Length::FILL)
        .min_height(0.0)
        .width(Length::FILL)
        .access(Access::new(Role::ScrollView, "Help text"));
    let dialog = Element::column([title, scroll])
        .spacing(12.0)
        .padding(20.0)
        .max_width(style::HELP_WIDTH)
        .width(Length::FILL)
        .height(Length::FILL)
        .min_height(0.0)
        .background(looks.fill)
        .radius(looks.radius)
        .opaque();
    Element::column([dialog])
        .width(Length::FILL)
        .height(Length::FILL)
        .padding(30.0)
        .align_x(Align::Center)
}

/// A button in the secondary role, keyed `key`.
fn button(key: &'static str, label: &'static str) -> Element {
    let role = ButtonRole::Secondary;
    Element::text(label, style::CONTROL_TEXT)
        .padding(style::BUTTON_PADDING)
        .radius(role.tokens().radius)
        .looks(role.looks())
        .key(key)
        .focusable()
        .access(Access::new(Role::Button, label).action(Action::Click))
}

/// Parameters, Inputs, or Outputs: a heading, then each entry's label,
/// its details, and its description, or "None.".
fn section(key: &'static str, title: &'static str, entries: &[EntryLines], looks: Looks) -> Element {
    let mut rows = vec![Element::text(title, style::HEADING_TEXT).colour(looks.text)];
    if entries.is_empty() {
        rows.push(
            Element::text("None.", style::BODY_TEXT)
                .colour(looks.quiet)
                .key("none")
                .access(Access::label("None.")),
        );
    }
    for (name, label, details, description) in entries {
        rows.push(
            Element::column([
                Element::text(label.clone(), style::CONTROL_TEXT)
                    .colour(looks.text)
                    .wrap(),
                Element::text(details.clone(), style::SMALL_TEXT)
                    .colour(looks.quiet)
                    .wrap(),
                Element::text(*description, style::BODY_TEXT).colour(looks.text).wrap(),
            ])
            .spacing(2.0)
            .key(*name)
            .access(Access::label(format!("{label}: {details} {description}"))),
        );
    }
    Element::column(rows)
        .spacing(12.0)
        .key(key)
        .access(Access::new(Role::Group, title))
}

/// The notes' blocks under `key`, each a rich paragraph keyed by its place
/// on the page, which does not change while it is open (ui-layer.md,
/// section 4).
fn notes(key: &'static str, notes: &Notes, looks: Looks) -> Element {
    let blocks = notes
        .blocks
        .iter()
        .zip(&notes.bundled)
        .enumerate()
        .filter_map(|(at, (block, bundled))| {
            let words = block.words();
            if words.trim().is_empty() {
                return None;
            }
            let (style, prefix) = match block {
                Block::Text {
                    kind: Kind::Heading, ..
                } => (style::HEADING_TEXT, None),
                Block::Text { kind: Kind::Bullet, .. } => (style::BODY_TEXT, Some("•")),
                Block::Text { .. } => (style::BODY_TEXT, None),
                Block::Code(_) => (style::SMALL_TEXT, None),
            };
            let text = Element::rich(pieces(block, bundled, looks), style)
                .colour(looks.text)
                .width(Length::FILL)
                .key(at as u32)
                .access(Access::label(words));
            Some(match prefix {
                Some(mark) => Element::row([Element::text(mark, style).colour(looks.text), text])
                    .spacing(6.0)
                    .width(Length::FILL),
                None => text,
            })
        });
    Element::column(blocks).spacing(8.0).key(key).width(Length::FILL)
}

/// The pieces of `block` as rich text: bold, inline code in the monospace
/// face, and each bundled link a link of its own, in the link colour and
/// underlined; a link to a page that is not bundled is plain text. A link
/// made of several pieces (a bold word inside it) is one piece here, in
/// its first piece's weight, so it is one part with one UI ID.
fn pieces(block: &Block, bundled: &[bool], looks: Looks) -> Vec<Inline> {
    let Block::Text { pieces, links, .. } = block else {
        return vec![Inline::new(block.words()).mono()];
    };
    let mut out: Vec<Inline> = Vec::new();
    let mut at = 0;
    while at < pieces.len() {
        let piece = &pieces[at];
        let link = piece.link.filter(|&n| bundled.get(n).copied().unwrap_or(false));
        let Some(n) = link else {
            let mut inline = Inline::new(piece.text.clone());
            if piece.bold {
                inline = inline.bold();
            }
            if piece.code {
                inline = inline.mono();
            }
            out.push(inline);
            at += 1;
            continue;
        };
        let mut text = String::new();
        while at < pieces.len() && pieces[at].link == Some(n) {
            text.push_str(&pieces[at].text);
            at += 1;
        }
        let mut inline = Inline::new(text);
        if piece.bold {
            inline = inline.bold();
        }
        out.push(inline.colour(looks.link).underline().link(
            link_key(n),
            Access::new(Role::Link, links[n].label.clone()).action(Action::Click),
        ));
    }
    out
}

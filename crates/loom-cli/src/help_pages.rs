//! `loom modules --markdown DIR`: one Markdown help page per module, made
//! from its `ModuleDescriptor` (FR-LIB-11). Spec: `docs/design/help-pages.md`.
//!
//! The descriptor is the one source of truth for names, ports, parameters,
//! and ranges, so the generated part of a page is never edited by hand.
//! Longer notes and an example patch go below [`MARKER`]; regenerating keeps
//! everything after it.

use loom_core::{Channels, ModuleDescriptor, Polarity, PortSpec, Registry, Widget, WidgetKind};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};

/// The line that ends the generated part of a page. Everything below it is
/// hand-written and kept when the page is generated again.
pub const MARKER: &str = "<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->";

/// The note at the top of every generated page.
const GENERATED: &str =
    "<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->";

/// File name of the index page.
pub const INDEX: &str = "README.md";

/// How to regenerate the pages, for messages.
pub const REGENERATE: &str = "mise run help-pages";

/// File name of a module's page: its ID, such as `loom.osc.sine.md`.
pub fn file_name(d: &ModuleDescriptor) -> String {
    format!("{}.md", d.id)
}

/// Writes every page and the index into `dir`, keeping the hand-written
/// notes of existing pages. Returns what it did, for the user.
///
/// A page without the marker line is not touched, so hand-written text is
/// never lost. Pages of modules that no longer exist are reported, not
/// deleted, because they may hold notes worth moving.
pub fn write_pages(registry: &Registry, dir: &Path) -> io::Result<Report> {
    std::fs::create_dir_all(dir)?;
    let mut report = Report::default();
    for (path, wanted) in expected(registry, dir)? {
        match wanted {
            Expected::Content(text) => {
                if read_normalized(&path)?.as_deref() != Some(text.as_str()) {
                    std::fs::write(&path, text)?;
                    report.written.push(path);
                }
            }
            Expected::NoMarker => report.no_marker.push(path),
        }
    }
    report.unknown = unknown_pages(registry, dir)?;
    Ok(report)
}

/// Lists every way the pages in `dir` differ from what [`write_pages`]
/// would write. Empty when they are current. Writes nothing.
pub fn stale_pages(registry: &Registry, dir: &Path) -> io::Result<Vec<String>> {
    let mut problems = Vec::new();
    for (path, wanted) in expected(registry, dir)? {
        let name = display_name(&path);
        match wanted {
            Expected::Content(text) => match read_normalized(&path)? {
                None => problems.push(format!("{name} is missing")),
                Some(found) if found != text => problems.push(format!("{name} is out of date")),
                Some(_) => {}
            },
            Expected::NoMarker => problems.push(format!("{name} has no marker line, so it cannot be regenerated")),
        }
    }
    for path in unknown_pages(registry, dir)? {
        problems.push(format!("{} is for a module that does not exist", display_name(&path)));
    }
    Ok(problems)
}

/// What [`write_pages`] did.
#[derive(Debug, Default)]
pub struct Report {
    /// Pages and index written because they were missing or out of date.
    pub written: Vec<PathBuf>,
    /// Existing pages left alone because they have no marker line.
    pub no_marker: Vec<PathBuf>,
    /// Pages whose module is not in the registry.
    pub unknown: Vec<PathBuf>,
}

/// What a file in the help folder should hold.
enum Expected {
    /// This exact text.
    Content(String),
    /// Nothing can be generated: the existing page has no marker line.
    NoMarker,
}

/// Every file the generator owns in `dir`, with what it should hold.
fn expected(registry: &Registry, dir: &Path) -> io::Result<Vec<(PathBuf, Expected)>> {
    let mut files = vec![(dir.join(INDEX), Expected::Content(index(registry)))];
    for d in registry.iter() {
        let path = dir.join(file_name(d));
        let notes = match read_normalized(&path)? {
            None => Some(String::new()),
            Some(existing) => notes_of(&existing),
        };
        let wanted = match notes {
            Some(notes) => Expected::Content(format!("{}{MARKER}\n{notes}", page(d))),
            None => Expected::NoMarker,
        };
        files.push((path, wanted));
    }
    Ok(files)
}

/// The text after the marker line, or `None` if the page has no marker.
///
/// Only a line that is exactly the marker counts. A page that quotes the
/// marker inside other text has no marker, so rule 3 keeps it as it is.
fn notes_of(page: &str) -> Option<String> {
    let mut start = 0;
    for line in page.split_inclusive('\n') {
        start += line.len();
        if line.trim_end_matches(['\r', '\n']) == MARKER {
            return Some(page[start..].to_string());
        }
    }
    None
}

/// Reads a file with Windows line endings turned into `\n`, so a checkout
/// that converts line endings does not look stale. `None` if it is missing.
fn read_normalized(path: &Path) -> io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text.replace("\r\n", "\n"))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Markdown pages in `dir` that belong to no registered module.
fn unknown_pages(registry: &Registry, dir: &Path) -> io::Result<Vec<PathBuf>> {
    let known: Vec<String> = registry.iter().map(file_name).collect();
    let mut unknown = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // No folder yet: no pages, so none unknown.
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(unknown),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.ends_with(".md") && name != INDEX && !known.iter().any(|k| k == name) {
            unknown.push(path);
        }
    }
    unknown.sort();
    Ok(unknown)
}

/// The file name of `path`, for messages.
fn display_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// The index page: every module by family, in browser order, with a link
/// to its page (ADR-0029).
pub fn index(registry: &Registry) -> String {
    let mut families: BTreeMap<loom_core::Family, Vec<&ModuleDescriptor>> = BTreeMap::new();
    for d in registry.iter() {
        families.entry(d.family).or_default().push(d);
    }
    let mut out = String::new();
    out.push_str("# Module help pages\n\n");
    out.push_str(GENERATED);
    out.push_str("\n\n");
    let _ = writeln!(
        out,
        "One page for each built-in module, made from its descriptor. After you change a descriptor, run `{REGENERATE}` and commit the pages. Longer notes and example patches go below the marker line at the end of a page. See the [help pages specification](../design/help-pages.md)."
    );
    for (family, mut modules) in families {
        modules.sort_by(|a, b| a.name.cmp(b.name).then(a.id.cmp(b.id)));
        let _ = writeln!(out, "\n## {}\n", family.label());
        out.push_str("| Module | ID | Description |\n|---|---|---|\n");
        for d in modules {
            let _ = writeln!(
                out,
                "| [{}]({}) | `{}` | {} |",
                cell(d.name),
                file_name(d),
                d.id,
                cell(d.description)
            );
        }
    }
    out
}

/// The generated part of a module's page, up to the marker line.
pub fn page(d: &ModuleDescriptor) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {}\n\n{GENERATED}\n\n{}\n", d.name, d.description);
    out.push_str("| | |\n|---|---|\n");
    let _ = writeln!(out, "| ID | `{}` |", d.id);
    let _ = writeln!(out, "| Version | {} |", d.version);
    let _ = writeln!(out, "| Family | {} |", d.family.label());
    let _ = writeln!(out, "| Category | {} |", cell(d.category));
    out.push_str("\n## Inputs\n\n");
    if d.inputs.is_empty() {
        out.push_str("None.\n");
    } else {
        out.push_str("| Port | Signal | Unconnected | Description |\n|---|---|---|---|\n");
        for p in d.inputs {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} |",
                port_name(p),
                signal(p),
                p.default.text(),
                cell(p.description)
            );
        }
    }
    out.push_str("\n## Outputs\n\n");
    if d.outputs.is_empty() {
        out.push_str("None.\n");
    } else {
        out.push_str("| Port | Signal | Polarity | Description |\n|---|---|---|---|\n");
        for p in d.outputs {
            let polarity = match p.polarity() {
                Polarity::Unipolar => "unipolar",
                Polarity::Bipolar => "bipolar",
            };
            let _ = writeln!(
                out,
                "| {} | {} | {polarity} | {} |",
                port_name(p),
                signal(p),
                cell(p.description)
            );
        }
    }
    out.push_str("\n## Parameters\n\n");
    if d.params.is_empty() {
        out.push_str("None.\n");
    } else {
        out.push_str("| Parameter | Range | Default | Description |\n|---|---|---|---|\n");
        for p in d.params {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} |",
                name(p.label, p.name),
                cell(&p.range_text()),
                cell(&p.default_text()),
                cell(p.description)
            );
        }
    }
    if !d.panel.widgets.is_empty() {
        let _ = writeln!(
            out,
            "\n## Panel\n\n{} by {} grid units.\n",
            d.panel.width, d.panel.height
        );
        for w in d.panel.widgets {
            let _ = writeln!(out, "- {}", widget(d, w));
        }
    }
    out.push('\n');
    out
}

/// `**Label** (`name`)`, or just the name when there is no label.
fn name(label: &str, name: &str) -> String {
    if label.trim().is_empty() {
        format!("`{name}`")
    } else {
        format!("**{}** (`{name}`)", cell(label))
    }
}

/// A port's label and stable name.
fn port_name(p: &PortSpec) -> String {
    name(p.label, p.name)
}

/// A port's signal type, and "stereo" when it handles both sides itself.
fn signal(p: &PortSpec) -> String {
    match p.channels {
        Channels::Stereo => format!("{}, stereo", p.signal.name()),
        _ => p.signal.name().to_string(),
    }
}

/// One panel widget, in words.
fn widget(d: &ModuleDescriptor, w: &Widget) -> String {
    let param = |bind: &str| {
        d.params
            .iter()
            .find(|p| p.name == bind)
            .map_or_else(|| format!("`{bind}`"), |p| name(p.label, p.name))
    };
    match w.kind {
        WidgetKind::Knob => format!("Knob: {}", param(w.bind)),
        WidgetKind::Toggle => format!("Toggle: {}", param(w.bind)),
        WidgetKind::Radio => {
            let choices = d
                .params
                .iter()
                .find(|p| p.name == w.bind)
                .map_or(&[][..], |p| p.options.choices);
            format!("Radio buttons: {} ({})", param(w.bind), cell(&choices.join(", ")))
        }
        WidgetKind::Select => format!("Select: {}", param(w.bind)),
        WidgetKind::Segmented { .. } => format!("Segmented control: {}", param(w.bind)),
        WidgetKind::Button { mode: "", hold } => format!("Button: {} (momentary)", param(hold)),
        WidgetKind::Button { mode, hold } => format!(
            "Button: {} (toggle), {} (momentary), chosen by {}",
            param(w.bind),
            param(hold),
            param(mode)
        ),
        WidgetKind::Light => {
            let light = d.displays.iter().find(|v| v.name == w.bind).map_or_else(
                || format!("`{}`", w.bind),
                |v| format!("{}: {}", name(v.label, v.name), cell(v.description)),
            );
            format!("Light: {light}")
        }
        WidgetKind::Meter => {
            let output = d
                .outputs
                .iter()
                .find(|p| p.name == w.bind)
                .map_or_else(|| format!("`{}`", w.bind), port_name);
            format!("Level meter: {output}")
        }
        WidgetKind::Scope => {
            let output = d
                .outputs
                .iter()
                .find(|p| p.name == w.bind)
                .map_or_else(|| format!("`{}`", w.bind), port_name);
            format!("Scope: {output}")
        }
        WidgetKind::Shape { label, .. } => format!("Shape preview: {} (read-only)", name(label, w.bind)),
        _ => format!("Widget: `{}`", w.bind),
    }
}

/// Text made safe for a Markdown table cell.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests;

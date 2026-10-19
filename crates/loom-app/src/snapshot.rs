//! What the user sees, as plain data: the tiles, the inspector, the status
//! bar, the undo and redo steps, and the window title.
//!
//! Tests read it with [`App::ui_snapshot`], and `loom-app --dump-ui` prints
//! it as JSON, so a person or an agent can check a patch without a window.
//! It is built from the same scene, panel values, and inspector model the
//! window draws, so it cannot drift from the view.

use crate::inspector::Inspector;
use crate::{App, Args};
use loom_canvas::{ModulationMark, WidgetViewKind};
use serde::Serialize;

/// Everything the snapshot holds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UiSnapshot {
    /// The window title.
    pub title: String,
    /// The status bar message.
    pub status: String,
    /// The step Undo would take back, by name.
    pub undo: Option<String>,
    /// The step Redo would replay, by name.
    pub redo: Option<String>,
    /// The selected module IDs.
    pub selection: Vec<u32>,
    /// One per module, in scene order.
    pub tiles: Vec<Tile>,
    pub inspector: Inspector,
}

/// One module tile on the canvas.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tile {
    pub id: u32,
    pub title: String,
    /// One per output: its label and how many assignments it drives.
    pub outputs: Vec<Output>,
    /// The panel's widgets, in focus order.
    pub widgets: Vec<Widget>,
}

/// An output port on a tile.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Output {
    pub label: String,
    /// The badge: assignments it drives, counting those that are off.
    pub badge: u32,
}

/// One panel widget and its value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Widget {
    pub label: String,
    /// knob, toggle, radio, button, light, or meter.
    pub kind: &'static str,
    /// Control position, light brightness, or meter RMS level.
    pub position: f32,
    /// A meter's peak level. 0 for other widgets.
    pub peak: f32,
    /// The value as text, for controls.
    pub text: String,
    pub modulation: Mark,
    /// A knob's live modulated value, as a position.
    pub live: Option<f32>,
}

/// What a control shows of its modulation assignments.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    None,
    Range { low: f32, high: f32 },
    Dot,
    Ring,
}

impl From<ModulationMark> for Mark {
    fn from(mark: ModulationMark) -> Self {
        match mark {
            ModulationMark::None => Self::None,
            ModulationMark::Range { low, high } => Self::Range { low, high },
            ModulationMark::Dot => Self::Dot,
            ModulationMark::Ring => Self::Ring,
        }
    }
}

fn kind_name(kind: &WidgetViewKind) -> &'static str {
    match kind {
        WidgetViewKind::Knob { .. } => "knob",
        WidgetViewKind::Toggle => "toggle",
        WidgetViewKind::Radio { .. } => "radio",
        WidgetViewKind::Light => "light",
        WidgetViewKind::Meter => "meter",
        WidgetViewKind::Button => "button",
    }
}

impl App {
    /// What the user sees now.
    pub(crate) fn ui_snapshot(&self) -> UiSnapshot {
        let tiles = self
            .scene
            .modules()
            .iter()
            .map(|m| Tile {
                id: m.id,
                title: m.title.clone(),
                outputs: m
                    .outputs
                    .iter()
                    .map(|p| Output {
                        label: p.label.clone(),
                        badge: p.badge,
                    })
                    .collect(),
                widgets: m
                    .panel
                    .widgets
                    .iter()
                    .enumerate()
                    .map(|(index, w)| {
                        let value = self.values.get(m.id, index).cloned().unwrap_or_default();
                        Widget {
                            label: w.label.clone(),
                            kind: kind_name(&w.kind),
                            position: value.position,
                            peak: value.peak,
                            text: value.text,
                            modulation: value.modulation.into(),
                            live: value.live,
                        }
                    })
                    .collect(),
            })
            .collect();
        UiSnapshot {
            title: self.title(),
            status: self.status.clone(),
            undo: self.history.undo_step().map(|s| s.name().to_string()),
            redo: self.history.redo_step().map(|s| s.name().to_string()),
            selection: self.selection.iter().copied().collect(),
            tiles,
            inspector: self.inspector_model(),
        }
    }
}

/// `loom-app --dump-ui`: loads the patch as the window does, without audio
/// or MIDI, selects the modules `--select` names, and returns the snapshot
/// as JSON.
pub fn run(args: &Args) -> Result<String, String> {
    let config = loom_core::EngineConfig {
        output_safety: args.output_safety,
        ..loom_core::EngineConfig::with_sample_rate(48_000.0)
    };
    let session = App::session(config, args.source())?;
    let mut app = App::assemble(Err("dump".to_string()), 48_000.0, session, 1, args, false);
    for &id in &args.select {
        if app.controller.graph().node(loom_core::ModuleId(id)).is_none() {
            return Err(format!("no module {id} in the patch"));
        }
        app.selection.insert(id);
    }
    serde_json::to_string_pretty(&app.ui_snapshot()).map_err(|e| e.to_string())
}

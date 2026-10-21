//! What the user sees, as plain data: the tiles, the inspector, the status
//! bar, the undo and redo steps, and the window title.
//!
//! Tests read it with [`App::ui_snapshot`], and `loom-app --dump-ui` prints
//! it as JSON, so a person or an agent can check a patch without a window.
//! It is built from the same scene, panel values, and inspector model the
//! window draws, so it cannot drift from the view.
//!
//! Places and sizes are in world units, the canvas's own coordinates, so a
//! script can check layout rules (a badge covers no port) without a
//! screenshot, and aim `--pointer` or `--center` without working them out.

use crate::inspector::Inspector;
use crate::live::Levels;
use crate::{App, Args};
use loom_canvas::{ModulationMark, PORT_HIT_RADIUS, PortRef, WidgetViewKind};
use serde::Serialize;

/// How much of a `--frames` run the meters show: the last 100 ms at
/// 48 kHz.
const METER_WINDOW: usize = 4_800;

/// Frames between reads of the meter queue. Four blocks of 128 frames, so a
/// patch with up to 256 meters never fills the queue.
const METER_READ: usize = 512;

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
    /// How far from a port's centre a click still hits it, in world units.
    /// The canvas widens it when zoomed out below 7 / 9 (about 0.78).
    pub port_hit_radius: f32,
    /// One per module, in scene order.
    pub tiles: Vec<Tile>,
    /// One per cable the canvas draws.
    pub cables: Vec<Cable>,
    pub inspector: Inspector,
}

/// A point in world units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl From<iced::Point> for Point {
    fn from(p: iced::Point) -> Self {
        Self { x: p.x, y: p.y }
    }
}

/// A rectangle in world units, from its top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl From<iced::Rectangle> for Rect {
    fn from(r: iced::Rectangle) -> Self {
        Self {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        }
    }
}

/// One module tile on the canvas.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tile {
    pub id: u32,
    pub title: String,
    pub bounds: Rect,
    /// One per input, from the top.
    pub inputs: Vec<Input>,
    /// One per output: its label and how many assignments it drives.
    pub outputs: Vec<Output>,
    /// The panel's widgets, in focus order.
    pub widgets: Vec<Widget>,
}

/// An input port on a tile.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Input {
    pub label: String,
    pub centre: Point,
}

/// An output port on a tile.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Output {
    pub label: String,
    pub centre: Point,
    /// The badge: assignments it drives, counting those that are off.
    pub badge: u32,
    /// Where the badge is drawn, or `None` when it shows none.
    pub badge_bounds: Option<Rect>,
}

/// A cable, by its two ends.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cable {
    pub from: CableEnd,
    pub to: CableEnd,
}

/// One end of a cable: an output for `from`, an input for `to`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CableEnd {
    pub module: u32,
    /// The port's index on its side of the tile, from the top.
    pub index: usize,
    /// The port's centre.
    pub point: Point,
}

/// One panel widget and its value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Widget {
    pub label: String,
    /// knob, toggle, radio, button, light, or meter.
    pub kind: &'static str,
    pub bounds: Rect,
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
    /// Runs the idle engine for `frames` frames, then takes the live values,
    /// lights, and meter levels it left, through the same messages the
    /// window uses. With no window to say what is on screen, every module
    /// counts as shown.
    fn run_engine(&mut self, frames: usize) {
        self.visible_displays = self.controller.graph().nodes().map(|(id, _)| id.0).collect();
        // Meters need their probes before the engine runs.
        self.sync_probes();
        let window = frames.min(METER_WINDOW);
        self.process(frames - window);
        // The meters show the end of the run: earlier readings are dropped.
        let _ = self.monitor.take_readings();
        let mut readings = Vec::new();
        let mut remaining = window;
        while remaining > 0 {
            let n = remaining.min(METER_READ);
            self.process(n);
            readings.extend(self.monitor.take_readings());
            remaining -= n;
        }
        // One update, so the levels do not fall with the time the run took.
        let mut levels = Levels::default();
        levels.update(&readings, std::time::Instant::now());
        let _ = self.update(crate::Message::Meters(levels.all().collect()));
        let _ = self.update(crate::Message::LiveFrame);
        if let Some(watch) = self.watched_lights() {
            let _ = self.update(crate::Message::Lights(watch.read()));
        }
    }

    /// What the user sees now.
    pub(crate) fn ui_snapshot(&self) -> UiSnapshot {
        let tiles = self
            .scene
            .modules()
            .iter()
            .map(|m| Tile {
                id: m.id,
                title: m.title.clone(),
                bounds: m.bounds().into(),
                inputs: m
                    .inputs
                    .iter()
                    .enumerate()
                    .map(|(index, p)| Input {
                        label: p.label.clone(),
                        centre: m.input_position(index).into(),
                    })
                    .collect(),
                outputs: m
                    .outputs
                    .iter()
                    .enumerate()
                    .map(|(index, p)| Output {
                        label: p.label.clone(),
                        centre: m.output_position(index).into(),
                        badge: p.badge,
                        badge_bounds: m.badge_bounds(index).map(Rect::from),
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
                            bounds: m.widget_bounds(index).expect("a widget of this panel").into(),
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
        // The canvas skips cables to modules or ports it does not have.
        let end = |module: u32, output: bool, index: usize| {
            let point = self.scene.port_position(PortRef { module, output, index })?;
            Some(CableEnd {
                module,
                index,
                point: point.into(),
            })
        };
        let cables = self
            .scene
            .cables()
            .iter()
            .filter_map(|c| {
                Some(Cable {
                    from: end(c.from.0, true, c.from.1)?,
                    to: end(c.to.0, false, c.to.1)?,
                })
            })
            .collect();
        UiSnapshot {
            title: self.title(),
            status: self.status.clone(),
            undo: self.history.undo_step().map(|s| s.name().to_string()),
            redo: self.history.redo_step().map(|s| s.name().to_string()),
            selection: self.selection.iter().copied().collect(),
            port_hit_radius: PORT_HIT_RADIUS,
            tiles,
            cables,
            inspector: self.inspector_model(),
        }
    }
}

/// The app as the window builds it, without audio or MIDI, with the
/// modules `--select` names selected and the engine run for `--frames`
/// frames. For `--dump-ui` and `--screenshot-window`.
pub fn headless(args: &Args) -> Result<App, String> {
    let config = loom_core::EngineConfig {
        output_safety: args.output_safety,
        ..loom_core::EngineConfig::with_sample_rate(48_000.0)
    };
    let session = App::session(config, args.source())?;
    let mut app = App::assemble(Err("headless".to_string()), 48_000.0, session, 1, args, false);
    for &id in &args.select {
        if app.controller.graph().node(loom_core::ModuleId(id)).is_none() {
            return Err(format!("no module {id} in the patch"));
        }
        app.selection.insert(id);
    }
    if let Some(frames) = args.frames {
        app.run_engine(frames);
    }
    Ok(app)
}

/// `loom-app --dump-ui`: returns the snapshot of the [`headless`] app as
/// JSON.
pub fn run(args: &Args) -> Result<String, String> {
    serde_json::to_string_pretty(&headless(args)?.ui_snapshot()).map_err(|e| e.to_string())
}

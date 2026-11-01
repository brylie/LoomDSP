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
use crate::matrix::MatrixModel;
use crate::{App, Args};
use loom_canvas::{ModulationMark, ModuleView, PORT_HIT_RADIUS, PortRef, TextKind, WidgetViewKind};
use serde::Serialize;

/// How much of a `--frames` run the meters show: the last 100 ms at
/// 48 kHz.
const METER_WINDOW: usize = 4_800;

/// How much of a `--frames` run the scopes keep: the last second at
/// 48 kHz, the history a trace looks back in for a trigger.
const SCOPE_WINDOW: usize = 48_000;

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
    /// The open module help view, if any.
    pub help: Option<crate::help::Model>,
    /// The active right-hand tab and visible modulation matrix rows.
    pub matrix: MatrixModel,
    /// The stats bar's audio numbers.
    pub stats: Stats,
    /// The pointer and what it is on, as the canvas last drew it. `None`
    /// with no window (`--dump-ui`) or without the control channel.
    pub pointer: Option<crate::pointer::PointerState>,
}

/// The stats bar's audio numbers, as the bar shows them (#234). Values
/// that change every frame, such as the frame rate, are left out, so two
/// snapshots of an idle app are equal.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Stats {
    /// The audio label: the device and its rate, such as
    /// `no device (clock) at 48000 Hz`, or why nothing plays.
    pub label: String,
    /// True when a stream plays. Only then does the bar show the load and
    /// the dropouts.
    pub playing: bool,
    /// The latest callback's load, as a share of its time budget (1.0 is
    /// 100%). 0 when nothing plays.
    pub load: f64,
    /// The highest tick peak in the last ten seconds.
    pub max_load: f64,
    /// Callbacks that ran late or failed since the stream started.
    pub dropouts: u64,
    /// Meter and scope readings the engine had to drop.
    pub telemetry_dropped: u64,
    /// Recent input underrun and overrun corrections.
    pub input_alert: Option<(u64, u64)>,
}

impl Stats {
    /// The load as the bar shows it: `load 3% (max 12%)`.
    pub fn load_text(&self) -> String {
        load_text(self.load, self.max_load)
    }
}

/// The load as the bar shows it, from the latest load and the highest, as
/// shares of the time budget: `load 3% (max 12%)`.
pub fn load_text(load: f64, max_load: f64) -> String {
    format!("load {:.0}% (max {:.0}%)", load * 100.0, max_load * 100.0)
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
    /// The label as drawn, when the tile cuts it with "…" to fit, as
    /// `Reson…` for `Resonance`. Left out when the whole label shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown: Option<String>,
    pub centre: Point,
    /// The transport signal the input follows while it has no cable, which
    /// shows the clock mark: `bar_phase`, `beat_phase`, or `playing`. Left
    /// out when it shows none (panels.md, Clock mark on transport inputs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follows_transport: Option<&'static str>,
    /// True while the output feeding it carries a signal (panels.md,
    /// Signal activity on ports).
    pub carrying: bool,
}

/// An output port on a tile.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Output {
    pub label: String,
    /// The label as drawn, when the tile cuts it with "…" to fit, as
    /// `Reson…` for `Resonance`. Left out when the whole label shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown: Option<String>,
    pub centre: Point,
    /// True while it carries a signal above -80 dB (panels.md, Signal
    /// activity on ports).
    pub carrying: bool,
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
    /// The caption.
    pub label: String,
    /// The label as drawn, when the tile cuts it with "…" to fit, as
    /// `Reson…` for `Resonance`. Left out when the whole label shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown: Option<String>,
    /// knob, toggle, radio, segmented, select, button, light, meter, or
    /// scope.
    pub kind: &'static str,
    /// Where the widget is drawn. Left out while it is hidden.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Rect>,
    /// True while the module's mode does not use this control (ADR-0030).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// Control position, light brightness, or meter RMS level.
    pub position: f32,
    /// A meter's peak level. 0 for other widgets.
    pub peak: f32,
    /// RMS and peak for a stereo meter's right channel.
    pub right: Option<(f32, f32)>,
    /// The value as text, for controls.
    pub text: String,
    pub modulation: Mark,
    /// A knob's live modulated value, as a position.
    pub live: Option<f32>,
    /// A scope's trace: how many points it draws, and the lowest and
    /// highest, from -1 at the bottom of the frame to 1 at the top. Left
    /// out for other widgets, and for a scope with no trace yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<TraceShown>,
}

/// What a scope's trace shows, for the snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct TraceShown {
    /// Points drawn. 0 further out, where only the level shows.
    pub points: usize,
    /// The lowest point, from -1 to 1.
    pub low: f32,
    /// The highest point, from -1 to 1.
    pub high: f32,
    /// The signal's level, from 0 to 1 (-60 dB to 0 dB).
    pub level: f32,
}

impl From<&loom_canvas::Trace> for TraceShown {
    fn from(trace: &loom_canvas::Trace) -> Self {
        let ys = trace.points.iter().map(|p| p[1]);
        let (low, high) = ys.fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), y| (lo.min(y), hi.max(y)));
        let empty = trace.points.is_empty();
        Self {
            points: trace.points.len(),
            low: if empty { 0.0 } else { low },
            high: if empty { 0.0 } else { high },
            level: trace.level,
        }
    }
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

/// Labels a tile cuts, by what they label.
struct Cut(Vec<(TextKind, String)>);

impl Cut {
    /// The label as drawn, if it is cut.
    fn get(&self, kind: &TextKind) -> Option<&String> {
        self.0.iter().find(|(k, _)| k == kind).map(|(_, shown)| shown)
    }
}

/// The port labels and captions a tile cuts with "…", as drawn: the
/// canvas's own text layout decides, so the snapshot cannot disagree with
/// the tile.
fn cut_labels(module: &ModuleView) -> Cut {
    let mut cut = Cut(Vec::new());
    loom_canvas::for_each_tile_text(module, |item| {
        let label = match item.kind {
            TextKind::Port(port) if port.output => module.outputs.get(port.index).map(|p| &p.label),
            TextKind::Port(port) => module.inputs.get(port.index).map(|p| &p.label),
            TextKind::Caption(index) => module.panel.widgets.get(index).map(|w| &w.label),
            TextKind::Title | TextKind::Unknown | TextKind::Option(..) => None,
        };
        if label.is_some_and(|label| *label != item.content) {
            cut.0.push((item.kind, item.content.into_owned()));
        }
    });
    cut
}

fn kind_name(kind: &WidgetViewKind) -> &'static str {
    match kind {
        WidgetViewKind::Knob { .. } => "knob",
        WidgetViewKind::Toggle => "toggle",
        WidgetViewKind::Radio { .. } => "radio",
        WidgetViewKind::Select { .. } => "select",
        WidgetViewKind::Segmented { .. } => "segmented",
        WidgetViewKind::Light => "light",
        WidgetViewKind::Meter => "meter",
        WidgetViewKind::Scope => "scope",
        WidgetViewKind::Shape { .. } => "shape",
        WidgetViewKind::Button => "button",
    }
}

impl App {
    /// True when the output feeding this input carries a signal: an input
    /// shows its source's activity.
    fn input_carrying(&self, module: u32, index: usize) -> bool {
        self.controller.graph().cables().any(|(from, to)| {
            to.module == loom_core::ModuleId(module) && to.port == index && self.carrying.contains(&from)
        })
    }

    /// Runs the idle engine for `frames` frames, then takes the live values,
    /// lights, meter levels, and scope traces it left, through the same
    /// messages the window uses. With no window to say what is on screen,
    /// every module counts as shown.
    pub(crate) fn run_engine(&mut self, frames: usize) {
        self.visible_displays = self.controller.graph().nodes().map(|(id, _)| id.0).collect();
        // Meters and scopes need their probes before the engine runs.
        self.sync_probes();
        let window = frames.min(SCOPE_WINDOW);
        self.process(frames - window);
        // The meters show the end of the run, and the scopes its last
        // second: earlier telemetry is dropped.
        let _ = self.monitor.take_telemetry();
        let watch = self.monitor.watched();
        let rate = self.monitor.sample_rate();
        let mut scopes: std::collections::HashMap<loom_core::ProbeId, crate::live::ScopeTrace> = watch
            .scopes
            .keys()
            .map(|&probe| (probe, crate::live::ScopeTrace::new(rate)))
            .collect();
        // The queues are read after every block, so they never fill.
        let block = self.controller.config().max_block;
        let mut readings = Vec::new();
        let mut remaining = window;
        while remaining > 0 {
            let n = remaining.min(block);
            self.process(n);
            let (meters, chunks) = self.monitor.take_telemetry();
            if remaining <= METER_WINDOW {
                readings.extend(meters);
            }
            for chunk in &chunks {
                if let Some(scope) = scopes.get_mut(&chunk.probe) {
                    scope.push(chunk);
                }
            }
            remaining -= n;
        }
        let traces = scopes
            .into_iter()
            .map(|(probe, mut scope)| {
                let want = watch.scopes[&probe];
                scope.update(want.window, want.trigger);
                (probe, scope.canvas_trace(watch.full_traces))
            })
            .collect();
        // One update, so the levels do not fall with the time the run took.
        let mut levels = Levels::default();
        levels.update(&readings, std::time::Instant::now());
        let _ = self.update(crate::Message::Meters(levels.all().collect()));
        // An output carries a signal if it peaked above -80 dB at the end
        // of the run.
        let activity = watch
            .activity
            .iter()
            .map(|&probe| {
                let peak = readings.iter().filter(|r| r.probe == probe).fold(0.0f32, |m, r| {
                    m.max(r.peak.abs()).max(r.right.map_or(0.0, |(peak, _)| peak.abs()))
                });
                (probe, peak > crate::monitor::ACTIVITY_THRESHOLD)
            })
            .collect();
        let _ = self.update(crate::Message::Telemetry(crate::monitor::Update {
            levels: Vec::new(),
            traces,
            activity,
        }));
        let _ = self.update(crate::Message::LiveFrame);
        if let Some(watch) = self.watched_lights() {
            let _ = self.update(crate::Message::Lights(watch.read()));
        }
    }

    /// The stats bar's audio numbers. `App::stats_bar` draws them and the
    /// snapshot holds them.
    pub(crate) fn stats_model(&self) -> Stats {
        let engine = self.controller.stats();
        let telemetry_dropped = engine.dropped_meters.load(std::sync::atomic::Ordering::Relaxed)
            + engine.dropped_scope.load(std::sync::atomic::Ordering::Relaxed);
        let (playing, load, max_load) = match self.stream.stats() {
            Some(stats) => {
                let s = stats.snapshot();
                (true, s.last_load, self.stream.load().1)
            }
            None => (false, 0.0, 0.0),
        };
        Stats {
            label: self.stream.label().to_string(),
            playing,
            load,
            max_load,
            dropouts: self.dropouts.count(),
            telemetry_dropped,
            input_alert: self.input_alert,
        }
    }

    /// What the user sees now.
    pub(crate) fn ui_snapshot(&self) -> UiSnapshot {
        let tiles = self
            .scene
            .modules()
            .iter()
            .map(|m| {
                let cut = cut_labels(m);
                Tile {
                    id: m.id,
                    title: m.title.clone(),
                    bounds: m.bounds().into(),
                    inputs: m
                        .inputs
                        .iter()
                        .enumerate()
                        .map(|(index, p)| Input {
                            label: p.label.clone(),
                            shown: cut
                                .get(&TextKind::Port(PortRef {
                                    module: m.id,
                                    output: false,
                                    index,
                                }))
                                .cloned(),
                            centre: m.input_position(index).into(),
                            follows_transport: p.follows_transport.map(|s| s.name()),
                            carrying: self.input_carrying(m.id, index),
                        })
                        .collect(),
                    outputs: m
                        .outputs
                        .iter()
                        .enumerate()
                        .map(|(index, p)| Output {
                            label: p.label.clone(),
                            shown: cut
                                .get(&TextKind::Port(PortRef {
                                    module: m.id,
                                    output: true,
                                    index,
                                }))
                                .cloned(),
                            centre: m.output_position(index).into(),
                            carrying: self.carrying.contains(&loom_core::OutputRef {
                                module: loom_core::ModuleId(m.id),
                                port: index,
                            }),
                            badge: p.badge,
                            badge_bounds: self.scene.badge_bounds(m.id, index).map(Rect::from),
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
                                shown: cut.get(&TextKind::Caption(index)).cloned(),
                                kind: kind_name(&w.kind),
                                bounds: m.widget_bounds(index).map(Rect::from),
                                hidden: w.hidden,
                                position: value.position,
                                peak: value.peak,
                                right: value.right,
                                text: value.text,
                                modulation: value.modulation.into(),
                                live: value.live,
                                trace: self.traces.get(m.id, index).map(TraceShown::from),
                            }
                        })
                        .collect(),
                }
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
            undo: self.edits.history.undo_step().map(|s| s.name().to_string()),
            redo: self.edits.history.redo_step().map(|s| s.name().to_string()),
            selection: self.selection.iter().copied().collect(),
            port_hit_radius: PORT_HIT_RADIUS,
            tiles,
            cables,
            inspector: self.inspector_model(),
            help: self.help.as_ref().map(|help| help.model.clone()),
            matrix: self.matrix.model(self.controller.graph(), &self.selection),
            stats: self.stats_model(),
            pointer: self.pointer_state(),
        }
    }
}

/// The app as the window builds it, without audio or MIDI, with the
/// modules `--select` names selected and the engine run for `--frames`
/// frames. For `--dump-ui` and `--screenshot-window`.
pub fn headless(args: &Args) -> Result<App, String> {
    let config = loom_core::EngineConfig {
        output_safety: args.output_safety.unwrap_or_default(),
        ..loom_core::EngineConfig::with_sample_rate(48_000.0)
    };
    let session = App::session(config, args.test_modules, args.source())?;
    let mut app = App::assemble(
        Err("headless".to_string()),
        48_000.0,
        session,
        1,
        args,
        crate::settings::Rig::empty(),
        false,
    );
    for &id in &args.select {
        if app.controller.graph().node(loom_core::ModuleId(id)).is_none() {
            return Err(format!("no module {id} in the patch"));
        }
        app.selection.insert(id);
    }
    if args.show_help {
        if app.selection.len() != 1 {
            return Err("--show-help needs exactly one selected module".to_string());
        }
        app.handle(crate::Message::ShowHelp);
    }
    if let Some(frames) = args.frames {
        if args.hold_note
            && let Some(midi) = &app.rig.midi_in
        {
            loom_host_desktop::send_midi(midi, loom_core::MidiEvent::note_on(0, 0, 57, 100));
        }
        app.run_engine(frames);
    }
    Ok(app)
}

/// `loom-app --dump-ui`: returns the snapshot of the [`headless`] app as
/// JSON.
pub fn run(args: &Args) -> Result<String, String> {
    serde_json::to_string_pretty(&headless(args)?.ui_snapshot()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;

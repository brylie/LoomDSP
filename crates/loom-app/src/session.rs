//! The patch that is open, and everything that lives and dies with it.
//!
//! New and Open replace the [`PatchSession`] whole and leave the rest of `App`
//! alone. So a field that belongs to one patch goes here, and a field that
//! outlives it stays on `App`, and neither needs copying by hand.
//!
//! This is a holder, not a part with rules of its own: its fields are visible
//! to the crate, so `App`'s methods can borrow two of them at once. The rules
//! are in the parts it holds, such as `Learn`, `Gestures`, and `Stream`
//! (ADR-0034).

use crate::live::Live;
use crate::{
    Loaded, clip, controls, devices, document, edits, gestures, learn, lights, matrix, monitor, patch_info,
    run_is_driven, scene, settings, stream,
};
use loom_canvas::{Camera, PanelValues, Scene};
use loom_core::{Controller, ModuleId, OutputRef, OutputSafety, ProbeId};
use loom_host_desktop::{AudioOutput, CallbackStats, SharedMidi};
use loom_patch::{Hierarchy, Metadata, ModulePath};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Each placeholder's position in its file, if it had one, and the position it
/// was first shown at. While it stays there, saving writes the original.
pub(crate) type UnknownPositions = BTreeMap<u32, (Option<(f32, f32)>, (f32, f32))>;

/// The open patch: its graph and layout, its edits, what the canvas shows of
/// it, and the engine that plays it.
pub(crate) struct PatchSession {
    /// Which patch this is: a number no other session of this run has. New,
    /// Open, and a restore make a new session, and with it a new number; a
    /// new engine for the same patch (a device change) keeps it. What waits
    /// on a dialog keeps it, so its answer goes to the patch it was asked
    /// about (`dialogs::Pending`).
    pub(crate) id: u64,
    pub(crate) controller: Controller,
    /// The sub-patch hierarchy beside the flat graph (ADR-0039): where
    /// each module sits, the definitions, the instance and port tiles, and
    /// the cables through ports. Every tile id comes from its table.
    pub(crate) hierarchy: Hierarchy,
    /// The level the canvas shows: the top, or the inside of a sub-patch.
    pub(crate) level: ModulePath,
    /// Each level's camera, kept for the session (sub-patches.md, Make,
    /// open, and dissolve). A level not here is framed when first shown.
    pub(crate) cameras: BTreeMap<ModulePath, Camera>,
    /// Changes when the shown level does, so the canvas adopts that
    /// level's camera.
    pub(crate) level_key: u64,
    /// A full command queue left fresh modules staged for a later commit.
    pub(crate) retry_commit: bool,
    pub(crate) positions: BTreeMap<u32, (f32, f32)>,
    /// Original placeholder positions and their initial displayed positions.
    pub(crate) unknown_positions: UnknownPositions,
    /// Saved tile dimensions in grid units, keyed by module ID.
    pub(crate) sizes: BTreeMap<u32, (u8, u8)>,
    pub(crate) selection: BTreeSet<u32>,
    /// The patch's file, title, and saved state.
    pub(crate) document: document::Document,
    /// Patch info's fields being typed in (`docs/design/patch-metadata.md`).
    pub(crate) patch_info: patch_info::State,
    /// Undo and redo (`docs/design/undo.md`): the history, and the edit
    /// or gesture being recorded.
    pub(crate) edits: edits::Edits,
    /// The name for the step the edit in progress makes, when a command
    /// names it (cut, paste, duplicate) rather than what changed.
    pub(crate) step_name: Option<String>,
    /// What each linked sub-patch would save as, taken when the edit in
    /// progress began, by definition key: an edit that changes one unlinks
    /// it when the step is recorded (sub-patches.md, Copies and editing).
    /// `None` outside an edit, and empty while nothing is linked.
    pub(crate) linked_before: Option<BTreeMap<String, String>>,
    /// Modulation gestures in progress: the Add modulation list, an output
    /// dragged onto the inspector, an arc drag, a placeholder label.
    pub(crate) gestures: gestures::Gestures,
    /// Text typed in an inspector value field, not committed yet.
    pub(crate) value_draft: Option<controls::ValueDraft>,
    /// MIDI learn mode, the armed parameter, and what the canvas shows.
    pub(crate) learn: learn::Learn,
    /// The matrix's filters, selection, and pending edit, for this patch.
    pub(crate) matrix: matrix::State,
    /// True when the last commit had a cable on the Transport's `run`
    /// input. When the cable goes, the Run toggle takes the state the input
    /// left (clock.md, Starting and stopping).
    pub(crate) run_driven: bool,
    pub(crate) scene: Scene,
    /// The mode values the scene was built with (ADR-0030). A change
    /// rebuilds the scene, so hidden controls follow the mode.
    pub(crate) mode_key: Vec<(u32, usize, i64)>,
    /// Live values of panel controls and lights, apart from the scene so a
    /// knob turn does not rebuild tiles or cables.
    pub(crate) values: PanelValues,
    /// Last brightness of each light, by module and widget.
    pub(crate) lights: BTreeMap<lights::Light, f32>,
    /// Latest level of each meter probe.
    pub(crate) meter_levels: std::collections::HashMap<ProbeId, crate::live::Level>,
    /// Each meter's clip light and peak hold. By widget, not by probe, because
    /// a probe goes away with the meter on screen (`clip.rs`).
    pub(crate) clip_holds: clip::ClipHolds,
    /// The traces of the scopes on screen (`docs/design/panels.md`, Drawing
    /// the scope trace).
    pub(crate) traces: loom_canvas::ScopeTraces,
    /// Outputs carrying a signal (`docs/design/panels.md`, Signal activity
    /// on ports).
    pub(crate) carrying: BTreeSet<OutputRef>,
    /// True while the canvas draws full scope traces (close zoom), false
    /// when it draws only their level.
    pub(crate) full_traces: bool,
    /// Modules whose lights or meters are on screen, as the canvas reports.
    pub(crate) visible_displays: Vec<u32>,
    /// The engine that plays this patch, and what reads from it.
    pub(crate) engine: EngineLink,
}

/// What is tied to one engine. A new patch gets a new engine, so these go with
/// it.
pub(crate) struct EngineLink {
    /// The output stream or idle engine, its counters, and how this run
    /// plays.
    pub(crate) stream: stream::Stream,
    /// Meter and scope data from the engine, drained on the telemetry
    /// thread so the queues never fill.
    pub(crate) monitor: monitor::Monitor,
    pub(crate) live: Live,
    /// Note input shared by every MIDI port and the "Hold note" button.
    pub(crate) midi: Option<SharedMidi>,
    pub(crate) holding: bool,
    /// The stream and its callback count at the last control `snapshot`,
    /// for its `advanced` field.
    pub(crate) control_seen: Option<(Arc<CallbackStats>, u64)>,
}

/// A session just built, and what `App::settle_session` needs to finish it.
pub(crate) struct Started {
    pub(crate) session: PatchSession,
    /// The rig had no MIDI inputs open yet.
    pub(crate) first_midi: bool,
    /// Why the recording ended, if the new stream refused it.
    pub(crate) recording_stopped: Option<String>,
    /// A new, empty patch, which takes the user's defaults.
    pub(crate) fresh: bool,
}

/// The next session's number.
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl PatchSession {
    /// Attaches audio and MIDI to a loaded patch. The only way to build a
    /// session: each field is named here, so a new one must say how it starts.
    pub(crate) fn start(
        loaded: Loaded,
        output: Result<AudioOutput, String>,
        sample_rate: f32,
        generation: u64,
        playback: stream::Playback,
        output_safety: OutputSafety,
        rig: &mut settings::Rig,
    ) -> Started {
        let Loaded {
            controller,
            hierarchy,
            engine,
            midi: sender,
            telemetry,
            positions,
            unknown_positions,
            sizes,
            info,
            fresh,
            file,
            stamp,
            notes,
        } = loaded;
        let mut recording_stopped = None;

        // Every MIDI input plays the patch, and the "Hold note" button works
        // alongside them, so a keyboard is never hidden behind another port.
        // The inputs are opened once the app is built, on its controller.
        let first_midi = rig.attach(sender);
        let midi = rig.midi_in.clone().expect("Rig::attach always sets midi_in");

        let (audio, midi, audio_name, idle_engine) = match output {
            Ok(mut output) => {
                let label = format!("{} at {} Hz", output.name(), output.sample_rate());
                rig.playing = (!playback.is_fake()).then(|| devices::Playing::of(&output));
                let refused = rig.record_into(&mut output);
                match output.start(engine) {
                    Ok(audio) => {
                        if let Some(e) = refused {
                            recording_stopped = Some(rig.end_recording(&e));
                        }
                        (Some(audio), Some(midi), label, None)
                    }
                    Err(e) => {
                        rig.playing = None;
                        (None, None, format!("no audio ({})", e.error), Some(e.engine))
                    }
                }
            }
            Err(e) => (None, None, format!("no audio ({e})"), Some(engine)),
        };

        let scene = scene::build(&controller, &hierarchy, &ModulePath::root(), &positions, &sizes);
        let mode_key = scene::mode_key(&controller);
        let mut values = PanelValues::new();
        scene::update_panel_values(&mut values, &controller, &hierarchy, &BTreeMap::new());
        // The loaded patch may already have a cable on run, before any
        // commit of the app's own (#413 review).
        let run_driven = run_is_driven(&controller);
        let session = Self {
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            controller,
            hierarchy,
            level: ModulePath::root(),
            cameras: BTreeMap::new(),
            level_key: 0,
            retry_commit: false,
            positions,
            unknown_positions,
            sizes,
            selection: BTreeSet::new(),
            document: document::Document::new(info, file, stamp, notes),
            patch_info: patch_info::State::new(&Metadata::default()),
            edits: edits::Edits::default(),
            step_name: None,
            linked_before: None,
            gestures: gestures::Gestures::default(),
            value_draft: None,
            learn: learn::Learn::default(),
            matrix: matrix::State::default(),
            run_driven,
            scene,
            mode_key,
            values,
            lights: BTreeMap::new(),
            meter_levels: std::collections::HashMap::new(),
            clip_holds: clip::ClipHolds::default(),
            traces: loom_canvas::ScopeTraces::new(),
            carrying: BTreeSet::new(),
            full_traces: true,
            visible_displays: Vec::new(),
            engine: EngineLink {
                stream: stream::Stream::new(audio, idle_engine, audio_name, sample_rate, playback, output_safety),
                monitor: monitor::Monitor::new(telemetry, sample_rate, generation),
                live: {
                    let mut live = Live::showing_activity();
                    live.set_activity(rig.store.saved().show_activity);
                    live
                },
                midi,
                holding: false,
                control_seen: None,
            },
        };
        Started {
            session,
            first_midi,
            recording_stopped,
            fresh,
        }
    }
}

impl PatchSession {
    /// Adds a module of `kind` to the shown level, with its id from the
    /// hierarchy's table: the only way the app gives out a module id
    /// (ADR-0039, invariant 1). The caller places it and commits.
    pub(crate) fn new_module(&mut self, kind: &str) -> Result<ModuleId, String> {
        // A module added straight to the controller (tests, generated
        // patches) is a top-level module the table must know first.
        self.hierarchy.adopt(self.controller.graph());
        let (_, id) = self.hierarchy.new_module(&self.level).map_err(|e| e.to_string())?;
        self.controller
            .add_module_with_id(id, kind)
            .map_err(|e| e.to_string())?;
        Ok(id)
    }
}

//! LoomDSP engine core.
//!
//! The crate is split along the thread boundary described in ADR-0001:
//!
//! - [`Controller`] lives on a control thread (UI, CLI, patch loader). It owns
//!   the editable [`Graph`], compiles it into an execution schedule, and sends
//!   changes to the engine. It may allocate and block.
//! - [`Engine`] lives on the audio thread. It runs the current schedule and
//!   applies changes at block boundaries. It never allocates, frees, locks,
//!   or waits.
//!
//! They talk through bounded single-producer, single-consumer queues.
//!
//! The core has no threads of its own and no audio or file I/O, so it builds
//! for any target, including `wasm32-unknown-unknown` (NFR-PORT-01, 02).

#![warn(missing_docs)]

pub mod audio_input;
mod compile;
mod controller;
mod descriptor;
mod display;
mod engine;
mod error;
mod graph;
mod live;
mod midi;
mod midi_map;
mod module;
mod panic;
mod registry;
pub mod safety;
pub mod signal;
mod telemetry;
pub mod transport;
mod width;

pub use compile::{CompileReport, FeedbackLoop};
pub use controller::{Controller, EngineParts, MidiSender, create_engine};
pub use descriptor::{
    ButtonMode, Channels, DisplayKind, DisplaySpec, Family, MAX_DISPLAYS, MissingText, ModuleConfig, ModuleDescriptor,
    ModuleRole, Panel, ParamMapping, ParamOptions, ParamRange, ParamSpec, Polarity, PortDefault, PortRange, PortSpec,
    SpecKind, Undocumented, Widget, WidgetKind, assert_documented,
};
pub use display::{DisplayReader, DisplaySource, TriggerFlag};
pub use engine::{Engine, EngineConfig, EngineStats, RestartFadeSender};
pub use error::LoomError;
pub use graph::{Assignment, AssignmentId, Graph, InputRef, ModuleId, Node, OutputRef, UnknownModule};
pub use midi::{MAX_MIDI_INPUTS, MidiEvent, MidiInputId, MidiMessage};
pub use midi_map::{ControlChangeSeen, MidiControl, MidiMapping, MidiMappingId, MidiReader, NoteOnSeen, control_value};
pub use module::{AsAny, Module, Param, PortIo, ProcessContext, copy_state, testing};
pub use panic::PanicTrigger;
pub use registry::{ModuleKind, Registry, RegistryNote};
pub use safety::OutputSafety;
pub use signal::SignalType;
pub use telemetry::{MeterReading, ProbeId, ProbeKind, SCOPE_CHUNK, ScopeChunk, Telemetry};
pub use transport::{
    TRANSPORT_GRID, TRANSPORT_MODULE, TimeSignature, TransportInfo, TransportRequest, TransportSettings,
};
pub use width::Width;

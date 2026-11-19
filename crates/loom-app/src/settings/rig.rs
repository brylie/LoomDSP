//! Hardware and persistence shared by every patch in the app.

use crate::devices::{self, Host, Wanted};
use crate::input_session::InputSession;
use crate::preferences::{AudioPrefs, BufferChoice, DeviceChoice, InputChoice, Preferences, RateChoice, Store};
use std::rc::Rc;

mod input;
mod midi;
mod output;
mod recovery;

use midi::MidiRig;
use output::OutputSession;
pub use output::open_output;

/// The library the rig reads linked sub-patches from (`Rig::library`).
pub enum RigLibrary {
    /// The library folder.
    Folder(crate::patch_file::FolderLibrary),
    /// No folder is known.
    None,
}

impl loom_patch::Library for RigLibrary {
    fn read(&self, path: &str) -> Result<loom_patch::Patch, loom_patch::LibraryError> {
        match self {
            Self::Folder(folder) => folder.read(path),
            Self::None => loom_patch::NoLibrary.read(path),
        }
    }
}

/// Devices and preferences that survive replacing a patch or engine.
pub struct Rig {
    /// The device API, or the fake a test supplies.
    pub host: Rc<dyn Host>,
    /// The preferences file and its saved values.
    pub store: Store,
    /// Recent files and the last folder used by a file dialog.
    pub recent: crate::recent::RecentList,
    /// Device flags for this run. Changing a choice clears its flag.
    pub flags: devices::Options,
    /// Audio input connection and recovery.
    pub(crate) input: InputSession,
    /// Audio output settings, recording, and recovery.
    pub(crate) output: OutputSession,
    /// MIDI inputs and clock routing across engines.
    pub(crate) midi: MidiRig,
}

impl Rig {
    /// Starts a rig with the saved choices and overrides for this run.
    pub fn new(host: Rc<dyn Host>, store: Store, flags: devices::Options, safety_flag: bool) -> Self {
        let midi = MidiRig::new(&store.saved().midi.send_clock);
        Self {
            recent: crate::recent::RecentList::in_memory(),
            host,
            store,
            flags,
            input: InputSession::new(),
            output: OutputSession::new(safety_flag),
            midi,
        }
    }

    /// A rig with no file and the real devices, for a headless app and as
    /// a stand-in while a rig moves to a new app.
    pub fn empty() -> Self {
        Self::new(
            Rc::new(devices::System),
            Store::at(None),
            devices::Options::default(),
            false,
        )
    }

    /// Where linked sub-patches are read from: the library folder, or
    /// nothing when none is known, so every linked tile keeps its stored
    /// copy (sub-patches.md, rule 5).
    pub fn library(&self) -> RigLibrary {
        match self.store.saved().library_folder() {
            Some(root) => RigLibrary::Folder(crate::patch_file::FolderLibrary { root }),
            None => RigLibrary::None,
        }
    }

    /// The audio choice the window shows: the saved one, with the flags for
    /// this run on top.
    pub fn shown_audio(&self) -> AudioPrefs {
        let mut audio = self.store.saved().audio.clone();
        if self.flags.no_input {
            audio.input = InputChoice::Off;
        } else if self.flags.input.is_some()
            && let Some(input) = self.input.port()
        {
            audio.input = InputChoice::Named(input.name().to_string());
        }
        if self.flags.device.is_some()
            && let Some(playing) = &self.output.playing
        {
            audio.device = DeviceChoice::Named(playing.device.clone());
        }
        if let Some(rate) = self.flags.sample_rate {
            audio.sample_rate = RateChoice::Hz(rate);
        }
        if let Some(frames) = self.flags.buffer {
            audio.buffer = BufferChoice::Frames(frames);
        }
        audio
    }

    /// What to open now: the saved choice with the flags on top.
    pub fn wanted(&self) -> Wanted {
        Wanted::new(&self.store.saved().audio, &self.flags)
    }

    /// Saves `preferences`. Returns the status bar line when it fails.
    pub(super) fn save(&mut self, preferences: Preferences) -> Option<String> {
        self.store.save(preferences).err()
    }
}

/// The preferences file for a run: `--preferences`, or the per-user file.
/// Tests never read the real file.
pub fn open_store(flags: &devices::Options) -> (Store, Vec<String>) {
    let path = match &flags.preferences {
        Some(path) => Some(path.clone()),
        None if cfg!(test) => None,
        None => crate::preferences::default_path(),
    };
    Store::open(path)
}

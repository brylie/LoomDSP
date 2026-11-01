//! The per-machine preferences file (`docs/design/preferences.md`).
//!
//! The app reads the file once at start and writes it only when the player
//! changes a setting. A problem with the file never stops the app: a file
//! that cannot be read or parsed gives the defaults, and a field with a
//! value the app does not know falls back to its own default alone (rule 3).
//! No engine crate reads this file (rule 2).

use loom_core::{OutputSafety, PanicTrigger};
use ron::value::RawValue;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

/// The file's name in the settings folder.
pub const FILE_NAME: &str = "preferences.ron";

/// The only format version until the first release.
const VERSION: u32 = 1;

/// Every setting the file holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preferences {
    pub output_safety: OutputSafety,
    /// Show signal activity on connected ports (panels.md, Signal activity).
    pub show_activity: bool,
    pub audio: AudioPrefs,
    pub midi: MidiPrefs,
    /// The MIDI message that runs panic (panic.md, MIDI trigger). It
    /// belongs to the rig, so it is saved here and never in a patch.
    pub panic: Option<PanicPref>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            output_safety: OutputSafety::default(),
            show_activity: true,
            audio: AudioPrefs::default(),
            midi: MidiPrefs::default(),
            panic: None,
        }
    }
}

/// The panic trigger as the file writes it: `Cc(channel, controller)` or
/// `Note(channel, note)`, with channels from 0 as in patch files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanicPref {
    Cc(u8, u8),
    Note(u8, u8),
}

impl PanicPref {
    /// The trigger the engine watches for.
    pub fn trigger(self) -> PanicTrigger {
        match self {
            Self::Cc(channel, controller) => PanicTrigger::Cc { channel, controller },
            Self::Note(channel, note) => PanicTrigger::Note { channel, note },
        }
    }
}

/// Audio input and output settings (preferences.md, What is saved).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioPrefs {
    pub device: DeviceChoice,
    /// The input device; independent of the output device.
    pub input: InputChoice,
    pub sample_rate: RateChoice,
    pub buffer: BufferChoice,
}

/// Which input device feeds Audio In modules.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputChoice {
    /// Use the system default input.
    #[default]
    SystemDefault,
    /// Use an input by its full name.
    Named(String),
    /// Do not open an input stream.
    Off,
}

/// Which output device plays the patch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceChoice {
    /// Whatever the operating system is set to, now.
    #[default]
    SystemDefault,
    /// One device, by its full name.
    Named(String),
}

/// The sample rate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RateChoice {
    #[default]
    DeviceDefault,
    Hz(u32),
}

/// The buffer size.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BufferChoice {
    #[default]
    DeviceDefault,
    Frames(u32),
}

impl RateChoice {
    pub fn hz(self) -> Option<u32> {
        match self {
            Self::DeviceDefault => None,
            Self::Hz(hz) => Some(hz),
        }
    }
}

impl BufferChoice {
    pub fn frames(self) -> Option<u32> {
        match self {
            Self::DeviceDefault => None,
            Self::Frames(frames) => Some(frames),
        }
    }
}

/// MIDI inputs. The file lists the inputs that are off, so a controller the
/// app has never seen plays at once (rule D7).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiPrefs {
    pub off: Vec<String>,
}

impl MidiPrefs {
    /// Turns an input on or off.
    pub fn set(&mut self, name: &str, on: bool) {
        self.off.retain(|off| off != name);
        if !on {
            self.off.push(name.to_string());
        }
    }
}

/// What reading the file gave.
#[derive(Debug, Default)]
pub struct Loaded {
    pub preferences: Preferences,
    /// Status bar lines about problems in the file, in file order.
    pub notes: Vec<String>,
    /// The file is there but did not parse. The first write keeps it as a
    /// `.broken.ron` copy.
    pub broken: bool,
}

/// Reads the file at `path`. A missing file gives the defaults and no note.
pub fn load(path: &Path) -> Loaded {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded::default(),
        Err(e) => {
            return Loaded {
                notes: vec![format!(
                    "Could not read preferences \"{}\": {e}. Using defaults.",
                    path.display()
                )],
                ..Loaded::default()
            };
        }
    };
    match parse(&text) {
        Ok((preferences, notes)) => Loaded {
            preferences,
            notes,
            broken: false,
        },
        Err(e) => Loaded {
            notes: vec![format!(
                "Could not read preferences \"{}\": {e}. Using defaults.",
                path.display()
            )],
            broken: true,
            ..Loaded::default()
        },
    }
}

/// Reads the file's text. Only a file that is not RON, or is not a struct
/// of fields, is an error. Field problems become notes.
fn parse(text: &str) -> Result<(Preferences, Vec<String>), String> {
    let fields = ron::from_str::<Fields>(text).map_err(|e| e.to_string())?;
    let mut notes = Vec::new();
    let mut preferences = Preferences::default();
    for (name, raw) in fields.0 {
        match name.as_str() {
            "version" => {
                let _: u32 = field(&mut notes, "version", &raw, &VERSION, |v| *v == VERSION);
            }
            "output_safety" => {
                let name: String = field(
                    &mut notes,
                    "output_safety",
                    &raw,
                    &OutputSafety::default().name().into(),
                    |n: &String| n.parse::<OutputSafety>().is_ok(),
                );
                preferences.output_safety = name.parse().unwrap_or_default();
            }
            "show_activity" => {
                preferences.show_activity = field(&mut notes, "show_activity", &raw, &true, |_| true);
            }
            "audio" => preferences.audio = audio(&mut notes, &raw),
            "panic" => {
                preferences.panic = field(&mut notes, "panic", &raw, &None, |p: &Option<PanicPref>| {
                    p.is_none_or(|p| p.trigger().is_valid())
                });
            }
            "midi" => {
                let off = section(&mut notes, "midi", &raw, |notes, name, raw| {
                    (name == "off").then(|| field::<Vec<String>>(notes, "midi.off", raw, &Vec::new(), |_| true))
                });
                if let Some(off) = off.into_iter().next_back() {
                    preferences.midi.off = off;
                }
            }
            other => notes.push(format!("Preferences: ignored unknown setting \"{other}\".")),
        }
    }
    Ok((preferences, notes))
}

fn audio(notes: &mut Vec<String>, raw: &RawValue) -> AudioPrefs {
    enum Part {
        Device(DeviceChoice),
        Input(InputChoice),
        Rate(RateChoice),
        Buffer(BufferChoice),
    }
    let mut audio = AudioPrefs::default();
    for part in section(notes, "audio", raw, |notes, name, raw| match name {
        "device" => Some(Part::Device(field(
            notes,
            "audio.device",
            raw,
            &DeviceChoice::default(),
            |_| true,
        ))),
        "input" => Some(Part::Input(field(
            notes,
            "audio.input",
            raw,
            &InputChoice::default(),
            |_| true,
        ))),
        "sample_rate" => Some(Part::Rate(field(
            notes,
            "audio.sample_rate",
            raw,
            &RateChoice::default(),
            |r| r.hz().is_none_or(|hz| hz > 0),
        ))),
        "buffer" => Some(Part::Buffer(field(
            notes,
            "audio.buffer",
            raw,
            &BufferChoice::default(),
            |b| b.frames().is_none_or(|frames| frames > 0),
        ))),
        _ => None,
    }) {
        match part {
            Part::Device(device) => audio.device = device,
            Part::Input(input) => audio.input = input,
            Part::Rate(rate) => audio.sample_rate = rate,
            Part::Buffer(buffer) => audio.buffer = buffer,
        }
    }
    audio
}

/// Reads a nested struct of fields. `read` gives a value for each field it
/// knows; the others are noted as unknown. A value that is not a struct
/// falls back to the section's defaults, with a note.
fn section<T>(
    notes: &mut Vec<String>,
    name: &str,
    raw: &RawValue,
    mut read: impl FnMut(&mut Vec<String>, &str, &RawValue) -> Option<T>,
) -> Vec<T> {
    let Ok(fields) = raw.into_rust::<Fields>() else {
        notes.push(format!(
            "Preferences: unknown {name} {}. Using the defaults.",
            raw.get_ron().trim()
        ));
        return Vec::new();
    };
    let mut values = Vec::new();
    for (field, raw) in fields.0 {
        match read(notes, &field, &raw) {
            Some(value) => values.push(value),
            None => notes.push(format!("Preferences: ignored unknown setting \"{name}.{field}\".")),
        }
    }
    values
}

/// Reads one field's value. A value that does not read as `T`, or that
/// `valid` refuses, gives `default` and a note naming the bad value.
fn field<T: for<'de> Deserialize<'de> + Serialize + Clone>(
    notes: &mut Vec<String>,
    name: &str,
    raw: &RawValue,
    default: &T,
    valid: impl Fn(&T) -> bool,
) -> T {
    match raw.into_rust::<T>() {
        Ok(value) if valid(&value) => value,
        _ => {
            notes.push(format!(
                "Preferences: unknown {name} {}. Using {}.",
                raw.get_ron().trim(),
                ron::to_string(default).unwrap_or_default()
            ));
            default.clone()
        }
    }
}

/// The fields of a RON struct, `(name: value, ...)`, in file order, each
/// value kept as text until its field reads it.
struct Fields(Vec<(String, Box<RawValue>)>);

impl<'de> Deserialize<'de> for Fields {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Fields;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a struct of settings, such as (version: 1)")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Fields, A::Error> {
                let mut fields = Vec::new();
                while let Some(entry) = map.next_entry::<String, Box<RawValue>>()? {
                    fields.push(entry);
                }
                Ok(Fields(fields))
            }
        }
        deserializer.deserialize_struct("Preferences", &[], Visitor)
    }
}

/// The file as written: every field, defaults included (Writing the file).
#[derive(Serialize)]
#[serde(rename = "Preferences")]
struct FileOut<'a> {
    version: u32,
    output_safety: &'static str,
    show_activity: bool,
    audio: AudioOut<'a>,
    midi: &'a MidiPrefs,
    panic: Option<PanicPref>,
}

#[derive(Serialize)]
#[serde(rename = "Audio")]
struct AudioOut<'a> {
    device: &'a DeviceChoice,
    input: &'a InputChoice,
    sample_rate: RateChoice,
    buffer: BufferChoice,
}

/// The file's text for `preferences`.
pub fn to_text(preferences: &Preferences) -> String {
    let file = FileOut {
        version: VERSION,
        output_safety: preferences.output_safety.name(),
        show_activity: preferences.show_activity,
        audio: AudioOut {
            device: &preferences.audio.device,
            input: &preferences.audio.input,
            sample_rate: preferences.audio.sample_rate,
            buffer: preferences.audio.buffer,
        },
        midi: &preferences.midi,
        panic: preferences.panic,
    };
    // Line feeds on every system, so the file is the same everywhere. RON
    // uses CRLF on Windows by default.
    let config = ron::ser::PrettyConfig::new()
        .indentor("    ".to_string())
        .new_line("\n".to_string())
        .struct_names(false);
    let mut text = ron::ser::to_string_pretty(&file, config).expect("preferences always serialize");
    text.push('\n');
    text
}

/// The preferences file and what is saved in it.
#[derive(Debug)]
pub struct Store {
    /// `None` when no settings folder was found, and in tests that do not
    /// name a file: nothing is read or written.
    path: Option<PathBuf>,
    saved: Preferences,
    /// The file did not parse at start and has not been kept aside yet.
    broken: bool,
}

impl Store {
    /// Reads the file at `path`. Returns the store and the notes for the
    /// status bar.
    pub fn open(path: Option<PathBuf>) -> (Self, Vec<String>) {
        let loaded = path.as_deref().map(load).unwrap_or_default();
        (
            Self {
                path,
                saved: loaded.preferences,
                broken: loaded.broken,
            },
            loaded.notes,
        )
    }

    /// A store at `path` holding the defaults, without reading anything.
    pub fn at(path: Option<PathBuf>) -> Self {
        Self {
            path,
            saved: Preferences::default(),
            broken: false,
        }
    }

    /// The saved values. When a write fails, they are still the values the
    /// player chose: the change applies until quit.
    pub fn saved(&self) -> &Preferences {
        &self.saved
    }

    /// Saves `preferences`, unless they are what is saved already. On
    /// failure the values are still kept for this run, and the error is the
    /// status bar line.
    pub fn save(&mut self, preferences: Preferences) -> Result<(), String> {
        if preferences == self.saved && !self.broken {
            return Ok(());
        }
        self.saved = preferences;
        let Some(path) = self.path.clone() else {
            return Err(
                "Could not save preferences: no settings folder was found. The change applies until you quit."
                    .to_string(),
            );
        };
        self.write(&path).map_err(|e| {
            format!(
                "Could not save preferences \"{}\": {e}. The change applies until you quit.",
                path.display()
            )
        })
    }

    fn write(&mut self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        if self.broken {
            // The player may have edited the broken file by hand: keep it.
            std::fs::rename(path, broken_path(path))?;
            self.broken = false;
        }
        crate::recovery::write_atomic(path, &to_text(&self.saved))
    }
}

/// Where a file that did not parse is kept: `<name>.broken.ron` next to it.
pub fn broken_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!("{stem}.broken.ron"))
}

/// An operating system family, for [`path_for`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    /// Linux and other Unix.
    Unix,
}

impl Os {
    pub const CURRENT: Self = if cfg!(target_os = "macos") {
        Self::Mac
    } else if cfg!(windows) {
        Self::Windows
    } else {
        Self::Unix
    };
}

/// The per-user preferences file on `os`, from its environment variables
/// (preferences.md, Where the file lives). `None` when the variable the
/// path is built from is unset.
pub fn path_for(os: Os, env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let set = |name: &str| env(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    let dir = match os {
        Os::Mac => set("HOME")?.join("Library").join("Application Support").join("LoomDSP"),
        Os::Windows => set("LOCALAPPDATA")?.join("LoomDSP"),
        Os::Unix => set("XDG_CONFIG_HOME")
            .or_else(|| set("HOME").map(|home| home.join(".config")))?
            .join("loomdsp"),
    };
    Some(dir.join(FILE_NAME))
}

/// The per-user preferences file on this machine.
pub fn default_path() -> Option<PathBuf> {
    path_for(Os::CURRENT, |name| std::env::var(name).ok())
}

#[cfg(test)]
mod tests;

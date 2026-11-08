//! What loaded differently from the file.
//!
//! [`crate::Patch::load_into`] still loads a patch with these, and returns
//! them so the app and the command line can tell the user. Callers match on
//! the variant, never on the text, so the wording can change freely.

use loom_core::ParamRange;
use std::fmt;

/// How serious a [`LoadWarning`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadLevel {
    /// Part of the patch will not play as it was made
    /// (`docs/design/unknown-modules.md`). `loom render` says `warning:`.
    Warning,
    /// The patch plays as made, but something was adjusted. `loom render`
    /// says `note:`.
    Note,
}

/// Why a saved control range was not kept (control-ranges.md, rule 7).
#[derive(Debug, Clone, PartialEq)]
pub enum RangeReason {
    /// The parameter offers the range, but it does not hold the saved value.
    DoesNotHold {
        /// The saved value, clamped to the parameter.
        value: f32,
    },
    /// The parameter does not offer the range.
    NotOffered,
}

/// One thing that loaded differently from the file.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadWarning {
    /// Modules whose kind this build does not have load as silent
    /// placeholders (`docs/design/unknown-modules.md`).
    UnknownModules {
        /// The kind of each unknown module, in file order. A kind used by
        /// two modules is listed twice.
        kinds: Vec<String>,
    },
    /// The patch had no Transport, so one was added (`docs/design/clock.md`).
    TransportAdded {
        /// The new Transport's module ID.
        module: u32,
    },
    /// A saved range on a parameter that offers no ranges is ignored.
    RangeIgnored {
        /// The module's ID in the patch.
        module: u32,
        /// The parameter's name.
        param: String,
    },
    /// A saved range was not kept; the parameter loads with another.
    RangeNotKept {
        /// The module's ID in the patch.
        module: u32,
        /// The parameter's name.
        param: String,
        /// The range in the file.
        saved: ParamRange,
        /// Why it was not kept.
        reason: RangeReason,
        /// The range the parameter loads with.
        loaded: ParamRange,
    },
    /// Two parameters share one range, and the saved ranges do not hold
    /// both values (control-ranges.md, Ranges on a range widget). Said once
    /// for the pair.
    SharedRangeNotKept {
        /// The module's ID in the patch.
        module: u32,
        /// The first parameter of the pair.
        param: String,
        /// The second parameter of the pair.
        partner: String,
        /// The range both load with.
        loaded: ParamRange,
    },
    /// Something inside a sub-patch definition loaded differently. The
    /// inner module IDs are the definition's own.
    InSubPatch {
        /// The definition's name.
        name: String,
        /// What loaded differently inside it.
        warning: Box<LoadWarning>,
    },
    /// Linked sub-patches whose library file differed from their stored
    /// copy, so the copy was replaced (`docs/design/sub-patches.md`, Linked
    /// library items).
    LibraryRefreshed {
        /// The refreshed sub-patches' names, in table order.
        names: Vec<String>,
    },
    /// A linked sub-patch's library file is missing, so its stored copy
    /// is used (sub-patches.md, rule 5: a patch always opens).
    LibraryMissing {
        /// The sub-patch's name.
        name: String,
        /// The library path the definition names.
        path: String,
    },
    /// A linked sub-patch's library file does not load, so its stored copy
    /// is used.
    LibraryUnreadable {
        /// The sub-patch's name.
        name: String,
        /// The library path the definition names.
        path: String,
        /// Why the file does not load.
        reason: String,
    },
    /// A refreshed library file no longer has a port or an exposed
    /// parameter the patch used, so what named it was removed
    /// (sub-patches.md, Linked library items).
    LinkDropped {
        /// The sub-patch's name.
        name: String,
        /// The port or exposed name the file no longer has.
        item: String,
        /// What was removed: `cable`, `assignment`, `MIDI mapping`, or
        /// `exposed parameter`.
        what: &'static str,
    },
    /// A chain of library links came back to a file already on it, so the
    /// linked sub-patch kept its stored copy (sub-patches.md, Patch file;
    /// rule 5).
    LibraryLoop {
        /// The sub-patch's name.
        name: String,
        /// The sub-patches along the loop, first to last.
        path: Vec<String>,
    },
    /// The refreshed library files could not be used together, so every
    /// linked sub-patch kept its stored copy (sub-patches.md, rule 5).
    LibraryNotUsed {
        /// Why the refreshed patch does not load.
        reason: String,
    },
    /// A patch-wide module (the Transport, or the Scale and tuning module)
    /// in a file inserted or refreshed as a sub-patch was left out, and its
    /// cables became ports on the tile (sub-patches.md, rule 8).
    PatchWideLeftOut {
        /// The sub-patch's name.
        name: String,
        /// What was left out, as the player knows it: `Transport`.
        module: String,
        /// What its cables carried, for the advice: `clock` or `pitch`.
        /// `None` when it had no cables, so no port was made.
        ports: Option<&'static str>,
    },
}

impl LoadWarning {
    /// How serious it is. Only unknown modules change what the patch plays.
    pub fn level(&self) -> LoadLevel {
        match self {
            Self::UnknownModules { .. } => LoadLevel::Warning,
            Self::InSubPatch { warning, .. } => warning.level(),
            _ => LoadLevel::Note,
        }
    }
}

impl fmt::Display for LoadWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownModules { kinds } => {
                let mut distinct: Vec<&String> = Vec::new();
                for kind in kinds {
                    if !distinct.contains(&kind) {
                        distinct.push(kind);
                    }
                }
                let names = distinct
                    .iter()
                    .take(3)
                    .map(|kind| format!("`{kind}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let more = distinct.len().saturating_sub(3);
                let extra = if more > 0 {
                    format!(" and {more} more")
                } else {
                    String::new()
                };
                let noun = if kinds.len() == 1 { "module is" } else { "modules are" };
                write!(
                    f,
                    "{} {noun} not in this version: {names}{extra}. They make no sound. Their settings are kept when you save. Open the patch in a newer LoomDSP to play them.",
                    kinds.len()
                )
            }
            Self::TransportAdded { module } => write!(
                f,
                "the patch had no Transport, so one was added as module {module} (120 BPM, 4/4, running)"
            ),
            Self::RangeIgnored { module, param } => write!(
                f,
                "module {module}: parameter `{param}` offers no ranges, so its saved range is ignored"
            ),
            Self::RangeNotKept {
                module,
                param,
                saved,
                reason,
                loaded,
            } => {
                let why = match reason {
                    RangeReason::DoesNotHold { value } => format!("does not hold its value {value}"),
                    RangeReason::NotOffered => "it does not offer".to_string(),
                };
                write!(
                    f,
                    "module {module}: parameter `{param}` was saved with range {saved}, which {why}; it loads with {loaded}"
                )
            }
            Self::SharedRangeNotKept {
                module,
                param,
                partner,
                loaded,
            } => write!(
                f,
                "module {module}: parameters `{param}` and `{partner}` share a range, and the saved ranges do not hold both values; they load with {loaded}"
            ),
            Self::InSubPatch { name, warning } => write!(f, "in sub-patch \"{name}\": {warning}"),
            Self::LibraryRefreshed { names } => write!(f, "Updated from the library: {}.", names.join(", ")),
            Self::LibraryMissing { name, path } => {
                write!(
                    f,
                    "{name}: the library file \"{path}\" is missing, so its stored copy is used."
                )
            }
            Self::LibraryUnreadable { name, path, reason } => write!(
                f,
                "{name}: the library file \"{path}\" does not load ({reason}), so its stored copy is used."
            ),
            Self::LinkDropped { name, item, what } => {
                write!(
                    f,
                    "{name}: the library file no longer has \"{item}\"; its {what} was removed."
                )
            }
            Self::LibraryLoop { name, path } => write!(
                f,
                "{name}: the library links loop ({}), so its stored copy is used.",
                path.join(" › ")
            ),
            Self::LibraryNotUsed { reason } => write!(
                f,
                "The library's changes could not be used ({reason}), so the stored copies are used."
            ),
            Self::PatchWideLeftOut {
                name,
                module,
                ports: Some(ports),
            } => write!(f, "{name}: its {module} was left out; connect its {ports} ports."),
            Self::PatchWideLeftOut {
                name,
                module,
                ports: None,
            } => write!(f, "{name}: its {module} was left out."),
        }
    }
}

#[cfg(test)]
mod tests;

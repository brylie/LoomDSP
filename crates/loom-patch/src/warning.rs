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
}

impl LoadWarning {
    /// How serious it is. Only unknown modules change what the patch plays.
    pub fn level(&self) -> LoadLevel {
        match self {
            Self::UnknownModules { .. } => LoadLevel::Warning,
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
        }
    }
}

#[cfg(test)]
mod tests;

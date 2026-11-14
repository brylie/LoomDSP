//! Built-in abstractions: patch files bundled with the app, listed first in
//! the module browser and inserted as a copy (`docs/design/sub-patches.md`,
//! Built-in abstractions). They are embedded at build time, so they are
//! trusted text, not files read from the player's library folder.

use loom_patch::Patch;
use std::sync::OnceLock;

/// One built-in abstraction.
#[derive(Debug)]
pub struct Builtin {
    /// The file's stem, which keys its browser row (`browser/builtin/<stem>`).
    pub stem: &'static str,
    /// The name the browser shows: the patch's own name.
    pub name: &'static str,
    /// The patch file's text.
    pub text: &'static str,
    /// The description, read from the text the first time it is asked for,
    /// so the browser does not parse the file each time it builds a row.
    description: OnceLock<String>,
}

impl Builtin {
    /// The patch. The text is fixed at build time and a test proves every
    /// built-in reads, so a failure here is a bug in the bundled file.
    pub fn patch(&self) -> Patch {
        Patch::from_ron(self.text).unwrap_or_else(|e| panic!("the built-in {} reads: {e}", self.stem))
    }

    /// The one-sentence description the browser shows and a screen reader
    /// reads: the patch's own.
    pub fn description(&self) -> &str {
        self.description.get_or_init(|| self.patch().metadata.description)
    }
}

/// Every built-in abstraction, in the order the browser lists them.
pub static ALL: [Builtin; 2] = [
    Builtin {
        stem: "synth-voice",
        name: "Synth voice",
        text: include_str!("../library/Synth voice.ron"),
        description: OnceLock::new(),
    },
    Builtin {
        stem: "effect-chain",
        name: "Effect chain",
        text: include_str!("../library/Effect chain.ron"),
        description: OnceLock::new(),
    },
];

#[cfg(test)]
mod tests;

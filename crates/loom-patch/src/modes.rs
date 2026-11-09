//! Sub-patch modes in a patch file (`docs/design/sub-patches.md`, Modes):
//! single, layer, select, and stack, the fields that go with them, the
//! modules the flattener adds for them, and the status texts of the
//! gestures the editor refuses.
//!
//! In layer and select mode, a definition's own modules are its members,
//! each an ordinary sub-patch instance, listed in order by
//! [`SubPatch::members`]. In stack mode the definition is one inside that
//! the flattener runs [`SubPatch::copies`] times. The flattener adds a sum,
//! a stereo spread sum, a Member module, and a Select per output as the
//! mode needs; each has a path step no module id can take
//! ([`crate::ModulePath::added`]), so it is never shown and never saved
//! (ADR-0040).

use crate::SubPatch;
use serde::{Deserialize, Serialize};

/// How a sub-patch runs its inside (FR-ENCAP-10).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Mode {
    /// One patch, as it is. The default; the file omits it.
    #[default]
    Single,
    /// Several members, all running, each output the sum of theirs.
    Layer,
    /// Several members, all running, each output the chosen member's,
    /// crossfaded over 10 ms when the choice changes.
    Select,
    /// One patch run as several copies, each output their sum at 1/√N.
    Stack,
}

impl Mode {
    /// Every mode, in the order the inspector lists them.
    pub const ALL: [Mode; 4] = [Mode::Single, Mode::Layer, Mode::Select, Mode::Stack];

    /// True for [`Mode::Single`], which the file omits.
    pub fn is_single(&self) -> bool {
        *self == Mode::Single
    }

    /// True for layer and select, whose inside is a list of members.
    pub fn has_members(self) -> bool {
        matches!(self, Mode::Layer | Mode::Select)
    }

    /// The mode's name in the inspector: `Single`, `Layer`, `Select`,
    /// `Stack`.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Single => "Single",
            Mode::Layer => "Layer",
            Mode::Select => "Select",
            Mode::Stack => "Stack",
        }
    }

    /// The mode's name inside a sentence: `single`, `a layer`'s `layer`, and
    /// so on.
    pub fn word(self) -> &'static str {
        match self {
            Mode::Single => "single",
            Mode::Layer => "layer",
            Mode::Select => "select",
            Mode::Stack => "stack",
        }
    }
}

/// The most members a layer or select holds and the most copies a stack
/// runs: the added modules have this many inputs (sub-patches.md, Members
/// are nested instances).
pub const MOST_MEMBERS: usize = 16;

/// The reserved exposed name under which a select instance holds its Member
/// value (sub-patches.md, Patch file).
pub const MEMBER_PARAM: &str = "member";

/// The kind of the Sum the flattener adds for a layer's and a mono stack's
/// outputs.
pub const SUM_KIND: &str = "loom.struct.sum@1";
/// The kind of the Stereo spread sum the flattener adds for a stack with
/// stereo spread.
pub const SPREAD_SUM_KIND: &str = "loom.struct.spread_sum@1";
/// The kind of the Member module the flattener adds for a select.
pub const MEMBER_KIND: &str = "loom.struct.member@1";
/// The kind of the Select the flattener adds for each output of a select.
pub const SELECT_KIND: &str = "loom.struct.select@1";
/// The kind of the constant source a spread's hidden assignments come from.
pub const SPREAD_SOURCE_KIND: &str = "loom.math.constant@1";

/// `Remove members until one is left to make Voice single` (`stack` in
/// place of `single`): a layer or select with several members cannot leave
/// the member modes.
pub fn several_members(name: &str, mode: Mode) -> String {
    format!("Remove members until one is left to make {name} {}", mode.word())
}

/// `Make Saw single before making Pad single` (`stack` in place of
/// `single`): a layer or select whose one member has a mode of its own
/// cannot leave the member modes, since its inside would lose that mode.
pub fn member_has_mode(member: &str, name: &str, mode: Mode) -> String {
    format!("Make {member} single before making {name} {}", mode.word())
}

/// `A layer needs at least one member` (`select` for a select).
pub fn last_member(mode: Mode) -> String {
    format!("A {} needs at least one member", mode.word())
}

/// `A layer holds at most 16 members` (`select` for a select).
pub fn too_many_members(mode: Mode) -> String {
    format!("A {} holds at most {MOST_MEMBERS} members", mode.word())
}

/// `A layer holds only its members; open a member to add modules`.
pub fn only_members(mode: Mode) -> String {
    format!("A {} holds only its members; open a member to add modules", mode.word())
}

/// `Make Voice single before dissolving it`.
pub fn dissolve_single_only(name: &str) -> String {
    format!("Make {name} single before dissolving it")
}

/// `Cannot save a layer to the library yet` (`select`, `stack`).
pub fn no_library(mode: Mode) -> String {
    format!("Cannot save a {} to the library yet", mode.word())
}

/// `A layer cannot fill a role yet` (`select`, `stack`).
pub fn no_role(mode: Mode) -> String {
    format!("A {} cannot fill a role yet", mode.word())
}

/// `Voice fills Filter; turn that off before changing its mode`.
pub fn fills_a_role(name: &str, role: &str) -> String {
    format!("{name} fills {role}; turn that off before changing its mode")
}

/// `Cannot stack Voice: it holds a slot`.
pub fn stack_holds_slot(name: &str) -> String {
    format!("Cannot stack {name}: it holds a slot")
}

/// The refusal of a mode change on a tile that is not a sub-patch.
pub const NOT_A_SUBPATCH: &str = "Only a sub-patch has a mode";

/// The refusal of Make slot inside a stack of more than one copy.
pub const NO_SLOT_IN_STACK: &str = "Cannot make a slot inside a stack yet";

/// The refusal of a mode change on a sub-patch that holds a slot.
pub const NO_MODE_WITH_SLOT: &str =
    "Cannot change the mode of a sub-patch that holds a slot yet; dissolve the slot first";

/// A definition's mode and the fields that go with it, as the editor keeps
/// them beside the definition (`crate::hierarchy::Definition`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModeSettings {
    /// The mode.
    pub mode: Mode,
    /// Layer and select: the members' level ids, in order.
    pub members: Vec<u32>,
    /// Stack: the copy count, 1 to 16 (0 reads as 1).
    pub copies: u32,
    /// Stack: exposed name to spread amount.
    pub spread: std::collections::BTreeMap<String, f32>,
    /// Stack: stereo spread on.
    pub stereo_spread: bool,
}

impl ModeSettings {
    /// The settings `definition` holds.
    pub fn of(definition: &SubPatch) -> Self {
        Self {
            mode: definition.mode,
            members: definition.members.clone(),
            copies: definition.copies,
            spread: definition.spread.clone(),
            stereo_spread: definition.stereo_spread,
        }
    }

    /// Writes these settings into `definition`, as the file holds them: a
    /// copy count of 1 or less as 0, so the file omits it.
    pub fn write(&self, definition: &mut SubPatch) {
        definition.mode = self.mode;
        definition.members = self.members.clone();
        definition.copies = if self.copies <= 1 { 0 } else { self.copies };
        definition.spread = self.spread.clone();
        definition.stereo_spread = self.stereo_spread;
    }

    /// The copy count: 1 to 16 in stack mode, 1 otherwise.
    pub fn copy_count(&self) -> u32 {
        if self.mode == Mode::Stack {
            self.copies.max(1)
        } else {
            1
        }
    }
}

impl SubPatch {
    /// How many copies the flattener runs: [`SubPatch::copies`] in stack
    /// mode, at least 1; 1 in every other mode.
    pub fn copy_count(&self) -> u32 {
        if self.mode == Mode::Stack {
            self.copies.max(1)
        } else {
            1
        }
    }

    /// Why the mode fields do not fit together, or `None` when they do
    /// (sub-patches.md, Patch file): members only in layer and select,
    /// listing every module once and nothing but instances, with nothing
    /// else at that level; Copies, spread, and stereo spread only in stack
    /// mode, Copies from 1 to 16, each spread name exposed, each amount
    /// from 0 to 1.
    pub fn mode_problem(&self) -> Option<String> {
        if self.mode.has_members() {
            if self.members.is_empty() {
                return Some(format!("a {} needs at least one member", self.mode.word()));
            }
            if self.members.len() > MOST_MEMBERS {
                return Some(format!("it has more than {MOST_MEMBERS} members"));
            }
            let mut listed = self.members.clone();
            listed.sort_unstable();
            listed.dedup();
            if listed.len() != self.members.len() {
                return Some("it lists a member twice".to_string());
            }
            let mut ids: Vec<u32> = self.modules.iter().map(|m| m.id).collect();
            ids.sort_unstable();
            if ids != listed {
                return Some("its members are not exactly its modules".to_string());
            }
            if let Some(module) = self.modules.iter().find(|m| !m.is_instance()) {
                return Some(format!("module {} is not a sub-patch", module.id));
            }
            if !self.cables.is_empty() || !self.modulation.is_empty() || !self.midi.is_empty() {
                return Some(format!(
                    "a {} holds only its members: no cables, modulation, or MIDI mappings",
                    self.mode.word()
                ));
            }
            if self.exposed.iter().any(|e| e.name == MEMBER_PARAM) && self.mode == Mode::Select {
                return Some(format!("`{MEMBER_PARAM}` is the Member control's name"));
            }
        } else if !self.members.is_empty() {
            return Some(format!("a {} has no members", self.mode.word()));
        }
        if self.mode == Mode::Stack {
            if self.copies > MOST_MEMBERS as u32 {
                return Some(format!("Copies is {}, above {MOST_MEMBERS}", self.copies));
            }
            for (name, amount) in &self.spread {
                if !self.exposed.iter().any(|e| &e.name == name) {
                    return Some(format!("it spreads `{name}`, which it does not expose"));
                }
                if !(0.0..=1.0).contains(amount) {
                    return Some(format!("the spread of `{name}` is {amount}, outside 0 to 1"));
                }
            }
        } else if self.copies > 1 || !self.spread.is_empty() || self.stereo_spread {
            return Some(format!(
                "a {} has no copies, spread, or stereo spread",
                self.mode.word()
            ));
        }
        None
    }
}

#[cfg(test)]
mod tests;

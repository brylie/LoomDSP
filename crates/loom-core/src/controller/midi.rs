//! MIDI mappings, learn, and inputs: one group of [`Controller`]'s methods.

use super::*;

/// # MIDI mappings, learn, and inputs
///
/// MIDI mappings and learn (`docs/design/midi-learn.md`), and the MIDI
/// inputs the engine listens to.
impl Controller {
    /// Maps a hardware control to a parameter. Takes effect on the next
    /// [`Controller::commit`].
    ///
    /// A parameter has one mapping, and so does a control, so this first
    /// removes any mapping on the parameter and any mapping on the control.
    /// Those are returned, at most two, so the UI can say what moved.
    /// Spec: `docs/design/midi-learn.md`.
    pub fn map_midi(
        &mut self,
        control: MidiControl,
        target: ModuleId,
        param: usize,
    ) -> Result<(MidiMappingId, Vec<MidiMapping>), LoomError> {
        if !control.is_valid() {
            return Err(LoomError::InvalidMidiControl(control));
        }
        self.check_target(target, param)?;
        let replaced: Vec<MidiMappingId> = self
            .graph
            .midi_mappings()
            .filter(|(_, mapping)| mapping.control == control || (mapping.target == target && mapping.param == param))
            .map(|(id, _)| id)
            .collect();
        let removed = replaced
            .into_iter()
            .filter_map(|id| {
                self.midi.forget(id);
                self.graph.midi_mappings.remove(&id)
            })
            .collect();
        let id = self.midi.new_mapping();
        self.graph
            .midi_mappings
            .insert(id, MidiMapping { control, target, param });
        self.dirty = true;
        Ok((id, removed))
    }

    /// Maps a hardware control to a parameter by name.
    pub fn map_midi_named(
        &mut self,
        control: MidiControl,
        target: ModuleId,
        param: &str,
    ) -> Result<(MidiMappingId, Vec<MidiMapping>), LoomError> {
        let param = self.param_index(target, param)?;
        self.map_midi(control, target, param)
    }

    /// Finishes MIDI learn: maps the control that moved to a parameter, and
    /// sets the parameter from that control change, so the user hears at
    /// once that the link works. Commit afterwards.
    pub fn learn_midi(
        &mut self,
        seen: ControlChangeSeen,
        target: ModuleId,
        param: usize,
    ) -> Result<(MidiMappingId, Vec<MidiMapping>), LoomError> {
        let result = self.map_midi(seen.control, target, param)?;
        // The control spans the selected range (control-ranges.md, rule 4).
        let spec = self.node(target)?.spec(param).ok_or_else(|| LoomError::NoSuchParam {
            module: target,
            param: param.to_string(),
        })?;
        self.set_param(target, param, control_value(&spec, seen.value))?;
        Ok(result)
    }

    /// Removes the MIDI mapping on a parameter, if there is one. Takes effect
    /// on the next [`Controller::commit`].
    pub fn unmap_midi(&mut self, target: ModuleId, param: usize) -> Result<Option<MidiMapping>, LoomError> {
        self.check_param(target, param)?;
        let Some((id, _)) = self.graph.midi_mapping_of(target, param) else {
            return Ok(None);
        };
        self.midi.forget(id);
        self.dirty = true;
        Ok(self.graph.midi_mappings.remove(&id))
    }

    /// The last control change the engine received, from any device, for
    /// MIDI learn. `None` before the first one.
    pub fn last_control_change(&self) -> Option<ControlChangeSeen> {
        self.midi.feedback().last()
    }

    /// The last note on the engine received, or `None` before the first.
    /// Its serial is counted with control changes, so learning a panic
    /// trigger can take whichever came first after it started.
    pub fn last_note_on(&self) -> Option<crate::midi_map::NoteOnSeen> {
        self.midi.feedback().last_note()
    }

    /// A handle that reads the last control change from any thread.
    pub fn midi_reader(&self) -> MidiReader {
        MidiReader::new(self.midi.feedback().clone())
    }

    /// Copies the values MIDI mappings set in the engine into the graph, so
    /// the UI shows and saves what plays. Call it whenever the UI draws.
    /// Returns the parameters that changed.
    pub fn sync_midi(&mut self) -> Vec<(ModuleId, usize)> {
        self.midi.sync(&mut self.graph)
    }

    /// Starts tracking the notes held on MIDI input `input`. Call it before
    /// events stamped with `input` reach the engine; events from an input
    /// that is not open are dropped. Opening an input that is open releases
    /// its notes first. Spec: `docs/design/device-changes.md`, Held notes
    /// belong to an input.
    pub fn open_midi_input(&mut self, input: crate::MidiInputId) -> Result<(), LoomError> {
        let index = check_input_id(input)?;
        self.send(Command::OpenInput {
            index,
            held: Box::default(),
        })
    }

    /// Stops tracking MIDI input `input`: the engine sends a note off for
    /// every note it held, unless another input still holds that note, and
    /// drops later events stamped with it. Call it when the input is closed
    /// or gone.
    pub fn release_midi_input(&mut self, input: crate::MidiInputId) -> Result<(), LoomError> {
        let index = check_input_id(input)?;
        self.send(Command::ReleaseInput(index))
    }

    /// Sets the MIDI message that runs panic in the engine, or `None` for
    /// no trigger. The engine looks for it in its own MIDI path, so it
    /// works with no UI thread. The trigger belongs to the player's rig,
    /// so it is not part of the graph or the patch.
    pub fn set_panic_trigger(&mut self, trigger: Option<crate::PanicTrigger>) -> Result<(), LoomError> {
        self.send(Command::SetPanicTrigger(trigger))
    }
}

/// The table index of a MIDI input id, if it is in range.
fn check_input_id(input: crate::MidiInputId) -> Result<usize, LoomError> {
    if input.0 == 0 || input.0 >= crate::MAX_MIDI_INPUTS {
        return Err(LoomError::InvalidMidiInput(input.0));
    }
    Ok(usize::from(input.0))
}

/// What the controller knows about MIDI mappings beyond the graph: the
/// engine's feedback table, the last control change applied per mapping,
/// and screen edits the engine has not confirmed yet (MIDI learn, rules 2
/// and 3). Only this type touches that bookkeeping, so the order between
/// screen edits and control changes is kept in one place.
pub(super) struct MidiSync {
    /// What the engine's MIDI mappings did. Shared with the engine.
    feedback: Arc<MidiFeedback>,
    /// The last feedback sequence number applied, per mapping.
    seen: BTreeMap<MidiMappingId, u32>,
    /// Screen edits on mapped parameters that the engine has not confirmed
    /// yet: the token of the latest one, per mapping.
    edits: BTreeMap<MidiMappingId, u32>,
    /// The token the next screen edit gets. Never 0, which means none.
    next_edit: u32,
    /// The id the next mapping gets.
    next_mapping: u32,
}

impl MidiSync {
    /// No mappings yet, reading the engine's `feedback`.
    pub(super) fn new(feedback: Arc<MidiFeedback>) -> Self {
        Self {
            feedback,
            seen: BTreeMap::new(),
            edits: BTreeMap::new(),
            next_edit: 1,
            next_mapping: 1,
        }
    }

    /// What the engine's MIDI mappings did: the last control change and
    /// note, for learn.
    pub(super) fn feedback(&self) -> &Arc<MidiFeedback> {
        &self.feedback
    }

    /// A new mapping id.
    pub(super) fn new_mapping(&mut self) -> MidiMappingId {
        let id = MidiMappingId(self.next_mapping);
        self.next_mapping += 1;
        id
    }

    /// Drops what is known about a mapping that is removed or replaced.
    pub(super) fn forget(&mut self, id: MidiMappingId) {
        self.seen.remove(&id);
        self.edits.remove(&id);
    }

    /// The token the next screen edit will carry. Read it to build the
    /// command, and call [`record_edit`](Self::record_edit) once the command
    /// is sent.
    pub(super) fn next_token(&self) -> u32 {
        self.next_edit
    }

    /// Notes that a screen edit on mapping `id` was sent with the token
    /// [`next_token`](Self::next_token) gave, and returns that token. Until
    /// the engine confirms it, [`sync`](Self::sync) ignores control changes
    /// on the mapping, because they came before the edit.
    pub(super) fn record_edit(&mut self, id: MidiMappingId) -> u32 {
        let token = self.next_edit;
        self.edits.insert(id, token);
        self.next_edit = self.next_edit.wrapping_add(1).max(1);
        token
    }

    /// The feedback sequence number the graph holds, per mapping, for the
    /// compiler. With a screen edit on its way, every control change so far
    /// is older than the graph's value.
    pub(super) fn synced(&self, graph: &Graph) -> BTreeMap<MidiMappingId, u32> {
        graph
            .midi_mappings()
            .filter_map(|(id, mapping)| {
                let entry = self.feedback.read(mapping.control);
                if self.edits.contains_key(&id) && entry.id == id {
                    Some((id, entry.sequence))
                } else {
                    self.seen.get(&id).map(|&sequence| (id, sequence))
                }
            })
            .collect()
    }

    /// Copies the values the engine's mappings set into `graph`. Returns the
    /// parameters that changed.
    pub(super) fn sync(&mut self, graph: &mut Graph) -> Vec<(ModuleId, usize)> {
        let mut changed = Vec::new();
        for (&id, mapping) in &graph.midi_mappings {
            let entry = self.feedback.read(mapping.control);
            let unseen = entry.id == id && self.seen.get(&id) != Some(&entry.sequence);
            let apply = match self.edits.get(&id) {
                // A screen edit is on its way to the engine. The graph has
                // its value already, and any control change read now came
                // before it, so wait.
                Some(&token) => {
                    let (applied, at) = self.feedback.read_edit(mapping.control);
                    if applied != token {
                        continue;
                    }
                    // The engine applied the edit. Only a control change it
                    // applied after the edit is new.
                    self.edits.remove(&id);
                    unseen && later(entry.sequence, at)
                }
                None => unseen,
            };
            if entry.id == id {
                self.seen.insert(id, entry.sequence);
            }
            if apply
                && let Some(node) = graph.nodes.get_mut(&mapping.target)
                && let Some(spec) = node.spec(mapping.param)
            {
                node.params[mapping.param] = control_value(&spec, entry.value);
                changed.push((mapping.target, mapping.param));
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests;

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
                self.midi_seen.remove(&id);
                self.midi_edits.remove(&id);
                self.graph.midi_mappings.remove(&id)
            })
            .collect();
        let id = MidiMappingId(self.next_midi_mapping);
        self.next_midi_mapping += 1;
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
        self.midi_seen.remove(&id);
        self.midi_edits.remove(&id);
        self.dirty = true;
        Ok(self.graph.midi_mappings.remove(&id))
    }

    /// The last control change the engine received, from any device, for
    /// MIDI learn. `None` before the first one.
    pub fn last_control_change(&self) -> Option<ControlChangeSeen> {
        self.midi_feedback.last()
    }

    /// The last note on the engine received, or `None` before the first.
    /// Its serial is counted with control changes, so learning a panic
    /// trigger can take whichever came first after it started.
    pub fn last_note_on(&self) -> Option<crate::midi_map::NoteOnSeen> {
        self.midi_feedback.last_note()
    }

    /// A handle that reads the last control change from any thread.
    pub fn midi_reader(&self) -> MidiReader {
        MidiReader::new(self.midi_feedback.clone())
    }

    /// Copies the values MIDI mappings set in the engine into the graph, so
    /// the UI shows and saves what plays. Call it whenever the UI draws.
    /// Returns the parameters that changed.
    pub fn sync_midi(&mut self) -> Vec<(ModuleId, usize)> {
        let mut changed = Vec::new();
        for (&id, mapping) in &self.graph.midi_mappings {
            let entry = self.midi_feedback.read(mapping.control);
            let unseen = entry.id == id && self.midi_seen.get(&id) != Some(&entry.sequence);
            let apply = match self.midi_edits.get(&id) {
                // A screen edit is on its way to the engine. The graph has
                // its value already, and any control change read now came
                // before it, so wait.
                Some(&token) => {
                    let (applied, at) = self.midi_feedback.read_edit(mapping.control);
                    if applied != token {
                        continue;
                    }
                    // The engine applied the edit. Only a control change it
                    // applied after the edit is new.
                    self.midi_edits.remove(&id);
                    unseen && later(entry.sequence, at)
                }
                None => unseen,
            };
            if entry.id == id {
                self.midi_seen.insert(id, entry.sequence);
            }
            if apply
                && let Some(node) = self.graph.nodes.get_mut(&mapping.target)
                && let Some(spec) = node.spec(mapping.param)
            {
                node.params[mapping.param] = control_value(&spec, entry.value);
                changed.push((mapping.target, mapping.param));
            }
        }
        changed
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

/// Clamps a modulation depth to -1..1. NaN becomes 0, which adds nothing.
/// The table index of a MIDI input id, if it is in range.
fn check_input_id(input: crate::MidiInputId) -> Result<usize, LoomError> {
    if input.0 == 0 || input.0 >= crate::MAX_MIDI_INPUTS {
        return Err(LoomError::InvalidMidiInput(input.0));
    }
    Ok(usize::from(input.0))
}

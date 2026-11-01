//! Parameters: one group of [`Controller`]'s methods.

use super::*;

/// # Parameters
///
/// Setting parameter values and ranges. These take effect at the next
/// buffer, without a recompile.
impl Controller {
    /// Sets several parameters of one module, or none of them.
    ///
    /// Takes effect at the next buffer. Each value is clamped. If the command
    /// queue cannot take every change, the graph is left as it was. A mode
    /// change that also clears a button's gate uses this, so a full queue
    /// cannot leave the mode on momentary and the gate high.
    pub fn set_params(&mut self, module: ModuleId, changes: &[(usize, f32)]) -> Result<(), LoomError> {
        let node = self.graph.nodes.get(&module).ok_or(LoomError::UnknownModule(module))?;
        let mut clamped = Vec::with_capacity(changes.len());
        for &(index, value) in changes {
            // Clamped to the selected range (control-ranges.md, rule 8).
            let spec = node.spec(index).ok_or_else(|| LoomError::NoSuchParam {
                module,
                param: index.to_string(),
            })?;
            clamped.push((index, spec.clamp(value)));
        }
        // Mapped parameters carry an edit token, as in `set_param`.
        let mappings: Vec<_> = clamped
            .iter()
            .map(|&(index, _)| {
                self.graph
                    .midi_mapping_of(module, index)
                    .map(|(id, mapping)| (id, mapping.control))
            })
            .collect();
        if let Some(fresh) = node.slot.and_then(|slot| self.fresh.get_mut(&slot)) {
            // Not in the engine yet: no command, and no edit to order
            // against MIDI.
            for &(index, value) in &clamped {
                fresh.module.set_param(index, value);
            }
        } else if let Some(slot) = node.slot {
            // Check first, then send, then write the graph. A full queue
            // then changes nothing the engine and the graph could disagree on.
            if self.commands.slots() < clamped.len() {
                return Err(LoomError::QueueFull);
            }
            for (&(index, value), mapping) in clamped.iter().zip(&mappings) {
                let edit = mapping.map(|(id, control)| {
                    let token = self.next_edit;
                    self.next_edit = self.next_edit.wrapping_add(1).max(1);
                    self.midi_edits.insert(id, token);
                    (control, token)
                });
                let pushed = self
                    .commands
                    .push(Command::SetParam {
                        slot,
                        index,
                        value,
                        edit,
                    })
                    .is_ok();
                debug_assert!(pushed, "capacity was checked above");
            }
        }
        let node = self
            .graph
            .nodes
            .get_mut(&module)
            .expect("module was present before the commands were sent");
        for (index, value) in clamped {
            node.params[index] = value;
        }
        Ok(())
    }

    /// Sets a parameter by index. Takes effect at the next buffer.
    /// The value is clamped to the parameter's range.
    pub fn set_param(&mut self, module: ModuleId, index: usize, value: f32) -> Result<f32, LoomError> {
        // On a mapped parameter, the edit carries a token, so `sync_midi`
        // can tell whether a control change came before or after it in the
        // engine (MIDI learn, rule 2).
        let mapping = self
            .graph
            .midi_mapping_of(module, index)
            .map(|(id, mapping)| (id, mapping.control));
        let node = self
            .graph
            .nodes
            .get_mut(&module)
            .ok_or(LoomError::UnknownModule(module))?;
        // Clamped to the selected range (control-ranges.md, rule 8).
        let spec = node.spec(index).ok_or_else(|| LoomError::NoSuchParam {
            module,
            param: index.to_string(),
        })?;
        let value = spec.clamp(value);
        node.params[index] = value;
        if let Some(fresh) = node.slot.and_then(|slot| self.fresh.get_mut(&slot)) {
            // Not in the engine yet: no command, and no edit to order
            // against MIDI.
            fresh.module.set_param(index, value);
            return Ok(value);
        }
        let edit = mapping.map(|(_, control)| (control, self.next_edit));
        if let Some(slot) = node.slot
            && self
                .commands
                .push(Command::SetParam {
                    slot,
                    index,
                    value,
                    edit,
                })
                .is_err()
        {
            return Err(LoomError::QueueFull);
        }
        if let Some((id, _)) = mapping {
            self.midi_edits.insert(id, self.next_edit);
            self.next_edit = self.next_edit.wrapping_add(1).max(1);
        }
        Ok(value)
    }

    /// Sets a parameter by name.
    pub fn set_param_named(&mut self, module: ModuleId, name: &str, value: f32) -> Result<f32, LoomError> {
        let index = self
            .node(module)?
            .descriptor
            .param_index(name)
            .ok_or_else(|| LoomError::NoSuchParam {
                module,
                param: name.to_string(),
            })?;
        self.set_param(module, index, value)
    }

    /// Selects the control range of parameter `index` (control-ranges.md).
    ///
    /// A smaller range clamps the value into it. A wider range keeps the
    /// value (rule 6). The knob, the inspector, MIDI, and modulation then
    /// span the range. `range` must be one the parameter offers, or its
    /// full range. Modulation and MIDI follow it after the next
    /// [`Controller::commit`], which puts the bounds into the schedule
    /// (rule 8). An error leaves the range and the value as they were.
    pub fn set_range(&mut self, module: ModuleId, index: usize, range: ParamRange) -> Result<(), LoomError> {
        let node = self
            .graph
            .nodes
            .get_mut(&module)
            .ok_or(LoomError::UnknownModule(module))?;
        let spec = node
            .descriptor
            .params
            .get(index)
            .ok_or_else(|| LoomError::NoSuchParam {
                module,
                param: index.to_string(),
            })?;
        // The full range is allowed too: a loaded value that no listed range
        // holds keeps it, so the value does not change (rule 7).
        let full = ParamRange::new(spec.min, spec.max);
        if spec.options.ranges.is_empty() || !(spec.options.ranges.contains(&range) || range == full) {
            return Err(LoomError::NoSuchRange {
                module,
                param: spec.name.to_string(),
                range: range.to_string(),
            });
        }
        let previous = node.ranges[index].replace(range);
        let value = node.params[index];
        let clamped = value.clamp(range.min, range.max);
        if clamped != value
            && let Err(error) = self.set_param(module, index, clamped)
        {
            if let Some(node) = self.graph.nodes.get_mut(&module) {
                node.ranges[index] = previous;
                node.params[index] = value;
            }
            return Err(error);
        }
        self.dirty = true;
        Ok(())
    }

    /// Clears a module's internal state.
    pub fn reset_module(&mut self, module: ModuleId) -> Result<(), LoomError> {
        if let Some(slot) = self.node(module)?.slot {
            if let Some(fresh) = self.fresh.get_mut(&slot) {
                fresh.module.reset();
                return Ok(());
            }
            self.commands
                .push(Command::ResetModule { slot })
                .map_err(|_| LoomError::QueueFull)?;
        }
        Ok(())
    }
}

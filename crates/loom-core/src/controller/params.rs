//! Parameters: one group of [`Controller`]'s methods.

use super::*;
use std::collections::BTreeSet;

/// # Parameters
///
/// Ordinary parameter values take effect at the next buffer without a
/// recompile. Graph-shaping values prepare and publish a new schedule on
/// the control thread, so its buffers and copies agree with the new value.
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
        if clamped
            .iter()
            .any(|&(index, _)| node.spec(index).is_some_and(|spec| spec.options.shapes_graph))
        {
            return self.set_params_across(
                &clamped
                    .iter()
                    .map(|&(index, value)| (module, index, value))
                    .collect::<Vec<_>>(),
            );
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
        if let Some(fresh) = node.slot.and_then(|slot| self.slots.fresh_mut(slot)) {
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
                // The push cannot fail (checked above), so the edit is
                // recorded before it.
                let edit = mapping.map(|(id, control)| (control, self.midi.record_edit(id)));
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
        if self
            .node(module)?
            .spec(index)
            .is_some_and(|spec| spec.options.shapes_graph)
        {
            self.set_params(module, &[(index, value)])?;
            return Ok(self.node(module)?.params[index]);
        }
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
        if let Some(fresh) = node.slot.and_then(|slot| self.slots.fresh_mut(slot)) {
            // Not in the engine yet: no command, and no edit to order
            // against MIDI.
            fresh.module.set_param(index, value);
            return Ok(value);
        }
        let edit = mapping.map(|(_, control)| (control, self.midi.next_token()));
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
            self.midi.record_edit(id);
        }
        Ok(value)
    }

    /// Sets parameters across modules as one transaction, or changes none.
    ///
    /// Values are clamped; the last entry for a target wins. A changed
    /// graph-shaping value prepares one schedule for the whole group. If
    /// structural values are unchanged, only ordinary commands are sent.
    /// Fresh modules and already-dirty graphs remain staged until `commit`;
    /// their initial parameters are applied before their first reset. Otherwise
    /// capacity errors leave all graph values and the running schedule intact.
    pub fn set_params_across(&mut self, changes: &[(ModuleId, usize, f32)]) -> Result<(), LoomError> {
        let mut targets = BTreeMap::new();
        for &(module, index, value) in changes {
            let spec = self.node(module)?.spec(index).ok_or_else(|| LoomError::NoSuchParam {
                module,
                param: index.to_string(),
            })?;
            targets.insert((module, index), (spec.clamp(value), spec.options.shapes_graph));
        }
        // Reconcile older MIDI feedback before staging any screen values.
        // The immediate commit below must not import feedback a second time.
        self.collect_garbage();
        self.sync_midi();
        let fresh: BTreeSet<_> = targets
            .keys()
            .filter_map(|&(module, _)| {
                self.graph.nodes[&module]
                    .slot
                    .filter(|&slot| self.slots.fresh_mut(slot).is_some())
                    .map(|_| module)
            })
            .collect();
        let structural_changed = targets.iter().any(|(&(module, index), &(value, structural))| {
            structural && self.graph.nodes[&module].params[index] != value
        });
        let mut ordinary: BTreeMap<ModuleId, Vec<(usize, f32)>> = BTreeMap::new();
        for (&(module, index), &(value, structural)) in &targets {
            if !structural {
                ordinary.entry(module).or_default().push((index, value));
            }
        }
        let ordinary_commands: usize = ordinary
            .iter()
            .map(|(module, changes)| {
                if self.graph.nodes[module].slot.is_some() && !fresh.contains(module) {
                    changes.len()
                } else {
                    0
                }
            })
            .sum();
        let live_structural = targets.iter().any(|(&(module, _), &(_, structural))| {
            structural && self.graph.nodes[&module].slot.is_some() && !fresh.contains(&module)
        });
        let defer = structural_changed && (self.dirty || !live_structural);
        if !structural_changed || defer {
            if self.commands.slots() < ordinary_commands {
                return Err(LoomError::QueueFull);
            }
            if defer {
                for (&(module, index), &(value, structural)) in &targets {
                    if structural {
                        self.graph.nodes.get_mut(&module).expect("validated module").params[index] = value;
                        if let Some(instance) = self.graph.nodes[&module]
                            .slot
                            .and_then(|slot| self.slots.fresh_mut(slot))
                        {
                            instance.module.set_param(index, value);
                        }
                    }
                }
                self.mark_dirty();
            }
            for (module, changes) in ordinary {
                self.set_params(module, &changes)?;
            }
            return Ok(());
        }
        let mut before = BTreeMap::new();
        for (&(module, index), &(value, _)) in &targets {
            let node = self.graph.nodes.get_mut(&module).expect("validated module");
            before.entry(module).or_insert_with(|| node.params.clone());
            node.params[index] = value;
        }
        let was_dirty = self.dirty;
        let widths = width::infer(&self.graph);
        let mut starting = 0;
        let mut stopping = 0;
        for (&id, node) in &self.graph.nodes {
            if !matches!(node.descriptor.role, ModuleRole::Dsp(_)) {
                continue;
            }
            let sides = if widths.per_channel.contains(&id) { 2 } else { 1 };
            starting += (0..widths.voices[&id])
                .flat_map(|voice| (0..sides).map(move |side| (voice, side)))
                .filter(|&key| key != (0, 0) && !node.copy_slots.contains_key(&key))
                .count();
            stopping += node
                .copy_slots
                .keys()
                .filter(|&&(voice, side)| voice >= widths.voices[&id] || side >= sides)
                .count();
        }
        let needed = self.slots.waiting() + starting + stopping + self.slots.retiring().len() + 1 + ordinary_commands;
        let error = if self.slots.free_count() < starting {
            Some(LoomError::TooManyModules(self.config.max_modules))
        } else if self.commands.slots() < needed {
            Some(LoomError::QueueFull)
        } else {
            None
        };
        if let Some(error) = error {
            for (module, params) in before {
                self.graph.nodes.get_mut(&module).expect("validated module").params = params;
            }
            return Err(error);
        }
        // Fresh targets get every initial value before reset and insertion.
        for (&(module, index), &(value, _)) in &targets {
            if let Some(instance) = self.graph.nodes[&module]
                .slot
                .and_then(|slot| self.slots.fresh_mut(slot))
            {
                instance.module.set_param(index, value);
            }
        }
        self.dirty = true;
        if let Err(error) = self.commit_synced() {
            for &(module, index) in targets.keys() {
                if let Some(instance) = self.graph.nodes[&module]
                    .slot
                    .and_then(|slot| self.slots.fresh_mut(slot))
                {
                    instance.module.set_param(index, before[&module][index]);
                }
            }
            for (module, params) in before {
                self.graph.nodes.get_mut(&module).expect("validated module").params = params;
            }
            self.dirty = was_dirty;
            return Err(error);
        }
        self.revision = self.revision.wrapping_add(1);
        for (module, changes) in ordinary {
            if !fresh.contains(&module) {
                self.set_params(module, &changes)?;
            }
        }
        Ok(())
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
    ///
    /// The two parameters of a range widget share one selected range, so
    /// this selects it for both (control-ranges.md, Ranges on a range widget).
    pub fn set_range(&mut self, module: ModuleId, index: usize, range: ParamRange) -> Result<(), LoomError> {
        let partner = self
            .graph
            .nodes
            .get(&module)
            .and_then(|node| node.descriptor.range_partner(index));
        // Check first, then change. Both parameters offer the same ranges (the
        // registry checks), so once this holds, the second cannot fail after
        // the first has already clamped a value and told the engine.
        if let Some(partner) = partner
            && let Some(node) = self.graph.nodes.get(&module)
            && node.slot.is_some_and(|slot| self.slots.fresh_mut(slot).is_none())
        {
            let clamping = [index, partner]
                .iter()
                .filter(|&&i| node.params.get(i).is_some_and(|&v| v.clamp(range.min, range.max) != v))
                .count();
            if self.commands.slots() < clamping {
                return Err(LoomError::QueueFull);
            }
        }
        self.set_one_range(module, index, range)?;
        if let Some(partner) = partner {
            self.set_one_range(module, partner, range)?;
        }
        Ok(())
    }

    /// Selects a range for one parameter. See [`Controller::set_range`].
    fn set_one_range(&mut self, module: ModuleId, index: usize, range: ParamRange) -> Result<(), LoomError> {
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
        // The same value now sits at another position under the hardware, so
        // a mapping on it starts again, and a Pickup waits (midi-learn.md).
        if previous != Some(range)
            && let Some((id, mapping)) = self.graph.midi_mapping_of(module, index)
        {
            let mapping = MidiMapping {
                learned: false,
                ..*mapping
            };
            self.renew_midi_mapping(id, mapping);
        }
        self.mark_dirty();
        Ok(())
    }

    /// Sets the range MIDI sweeps on each listed parameter, and clears it on
    /// every other one: a sub-patch surface's exposed range (sub-patches.md,
    /// Exposing). A mapped control then covers the limit from one end of its
    /// travel to the other, inside the selected range (see
    /// [`Node::midi_spec`]). Knobs, values, and modulation are unchanged:
    /// the caller clamps values itself, as the surface does.
    ///
    /// A limit is cut to the parameter's own bounds and must stay a finite
    /// span. Each parameter whose limit changes has its MIDI mapping renewed,
    /// like a new selected range: the same value now sits at another place
    /// under the hardware, so the engine starts the mapping again and a
    /// Pickup waits. The engine follows at the next [`Controller::commit`],
    /// which compiles the new span into the schedule (control-ranges.md,
    /// rule 8). An error changes nothing.
    pub fn set_limits(
        &mut self,
        limits: impl IntoIterator<Item = (ModuleId, usize, ParamRange)>,
    ) -> Result<(), LoomError> {
        let mut wanted: BTreeMap<(ModuleId, usize), ParamRange> = BTreeMap::new();
        for (module, index, limit) in limits {
            let node = self.node(module)?;
            let spec = node
                .descriptor
                .params
                .get(index)
                .ok_or_else(|| LoomError::NoSuchParam {
                    module,
                    param: index.to_string(),
                })?;
            let cut = ParamRange::new(limit.min.max(spec.min), limit.max.min(spec.max));
            // The limit itself is checked too, because `f32::max` drops NaN.
            // Written so that NaN fails here.
            if !(limit.min.is_finite() && limit.max.is_finite() && limit.min < limit.max && cut.min < cut.max) {
                return Err(LoomError::InvalidLimit {
                    module,
                    param: spec.name.to_string(),
                    range: limit.to_string(),
                });
            }
            wanted.insert((module, index), cut);
        }
        let mut changed = Vec::new();
        for (&module, node) in &mut self.graph.nodes {
            for (index, slot) in node.limits.iter_mut().enumerate() {
                let limit = wanted.get(&(module, index)).copied();
                if *slot != limit {
                    *slot = limit;
                    changed.push((module, index));
                }
            }
        }
        for (module, index) in changed {
            self.mark_dirty();
            if let Some((id, mapping)) = self.graph.midi_mapping_of(module, index) {
                let mapping = MidiMapping {
                    learned: false,
                    ..*mapping
                };
                self.renew_midi_mapping(id, mapping);
            }
        }
        Ok(())
    }

    /// Clears a module's internal state.
    pub fn reset_module(&mut self, module: ModuleId) -> Result<(), LoomError> {
        if let Some(slot) = self.node(module)?.slot {
            if let Some(fresh) = self.slots.fresh_mut(slot) {
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

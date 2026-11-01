//! Modules: one group of [`Controller`]'s methods.

use super::*;

/// # Modules
///
/// Adding and removing modules. A new module is held until the next
/// [`commit`](Controller::commit), so the values set before it are its first values.
impl Controller {
    /// The patch's Transport module ([`TRANSPORT_MODULE`](crate::TRANSPORT_MODULE)),
    /// if it has one.
    pub fn transport_module(&self) -> Option<ModuleId> {
        self.graph
            .nodes
            .iter()
            .find(|(_, node)| node.descriptor.id == crate::transport::TRANSPORT_MODULE)
            .map(|(&id, _)| id)
    }

    /// Adds a module by kind (`"id@version"` or `"id"`). Returns its new ID.
    /// Fails with [`LoomError::ModuleIdOutOfRange`] once the next free ID
    /// would pass [`ModuleId::MAX`].
    pub fn add_module(&mut self, kind: &str) -> Result<ModuleId, LoomError> {
        let descriptor = self.registry.resolve(kind)?;
        let id = ModuleId(self.next_id);
        self.add_descriptor(id, descriptor)?;
        Ok(id)
    }

    /// Adds a module with a specific ID, for example when loading a patch.
    /// Fails with [`LoomError::ModuleIdOutOfRange`] for an ID above
    /// [`ModuleId::MAX`], and with [`LoomError::DuplicateModuleId`] for one
    /// already in use.
    pub fn add_module_with_id(&mut self, id: ModuleId, kind: &str) -> Result<(), LoomError> {
        let descriptor = self.registry.resolve(kind)?;
        self.add_descriptor(id, descriptor)
    }

    /// Adds an inert placeholder with owned metadata for a missing module.
    ///
    /// It is intended for patch loading; interactive connection rules are
    /// enforced by the editor. Inputs are limited to the engine's 64-bit
    /// cable mask, as for registered descriptors.
    pub fn add_unknown_with_id(&mut self, id: ModuleId, unknown: crate::UnknownModule) -> Result<(), LoomError> {
        if unknown.inputs.len() > 64 {
            return Err(LoomError::InvalidDescriptor {
                id: unknown.kind.clone(),
                reason: "more than 64 inputs".into(),
            });
        }
        self.add_descriptor(id, &crate::graph::UNKNOWN_DESCRIPTOR)?;
        self.graph.nodes.get_mut(&id).expect("inserted placeholder").unknown = Some(unknown);
        Ok(())
    }

    /// Adds a placeholder with the next available module ID.
    pub fn add_unknown(&mut self, unknown: crate::UnknownModule) -> Result<ModuleId, LoomError> {
        let id = ModuleId(self.next_id);
        self.add_unknown_with_id(id, unknown)?;
        Ok(id)
    }

    /// Sets a placeholder's saved label without changing its audio graph.
    /// Returns false if the module is absent or is not a placeholder.
    pub fn set_unknown_label(&mut self, id: ModuleId, label: Option<String>) -> bool {
        let Some(unknown) = self.graph.nodes.get_mut(&id).and_then(|node| node.unknown.as_mut()) else {
            return false;
        };
        unknown.label = label;
        true
    }

    fn add_descriptor(&mut self, id: ModuleId, descriptor: &'static ModuleDescriptor) -> Result<(), LoomError> {
        if self.graph.nodes.contains_key(&id) {
            return Err(LoomError::DuplicateModuleId(id));
        }
        if id > ModuleId::MAX {
            return Err(LoomError::ModuleIdOutOfRange(id));
        }
        // A patch has exactly one Transport (clock.md).
        if descriptor.id == crate::transport::TRANSPORT_MODULE
            && let Some(existing) = self.transport_module()
        {
            return Err(LoomError::SecondTransport(existing));
        }
        self.collect_garbage();
        let slot = match descriptor.role {
            ModuleRole::Dsp(factory) => {
                let slot = self
                    .slots
                    .take()
                    .ok_or(LoomError::TooManyModules(self.config.max_modules))?;
                let mut module = factory(&ModuleConfig {
                    sample_rate: self.config.sample_rate,
                    max_block: self.config.max_block,
                });
                for (index, param) in descriptor.params.iter().enumerate() {
                    module.set_param(index, param.default);
                }
                // Held until the commit, so the values set before it are
                // the module's first values, not a glide target.
                let fresh = Fresh {
                    module,
                    id: id.0,
                    displays: descriptor.displays.len() as u8,
                    triggers: descriptor
                        .displays
                        .iter()
                        .enumerate()
                        .filter(|(_, d)| d.kind == crate::descriptor::DisplayKind::Trigger)
                        .fold(0, |mask, (i, _)| mask | (1 << i)),
                };
                self.slots.hold(slot, fresh);
                Some(slot)
            }
            ModuleRole::UnitDelay | ModuleRole::AudioOutput => None,
        };
        self.graph.nodes.insert(id, Node::new(descriptor, slot));
        self.next_id = self.next_id.max(id.0 + 1);
        self.dirty = true;
        Ok(())
    }

    /// Removes a module, every cable touching it, every modulation assignment
    /// it is part of, and every probe on it.
    ///
    /// Refuses the Transport module: every patch has one (clock.md).
    pub fn remove_module(&mut self, id: ModuleId) -> Result<(), LoomError> {
        if self.transport_module() == Some(id) {
            return Err(LoomError::TransportCannotBeRemoved(id));
        }
        let node = self.graph.nodes.remove(&id).ok_or(LoomError::UnknownModule(id))?;
        self.graph
            .cables
            .retain(|to, from| to.module != id && from.module != id);
        self.graph
            .assignments
            .retain(|_, assignment| assignment.source.module != id && assignment.target != id);
        self.probes.remove_on(id);
        self.graph.midi_mappings.retain(|_, mapping| mapping.target != id);
        if let Some(slot) = node.slot {
            self.slots.release(slot);
        }
        if let Some(copy) = node.copy_slot {
            self.slots.retire(copy);
        }
        self.dirty = true;
        Ok(())
    }

    /// How many more modules that run DSP the engine can take now. Undo
    /// checks this before it puts removed modules back. A removed module's
    /// slot counts only after the commit that follows its removal.
    pub fn free_module_slots(&self) -> usize {
        self.slots.free_count()
    }
}

/// The engine's module slots, as the controller sees them. Each slot holds
/// at most one module instance. A slot is free, held by a module added
/// since the last commit, used by the running schedule, or retiring: still
/// used by the running schedule, and freed once the next schedule swaps in.
pub(super) struct Slots {
    /// Free slots, taken from the end.
    free: Vec<usize>,
    /// Modules added since the last commit, by slot. They stay here, where
    /// their first values are set directly, until `commit` resets them and
    /// sends them to the engine (live patching spec, rule 7).
    fresh: BTreeMap<usize, Fresh>,
    /// Slots to free after the next swap, in the order they were retired.
    retiring: Vec<usize>,
}

impl Slots {
    /// `count` free slots, numbered from 0.
    pub(super) fn new(count: usize) -> Self {
        Self {
            free: (0..count).rev().collect(),
            fresh: BTreeMap::new(),
            retiring: Vec::new(),
        }
    }

    /// How many slots are free now. A retiring slot counts only after the
    /// swap that frees it.
    pub(super) fn free_count(&self) -> usize {
        self.free.len()
    }

    /// Takes a free slot.
    pub(super) fn take(&mut self) -> Option<usize> {
        self.free.pop()
    }

    /// Holds a new module in a slot just taken, until the next commit.
    pub(super) fn hold(&mut self, slot: usize, fresh: Fresh) {
        self.fresh.insert(slot, fresh);
    }

    /// The module held in `slot`, if it has not been sent to the engine yet.
    pub(super) fn fresh_mut(&mut self, slot: usize) -> Option<&mut Fresh> {
        self.fresh.get_mut(&slot)
    }

    /// Gives back the slot of a removed module. A module never sent frees
    /// its slot at once: the engine has nothing to remove. One in the
    /// running schedule retires until the next swap.
    pub(super) fn release(&mut self, slot: usize) {
        if self.fresh.remove(&slot).is_some() {
            self.free.push(slot);
        } else {
            self.retire(slot);
        }
    }

    /// Frees `slot` after the next swap: the running schedule still uses it.
    pub(super) fn retire(&mut self, slot: usize) {
        self.retiring.push(slot);
    }

    /// How many new modules wait to be sent.
    pub(super) fn waiting(&self) -> usize {
        self.fresh.len()
    }

    /// The slots that retire at the next swap.
    pub(super) fn retiring(&self) -> &[usize] {
        &self.retiring
    }

    /// Takes up to `room` of the new modules, lowest slot first, to send.
    /// The rest stay held for the next commit.
    pub(super) fn send_fresh(&mut self, room: usize) -> Vec<(usize, Fresh)> {
        let sending: Vec<usize> = self.fresh.keys().copied().take(room).collect();
        sending
            .into_iter()
            .filter_map(|slot| self.fresh.remove(&slot).map(|fresh| (slot, fresh)))
            .collect()
    }

    /// Frees the retiring slots, once the swap that stops using them is sent.
    pub(super) fn free_retired(&mut self) {
        self.free.append(&mut self.retiring);
    }
}

#[cfg(test)]
mod tests;

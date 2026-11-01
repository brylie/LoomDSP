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
                    .free_slots
                    .pop()
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
                self.fresh.insert(slot, fresh);
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
        self.probes.retain(|_, (from, _)| from.module != id);
        self.graph.midi_mappings.retain(|_, mapping| mapping.target != id);
        if let Some(slot) = node.slot {
            if self.fresh.remove(&slot).is_some() {
                // Never sent: the engine has nothing to remove.
                self.free_slots.push(slot);
            } else {
                // The running schedule still uses this slot. The instance
                // is removed after the next schedule swap (see `commit`).
                self.pending_removals.push(slot);
            }
        }
        self.pending_removals.extend(node.copy_slot);
        self.dirty = true;
        Ok(())
    }

    /// How many more modules that run DSP the engine can take now. Undo
    /// checks this before it puts removed modules back. A removed module's
    /// slot counts only after the commit that follows its removal.
    pub fn free_module_slots(&self) -> usize {
        self.free_slots.len()
    }
}

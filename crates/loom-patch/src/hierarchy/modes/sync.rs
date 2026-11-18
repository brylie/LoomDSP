//! Reconciles implicit mode modules and their links with the engine.

use super::*;

impl Hierarchy {
    /// Keeps what the flattener said about copies and added modules.
    pub(in crate::hierarchy) fn note_flat(&mut self, flat: &Flat) {
        self.copies = flat.copies.clone();
        self.added = flat.added.clone();
    }

    /// Makes the controller's implied modules the ones `flat` has: those
    /// gone are removed, new ones added at their values and given their
    /// paths, and the values of the others set to the flattener's. Runs
    /// before the cables are settled, so every cable has both ends.
    pub(in crate::hierarchy) fn sync_implied_modules(
        &mut self,
        controller: &mut Controller,
        flat: &Flat,
        scratch: &FlatIds,
    ) -> Result<(), PatchError> {
        let desired: BTreeMap<ModuleId, &crate::FlatModule> = flat
            .modules
            .iter()
            .filter(|m| m.path.is_implied())
            .map(|m| (m.id, m))
            .collect();
        // A leaving part or a fade of a stack copy sits at an implied path,
        // but the swap made it, not the flattener (ADR-0044).
        let fading = self.fading_ids();
        let current: Vec<ModuleId> = controller
            .graph()
            .nodes()
            .map(|(id, _)| id)
            .filter(|id| self.is_implied_module(*id) && !fading.contains(id))
            .collect();
        for id in current.iter().filter(|id| !desired.contains_key(id)) {
            controller.remove_module(*id)?;
            if let Some(path) = self.ids.path_of(*id).cloned() {
                self.ids.forget(&path);
            }
        }
        for (id, module) in &desired {
            if controller.graph().node(*id).is_none() {
                if let Some(path) = scratch.path_of(*id) {
                    self.ids.reserve(path.clone(), *id)?;
                }
                module.module.add_to(controller, *id)?;
                continue;
            }
            // Ranges first, as a load sets them, so a range never clamps a
            // value; spread works in the control's range, so copies must share
            // copy 0's.
            let ranges: Vec<(usize, loom_core::ParamRange)> = {
                let node = controller.graph().node(*id).expect("checked above");
                module
                    .module
                    .ranges
                    .iter()
                    .enumerate()
                    .filter_map(|(index, range)| range.filter(|r| node.range(index) != Some(*r)).map(|r| (index, r)))
                    .collect()
            };
            for (index, range) in ranges {
                controller.set_range(*id, index, range)?;
            }
            let changes: Vec<(usize, f32)> = {
                let node = controller.graph().node(*id).expect("checked above");
                module
                    .module
                    .params
                    .iter()
                    .enumerate()
                    .filter(|(index, value)| {
                        !module.module.descriptor.params[*index].is_transient()
                            && node.params.get(*index) != Some(value)
                    })
                    .map(|(index, value)| (index, *value))
                    .collect()
            };
            for (index, value) in changes {
                controller.set_param(*id, index, value)?;
            }
        }
        Ok(())
    }

    /// Makes the controller's implied assignments (those with an implied
    /// source or target, other than onto a select's Member) the ones `flat`
    /// has, keeping the id of each that stays, and sets the followers of
    /// each mapping (ADR-0040).
    pub(in crate::hierarchy) fn sync_implied_links(
        &mut self,
        controller: &mut Controller,
        flat: &Flat,
        through_fade: &BTreeMap<End, End>,
    ) -> Result<(), PatchError> {
        // An assignment onto a fading module keeps going until the fade
        // ends: a leaving copy keeps its spread (ADR-0044).
        let fading = self.fading_ids();
        let is_implied = |hierarchy: &Self, end: u32| {
            let id = ModuleId(end);
            hierarchy.is_implied_module(id) && !matches!(hierarchy.added.get(&id), Some(Added::Member { .. }))
        };
        let graph = controller.graph();
        let mut current: BTreeMap<AssignmentEnds, Vec<AssignmentId>> = BTreeMap::new();
        for (id, a) in graph.assignments() {
            let source = (a.source.module.0, crate::port_name(graph, a.source));
            let target = (a.target.0, crate::param_name(graph, a.target, a.param));
            if fading.contains(&a.target) {
                continue;
            }
            if is_implied(self, source.0) || is_implied(self, target.0) {
                current.entry((source, target)).or_default().push(id);
            }
        }
        let mut wanted: Vec<PatchModulation> = Vec::new();
        for entry in &flat.modulation {
            if is_implied(self, entry.source.0) || is_implied(self, entry.target.0) {
                let mut entry = entry.clone();
                if let Some(fade_out) = through_fade.get(&entry.source) {
                    entry.source = fade_out.clone();
                }
                wanted.push(entry);
            }
        }
        let mut add = Vec::new();
        for entry in wanted {
            let ends = (entry.source.clone(), entry.target.clone());
            match current
                .get_mut(&ends)
                .and_then(|ids| (!ids.is_empty()).then(|| ids.remove(0)))
            {
                Some(id) => {
                    let assignment = *controller.graph().assignment(id).expect("listed above");
                    if assignment.depth != entry.depth {
                        controller.set_assignment_depth(id, entry.depth)?;
                    }
                    let polarity = entry.polarity.map(Into::into);
                    if assignment.polarity != polarity {
                        controller.set_assignment_polarity(id, polarity)?;
                    }
                    if assignment.enabled != entry.enabled {
                        controller.set_assignment_enabled(id, entry.enabled)?;
                    }
                }
                None => add.push(entry),
            }
        }
        for ids in current.into_values() {
            for id in ids {
                controller.remove_assignment(id)?;
            }
        }
        for entry in add {
            let id = controller.add_assignment_named(
                ModuleId(entry.source.0),
                &entry.source.1,
                ModuleId(entry.target.0),
                &entry.target.1,
                entry.depth,
            )?;
            if let Some(polarity) = entry.polarity {
                controller.set_assignment_polarity(id, Some(polarity.into()))?;
            }
            if !entry.enabled {
                controller.set_assignment_enabled(id, false)?;
            }
        }
        controller.set_midi_followers(flat.followers_in(controller.graph()))?;
        Ok(())
    }
}

//! Preserves links and mappings when a slot changes its part.

use super::*;

impl Hierarchy {
    /// Removes every assignment onto or from a module under `part`, and
    /// every MIDI mapping onto one, returning their labels as a swap's
    /// status names them.
    pub(in crate::hierarchy::slots) fn drop_links_under(
        &self,
        controller: &mut Controller,
        part: &ModulePath,
    ) -> Vec<String> {
        let graph = controller.graph();
        let under = |id: ModuleId| self.ids.path_of(id).is_some_and(|p| p.starts_with(part));
        let mut labels = Vec::new();
        let mut drops = Vec::new();
        for (aid, a) in graph.assignments() {
            // An implied assignment, such as a stack's spread, is the
            // flattener's, not the player's: it is not named, and `sync`
            // takes it away with the part (ADR-0044).
            if self.is_implied_module(a.source.module) || self.is_implied_module(a.target) {
                continue;
            }
            if under(a.target) {
                let param = crate::param_name(graph, a.target, a.param);
                labels.push(param_label(graph, a.target, &param));
                drops.push(aid);
            } else if under(a.source.module) {
                labels.push(format!("{} (a modulation source)", crate::port_name(graph, a.source)));
                drops.push(aid);
            }
        }
        let unmaps: Vec<(ModuleId, usize, String)> = graph
            .midi_mappings()
            .filter(|(_, m)| under(m.target))
            .map(|(_, m)| {
                let param = crate::param_name(graph, m.target, m.param);
                (m.target, m.param, param_label(graph, m.target, &param))
            })
            .collect();
        for aid in drops {
            let _ = controller.remove_assignment(aid);
        }
        for (target, param, label) in unmaps {
            let _ = controller.unmap_midi(target, param);
            labels.push(label);
        }
        labels
    }

    /// The flat parameter behind the role parameter `param` of a sub-patch
    /// part at `part_path`, by exposed name.
    pub(in crate::hierarchy::slots) fn slot_param_target_for_part(
        &self,
        part_path: &ModulePath,
        param: &str,
    ) -> Option<(ModuleId, String)> {
        let definition = self.definition_at(part_path)?;
        let inner = definition.exposed.iter().find(|e| e.name == param)?;
        self.resolve_exposed(part_path, inner)
    }

    /// Moves the assignments and mappings on the leaving part `old` to the
    /// new part `new` by role name, removes those on parameters outside
    /// the role (returning their labels), and adds one swap fade per role
    /// output both parts have. Returns the fades' ids, the labels, and the
    /// fades as `(role output, flat id)`.
    #[allow(clippy::type_complexity)]
    pub(in crate::hierarchy::slots) fn move_links_to_new_part(
        &mut self,
        controller: &mut Controller,
        slot_path: &ModulePath,
        old_state: &SlotState,
        old: ModuleId,
        new: ModuleId,
    ) -> Result<(Vec<ModuleId>, Vec<String>, Vec<(String, ModuleId)>), String> {
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let new_state = self.slots.get(slot_path).cloned().expect("the slot");
        let graph = controller.graph();
        let old_path = self.path_of(old);
        let new_path = self.path_of(new);
        // Targets under the leaving part: its own parameters, or anything
        // inside a sub-patch part.
        let under_old = |id: ModuleId| self.path_of(id).starts_with(&old_path);
        let mut removed = Vec::new();
        let mut retargets = Vec::new();
        let mut drops = Vec::new();
        for (aid, a) in graph.assignments() {
            let target_under = under_old(a.target);
            let source_under = under_old(a.source.module);
            if !target_under && !source_under {
                continue;
            }
            // An implied assignment, such as a stack's spread, is the
            // flattener's: the new part gets its own from `sync`, and the
            // leaving part keeps this one until the fade ends (ADR-0044).
            if self.is_implied_module(a.source.module) || self.is_implied_module(a.target) {
                continue;
            }
            let mut source = a.source;
            let mut target = (a.target, a.param);
            let mut keep = true;
            if target_under {
                let param = crate::param_name(graph, a.target, a.param);
                // A role parameter the new part lacks counts as one only
                // the leaving part had.
                let behind = self
                    .role_name_of_param(graph, &level, old_state, a.target, &param)
                    .and_then(|role_name| self.slot_param_target(slot_path, &role_name))
                    .and_then(|(id, own)| graph.node(id).and_then(|n| n.parameter_index(&own)).map(|i| (id, i)));
                match behind {
                    Some(found) => target = found,
                    None => {
                        keep = false;
                        removed.push(param_label(graph, a.target, &param));
                    }
                }
            }
            if source_under && keep {
                let output = crate::port_name(graph, a.source);
                match self.role_name_of_output(graph, &level, old_state, a.source.module, &output) {
                    Some(role_name) => match self.part_output_of_state(graph, &level, &new_state, &role_name) {
                        Some((id, own)) => match graph.node(id).and_then(|n| n.output_index(&own)) {
                            Some(port) => source = OutputRef { module: id, port },
                            None => keep = false,
                        },
                        None => keep = false,
                    },
                    None => {
                        keep = false;
                        removed.push(format!("{output} (a modulation source)"));
                    }
                }
            }
            if keep {
                retargets.push((aid, source, target));
            } else {
                drops.push(aid);
            }
        }
        let mut remaps = Vec::new();
        let mut unmaps = Vec::new();
        for (_, mapping) in graph.midi_mappings() {
            if !under_old(mapping.target) {
                continue;
            }
            let param = crate::param_name(graph, mapping.target, mapping.param);
            let behind = self
                .role_name_of_param(graph, &level, old_state, mapping.target, &param)
                .and_then(|role_name| self.slot_param_target(slot_path, &role_name))
                .and_then(|(id, own)| graph.node(id).and_then(|n| n.parameter_index(&own)).map(|i| (id, i)));
            match behind {
                Some((id, index)) => {
                    remaps.push((
                        mapping.target,
                        mapping.param,
                        mapping.control,
                        id,
                        index,
                        mapping.options,
                    ));
                }
                None => {
                    unmaps.push((mapping.target, mapping.param));
                    removed.push(param_label(graph, mapping.target, &param));
                }
            }
        }
        // Fades: one per role output both parts have, unless its signal
        // switches rather than fades (sub-patches.md, How a swap crossfades,
        // point 6).
        let mut fade_outputs = Vec::new();
        for output in new_state.role.outputs {
            let old_has = self
                .part_output_of_state(graph, &level, old_state, output.name)
                .is_some();
            let new_has = self
                .part_output_of_state(graph, &level, &new_state, output.name)
                .is_some();
            if old_has && new_has && fades(output) {
                fade_outputs.push(output.name.to_string());
            }
        }
        let _ = new_path;
        // The new part's output behind each faded role output, so a
        // modulation from the slot's output goes through its fade while the
        // slot fades, and a Modulator or Envelope slot swaps click-free as
        // a source too; `finish_fade` points it at the new part.
        let new_outputs: Vec<(String, OutputRef)> = fade_outputs
            .iter()
            .filter_map(|role_out| {
                let (id, own) = self.part_output_of_state(graph, &level, &new_state, role_out)?;
                let port = graph.node(id)?.output_index(&own)?;
                Some((role_out.clone(), OutputRef { module: id, port }))
            })
            .collect();
        let with_laws: Vec<(String, Law)> = fade_outputs
            .into_iter()
            .filter_map(|role_out| {
                let law = new_state
                    .role
                    .outputs
                    .iter()
                    .find(|o| o.name == role_out)
                    .and_then(fade_law)?;
                Some((role_out, law))
            })
            .collect();
        let fades_made = self.add_fades(controller, slot_path, with_laws)?;
        let added: Vec<ModuleId> = fades_made.iter().map(|(_, id)| *id).collect();
        let through_fade: Vec<(OutputRef, ModuleId)> = fades_made
            .iter()
            .filter_map(|(role_out, fade)| {
                let (_, output) = new_outputs.iter().find(|(name, _)| name == role_out)?;
                Some((*output, *fade))
            })
            .collect();

        for (aid, mut source, (target, param)) in retargets {
            if let Some((_, fade)) = through_fade.iter().find(|(output, _)| *output == source) {
                source = OutputRef {
                    module: *fade,
                    port: FADE_OUT,
                };
            }
            controller
                .retarget_assignment(aid, source, target, param)
                .map_err(|e| e.to_string())?;
        }
        for aid in drops {
            let _ = controller.remove_assignment(aid);
        }
        for (target, param) in unmaps {
            let _ = controller.unmap_midi(target, param);
        }
        for (old_target, old_param, control, target, param, options) in remaps {
            let _ = controller.unmap_midi(old_target, old_param);
            controller
                .map_midi_with(control, target, param, options)
                .map_err(|e| e.to_string())?;
        }
        Ok((added, removed, fades_made))
    }

    /// The role name of a parameter of the leaving part: `param` of the
    /// flat module `module`, which is the part itself or inside a sub-patch
    /// part.
    fn role_name_of_param(
        &self,
        graph: &Graph,
        level: &ModulePath,
        state: &SlotState,
        module: ModuleId,
        param: &str,
    ) -> Option<String> {
        let part_path = level.child(state.part?);
        if self.instances.contains_key(&part_path) {
            let name = self.exposed_name_for(&part_path, &(module, param.to_string()))?;
            return state.role.param(&name).map(|p| p.name.to_string());
        }
        let _ = graph;
        state.role_param_of(param).map(str::to_string)
    }

    /// The role name of an output of the leaving part.
    fn role_name_of_output(
        &self,
        graph: &Graph,
        level: &ModulePath,
        state: &SlotState,
        module: ModuleId,
        output: &str,
    ) -> Option<String> {
        let part_path = level.child(state.part?);
        if self.instances.contains_key(&part_path) {
            let port = self.port_for_output(graph, &part_path, &(module, output.to_string()))?;
            return state.role.output(&port).map(|p| p.name.to_string());
        }
        state.role_output_of(output).map(str::to_string)
    }

    /// The flat output behind a role output of the part `state` names.
    pub(in crate::hierarchy::slots) fn part_output_of_state(
        &self,
        graph: &Graph,
        level: &ModulePath,
        state: &SlotState,
        role_out: &str,
    ) -> Option<(ModuleId, String)> {
        let part = state.part?;
        let part_path = level.child(part);
        if self.instances.contains_key(&part_path) {
            return self.source_behind(graph, level, &(part, role_out.to_string()));
        }
        let own = state.own_output(role_out);
        let id = self.ids.get(&part_path)?;
        graph.node(id)?.output_index(&own)?;
        Some((id, own))
    }
}

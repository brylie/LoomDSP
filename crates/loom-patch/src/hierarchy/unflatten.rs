//! Writes the editor hierarchy back as a structured patch.

use super::*;

impl Hierarchy {
    /// The patch as it would be saved: every level's modules, ports,
    /// instances, cables, assignments, and mappings in the level's ids,
    /// with `layout` giving each tile's position and size by flat id.
    /// The patch's own metadata is left for the caller.
    pub fn unflatten(&self, graph: &Graph, layout: impl Fn(u32) -> Layout) -> Patch {
        self.unflatten_with(graph, layout, Some(&self.implied), &BTreeSet::new())
    }

    /// [`Hierarchy::unflatten`], with or without the graph's own cables. Without
    /// them, the file holds only what the links say, so flattening it gives
    /// exactly the cables the links imply.
    pub(super) fn unflatten_with(
        &self,
        graph: &Graph,
        layout: impl Fn(u32) -> Layout,
        implied: Option<&BTreeSet<(End, End)>>,
        skip: &BTreeSet<ModuleId>,
    ) -> Patch {
        let leaving = self.leaving_paths();
        let mut levels: BTreeMap<ModulePath, Level> = BTreeMap::new();
        levels.entry(ModulePath::root()).or_default();
        for path in self.instances.keys() {
            levels.entry(path.clone()).or_default();
        }
        // What exists only while a slot fades is never written.
        let mut skip = skip.clone();
        skip.extend(self.transient_ids());
        let skip = &skip;

        // Modules, with the definition-side values of exposed parameters.
        for (id, node) in graph.nodes().filter(|(id, _)| !skip.contains(id)) {
            let path = self.path_of(id);
            let level = path.parent().unwrap_or_else(ModulePath::root);
            let mut module = ResolvedModule::from_node(id, node, layout(id.0)).write();
            module.id = path.last();
            if let Some(definition) = self.definition_at(&level) {
                for ((member, param), stored) in &definition.defaults {
                    if *member == module.id {
                        apply_stored(&mut module, param, stored);
                    }
                }
                // Exposed through a slot: the stored value is the part's,
                // under the part's own name.
                let lid = module.id;
                for (slot_path, slot) in self
                    .slots
                    .iter()
                    .filter(|(p, s)| p.parent().as_ref() == Some(&level) && s.part == Some(lid))
                {
                    for ((member, param), stored) in &definition.defaults {
                        // An optional role parameter the part lacks has no
                        // place on it: the exposure stays on the slot with
                        // nothing behind it until a part that has it is in.
                        let own = slot.own_param(param);
                        if *member == slot_path.last() && node.parameter_index(&own).is_some() {
                            apply_stored(&mut module, &own, stored);
                        }
                    }
                }
            }
            levels.entry(level).or_default().modules.push(module);
        }
        // Slots, with their held parts.
        for (path, slot) in &self.slots {
            let Some(level) = path.parent() else { continue };
            let Layout { position, size } = self.ids.get(path).map_or_else(Layout::default, |id| layout(id.0));
            levels.entry(level).or_default().modules.push(PatchModule {
                id: path.last(),
                kind: SLOT_KIND.to_string(),
                subpatch: None,
                slot: Some(Slot {
                    role: slot.role.kind(),
                    part: slot.part,
                    held: slot.held.clone(),
                    values: if slot.part.is_none() {
                        slot.values.clone()
                    } else {
                        BTreeMap::new()
                    },
                }),
                label: slot.label.clone(),
                params: BTreeMap::new(),
                position,
                size,
                ranges: BTreeMap::new(),
            });
        }
        // Ports.
        for (path, port) in &self.ports {
            let Some(level) = path.parent() else { continue };
            if leaving.covers(path) {
                continue;
            }
            let Layout { position, size } = self.ids.get(path).map_or_else(Layout::default, |id| layout(id.0));
            levels.entry(level).or_default().modules.push(PatchModule {
                id: path.last(),
                kind: port.direction.kind().to_string(),
                subpatch: None,
                slot: None,
                label: Some(port.name.clone()),
                params: BTreeMap::new(),
                position,
                size,
                ranges: BTreeMap::new(),
            });
        }
        // Instances, with their exposed values. One that is leaving a slot
        // is not written; its definition is stored whole instead.
        for (path, instance) in &self.instances {
            let Some(level) = path.parent() else { continue };
            if leaving.covers(path) {
                continue;
            }
            let Layout { position, size } = self.ids.get(path).map_or_else(Layout::default, |id| layout(id.0));
            levels.entry(level.clone()).or_default().modules.push(PatchModule {
                id: path.last(),
                kind: SUBPATCH_KIND.to_string(),
                subpatch: Some(instance.key.clone()),
                slot: None,
                label: instance.label.clone(),
                params: self.instance_values(graph, path),
                position,
                size,
                ranges: BTreeMap::new(),
            });
        }
        // Cables: the graph's between two modules of one level, unless a
        // link implies them, then the links.
        for (from, to) in graph.cables().filter(|_| implied.is_some()) {
            if skip.contains(&from.module) || skip.contains(&to.module) {
                continue;
            }
            let (from_end, to_end) = cable_ends(graph, from, to);
            if implied.is_some_and(|implied| implied.contains(&(from_end.clone(), to_end.clone()))) {
                continue;
            }
            let (from_path, to_path) = (self.path_of(from.module), self.path_of(to.module));
            if from_path.parent() != to_path.parent() {
                // A cable across a boundary that no link made: the file has
                // no place for it (rule 4).
                continue;
            }
            let level = from_path.parent().unwrap_or_else(ModulePath::root);
            // A link into the same input wins (invariant 2): the graph's
            // cable is the one a sync replaces.
            if self.link_into(&level, &(to_path.last(), to_end.1.clone())).is_some() {
                continue;
            }
            levels.entry(level).or_default().cables.push(PatchCable {
                from: (from_path.last(), from_end.1),
                to: (to_path.last(), to_end.1),
            });
        }
        for link in &self.links {
            if leaving.covers(&link.level.child(0)) && !link.level.is_root() {
                continue;
            }
            levels.entry(link.level.clone()).or_default().cables.push(PatchCable {
                from: link.from.clone(),
                to: link.to.clone(),
            });
        }
        // Assignments: within a level as they are; across levels through
        // the exposed name or the output port they cross.
        // One from a swap fade is written from the new part's output behind
        // it, which is what finishing the fade makes it.
        for (_, assignment) in graph.assignments() {
            let mut assignment = *assignment;
            if let Some(source) = self.output_behind_fade(graph, assignment.source) {
                assignment.source = source;
            }
            // Onto a select's Member: written on the instance, by name.
            let member_target = matches!(self.added.get(&assignment.target), Some(Added::Member { .. }));
            if skip.contains(&assignment.target) && !member_target {
                continue;
            }
            // From a mode's sum or select: written from the instance's port.
            let entry = match self.added.get(&assignment.source.module) {
                Some(Added::Output { instance, port }) => {
                    self.assignment_entry_at(graph, instance.clone(), port.clone(), &assignment)
                }
                _ if skip.contains(&assignment.source.module) => continue,
                _ => self.assignment_entry(graph, &assignment),
            };
            let Some(entry) = entry else {
                continue;
            };
            levels.entry(entry.0).or_default().modulation.push(entry.1);
        }
        // Mappings live with their module; on a slot's part, through the
        // slot by the role's name.
        for (_, mapping) in graph.midi_mappings() {
            // A mapping on a select's Member is written on the instance.
            if let Some(Added::Member { instance }) = self.added.get(&mapping.target) {
                let Some(level) = instance.parent() else { continue };
                let target = (instance.last(), MEMBER_PARAM.to_string());
                levels
                    .entry(level)
                    .or_default()
                    .midi
                    .push(crate::midi_entry(mapping, target));
                continue;
            }
            if skip.contains(&mapping.target) {
                continue;
            }
            let path = self.path_of(mapping.target);
            let level = path.parent().unwrap_or_else(ModulePath::root);
            let param = crate::param_name(graph, mapping.target, mapping.param);
            let target = self.slot_end_for_param(&level, path.last(), &param);
            levels
                .entry(level)
                .or_default()
                .midi
                .push(crate::midi_entry(mapping, target));
        }

        let mut patch = Patch::new("");
        let top = levels.remove(&ModulePath::root()).unwrap_or_default();
        patch.modules = top.modules;
        patch.cables = top.cables;
        patch.modulation = top.modulation;
        patch.midi = top.midi;
        for (path, instance) in &self.instances {
            if leaving.covers(path) {
                continue;
            }
            let Some(definition) = self.definitions.get(&instance.key) else {
                continue;
            };
            let level = levels.remove(path).unwrap_or_default();
            let mut written = SubPatch {
                metadata: definition.metadata.clone(),
                modules: level.modules,
                cables: level.cables,
                modulation: level.modulation,
                midi: level.midi,
                exposed: definition
                    .exposed
                    .iter()
                    .filter(|e| !Self::is_member_entry(e))
                    .cloned()
                    .collect(),
                library: definition.library.clone(),
                roles: definition.roles.clone(),
                ..SubPatch::new("")
            };
            definition.modes.write(&mut written);
            patch.subpatches.insert(instance.key.clone(), written);
        }
        // Definitions a slot holds but does not play, with what nests in
        // them, rekeyed where a live definition took the key since.
        for (key, definition) in &self.definitions {
            let Some(stored) = &definition.stored else { continue };
            if patch.subpatches.contains_key(key) {
                continue;
            }
            let mut root = stored.definition.clone();
            let mut nested = stored.nested.clone();
            crate::library::rekey(&mut root, &mut nested, |candidate| {
                candidate == key || patch.subpatches.contains_key(candidate)
            });
            patch.subpatches.insert(key.clone(), root);
            for (nested_key, nested) in nested {
                patch.subpatches.entry(nested_key).or_insert(nested);
            }
        }
        // The top level's surface, less what no longer stands behind it: an
        // entry names a top-level id, which is a flat id only while the
        // module is still at the top (collapsing keeps its id inside).
        patch.exposed = self
            .top_exposed
            .iter()
            .filter(|exposed| {
                let member = ModulePath::top(exposed.module);
                if let Some(definition) = self.definition_at(&member) {
                    return definition.exposed.iter().any(|e| e.name == exposed.param);
                }
                if let Some(slot) = self.slots.get(&member) {
                    return slot.role.param(&exposed.param).is_some();
                }
                let id = ModuleId(exposed.module);
                !skip.contains(&id)
                    && self.path_of(id) == member
                    && graph.node(id).is_some_and(|node| match &node.unknown {
                        Some(unknown) => unknown.params.contains_key(&exposed.param),
                        None => node.parameter_index(&exposed.param).is_some(),
                    })
            })
            .cloned()
            .collect();
        patch.canonicalise();
        patch
    }

    /// The `params` of the instance at `path`: the live value of each
    /// exposed parameter the outer level does not expose again, else the
    /// stored definition-side value.
    pub fn instance_values(&self, graph: &Graph, path: &ModulePath) -> BTreeMap<String, f32> {
        let mut values = BTreeMap::new();
        let Some(definition) = self.definition_at(path) else {
            return values;
        };
        let outer = path.parent().and_then(|level| self.definition_at(&level));
        for exposed in &definition.exposed {
            let stored = outer.and_then(|outer| outer.defaults.get(&(path.last(), exposed.name.clone())));
            let value = match stored {
                Some(stored) => stored.value,
                None => self.exposed_value(graph, path, exposed),
            };
            if let Some(value) = value {
                values.insert(exposed.name.clone(), value);
            }
        }
        // A select's Member value, from its Member module.
        if definition.modes.mode == Mode::Select
            && let Some(value) = self.member_value(graph, path)
        {
            values.insert(MEMBER_PARAM.to_string(), value);
        }
        values
    }

    /// An assignment as its level writes it, or `None` when it crosses a
    /// boundary that no exposed parameter or port explains.
    fn assignment_entry(
        &self,
        graph: &Graph,
        assignment: &loom_core::Assignment,
    ) -> Option<(ModulePath, PatchModulation)> {
        let source_path = self.path_of(assignment.source.module);
        let output = crate::port_name(graph, assignment.source);
        self.assignment_entry_at(graph, source_path, output, assignment)
    }

    /// [`Hierarchy::assignment_entry`] with the source given as a tile's
    /// path and its output's name: a mode's added output stands for the
    /// instance's port (sub-patches.md, The modules the flattener adds).
    fn assignment_entry_at(
        &self,
        graph: &Graph,
        source_path: ModulePath,
        output: String,
        assignment: &loom_core::Assignment,
    ) -> Option<(ModulePath, PatchModulation)> {
        let target_path = self.path_of(assignment.target);
        let source_level = source_path.parent().unwrap_or_else(ModulePath::root);
        let target_level = target_path.parent().unwrap_or_else(ModulePath::root);
        let param = crate::param_name(graph, assignment.target, assignment.param);
        let (level, source, target) = if source_level == target_level {
            // On a slot's part, by the role's name through the slot.
            let source = self.slot_end_for_output(&source_level, source_path.last(), &output);
            let target = self.slot_end_for_param(&target_level, target_path.last(), &param);
            (source_level, source, target)
        } else if target_level.starts_with(&source_level) {
            // The target is inside an instance of the source's level.
            let instance = target_level.prefix(source_level.0.len() + 1);
            let name = self.exposed_name_for(&instance, &(assignment.target, param))?;
            let source = self.slot_end_for_output(&source_level, source_path.last(), &output);
            let target = self.slot_end_for_param(&source_level, instance.last(), &name);
            (source_level, source, target)
        } else if source_level.starts_with(&target_level) {
            // The source is inside an instance of the target's level.
            let instance = source_level.prefix(target_level.0.len() + 1);
            let port = self.port_for_output(graph, &instance, &(assignment.source.module, output))?;
            let target = self.slot_end_for_param(&target_level, target_path.last(), &param);
            let source = self.slot_end_for_output(&target_level, instance.last(), &port);
            (target_level, source, target)
        } else {
            return None;
        };
        Some((
            level,
            PatchModulation {
                source,
                target,
                depth: assignment.depth,
                polarity: assignment.polarity.map(PatchPolarity::from),
                enabled: assignment.enabled,
            },
        ))
    }

    /// The flat cables of `patch` that are cables between two modules of
    /// one level, in flat ids and names.
    pub(super) fn direct_cables(&self, patch: &Patch) -> BTreeSet<(End, End)> {
        let mut direct = BTreeSet::new();
        let mut levels: Vec<(ModulePath, &[PatchModule], &[PatchCable])> =
            vec![(ModulePath::root(), &patch.modules, &patch.cables)];
        for (path, instance) in &self.instances {
            if let Some(definition) = patch.subpatches.get(&instance.key) {
                levels.push((path.clone(), &definition.modules, &definition.cables));
            }
        }
        for (level, modules, cables) in levels {
            let structural: BTreeSet<u32> = modules
                .iter()
                .filter(|m| {
                    m.is_instance()
                        || m.subpatch.is_some()
                        || m.is_slot()
                        || (!level.is_root() && (m.kind == INPUT_KIND || m.kind == OUTPUT_KIND))
                })
                .map(|m| m.id)
                .collect();
            for cable in cables {
                if structural.contains(&cable.from.0) || structural.contains(&cable.to.0) {
                    continue;
                }
                if let (Some(from), Some(to)) = (
                    self.ids.get(&level.child(cable.from.0)),
                    self.ids.get(&level.child(cable.to.0)),
                ) {
                    direct.insert(((from.0, cable.from.1.clone()), (to.0, cable.to.1.clone())));
                }
            }
        }
        direct
    }
}

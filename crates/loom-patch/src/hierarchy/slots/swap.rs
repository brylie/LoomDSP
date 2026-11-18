//! Replaces a slot part and preserves the role values between swaps.

use super::*;

impl Hierarchy {
    /// Ends a fade on the slot at `slot_path` in the record: the leaving
    /// part and the fades are forgotten here, and their flat ids returned
    /// for the controller to remove.
    pub(in crate::hierarchy) fn end_fade(&mut self, slot_path: &ModulePath) -> Vec<ModuleId> {
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let Some(fading) = self.slots.get_mut(slot_path).and_then(|s| s.fading.take()) else {
            return Vec::new();
        };
        let mut removed: Vec<ModuleId> = fading.fades.iter().map(|(_, id)| *id).collect();
        // Every other stack copy's fades and leaving part, with what is
        // under it, go too, and their paths are forgotten (ADR-0044).
        for copy in &fading.copies {
            let mut gone: Vec<ModulePath> = copy
                .fades
                .iter()
                .filter_map(|(_, id)| self.ids.path_of(*id).cloned())
                .collect();
            if let Some(lid) = fading.leaving {
                let leaving = copy.level.child(lid);
                gone.extend(
                    self.ids
                        .paths()
                        .filter(|(p, _)| p.starts_with(&leaving))
                        .map(|(p, _)| p.clone()),
                );
            }
            for path in gone {
                if let Some(id) = self.ids.get(&path) {
                    removed.push(id);
                }
                self.ids.forget(&path);
            }
        }
        let Some(leaving) = fading.leaving.map(|lid| level.child(lid)) else {
            // A part faded in from the thru: only the fades go.
            return removed;
        };
        if self.instances.contains_key(&leaving) {
            // A leaving sub-patch part: everything under it goes.
            let under: Vec<ModulePath> = self
                .ids
                .paths()
                .filter(|(p, _)| p.starts_with(&leaving))
                .map(|(p, _)| p.clone())
                .collect();
            for path in under {
                if let Some(instance) = self.instances.remove(&path) {
                    // The held definition stays, stored whole; nested ones
                    // live inside it.
                    if self.definitions.get(&instance.key).is_some_and(|d| d.stored.is_none()) {
                        self.definitions.remove(&instance.key);
                    }
                } else if self.ports.remove(&path).is_none()
                    && let Some(id) = self.ids.get(&path)
                {
                    removed.push(id);
                }
            }
            self.links.retain(|link| !link.level.starts_with(&leaving));
        } else if let Some(id) = self.ids.get(&leaving) {
            removed.push(id);
        }
        removed
    }

    /// Finishes the fade on the slot tile `slot`: the leaving part and the
    /// fades are removed from the controller. The caller syncs and
    /// commits. Nothing happens on a slot that is not fading.
    pub fn finish_fade(&mut self, controller: &mut Controller, slot: ModuleId) -> Vec<ModuleId> {
        let Some(slot_path) = self.ids.path_of(slot).cloned() else {
            return Vec::new();
        };
        self.retarget_fade_sources(controller, &slot_path);
        let removed = self.end_fade(&slot_path);
        for id in &removed {
            let _ = controller.remove_module(*id);
        }
        removed
    }

    /// The new part's output behind a swap fade's `out`, when `source` is
    /// one: what an assignment from the fade stands for once the fade is
    /// done, and what the file and an undo snapshot write for it
    /// (sub-patches.md, How a swap crossfades, point 4). `None` when
    /// `source` is not a fade.
    pub fn output_behind_fade(&self, graph: &Graph, source: OutputRef) -> Option<OutputRef> {
        let fade_path = self.ids.path_of(source.module)?;
        let slot_path = fade_path.parent()?;
        let state = self.slots.get(&slot_path)?;
        let fading = state.fading.as_ref()?;
        let (role_out, _) = fading.fades.iter().find(|(_, fade)| *fade == source.module)?;
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let (id, own) = self.part_output_of_state(graph, &level, state, role_out)?;
        let port = graph.node(id)?.output_index(&own)?;
        Some(OutputRef { module: id, port })
    }

    /// Points every assignment whose source is one of the slot's fades at
    /// the new part's output behind that fade, before the fade goes.
    fn retarget_fade_sources(&self, controller: &mut Controller, slot_path: &ModulePath) {
        let graph = controller.graph();
        let moves: Vec<_> = graph
            .assignments()
            .filter(|(_, a)| self.ids.path_of(a.source.module).and_then(|p| p.parent()).as_ref() == Some(slot_path))
            .filter_map(|(aid, a)| Some((aid, self.output_behind_fade(graph, a.source)?, a.target, a.param)))
            .collect();
        for (aid, source, target, param) in moves {
            let _ = controller.retarget_assignment(aid, source, target, param);
        }
    }

    /// Finishes every fade in progress, as before an undo or a swap during
    /// a fade (sub-patches.md, How a swap crossfades, point 5).
    pub fn finish_all_fades(&mut self, controller: &mut Controller) -> Vec<ModuleId> {
        let fading: Vec<ModuleId> = self.fading_slots().into_iter().map(|(slot, _)| slot).collect();
        fading
            .into_iter()
            .flat_map(|slot| self.finish_fade(controller, slot))
            .collect()
    }

    /// The role values of the slot's current part, by role name: what
    /// carries across a swap.
    fn role_values(&self, graph: &Graph, slot_path: &ModulePath) -> BTreeMap<String, f32> {
        let mut values = BTreeMap::new();
        let Some(state) = self.slots.get(slot_path) else {
            return values;
        };
        // An empty slot carries the values the last part left with it.
        if state.part.is_none() {
            return state.values.clone();
        }
        for param in state.role.params {
            let Some((id, own)) = self.slot_param_target(slot_path, param.name) else {
                continue;
            };
            let Some(node) = graph.node(id) else { continue };
            let Some(index) = node.parameter_index(&own) else {
                continue;
            };
            values.insert(
                param.name.to_string(),
                node.descriptor.saved_values(&node.params)[index],
            );
        }
        values
    }

    /// Puts the current part of the slot at `slot_path` into its held
    /// parts: a module's values and ranges by its own names, a sub-patch's
    /// exposed values with its definition stored whole.
    fn hold_part(
        &mut self,
        graph: &Graph,
        registry: &loom_core::Registry,
        layout: &dyn Fn(u32) -> Layout,
        slot_path: &ModulePath,
    ) -> Option<PartKey> {
        let state = self.slots.get(slot_path)?;
        let level = slot_path.parent()?;
        let part_path = level.child(state.part?);
        let part_id = self.ids.get(&part_path)?;
        if let Some(instance) = self.instances.get(&part_path).cloned() {
            let params = self.instance_values(graph, &part_path);
            // `definition_file` writes the definition as a file's top level
            // with the nested definitions in its table; a file the
            // hierarchy wrote holds no patch-wide module, so nothing is
            // left out on the way back.
            let file = self.definition_file(graph, layout, part_id)?;
            let (definition, _) = SubPatch::from_patch(&file, registry, instance_library(self, &instance.key));
            let nested = file.subpatches.clone();
            let key = PartKey::SubPatch(instance.key.clone());
            let record = self.definitions.get_mut(&instance.key)?;
            record.stored = Some(Box::new(StoredPart { definition, nested }));
            let state = self.slots.get_mut(slot_path)?;
            state.held.insert(
                key.clone(),
                Held {
                    params,
                    ranges: BTreeMap::new(),
                },
            );
            return Some(key);
        }
        let node = graph.node(part_id)?;
        let values = node.descriptor.saved_values(&node.params);
        let mut held = Held::default();
        for (index, spec) in node.descriptor.params.iter().enumerate() {
            held.params.insert(spec.name.to_string(), values[index]);
            if let Some(range) = stored_range(node, index) {
                held.ranges.insert(spec.name.to_string(), range);
            }
        }
        let key = PartKey::Module(node.descriptor.kind());
        let state = self.slots.get_mut(slot_path)?;
        state.held.insert(key.clone(), held);
        Some(key)
    }

    /// Swaps the slot tile `slot` to the module type `kind`, which must
    /// declare the slot's role (sub-patches.md, Slots). Role values carry
    /// across by role name, clamped; the new part's other values come from
    /// its held entry, else its defaults; the leaving part goes into the
    /// held parts and fades out over one swap fade per role output both
    /// parts have. Assignments and mappings on role parameters move to the
    /// new part; those on other parameters of the leaving part are
    /// removed and their labels returned. A fade already in progress is
    /// finished first. `layout` gives tile layouts for a sub-patch part
    /// that is stored. The caller syncs and commits.
    pub fn swap_to_module(
        &mut self,
        controller: &mut Controller,
        layout: &dyn Fn(u32) -> Layout,
        slot: ModuleId,
        kind: &str,
    ) -> Result<Swapped, String> {
        self.swap_to_module_at(controller, layout, slot, kind, None)
    }

    /// [`Hierarchy::swap_to_module`], with the new part at the level id
    /// `at` when given: undo and redo put a part back at the id it had, so
    /// the step's assignments and mappings find it (sub-patches.md, Slots:
    /// undo of a swap fades too).
    pub fn swap_to_module_at(
        &mut self,
        controller: &mut Controller,
        layout: &dyn Fn(u32) -> Layout,
        slot: ModuleId,
        kind: &str,
        at: Option<u32>,
    ) -> Result<Swapped, String> {
        let slot_path = self
            .ids
            .path_of(slot)
            .cloned()
            .ok_or_else(|| "select a slot".to_string())?;
        let state = self
            .slots
            .get(&slot_path)
            .cloned()
            .ok_or_else(|| "select a slot".to_string())?;
        let descriptor = controller.registry().resolve(kind).map_err(|e| e.to_string())?;
        let Some(fills) = descriptor.fills(state.role.id) else {
            return Err(format!("{} does not fill {}", descriptor.name, state.role.name));
        };
        self.finish_fade(controller, slot);
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let old_values = self.role_values(controller.graph(), &slot_path);
        let old_part = state.part;
        let old_id = old_part.and_then(|lid| self.ids.get(&level.child(lid)));
        let held_key = PartKey::Module(descriptor.kind());
        let restore = self.slots.get(&slot_path).and_then(|s| s.held.get(&held_key).cloned());
        self.hold_part(controller.graph(), controller.registry(), layout, &slot_path);

        // The new part, at its held values, then the role values.
        let (new_path, new_id) = match at {
            Some(lid) => {
                let path = level.child(lid);
                let id = self.ids.id_of(&path).map_err(|e| e.to_string())?;
                (path, id)
            }
            None => self.new_module(&level).map_err(|e| e.to_string())?,
        };
        controller.add_module_with_id(new_id, kind).map_err(|e| e.to_string())?;
        let mut added = vec![new_id];
        if let Some(held) = &restore {
            for (name, &value) in &held.params {
                let _ = controller.set_param_named(new_id, name, value);
            }
            for (name, &(min, max)) in &held.ranges {
                if let Some(index) = descriptor.param_index(name) {
                    let _ = controller.set_range(new_id, index, ParamRange::new(min, max));
                }
            }
        }
        for (role_name, value) in &old_values {
            let own = fills.own_param(role_name);
            if descriptor.param_index(&own).is_some() {
                let _ = controller.set_param_named(new_id, &own, *value);
            }
        }
        {
            let record = self.slots.get_mut(&slot_path).expect("checked above");
            record.held.remove(&held_key);
            record.part = Some(new_path.last());
            record.part_fills = Some(fills);
            record.values.clear();
        }

        // Assignments and mappings move by role name.
        let mut removed = Vec::new();
        if let Some(old_id) = old_id {
            let (moved, mut gone, fades) =
                self.move_links_to_new_part(controller, &slot_path, &state, old_id, new_id)?;
            added.extend(moved);
            removed.append(&mut gone);
            let copies = self.add_copy_fades(controller, &slot_path, old_part, &fades)?;
            added.extend(copies.iter().flat_map(|c| c.fades.iter().map(|(_, id)| *id)));
            let record = self.slots.get_mut(&slot_path).expect("checked above");
            record.fading = Some(Fading {
                leaving: old_part,
                fades,
                copies,
                thru_source: None,
                thru_dests: Vec::new(),
            });
        } else if self.fades_with_thru(&slot_path) {
            // Into an empty thru slot: the part fades in from the thru
            // signal, in every stack copy (sub-patches.md, Empty slots).
            let outputs = self.thru_fade_outputs(controller.graph(), &slot_path, new_path.last());
            let fades = self.add_fades(controller, &slot_path, outputs)?;
            added.extend(fades.iter().map(|(_, id)| *id));
            let copies = self.add_copy_fades(controller, &slot_path, None, &fades)?;
            added.extend(copies.iter().flat_map(|c| c.fades.iter().map(|(_, id)| *id)));
            let record = self.slots.get_mut(&slot_path).expect("checked above");
            record.fading = Some(Fading {
                leaving: None,
                fades,
                copies,
                thru_source: None,
                thru_dests: Vec::new(),
            });
        }
        Ok(Swapped {
            part: new_id,
            part_name: descriptor.name.to_string(),
            added,
            removed,
        })
    }

    /// Swaps the slot tile `slot` to a sub-patch: the file `file` inserted
    /// as a copy (or linked to `library`), which must declare the role and
    /// fit it. See [`Hierarchy::swap_to_module`] for what carries across.
    pub fn swap_to_subpatch(
        &mut self,
        controller: &mut Controller,
        layout: &dyn Fn(u32) -> Layout,
        slot: ModuleId,
        file: &Patch,
        library: Option<&str>,
    ) -> Result<Swapped, String> {
        self.swap_to_subpatch_at(controller, layout, slot, file, library, None, None)
    }

    /// [`Hierarchy::swap_to_subpatch`], with the new instance at the level
    /// id `at` and its definition under `key` when given, as
    /// [`Hierarchy::swap_to_module_at`] does for undo.
    #[allow(clippy::too_many_arguments)]
    pub fn swap_to_subpatch_at(
        &mut self,
        controller: &mut Controller,
        layout: &dyn Fn(u32) -> Layout,
        slot: ModuleId,
        file: &Patch,
        library: Option<&str>,
        at: Option<u32>,
        key: Option<&str>,
    ) -> Result<Swapped, String> {
        let slot_path = self
            .ids
            .path_of(slot)
            .cloned()
            .ok_or_else(|| "select a slot".to_string())?;
        let state = self
            .slots
            .get(&slot_path)
            .cloned()
            .ok_or_else(|| "select a slot".to_string())?;
        let (definition, _) = SubPatch::from_patch(file, controller.registry(), library);
        if !definition.fills(state.role, controller.registry(), &file.subpatches) {
            let why: Vec<String> = definition
                .role_mismatches(state.role, controller.registry(), &file.subpatches)
                .iter()
                .map(ToString::to_string)
                .collect();
            let why = if why.is_empty() {
                "it does not declare the role".to_string()
            } else {
                why.join(", ")
            };
            return Err(format!(
                "{} does not fill {}: {why}",
                file.metadata.name, state.role.name
            ));
        }
        self.finish_fade(controller, slot);
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let old_values = self.role_values(controller.graph(), &slot_path);
        let old_part = state.part;
        let old_id = old_part.and_then(|lid| self.ids.get(&level.child(lid)));
        self.hold_part(controller.graph(), controller.registry(), layout, &slot_path);

        let inserted = self
            .insert_with(controller, &level, file, library, at, key)
            .map_err(|e| e.to_string())?;
        let new_id = inserted.instance;
        let new_path = self.ids.path_of(new_id).cloned().expect("just inserted");
        let mut added = inserted.modules.clone();
        // A held entry for this definition would be keyed by the key the
        // insert gave it; a fresh insert has none. The role values land on
        // the exposed items by role name.
        for (role_name, value) in &old_values {
            if let Some((id, own)) = self.slot_param_target_for_part(&new_path, role_name)
                && let Some(index) = controller.graph().node(id).and_then(|n| n.parameter_index(&own))
            {
                let _ = controller.set_param(id, index, *value);
            }
        }
        {
            let record = self.slots.get_mut(&slot_path).expect("checked above");
            record.part = Some(new_path.last());
            record.part_fills = None;
            record.values.clear();
        }
        let mut removed = Vec::new();
        if let Some(old_id) = old_id {
            let (moved, mut gone, fades) =
                self.move_links_to_new_part(controller, &slot_path, &state, old_id, new_id)?;
            added.extend(moved);
            removed.append(&mut gone);
            let copies = self.add_copy_fades(controller, &slot_path, old_part, &fades)?;
            added.extend(copies.iter().flat_map(|c| c.fades.iter().map(|(_, id)| *id)));
            let record = self.slots.get_mut(&slot_path).expect("checked above");
            record.fading = Some(Fading {
                leaving: old_part,
                fades,
                copies,
                thru_source: None,
                thru_dests: Vec::new(),
            });
        } else if self.fades_with_thru(&slot_path) {
            // Into an empty thru slot: the part fades in from the thru
            // signal, in every stack copy (sub-patches.md, Empty slots).
            let outputs = self.thru_fade_outputs(controller.graph(), &slot_path, new_path.last());
            let fades = self.add_fades(controller, &slot_path, outputs)?;
            added.extend(fades.iter().map(|(_, id)| *id));
            let copies = self.add_copy_fades(controller, &slot_path, None, &fades)?;
            added.extend(copies.iter().flat_map(|c| c.fades.iter().map(|(_, id)| *id)));
            let record = self.slots.get_mut(&slot_path).expect("checked above");
            record.fading = Some(Fading {
                leaving: None,
                fades,
                copies,
                thru_source: None,
                thru_dests: Vec::new(),
            });
        }
        Ok(Swapped {
            part: new_id,
            part_name: inserted.name,
            added,
            removed,
        })
    }

    /// Swaps the slot tile `slot` back to a held sub-patch part, whose
    /// definition was stored whole when it left.
    pub fn swap_to_held_subpatch(
        &mut self,
        controller: &mut Controller,
        layout: &dyn Fn(u32) -> Layout,
        slot: ModuleId,
        key: &str,
    ) -> Result<Swapped, String> {
        self.swap_to_held_subpatch_at(controller, layout, slot, key, None)
    }

    /// [`Hierarchy::swap_to_held_subpatch`], with the instance back at the
    /// level id `at` when given, under the key it had.
    pub fn swap_to_held_subpatch_at(
        &mut self,
        controller: &mut Controller,
        layout: &dyn Fn(u32) -> Layout,
        slot: ModuleId,
        key: &str,
        at: Option<u32>,
    ) -> Result<Swapped, String> {
        let stored = self
            .definitions
            .get(key)
            .and_then(|d| d.stored.clone())
            .ok_or_else(|| format!("the slot no longer holds `{key}`"))?;
        let held = self
            .slot(slot)
            .and_then(|s| s.held.get(&PartKey::SubPatch(key.to_string())).cloned())
            .unwrap_or_default();
        let library = stored.definition.library.clone();
        let file = Patch::from_definition(&stored.definition, &stored.nested);
        // The stored definition goes, so the insert can take its key back.
        self.definitions.remove(key);
        if let Some(path) = self.ids.path_of(slot).cloned()
            && let Some(state) = self.slots.get_mut(&path)
        {
            state.held.remove(&PartKey::SubPatch(key.to_string()));
        }
        let swapped = self.swap_to_subpatch_at(controller, layout, slot, &file, library.as_deref(), at, Some(key))?;
        // Its own values, by exposed name, under the role values.
        let new_path = self.ids.path_of(swapped.part).cloned().expect("just inserted");
        if let Some(definition) = self.definition_at(&new_path).cloned() {
            let role = self.slot(slot).map(|s| s.role);
            for (name, value) in &held.params {
                if role.is_some_and(|r| r.param(name).is_some()) {
                    continue;
                }
                if let Some(exposed) = definition.exposed.iter().find(|e| e.name == *name)
                    && let Some((id, param)) = self.resolve_exposed(&new_path, exposed)
                    && let Some(index) = controller.graph().node(id).and_then(|n| n.parameter_index(&param))
                {
                    let _ = controller.set_param(id, index, *value);
                }
            }
        }
        Ok(swapped)
    }

    /// Remove part: empties the slot tile `slot` (sub-patches.md, Empty
    /// slots). The leaving part goes into `held`, so swapping it back
    /// restores its values. Assignments and MIDI mappings on it, and
    /// assignments from it, have nowhere to go: they are removed, and
    /// their labels returned for the status text. A thru role fades the
    /// part out to the thru signal, Linear, in every stack copy, each to
    /// its own thru signal (ADR-0046); any other slot lets it go at once,
    /// and `Emptied::gone` names the modules the caller leaves out of its
    /// sync and then removes. When the fades cannot be added, as at the
    /// module limit, the part goes at once too: a removal never fails for
    /// room. A fade in progress is finished first. Refused on an empty
    /// slot.
    pub fn swap_to_empty(
        &mut self,
        controller: &mut Controller,
        layout: &dyn Fn(u32) -> Layout,
        slot: ModuleId,
    ) -> Result<Emptied, String> {
        let slot_path = self
            .ids
            .path_of(slot)
            .cloned()
            .ok_or_else(|| "select a slot".to_string())?;
        let Some(old_lid) = self.slots.get(&slot_path).and_then(|s| s.part) else {
            return Err("it holds no part".to_string());
        };
        self.finish_fade(controller, slot);
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let old_path = level.child(old_lid);
        let old_id = self
            .ids
            .get(&old_path)
            .ok_or_else(|| "its part is missing".to_string())?;
        let part_name = self
            .tile_name(old_id)
            .or_else(|| controller.graph().node(old_id).map(|n| n.descriptor.name.to_string()))
            .unwrap_or_default();
        // Worked out while the part is still the slot's: each copy's thru
        // ends are read from the graph before anything changes (ADR-0046).
        let fade_outputs = if self.fades_with_thru(&slot_path) {
            Some(self.thru_fade_outputs(controller.graph(), &slot_path, old_lid))
        } else {
            None
        };
        let thru_ends: Vec<(ModulePath, ThruEnds)> = match fade_outputs {
            Some(_) if self.in_stack(&level) => std::iter::once(level.clone())
                .chain(self.copy_levels(&level))
                .map(|level_k| {
                    let ends = self.copy_thru_ends(controller.graph(), &level, old_lid, &level_k);
                    (level_k, ends)
                })
                .collect(),
            _ => Vec::new(),
        };
        // The role values stay with the slot, so the next part takes them,
        // as a swap between two parts carries them (FR-ENCAP-08).
        let values = self.role_values(controller.graph(), &slot_path);
        self.hold_part(controller.graph(), controller.registry(), layout, &slot_path);
        let removed = self.drop_links_under(controller, &old_path);
        {
            let record = self.slots.get_mut(&slot_path).expect("checked above");
            record.part = None;
            record.part_fills = None;
            record.values = values;
        }
        let faded = match fade_outputs {
            Some(outputs) => self.add_removal_fades(controller, &slot_path, old_lid, outputs, thru_ends),
            None => None,
        };
        let gone = match faded {
            Some(fading) => {
                let record = self.slots.get_mut(&slot_path).expect("checked above");
                record.fading = Some(fading);
                Vec::new()
            }
            None => self.let_part_go(controller, &slot_path, old_lid),
        };
        Ok(Emptied {
            part_name,
            removed,
            gone,
        })
    }

    /// Adds the fades that take the leaving part `lid` of the slot at
    /// `slot_path` out to its thru signal, in copy 0 and every stack copy,
    /// with each copy's thru ends (ADR-0046). `None` when they cannot all
    /// be added, as when the engine has no slot left for one: whatever was
    /// added is taken away again, and was never sent to the engine.
    fn add_removal_fades(
        &mut self,
        controller: &mut Controller,
        slot_path: &ModulePath,
        lid: u32,
        outputs: Vec<(String, Law)>,
        thru_ends: Vec<(ModulePath, ThruEnds)>,
    ) -> Option<Fading> {
        let modules: BTreeSet<ModuleId> = controller.graph().nodes().map(|(id, _)| id).collect();
        let paths: BTreeSet<ModulePath> = self.ids.paths().map(|(p, _)| p.clone()).collect();
        let added = self.add_fades(controller, slot_path, outputs).and_then(|fades| {
            let copies = self.add_copy_fades(controller, slot_path, Some(lid), &fades)?;
            Ok((fades, copies))
        });
        match added {
            Ok((fades, mut copies)) => {
                let ends_at = |level: &ModulePath| {
                    thru_ends
                        .iter()
                        .find(|(at, _)| at == level)
                        .map(|(_, ends)| ends.clone())
                        .unwrap_or_default()
                };
                for copy in &mut copies {
                    (copy.thru_source, copy.thru_dests) = ends_at(&copy.level);
                }
                let level = slot_path.parent().unwrap_or_else(ModulePath::root);
                let (thru_source, thru_dests) = ends_at(&level);
                Some(Fading {
                    leaving: Some(lid),
                    fades,
                    copies,
                    thru_source,
                    thru_dests,
                })
            }
            Err(_) => {
                let new: Vec<ModuleId> = controller
                    .graph()
                    .nodes()
                    .map(|(id, _)| id)
                    .filter(|id| !modules.contains(id))
                    .collect();
                for id in new {
                    let _ = controller.remove_module(id);
                }
                let reserved: Vec<ModulePath> = self
                    .ids
                    .paths()
                    .map(|(p, _)| p.clone())
                    .filter(|p| !paths.contains(p))
                    .collect();
                for path in reserved {
                    self.ids.forget(&path);
                }
                None
            }
        }
    }

    /// Lets the leaving part `lid` of the now empty slot at `slot_path` go
    /// at once, in copy 0 and every stack copy, as a pulled cable does:
    /// the slot's inputs change as a cable change, which live-patching
    /// rule 9 ramps over 5 ms. Returns the modules the caller leaves out of
    /// its sync and then removes. Every copy's leaving part is named here,
    /// so nothing is removed before the caller's sync succeeds.
    fn let_part_go(&mut self, controller: &mut Controller, slot_path: &ModulePath, lid: u32) -> Vec<ModuleId> {
        // No fade module is added: an empty list of fades only names each
        // copy's leaving part.
        let copies = self
            .add_copy_fades(controller, slot_path, Some(lid), &[])
            .unwrap_or_default();
        if let Some(record) = self.slots.get_mut(slot_path) {
            record.fading = Some(Fading {
                leaving: Some(lid),
                fades: Vec::new(),
                copies,
                thru_source: None,
                thru_dests: Vec::new(),
            });
        }
        self.end_fade(slot_path)
    }

    /// Takes back the fades a removal added on the slot tile `slot`, when
    /// the edit does not fit the module limit (sub-patches.md, The module
    /// limit): the fades were never sent to the engine, so their slots are
    /// free at once, and their copies were never given slots. The slot
    /// then lets its part go as a slot with no thru does, and the leaving
    /// parts are returned for the caller to leave out of its sync and then
    /// remove. Nothing happens, and nothing is returned, on a slot that is
    /// not fading out to its thru signal.
    pub fn drop_removal_fades(&mut self, controller: &mut Controller, slot: ModuleId) -> Vec<ModuleId> {
        let Some(slot_path) = self.ids.path_of(slot).cloned() else {
            return Vec::new();
        };
        let Some(state) = self.slots.get(&slot_path) else {
            return Vec::new();
        };
        let Some(fading) = state
            .fading
            .as_ref()
            .filter(|f| state.part.is_none() && f.leaving.is_some())
        else {
            return Vec::new();
        };
        let fades: BTreeSet<ModuleId> = fading
            .fades
            .iter()
            .chain(fading.copies.iter().flat_map(|c| c.fades.iter()))
            .map(|(_, id)| *id)
            .collect();
        let removed = self.end_fade(&slot_path);
        for id in &fades {
            let _ = controller.remove_module(*id);
            if let Some(path) = self.ids.path_of(*id).cloned() {
                self.ids.forget(&path);
            }
        }
        removed.into_iter().filter(|id| !fades.contains(id)).collect()
    }
}

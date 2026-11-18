//! Makes, dissolves, and removes slots.

use super::*;

impl Hierarchy {
    /// Adds an empty slot for `role` at `level`, labelled with the role's
    /// name made unique among the level's slots ("Filter 2"), as one tile
    /// with no part (sub-patches.md, Empty slots). Refused at a layer or
    /// select member level. Nothing is added to the graph: a slot is routed
    /// through, and an empty one has nothing behind it. The caller syncs
    /// and commits.
    pub fn add_empty_slot(&mut self, level: &ModulePath, role: &'static RoleDescriptor) -> Result<ModuleId, String> {
        if let Some(why) = self.level_refusal(level) {
            return Err(why);
        }
        let taken: BTreeSet<String> = self
            .slots
            .iter()
            .filter(|(path, _)| path.parent().as_ref() == Some(level))
            .map(|(_, state)| state.name())
            .collect();
        let label = crate::unique_name(role.name, |name| taken.contains(name));
        let (slot_path, slot_id) = self.new_module(level).map_err(|e| e.to_string())?;
        self.slots.insert(
            slot_path,
            SlotState {
                role,
                part: None,
                part_fills: None,
                held: BTreeMap::new(),
                label: (label != role.name).then_some(label),
                fading: None,
                values: BTreeMap::new(),
            },
        );
        Ok(slot_id)
    }

    /// Makes the module or sub-patch tile `member` the part of a new slot
    /// for `role`, in place (sub-patches.md, Slots: Make slot). Its cables
    /// on role ports move to the slot's ports; a cable on another port is
    /// removed, and its label returned, as are exposures of parameters
    /// outside the role. Refused for a tile that does not declare the
    /// role, or is already a slot or a part. The caller syncs and commits.
    pub fn make_slot(
        &mut self,
        graph: &Graph,
        member: ModuleId,
        role: &'static RoleDescriptor,
    ) -> Result<MadeSlot, String> {
        let path = self.path_of(member);
        let level = path.parent().unwrap_or_else(ModulePath::root);
        let lid = path.last();
        if self.slots.contains_key(&path) {
            return Err("it is a slot already".to_string());
        }
        if self.slot_owning(&level, lid).is_some() {
            return Err("it is the part of a slot already".to_string());
        }
        let part_fills = if let Some(definition) = self.definition_at(&path) {
            if !definition.roles.iter().any(|r| crate::role_id(r) == role.id) {
                return Err(format!("{} does not fill {}", definition.metadata.name, role.name));
            }
            None
        } else {
            let Some(node) = graph.node(member) else {
                return Err("select a module or a sub-patch".to_string());
            };
            if node.unknown.is_some() {
                return Err(format!("{} is not available in this build", node.descriptor.name));
            }
            let Some(fills) = node.descriptor.fills(role.id) else {
                return Err(format!("{} does not fill {}", node.descriptor.name, role.name));
            };
            Some(fills)
        };
        let (slot_path, slot_id) = self.new_module(&level).map_err(|e| e.to_string())?;
        let slot_lid = slot_path.last();
        let state = SlotState {
            role,
            part: Some(lid),
            part_fills,
            held: BTreeMap::new(),
            label: None,
            fading: None,
            values: BTreeMap::new(),
        };

        // Cables: a role port's move to the slot; another port's go.
        let mut removed = Vec::new();
        let shown = self.level_cables(graph, &level);
        for cable in shown {
            let (is_from, is_to) = (cable.from.0 == lid, cable.to.0 == lid);
            if !is_from && !is_to {
                continue;
            }
            if cable.link {
                self.links.remove(&Link {
                    level: level.clone(),
                    to: cable.to.clone(),
                    from: cable.from.clone(),
                });
            }
            let (from, to) = (cable.from.clone(), cable.to.clone());
            let mapped_from = if is_from {
                state.role_output_of(&from.1).map(|name| (slot_lid, name.to_string()))
            } else {
                Some(from.clone())
            };
            let mapped_to = if is_to {
                state.role_input_of(&to.1).map(|name| (slot_lid, name.to_string()))
            } else {
                Some(to.clone())
            };
            match (mapped_from, mapped_to) {
                (Some(from), Some(to)) => {
                    self.links.insert(Link {
                        level: level.clone(),
                        to,
                        from,
                    });
                }
                _ => {
                    let own = if is_from { &from.1 } else { &to.1 };
                    removed.push(port_label(graph, member, own, is_from));
                }
            }
        }

        // Exposures of the part: a role parameter's follows it onto the
        // slot; another's goes.
        let mut dropped = Vec::new();
        if let Some(key) = self.instances.get(&level).map(|i| i.key.clone()) {
            let definition = self.definitions.get_mut(&key).expect("the level's definition");
            let mut moved = Vec::new();
            for entry in definition.exposed.iter_mut().filter(|e| e.module == lid) {
                match state.role_param_of(&entry.param) {
                    Some(role_name) => {
                        let stored = definition
                            .defaults
                            .remove(&(lid, entry.param.clone()))
                            .unwrap_or_default();
                        entry.module = slot_lid;
                        entry.param = role_name.to_string();
                        moved.push(((slot_lid, role_name.to_string()), stored));
                    }
                    None => dropped.push(entry.param.clone()),
                }
            }
            definition.defaults.extend(moved);
            if !dropped.is_empty() {
                self.drop_exposed(&level, lid, Some(&dropped));
            }
        } else {
            for entry in self.top_exposed.iter_mut().filter(|e| e.module == lid) {
                if let Some(role_name) = state.role_param_of(&entry.param) {
                    entry.module = slot_lid;
                    entry.param = role_name.to_string();
                }
            }
            self.top_exposed.retain(|e| e.module != lid);
        }
        self.slots.insert(slot_path, state);
        Ok(MadeSlot { slot: slot_id, removed })
    }

    /// Makes the slot tile `slot`'s part a plain module again, in place,
    /// with its cables on its own ports, and forgets the held parts
    /// (sub-patches.md, Slots: Dissolve slot). A fade in progress is
    /// finished first; the modules to remove are returned. The caller
    /// removes them, syncs, and commits.
    pub fn dissolve_slot(&mut self, graph: &Graph, slot: ModuleId) -> Result<Vec<ModuleId>, String> {
        let Some(slot_path) = self.ids.path_of(slot).cloned() else {
            return Err("select a slot".to_string());
        };
        let Some(state) = self.slots.get(&slot_path).cloned() else {
            return Err("select a slot".to_string());
        };
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let slot_lid = slot_path.last();
        let transient = self.end_fade(&slot_path);
        let Some(part) = state.part else {
            // An empty slot leaves nothing behind.
            self.remove_slot_record(&slot_path, &state);
            return Ok(transient);
        };
        let links: Vec<Link> = self
            .links_at(&level)
            .filter(|l| l.from.0 == slot_lid || l.to.0 == slot_lid)
            .cloned()
            .collect();
        for link in links {
            self.links.remove(&link);
            let from = if link.from.0 == slot_lid {
                (part, state.own_output(&link.from.1))
            } else {
                link.from.clone()
            };
            let to = if link.to.0 == slot_lid {
                (part, state.own_input(&link.to.1))
            } else {
                link.to.clone()
            };
            // Between two modules the graph holds the cable already; it
            // becomes direct. A link stays a link.
            if !(self.is_module_end(graph, &level, &from) && self.is_module_end(graph, &level, &to)) {
                self.links.insert(Link {
                    level: level.clone(),
                    to,
                    from,
                });
            }
        }
        // An exposure of a role parameter the part has follows it; one of an
        // optional parameter the part lacks has nothing behind it and goes,
        // with its re-exposures above.
        let has = |role_param: &str| {
            self.slot_param_target(&slot_path, role_param)
                .is_some_and(|(id, own)| graph.node(id).is_some_and(|n| n.parameter_index(&own).is_some()))
        };
        let gone: Vec<String> = self
            .exposures_on(&level, slot_lid)
            .into_iter()
            .filter(|role_param| !has(role_param))
            .collect();
        if !gone.is_empty() {
            self.drop_exposed(&level, slot_lid, Some(&gone));
            self.top_exposed
                .retain(|e| !(level.is_root() && e.module == slot_lid && gone.contains(&e.param)));
        }
        if let Some(key) = self.instances.get(&level).map(|i| i.key.clone()) {
            let definition = self.definitions.get_mut(&key).expect("the level's definition");
            let mut moved = Vec::new();
            for entry in definition.exposed.iter_mut().filter(|e| e.module == slot_lid) {
                let stored = definition
                    .defaults
                    .remove(&(slot_lid, entry.param.clone()))
                    .unwrap_or_default();
                entry.module = part;
                entry.param = state.own_param(&entry.param);
                moved.push(((part, entry.param.clone()), stored));
            }
            definition.defaults.extend(moved);
        } else {
            for entry in self.top_exposed.iter_mut().filter(|e| e.module == slot_lid) {
                entry.module = part;
                entry.param = state.own_param(&entry.param);
            }
        }
        self.remove_slot_record(&slot_path, &state);
        Ok(transient)
    }

    /// The parameter names exposed on the member `lid` of `level`: by the
    /// level's definition, or by the patch's own surface at the top.
    fn exposures_on(&self, level: &ModulePath, lid: u32) -> Vec<String> {
        match self.definition_at(level) {
            Some(definition) => definition
                .exposed
                .iter()
                .filter(|e| e.module == lid)
                .map(|e| e.param.clone())
                .collect(),
            None if level.is_root() => self
                .top_exposed
                .iter()
                .filter(|e| e.module == lid)
                .map(|e| e.param.clone())
                .collect(),
            None => Vec::new(),
        }
    }

    /// Forgets a slot and the definitions its held sub-patches kept.
    pub(in crate::hierarchy) fn remove_slot_record(&mut self, slot_path: &ModulePath, state: &SlotState) {
        for key in state.held.keys() {
            if let PartKey::SubPatch(key) = key {
                self.definitions.remove(key);
            }
        }
        self.slots.remove(slot_path);
    }
}

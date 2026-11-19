//! Moves a selection into a new sub-patch or puts its contents back.

use super::*;

impl Hierarchy {
    /// Collapses the selected tiles of `level` into a new sub-patch
    /// (Collapsing, points 1 to 7). The Transport, the ports of this
    /// level, and Patch input and output modules at the top count as
    /// outside. Nothing in the graph changes; the caller places the new
    /// tiles and syncs.
    pub fn collapse(&mut self, graph: &Graph, level: &ModulePath, selection: &[u32]) -> Result<Collapsed, String> {
        let is_outside_kind = |id: ModuleId| {
            graph.node(id).is_some_and(|node| {
                node.descriptor.role.is_transport()
                    || (level.is_root()
                        && (node.descriptor.kind() == INPUT_KIND || node.descriptor.kind() == OUTPUT_KIND))
            })
        };
        let mut members: BTreeSet<u32> = BTreeSet::new();
        for &id in selection {
            let id = ModuleId(id);
            if self.level_of(id) != *level || is_outside_kind(id) {
                continue;
            }
            let path = self.path_of(id);
            if self.is_hidden(id) {
                continue;
            }
            if graph.node(id).is_some() || self.instances.contains_key(&path) || self.slots.contains_key(&path) {
                members.insert(path.last());
            }
        }
        if members.is_empty() {
            return Err(NOTHING_TO_COLLAPSE.to_string());
        }

        let paths: Vec<_> = members.iter().map(|lid| level.child(*lid)).collect();
        self.check_moving_fades(&paths)?;
        let copies = self.copy_levels(level);

        // The definition and its instance.
        let name = unique_name(DEFAULT_NAME, |name| {
            self.definitions.values().any(|d| d.metadata.name == name)
        });
        let key = unique_name(&name.to_lowercase(), |key| self.definitions.contains_key(key));
        let (instance_path, instance_id) = self.new_module(level).map_err(|e| e.to_string())?;
        let ilid = instance_path.last();
        self.instances.insert(
            instance_path.clone(),
            Instance {
                key: key.clone(),
                label: None,
            },
        );
        self.definitions.insert(
            key.clone(),
            Definition {
                metadata: Metadata {
                    name,
                    ..Metadata::default()
                },
                exposed: Vec::new(),
                defaults: BTreeMap::new(),
                library: None,
                roles: Vec::new(),
                modes: ModeSettings::default(),
                stored: None,
            },
        );

        // What the level showed, before anything moves.
        let shown = self.level_cables(graph, level);
        let member_ids: Vec<ModuleId> = members.iter().filter_map(|lid| self.id_at(level, *lid)).collect();

        // Each member keeps its flat id in every running stack copy.
        for lid in &members {
            if let Some(part) = self.slots.get(&level.child(*lid)).and_then(|slot| slot.part) {
                self.move_into(level, &copies, ilid, part)?;
            }
            self.move_into(level, &copies, ilid, *lid)?;
        }

        // Cables: inside stays inside, a crossing cable gets a port.
        let mut inputs: BTreeMap<End, (u32, String)> = BTreeMap::new();
        let mut outputs: BTreeMap<End, (u32, String)> = BTreeMap::new();
        let mut input_tiles = Vec::new();
        let mut output_tiles = Vec::new();
        for cable in shown {
            let (from_in, to_in) = (members.contains(&cable.from.0), members.contains(&cable.to.0));
            match (from_in, to_in) {
                (true, true) => {
                    if cable.link {
                        self.links.remove(&Link {
                            level: level.clone(),
                            to: cable.to.clone(),
                            from: cable.from.clone(),
                        });
                        self.links.insert(Link {
                            level: instance_path.clone(),
                            to: cable.to,
                            from: cable.from,
                        });
                    }
                }
                (false, true) => {
                    if cable.link {
                        self.links.remove(&Link {
                            level: level.clone(),
                            to: cable.to.clone(),
                            from: cable.from.clone(),
                        });
                    }
                    let port = match inputs.get(&cable.from) {
                        Some(port) => port.clone(),
                        None => {
                            let label = self.inner_port_label(graph, &instance_path, &cable.to, Direction::Input);
                            let id = self
                                .add_port(&instance_path, Direction::Input, &label)
                                .map_err(|e| e.to_string())?;
                            input_tiles.push(id);
                            let path = self.ids.path_of(id).cloned().expect("just added");
                            let port = (path.last(), self.ports[&path].name.clone());
                            inputs.insert(cable.from.clone(), port.clone());
                            port
                        }
                    };
                    self.links.insert(Link {
                        level: level.clone(),
                        to: (ilid, port.1.clone()),
                        from: cable.from,
                    });
                    self.links.insert(Link {
                        level: instance_path.clone(),
                        to: cable.to,
                        from: (port.0, "out".to_string()),
                    });
                }
                (true, false) => {
                    if cable.link {
                        self.links.remove(&Link {
                            level: level.clone(),
                            to: cable.to.clone(),
                            from: cable.from.clone(),
                        });
                    }
                    let port = self
                        .output_port_for(graph, &instance_path, &cable.from, &mut outputs, &mut output_tiles)
                        .map_err(|e| e.to_string())?;
                    self.links.insert(Link {
                        level: level.clone(),
                        to: cable.to,
                        from: (ilid, port.1.clone()),
                    });
                }
                (false, false) => {}
            }
        }

        // Assignments across the new boundary (point 4): each target is
        // exposed once, however many assignments reach it.
        for (_, assignment) in graph.assignments() {
            let source_inside = self.path_of(assignment.source.module).starts_with(&instance_path);
            let target_inside = self.path_of(assignment.target).starts_with(&instance_path);
            let param = crate::param_name(graph, assignment.target, assignment.param);
            if !source_inside && target_inside && self.level_of(assignment.source.module) == *level {
                let target_path = self.path_of(assignment.target);
                let member = instance_path.child(target_path.0[instance_path.0.len()]);
                let (inner_param, stored) = if self.instances.contains_key(&member) {
                    let Some(name) = self.exposed_name_for(&member, &(assignment.target, param.clone())) else {
                        continue;
                    };
                    let value = self
                        .definition_at(&member)
                        .and_then(|d| d.exposed.iter().find(|e| e.name == name).cloned())
                        .and_then(|e| self.exposed_value(graph, &member, &e));
                    (name, Stored { value, range: None })
                } else {
                    let node = graph.node(assignment.target);
                    let value = node.map(|n| n.descriptor.saved_values(&n.params)[assignment.param]);
                    let range = node.and_then(|n| stored_range(n, assignment.param));
                    (param.clone(), Stored { value, range })
                };
                let label = graph
                    .node(assignment.target)
                    .and_then(|n| n.descriptor.params.get(assignment.param))
                    .map_or(inner_param.clone(), |spec| spec.label.to_string());
                let (lid, param) = self.slot_end_for_param(&instance_path, member.last(), &inner_param);
                self.expose_on(&key, lid, &param, &label, None, stored);
            } else if source_inside && !target_inside && self.level_of(assignment.target) == *level {
                let source_path = self.path_of(assignment.source.module);
                let member = instance_path.child(source_path.0[instance_path.0.len()]);
                let output = crate::port_name(graph, assignment.source);
                let end = if self.instances.contains_key(&member) {
                    match self.port_for_output(graph, &member, &(assignment.source.module, output)) {
                        Some(port) => (member.last(), port),
                        None => continue,
                    }
                } else {
                    (member.last(), output)
                };
                let end = self.slot_end_for_output(&instance_path, end.0, &end.1);
                self.output_port_for(graph, &instance_path, &end, &mut outputs, &mut output_tiles)
                    .map_err(|e| e.to_string())?;
            }
        }

        // What the enclosing definition exposed on a member now goes through
        // the new sub-patch: exposed there under the same name, and the
        // enclosing entry points at the new instance.
        if let Some(outer_key) = self.instances.get(level).map(|i| i.key.clone()) {
            let outer = self.definitions[&outer_key].clone();
            for entry in outer.exposed.iter().filter(|e| members.contains(&e.module)) {
                let stored = outer
                    .defaults
                    .get(&(entry.module, entry.param.clone()))
                    .cloned()
                    .unwrap_or_default();
                let name = self.expose_on(&key, entry.module, &entry.param, &entry.name, entry.range, stored);
                let outer = self.definitions.get_mut(&outer_key).expect("the enclosing definition");
                if let Some(e) = outer.exposed.iter_mut().find(|e| e.name == entry.name) {
                    e.module = ilid;
                    e.param = name.clone();
                }
                outer.defaults.remove(&(entry.module, entry.param.clone()));
                outer.defaults.insert((ilid, name), Stored::default());
            }
        } else if level.is_root() {
            // The top level's own surface (a library file opened on its
            // own) goes through the new sub-patch the same way. An entry
            // the member cannot carry is dropped.
            let entries: Vec<Exposed> = self
                .top_exposed
                .iter()
                .filter(|e| members.contains(&e.module))
                .cloned()
                .collect();
            for entry in entries {
                let carried = self.expose(graph, &instance_path, entry.module, &entry.param).ok();
                let index = self
                    .top_exposed
                    .iter()
                    .position(|e| e.module == entry.module && e.param == entry.param)
                    .expect("the entry is still there");
                match carried {
                    Some(name) => {
                        self.top_exposed[index].module = ilid;
                        self.top_exposed[index].param = name;
                    }
                    None => {
                        self.top_exposed.remove(index);
                    }
                }
            }
        }

        Ok(Collapsed {
            instance: instance_id,
            inputs: input_tiles,
            outputs: output_tiles,
            members: member_ids,
        })
    }

    /// Refuses to move a slot while its leaving part still runs.
    fn check_moving_fades(&self, paths: &[ModulePath]) -> Result<(), String> {
        if self
            .slots
            .iter()
            .any(|(slot, state)| state.fading.is_some() && paths.iter().any(|path| slot.starts_with(path)))
        {
            return Err("Finish the slot swap before moving it".to_string());
        }
        Ok(())
    }

    /// Moves one member and its running stack copies into the new instance.
    fn move_into(&mut self, level: &ModulePath, copies: &[ModulePath], instance: u32, lid: u32) -> Result<(), String> {
        for at in std::iter::once(level).chain(copies) {
            self.rename_prefix(&at.child(lid), &at.child(instance).child(lid))
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    /// The output port of the new definition at `instance` that `from`
    /// (an end inside it) feeds, made when `from` has none yet. Returns the
    /// port's level id and name.
    fn output_port_for(
        &mut self,
        graph: &Graph,
        instance: &ModulePath,
        from: &End,
        made: &mut BTreeMap<End, (u32, String)>,
        tiles: &mut Vec<ModuleId>,
    ) -> Result<(u32, String), PatchError> {
        if let Some(port) = made.get(from) {
            return Ok(port.clone());
        }
        let label = self.inner_port_label(graph, instance, from, Direction::Output);
        let id = self.add_port(instance, Direction::Output, &label)?;
        tiles.push(id);
        let path = self.ids.path_of(id).cloned().expect("just added");
        let port = (path.last(), self.ports[&path].name.clone());
        made.insert(from.clone(), port.clone());
        self.links.insert(Link {
            level: instance.clone(),
            to: (port.0, "in".to_string()),
            from: from.clone(),
        });
        Ok(port)
    }

    /// Exposes `param` of the member `lid` on the definition `key`, or
    /// finds it already exposed, and returns the exposed name: `label` made
    /// unique among the definition's names. `stored` is the member's own
    /// value and range, kept for the file.
    pub(super) fn expose_on(
        &mut self,
        key: &str,
        lid: u32,
        param: &str,
        label: &str,
        range: Option<(f32, f32)>,
        stored: Stored,
    ) -> String {
        let definition = self.definitions.get_mut(key).expect("the definition exists");
        if let Some(entry) = definition.exposed.iter().find(|e| e.module == lid && e.param == param) {
            return entry.name.clone();
        }
        let name = unique_name(label, |name| definition.exposed.iter().any(|e| e.name == name));
        definition.exposed.push(Exposed {
            name: name.clone(),
            module: lid,
            param: param.to_string(),
            range,
        });
        definition.defaults.insert((lid, param.to_string()), stored);
        name
    }

    /// The label a new port takes from the inner end it stands for
    /// (Collapsing, point 3): a module's port label, or a nested
    /// instance's port name.
    fn inner_port_label(&self, graph: &Graph, level: &ModulePath, end: &End, direction: Direction) -> String {
        let path = level.child(end.0);
        if let Some(slot) = self.slots.get(&path) {
            let port = match direction {
                Direction::Input => slot.role.input(&end.1),
                Direction::Output => slot.role.output(&end.1),
            };
            if let Some(port) = port {
                return port.label.to_string();
            }
        }
        if self.instances.contains_key(&path) {
            return end.1.clone();
        }
        let Some(node) = self.ids.get(&path).and_then(|id| graph.node(id)) else {
            return end.1.clone();
        };
        let spec = match direction {
            Direction::Input => node.input_index(&end.1).and_then(|i| node.descriptor.inputs.get(i)),
            Direction::Output => node.output_index(&end.1).and_then(|i| node.descriptor.outputs.get(i)),
        };
        spec.map_or_else(|| end.1.clone(), |spec| spec.label.to_string())
    }

    /// Puts an instance's inside back into its level (the reverse of
    /// collapsing). Cables through its ports become cables of the level.
    /// Nothing in the graph changes; the caller places the tiles and
    /// syncs.
    pub fn dissolve(&mut self, graph: &Graph, instance: ModuleId) -> Result<Dissolved, String> {
        let Some(instance_path) = self.ids.path_of(instance).cloned() else {
            return Err("Select a sub-patch to dissolve".to_string());
        };
        let Some(record) = self.instances.get(&instance_path).cloned() else {
            return Err("Select a sub-patch to dissolve".to_string());
        };
        let name = self
            .definitions
            .get(&record.key)
            .map(|d| d.metadata.name.clone())
            .unwrap_or_default();
        let level = instance_path.parent().unwrap_or_else(ModulePath::root);
        let ilid = instance_path.last();
        self.check_moving_fades(std::slice::from_ref(&instance_path))?;
        let copies = self.copy_levels(&level);

        // The members and the ports of the inside.
        let mut members: Vec<ModulePath> = Vec::new();
        let mut ports: Vec<(ModulePath, Port)> = Vec::new();
        for (path, id) in self.ids.paths() {
            if path.parent().as_ref() != Some(&instance_path) {
                continue;
            }
            if let Some(port) = self.ports.get(path) {
                ports.push((path.clone(), port.clone()));
            } else if self.instances.contains_key(path) || self.slots.contains_key(path) || graph.node(id).is_some() {
                members.push(path.clone());
            }
        }

        // The cables each port joins, in the ids of the two levels.
        let outside_source = |port: &Port| -> Option<End> {
            self.link_into(&level, &(ilid, port.name.clone()))
                .map(|l| l.from.clone())
        };
        let outside_dests = |port: &Port| -> Vec<End> {
            self.links_from(&level, &(ilid, port.name.clone()))
                .map(|l| l.to.clone())
                .collect()
        };
        // Select which endpoints are local to the instance when joining
        // across a boundary. The outer level can use the same numbers.
        let mut joins = Vec::new();
        for (path, port) in &ports {
            match port.direction {
                Direction::Input => {
                    let Some(source) = outside_source(port) else { continue };
                    for link in self.links_from(&instance_path, &(path.last(), "out".to_string())) {
                        // Straight into an output port: a pass-through.
                        if let Some(out) = ports
                            .iter()
                            .find(|(p, q)| p.last() == link.to.0 && q.direction == Direction::Output)
                        {
                            for dest in outside_dests(&out.1) {
                                joins.push((source.clone(), dest, None));
                            }
                        } else {
                            joins.push((source.clone(), link.to.clone(), Some(Direction::Input)));
                        }
                    }
                }
                Direction::Output => {
                    let Some(link) = self.link_into(&instance_path, &(path.last(), "in".to_string())) else {
                        continue;
                    };
                    if ports
                        .iter()
                        .any(|(p, q)| p.last() == link.from.0 && q.direction == Direction::Input)
                    {
                        continue; // Handled from the input side.
                    }
                    for dest in outside_dests(port) {
                        joins.push((link.from.clone(), dest, Some(Direction::Output)));
                    }
                }
            }
        }
        let inside_links: Vec<Link> = self
            .links_at(&instance_path)
            .filter(|link| {
                !ports
                    .iter()
                    .any(|(p, _)| p.last() == link.from.0 || p.last() == link.to.0)
            })
            .cloned()
            .collect();

        // New level ids: kept when the level never had them, else the next.
        let mut renamed: BTreeMap<u32, u32> = BTreeMap::new();
        let mut next = self.next_level_id(&level);
        for path in &members {
            let lid = path.last();
            let new = if level.is_root() {
                self.ids.get(path).map_or(lid, |id| id.0)
            } else if self.ids.get(&level.child(lid)).is_none() && !renamed.values().any(|v| *v == lid) {
                lid
            } else {
                next += 1;
                next - 1
            };
            renamed.insert(lid, new);
        }
        let rename = |end: &End| -> End { (renamed.get(&end.0).copied().unwrap_or(end.0), end.1.clone()) };

        // Take the ports, their links, and the instance out.
        let mut removed = vec![instance];
        for (path, _) in &ports {
            if let Some(id) = self.ids.get(path) {
                removed.push(id);
            }
            self.ports.remove(path);
        }
        self.links.retain(|link| link.level != instance_path);
        self.links
            .retain(|link| !(link.level == level && (link.from.0 == ilid || link.to.0 == ilid)));
        self.instances.remove(&instance_path);
        let inner = self.definitions.remove(&record.key);

        // Move the members out.
        let member_ids = members
            .iter()
            .filter_map(|path| self.ids.get(path))
            .filter(|id| !self.is_hidden(*id))
            .collect();
        for path in &members {
            let lid = path.last();
            for at in std::iter::once(&level).chain(&copies) {
                self.rename_prefix(&at.child(ilid).child(lid), &at.child(renamed[&lid]))
                    .map_err(|error| error.to_string())?;
            }
        }
        // Every slot and part now has its destination path. The slot's
        // part reference uses the destination level's local id too.
        for path in &members {
            if let Some(slot) = self.slots.get_mut(&level.child(renamed[&path.last()])) {
                slot.part = slot.part.map(|part| renamed[&part]);
            }
        }
        for link in inside_links {
            self.links.insert(Link {
                level: level.clone(),
                to: rename(&link.to),
                from: rename(&link.from),
            });
        }
        // What the enclosing definition exposed through the dissolved
        // sub-patch now names the member itself; at the top level, the
        // patch's own surface does the same.
        if let (Some(inner), true) = (&inner, level.is_root()) {
            for entry in self.top_exposed.iter_mut().filter(|e| e.module == ilid) {
                let Some(through) = inner.exposed.iter().find(|e| e.name == entry.param) else {
                    continue;
                };
                entry.module = renamed.get(&through.module).copied().unwrap_or(through.module);
                entry.param = through.param.clone();
                entry.range = fold_ranges(entry.range.into_iter().chain(through.range));
            }
            self.top_exposed.retain(|e| e.module != ilid);
        }
        if let (Some(inner), Some(outer_key)) = (inner, self.instances.get(&level).map(|i| i.key.clone())) {
            let outer = self.definitions.get_mut(&outer_key).expect("the enclosing definition");
            let mut moved = Vec::new();
            for entry in outer.exposed.iter_mut().filter(|e| e.module == ilid) {
                let Some(through) = inner.exposed.iter().find(|e| e.name == entry.param) else {
                    continue;
                };
                let stored = inner
                    .defaults
                    .get(&(through.module, through.param.clone()))
                    .cloned()
                    .unwrap_or_default();
                entry.module = renamed.get(&through.module).copied().unwrap_or(through.module);
                entry.param = through.param.clone();
                entry.range = fold_ranges(entry.range.into_iter().chain(through.range));
                moved.push(((entry.module, entry.param.clone()), stored));
            }
            outer.exposed.retain(|e| e.module != ilid);
            outer.defaults.retain(|(member, _), _| *member != ilid);
            outer.defaults.extend(moved);
        }
        for (mut from, mut to, inner) in joins {
            match inner {
                Some(Direction::Input) => to = rename(&to),
                Some(Direction::Output) => from = rename(&from),
                None => {}
            }
            if self.keep_direct_ends(graph, &level, &from, &to) {
                continue;
            }
            self.links.insert(Link {
                level: level.clone(),
                to,
                from,
            });
        }
        Ok(Dissolved {
            name,
            members: member_ids,
            removed,
        })
    }
}

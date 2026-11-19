//! Edits surface parameters and follows them down to their targets.

use super::*;

impl Hierarchy {
    /// The exposed name of `instance` that stands for the flat parameter
    /// `target`, through any depth of nesting.
    pub fn exposed_name_for(&self, instance: &ModulePath, target: &(ModuleId, String)) -> Option<String> {
        if self.member_target(instance).as_ref() == Some(target) {
            return Some(MEMBER_PARAM.to_string());
        }
        let definition = self.definition_at(instance)?;
        definition
            .exposed
            .iter()
            .find(|e| self.resolve_exposed(instance, e).as_ref() == Some(target))
            .map(|e| e.name.clone())
    }

    /// The flat module and parameter an exposed entry of the definition at
    /// `instance` stands for.
    pub fn resolve_exposed(&self, instance: &ModulePath, exposed: &Exposed) -> Option<(ModuleId, String)> {
        let member = instance.child(exposed.module);
        if exposed.param == MEMBER_PARAM
            && let Some(target) = self.member_target(&member)
        {
            return Some(target);
        }
        if let Some(definition) = self.definition_at(&member) {
            let inner = definition.exposed.iter().find(|e| e.name == exposed.param)?;
            return self.resolve_exposed(&member, inner);
        }
        if self.slots.contains_key(&member) {
            return self.slot_param_target(&member, &exposed.param);
        }
        Some((self.ids.get(&member)?, exposed.param.clone()))
    }

    /// The live value of an exposed parameter of the instance at `path`,
    /// read through any nesting and clamped to each level's range on the
    /// way in, outer first, as the flattener writes it (flattening rule 12).
    pub fn exposed_value(&self, graph: &Graph, instance: &ModulePath, exposed: &Exposed) -> Option<f32> {
        let (id, param) = self.resolve_exposed(instance, exposed)?;
        let node = graph.node(id)?;
        let value = match &node.unknown {
            Some(unknown) => *unknown.params.get(&param)?,
            None => {
                let index = node.parameter_index(&param)?;
                node.descriptor.saved_values(&node.params)[index]
            }
        };
        Some(
            self.range_chain(instance, exposed)
                .into_iter()
                .fold(value, |value, range| clamp_exposed(value, Some(range))),
        )
    }

    /// Every finite range on the way from an exposed entry of the instance
    /// at `instance` to the flat parameter, outer first.
    fn range_chain(&self, instance: &ModulePath, exposed: &Exposed) -> Vec<(f32, f32)> {
        let mut chain = Vec::new();
        if let Some(range) = exposed.range.filter(|r| is_span(*r)) {
            chain.push(range);
        }
        let member = instance.child(exposed.module);
        if let Some(definition) = self.definition_at(&member)
            && let Some(through) = definition.exposed.iter().find(|e| e.name == exposed.param)
        {
            chain.extend(self.range_chain(&member, through));
        }
        chain
    }

    /// The range a surface control for `exposed` spans: the overlap of
    /// every level's range on the way in, or `None` when no level narrows
    /// it. Clamping to each range in turn (as [`Hierarchy::exposed_value`]
    /// does) gives the same value as clamping to the overlap, so the
    /// control and the saved value agree. Ranges that do not overlap, which
    /// only a hand-written file can make, give the innermost one, because
    /// its clamp is applied last.
    pub fn exposed_range(&self, instance: &ModulePath, exposed: &Exposed) -> Option<(f32, f32)> {
        fold_ranges(self.range_chain(instance, exposed))
    }

    /// What the `index`th exposed entry of the instance tile `instance`
    /// stands for, or `None` when the tile is not an instance or has no
    /// such entry.
    pub fn exposed_target(&self, instance: ModuleId, index: usize) -> Option<ExposedTarget> {
        let path = self.ids.path_of(instance)?;
        let definition = self.definition_at(path)?;
        let exposed = definition.exposed.get(index)?;
        let (module, param) = self.resolve_exposed(path, exposed)?;
        Some(ExposedTarget {
            module,
            param,
            range: self.exposed_range(path, exposed),
        })
    }

    /// The instance tile one level up that exposes `param` of the tile
    /// `member` (a module's parameter, or a nested instance's exposed
    /// name), and the name it has on that surface. `None` at the top level
    /// or when it is not exposed.
    pub fn exposure_of(&self, member: ModuleId, param: &str) -> Option<(ModuleId, String)> {
        let path = self.ids.path_of(member)?;
        let level = path.parent()?;
        let definition = self.definition_at(&level)?;
        // A slot's part is exposed through the slot, by the role's name.
        let (lid, param) = self.slot_end_for_param(&level, path.last(), param);
        let entry = definition
            .exposed
            .iter()
            .find(|e| e.module == lid && e.param == param)?;
        Some((self.ids.get(&level)?, entry.name.clone()))
    }

    /// The outermost surface that exposes `param` of the tile `member`,
    /// through every level above it: that instance tile and the index of
    /// its surface item. `None` when nothing exposes it.
    pub fn outermost_surface(&self, member: ModuleId, param: &str) -> Option<(ModuleId, usize)> {
        let mut found = None;
        let mut at = (member, param.to_string());
        while let Some((instance, name)) = self.exposure_of(at.0, &at.1) {
            let index = self
                .definition_of(instance)?
                .exposed
                .iter()
                .position(|e| e.name == name)?;
            found = Some((instance, index));
            at = (instance, name);
        }
        found
    }

    /// The instance tile on `level` whose exposures reach the flat
    /// parameter `target`, through any depth, and the index of the exposed
    /// entry on its surface. `None` when `target` is on `level` itself or
    /// no exposure reaches it. A parameter exposed twice resolves to the
    /// first entry.
    pub fn surface_for(&self, level: &ModulePath, target: &(ModuleId, String)) -> Option<(ModuleId, usize)> {
        let target_level = self.level_of(target.0);
        if target_level == *level || !target_level.starts_with(level) {
            return None;
        }
        let instance = target_level.prefix(level.0.len() + 1);
        let definition = self.definition_at(&instance)?;
        let index = definition
            .exposed
            .iter()
            .position(|e| self.resolve_exposed(&instance, e).as_ref() == Some(target))?;
        Some((self.ids.get(&instance)?, index))
    }

    /// Exposes `param` of the member `member` of the definition at `level`
    /// on that definition's surface (sub-patches.md, Exposing). The member
    /// is a module, and `param` one of its parameters, or a nested
    /// instance, and `param` one of its exposed names. Returns the surface
    /// name: the parameter's label, made unique. A parameter already
    /// exposed keeps its name and nothing changes. Refused at the top
    /// level, for a port, for a transient parameter, and for a name the
    /// member does not have. The caller syncs and commits.
    pub fn expose(&mut self, graph: &Graph, level: &ModulePath, member: u32, param: &str) -> Result<String, String> {
        let Some(key) = self.instances.get(level).map(|i| i.key.clone()) else {
            return Err(EXPOSE_INSIDE_ONLY.to_string());
        };
        // A slot's part is exposed through the slot, by the role's name;
        // a parameter only the part has is refused (sub-patches.md, Slots).
        let (member, param) = if let Some((slot_path, slot)) = self.slot_owning(level, member) {
            match slot.role_param_of(param) {
                Some(role_name) => (slot_path.last(), role_name.to_string()),
                None => {
                    let part_id = self.ids.get(&path_of_member(level, member));
                    let node = part_id.and_then(|id| graph.node(id));
                    let label = node
                        .and_then(|n| n.descriptor.params.iter().find(|p| p.name == param))
                        .map_or(param.to_string(), |p| p.label.to_string());
                    let part_name = node.map_or_else(
                        || {
                            self.definition_at(&level.child(member))
                                .map_or_else(String::new, |d| d.metadata.name.clone())
                        },
                        |n| n.descriptor.name.to_string(),
                    );
                    return Err(expose_role_only_text(slot.role, &label, &part_name));
                }
            }
        } else {
            (member, param.to_string())
        };
        let param = param.as_str();
        let path = level.child(member);
        let (label, stored) = if let Some(definition) = self.definition_at(&path) {
            let Some(through) = definition.exposed.iter().find(|e| e.name == param).cloned() else {
                return Err(format!("{} exposes no `{param}`", definition.metadata.name));
            };
            let value = self.exposed_value(graph, &path, &through);
            (param.to_string(), Stored { value, range: None })
        } else if let Some(slot) = self.slots.get(&path) {
            let Some(role_param) = slot.role.param(param) else {
                return Err(format!("{} has no role parameter `{param}`", slot.role.name));
            };
            let stored = match self.slot_param_target(&path, param) {
                Some((id, own)) => {
                    let node = graph.node(id);
                    let index = node.and_then(|n| n.parameter_index(&own));
                    match (node, index) {
                        (Some(node), Some(index)) => Stored {
                            value: Some(node.descriptor.saved_values(&node.params)[index]),
                            range: stored_range(node, index),
                        },
                        _ => Stored::default(),
                    }
                }
                None => Stored::default(),
            };
            (role_param.label.to_string(), stored)
        } else if self.ports.contains_key(&path) {
            return Err("a port has no parameters".to_string());
        } else {
            let Some(node) = self.ids.get(&path).and_then(|id| graph.node(id)) else {
                return Err(format!("there is no module {member}"));
            };
            match &node.unknown {
                Some(unknown) => {
                    let Some(&value) = unknown.params.get(param) else {
                        return Err(format!("{} has no parameter `{param}`", unknown.kind));
                    };
                    (
                        param.to_string(),
                        Stored {
                            value: Some(value),
                            range: None,
                        },
                    )
                }
                None => {
                    let Some(index) = node.parameter_index(param) else {
                        return Err(format!("{} has no parameter `{param}`", node.descriptor.name));
                    };
                    let spec = &node.descriptor.params[index];
                    if spec.is_transient() {
                        return Err(format!("{} is a live gesture, not a value to expose", spec.label));
                    }
                    (
                        spec.label.to_string(),
                        Stored {
                            value: Some(node.descriptor.saved_values(&node.params)[index]),
                            range: stored_range(node, index),
                        },
                    )
                }
            }
        };
        Ok(self.expose_on(&key, member, param, &label, None, stored))
    }

    /// Takes `param` of the member `member` off the surface of the
    /// definition at `level`, and off every surface above that re-exposed
    /// it. Exposures below, inside a nested member, stay. Returns whether
    /// it was exposed. The caller removes the assignments that reached it
    /// from other levels, syncs, and commits.
    pub fn unexpose(&mut self, level: &ModulePath, member: u32, param: &str) -> bool {
        let exposed = self
            .definition_at(level)
            .is_some_and(|d| d.exposed.iter().any(|e| e.module == member && e.param == param));
        if exposed {
            self.drop_exposed(level, member, Some(&[param.to_string()]));
        }
        exposed
    }

    /// Drops the enclosing definition's exposed entries that name `member`
    /// (all of them, or only those on `names`), and the entries above that
    /// re-exposed them, level by level.
    pub(super) fn drop_exposed(&mut self, level: &ModulePath, member: u32, names: Option<&[String]>) {
        let Some(key) = self.instances.get(level).map(|i| i.key.clone()) else {
            return;
        };
        let Some(definition) = self.definitions.get_mut(&key) else {
            return;
        };
        let goes = |e: &Exposed| e.module == member && names.is_none_or(|names| names.contains(&e.param));
        let gone: Vec<String> = definition
            .exposed
            .iter()
            .filter(|e| goes(e))
            .map(|e| e.name.clone())
            .collect();
        if gone.is_empty() {
            return;
        }
        definition.exposed.retain(|e| !goes(e));
        definition
            .defaults
            .retain(|(m, param), _| !(*m == member && names.is_none_or(|names| names.contains(param))));
        if let Some(parent) = level.parent() {
            self.drop_exposed(&parent, level.last(), Some(&gone));
        }
    }
}

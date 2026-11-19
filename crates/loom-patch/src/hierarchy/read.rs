//! Rebuilds the editor hierarchy and its layout from a loaded patch.

use super::*;

impl Hierarchy {
    /// Reads the hierarchy of `patch` after it was flattened with `ids`
    /// into `graph`: every instance and port gets a flat id in path order,
    /// the definitions keep their exposed parameters and stored values,
    /// and the boundary cables become links.
    pub fn from_patch(patch: &Patch, graph: &Graph, ids: FlatIds, registry: &Registry) -> Result<Self, PatchError> {
        let mut hierarchy = Self::with_ids(ids);
        hierarchy.top_exposed = patch.exposed.clone();
        hierarchy.read_level(
            &patch.modules,
            &patch.cables,
            &ModulePath::root(),
            &patch.subpatches,
            registry,
        )?;
        hierarchy.refresh_member_entries();
        hierarchy.adopt(graph);
        hierarchy.implied = graph_cables(graph)
            .difference(&hierarchy.direct_cables(patch))
            .cloned()
            .collect();
        // What the flattener added for modes, from the same ids, so a
        // parameter write reaches every copy from the first edit on.
        if !hierarchy.instances.is_empty() {
            let mut scratch = hierarchy.ids.clone();
            let flat = patch.flatten(registry, &mut scratch)?;
            hierarchy.note_flat(&flat);
        }
        Ok(hierarchy)
    }

    /// One level of a file: its instances (recursively), its ports, and
    /// its links.
    pub(super) fn read_level(
        &mut self,
        modules: &[PatchModule],
        cables: &[PatchCable],
        level: &ModulePath,
        table: &BTreeMap<String, SubPatch>,
        registry: &Registry,
    ) -> Result<(), PatchError> {
        let mut structural = BTreeSet::new();
        let mut sorted: Vec<&PatchModule> = modules.iter().collect();
        sorted.sort_by_key(|m| m.id);
        for module in sorted {
            let path = level.child(module.id);
            if module.is_instance() || module.subpatch.is_some() {
                let Some(key) = module.subpatch.clone() else {
                    return Err(PatchError::NotAnInstance {
                        module: module.id,
                        kind: module.kind.clone(),
                    });
                };
                let Some(definition) = table.get(&key) else {
                    return Err(PatchError::UnknownSubPatch {
                        module: module.id,
                        key: key.clone(),
                    });
                };
                self.ids.id_of(&path)?;
                self.instances.insert(
                    path.clone(),
                    Instance {
                        key: key.clone(),
                        label: module.label.clone(),
                    },
                );
                self.definitions.insert(
                    key,
                    Definition {
                        metadata: definition.metadata.clone(),
                        exposed: definition.exposed.clone(),
                        defaults: defaults_of(definition, registry),
                        library: definition.library.clone(),
                        roles: definition.roles.clone(),
                        modes: ModeSettings::of(definition),
                        stored: None,
                    },
                );
                self.read_level(&definition.modules, &definition.cables, &path, table, registry)?;
                structural.insert(module.id);
            } else if module.is_slot() || module.slot.is_some() {
                let Some(slot) = &module.slot else {
                    return Err(PatchError::BadSlot {
                        module: module.id,
                        reason: "it has no `slot` field".to_string(),
                    });
                };
                let role = registry.resolve_role(&slot.role).map_err(|e| PatchError::BadSlot {
                    module: module.id,
                    reason: e.to_string(),
                })?;
                self.ids.id_of(&path)?;
                let part_fills = slot
                    .part
                    .and_then(|part| modules.iter().find(|m| m.id == part))
                    .filter(|part| part.subpatch.is_none())
                    .and_then(|part| registry.resolve(&part.kind).ok())
                    .and_then(|d| d.fills(role.id));
                self.slots.insert(
                    path,
                    SlotState {
                        role,
                        part: slot.part,
                        part_fills,
                        held: slot.held.clone(),
                        label: module.label.clone(),
                        fading: None,
                        // Only the role's own parameters carry, and only
                        // through an empty slot: a name the role does not
                        // have, from a hand-edited file, is dropped.
                        values: if slot.part.is_none() {
                            slot.values
                                .iter()
                                .filter(|(name, _)| role.params.iter().any(|p| p.name == name.as_str()))
                                .map(|(name, value)| (name.clone(), *value))
                                .collect()
                        } else {
                            BTreeMap::new()
                        },
                    },
                );
                // A held sub-patch keeps its definition, whole, since no
                // instance carries its modules.
                for key in slot.held.keys() {
                    let crate::PartKey::SubPatch(key) = key else { continue };
                    let Some(definition) = table.get(key) else { continue };
                    let nested: BTreeMap<String, SubPatch> = crate::reachable_keys(definition, table)
                        .into_iter()
                        .filter_map(|k| table.get(&k).map(|d| (k, d.clone())))
                        .collect();
                    self.definitions.insert(
                        key.clone(),
                        Definition {
                            metadata: definition.metadata.clone(),
                            exposed: definition.exposed.clone(),
                            defaults: BTreeMap::new(),
                            library: definition.library.clone(),
                            roles: definition.roles.clone(),
                            modes: ModeSettings::of(definition),
                            stored: Some(Box::new(StoredPart {
                                definition: definition.clone(),
                                nested,
                            })),
                        },
                    );
                }
                structural.insert(module.id);
            } else if !level.is_root() && (module.kind == INPUT_KIND || module.kind == OUTPUT_KIND) {
                let direction = if module.kind == INPUT_KIND {
                    Direction::Input
                } else {
                    Direction::Output
                };
                self.ids.id_of(&path)?;
                self.ports.insert(
                    path,
                    Port {
                        direction,
                        name: port_name(module, direction.own_port_fallback()),
                    },
                );
                structural.insert(module.id);
            }
        }
        for cable in cables {
            if structural.contains(&cable.from.0) || structural.contains(&cable.to.0) {
                // One link per input, the later of two kept, as a flat
                // patch keeps the later cable: two would let a crafted
                // file reach one end by two routes.
                if let Some(old) = self.links.linked_into(level, &cable.to).cloned() {
                    self.links.remove(&old);
                }
                self.links.insert(Link {
                    level: level.clone(),
                    to: cable.to.clone(),
                    from: cable.from.clone(),
                });
            }
        }
        Ok(())
    }

    /// Gives every graph module the table does not know a top-level path.
    /// A module added straight to the controller is a top-level module.
    pub fn adopt(&mut self, graph: &Graph) {
        for (id, _) in graph.nodes() {
            if self.ids.path_of(id).is_none() {
                // Refused only when another path owns the id, which a
                // module the controller holds never has.
                let _ = self.ids.reserve(ModulePath::top(id.0), id);
            }
        }
    }

    /// Each tile's layout as `patch` wrote it, by flat id: the top level's
    /// modules, and every definition's through its instance.
    pub fn layouts(&self, patch: &Patch) -> Vec<(ModuleId, Layout)> {
        let mut levels: Vec<(ModulePath, &[PatchModule])> = vec![(ModulePath::root(), &patch.modules)];
        for (path, instance) in &self.instances {
            if let Some(definition) = patch.subpatches.get(&instance.key) {
                levels.push((path.clone(), &definition.modules));
            }
        }
        let mut layouts = Vec::new();
        for (level, modules) in levels {
            for module in modules {
                if let Some(id) = self.ids.get(&level.child(module.id)) {
                    layouts.push((
                        id,
                        Layout {
                            position: module.position,
                            size: module.size,
                        },
                    ));
                }
            }
        }
        layouts
    }
}

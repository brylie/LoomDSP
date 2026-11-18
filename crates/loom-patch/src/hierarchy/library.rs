//! Inserts, links, and exports sub-patch definitions.

use super::*;

impl Hierarchy {
    /// Inserts a clipboard slot with its own part and held definitions. A
    /// temporary definition lets the normal sub-patch loader validate and
    /// remap the whole slot before its tiles are moved to the destination.
    pub fn insert_slot_copy(
        &mut self,
        controller: &mut Controller,
        level: &ModulePath,
        slot: PatchModule,
        part: Option<PatchModule>,
        definitions: BTreeMap<String, SubPatch>,
    ) -> Result<Inserted, String> {
        if let Some(why) = self.level_refusal(level) {
            return Err(why);
        }
        if !slot.is_slot() || slot.slot.as_ref().and_then(|slot| slot.part) != part.as_ref().map(|m| m.id) {
            return Err("the slot and its part do not match".into());
        }
        let modules = std::iter::once(slot).chain(part).collect();
        let inserted = self
            .insert_definition(
                controller,
                level,
                SubPatch {
                    modules,
                    ..SubPatch::new("Slot copy")
                },
                definitions,
                None,
                None,
                Vec::new(),
            )
            .map_err(|error| error.to_string())?;
        let slot = match self.dissolve(controller.graph(), inserted.instance) {
            Ok(dissolved) => dissolved
                .members
                .into_iter()
                .next()
                .expect("the inserted slot is visible"),
            Err(error) => {
                for id in inserted.modules {
                    let _ = controller.remove_module(id);
                }
                return Err(error);
            }
        };
        Ok(Inserted {
            instance: slot,
            ..inserted
        })
    }

    /// Inserts a patch file as a new sub-patch tile at `level`, as a copy
    /// or, with `library`, linked to that path (sub-patches.md, Library).
    /// The file becomes a definition ([`SubPatch::from_patch`]): its
    /// patch-wide modules are left out and their cables become ports. Its
    /// modules are added to the controller with ids from the table, and
    /// the tile, its ports, and the definitions it brings are recorded.
    /// Refused, with nothing added, when the file does not flatten, or
    /// when `library` is the file an enclosing linked sub-patch follows,
    /// which would make that file contain itself (Patch file). The caller
    /// syncs and commits; on a later refusal it removes `modules`.
    pub fn insert(
        &mut self,
        controller: &mut Controller,
        level: &ModulePath,
        file: &Patch,
        library: Option<&str>,
    ) -> Result<Inserted, PatchError> {
        self.insert_with(controller, level, file, library, None, None)
    }

    /// [`Hierarchy::insert`], with the new tile at the level id `at` and the
    /// definition under `key` when given and free: undo puts a sub-patch
    /// part back at the ids it had, so the step's records still apply.
    pub fn insert_with(
        &mut self,
        controller: &mut Controller,
        level: &ModulePath,
        file: &Patch,
        library: Option<&str>,
        at: Option<u32>,
        key: Option<&str>,
    ) -> Result<Inserted, PatchError> {
        if let Some(path) = library {
            for n in 1..=level.0.len() {
                let prefix = level.prefix(n);
                if self
                    .definition_at(&prefix)
                    .is_some_and(|d| d.library.as_deref() == Some(path))
                {
                    let mut loop_path = self.path_names(level).split_off(n - 1);
                    loop_path.push(file.metadata.name.clone());
                    return Err(PatchError::SubPatchLoop { path: loop_path });
                }
            }
        }
        let (definition, left_out) = SubPatch::from_patch(file, controller.registry(), library);
        self.insert_definition(
            controller,
            level,
            definition,
            file.subpatches.clone(),
            at,
            key,
            left_out,
        )
    }

    /// Inserts `definition`, with the definitions `nested` in it, as a new
    /// sub-patch tile at `level`: at level id `at` and under definition key
    /// `key` when they are free. The keys are made unique. At a layer or
    /// select level the new tile is a member, added last; a seventeenth is
    /// refused (sub-patches.md, The editor for modes). The caller syncs and
    /// commits, and removes the added modules on a later refusal.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_definition(
        &mut self,
        controller: &mut Controller,
        level: &ModulePath,
        mut definition: SubPatch,
        mut nested: BTreeMap<String, SubPatch>,
        at: Option<u32>,
        key: Option<&str>,
        left_out: Vec<LeftOut>,
    ) -> Result<Inserted, PatchError> {
        let member_mode = self
            .definition_at(level)
            .map(|d| d.modes.mode)
            .filter(|m| m.has_members());
        if let Some(mode) = member_mode
            && self
                .definition_at(level)
                .is_some_and(|d| d.modes.members.len() >= crate::MOST_MEMBERS)
        {
            return Err(PatchError::BadMode {
                reason: crate::modes::too_many_members(mode),
            });
        }
        let name = definition.metadata.name.clone();
        let taken = |key: &str| self.definitions.contains_key(key) || nested.contains_key(key);
        let key = match key {
            Some(wanted) if !taken(wanted) => wanted.to_string(),
            _ => unique_name(&name.to_lowercase(), taken),
        };
        rekey(&mut definition, &mut nested, |candidate| {
            candidate == key || self.definitions.contains_key(candidate)
        });
        self.adopt(controller.graph());
        let (instance_path, instance_id) = match at {
            Some(lid) => {
                let path = level.child(lid);
                let id = self.ids.id_of(&path)?;
                (path, id)
            }
            None => self.new_module(level)?,
        };

        // The file as a patch of one instance, flattened on a scratch table
        // whose paths start at the instance's level id.
        let mut temp = Patch::new("");
        temp.subpatches = nested;
        temp.subpatches.insert(key.clone(), definition);
        temp.modules.push(PatchModule {
            id: instance_path.last(),
            kind: SUBPATCH_KIND.to_string(),
            subpatch: Some(key),
            slot: None,
            label: None,
            params: BTreeMap::new(),
            position: None,
            size: None,
            ranges: BTreeMap::new(),
        });
        let flat = temp.flatten(controller.registry(), &mut FlatIds::new())?;
        let mut real: BTreeMap<ModuleId, ModuleId> = BTreeMap::new();
        for module in &flat.modules {
            let mut path = level.clone();
            path.0.extend_from_slice(&module.path.0);
            real.insert(module.id, self.ids.id_of(&path)?);
        }
        let added: Vec<ModuleId> = real.values().copied().collect();
        if let Err(error) = flat.add_to(controller, &|id| real.get(&id).copied().unwrap_or(id)) {
            for id in &added {
                let _ = controller.remove_module(*id);
            }
            return Err(error);
        }
        if let Err(error) = self.read_level(&temp.modules, &[], level, &temp.subpatches, controller.registry()) {
            for id in &added {
                let _ = controller.remove_module(*id);
            }
            return Err(error);
        }
        let mut layouts = Vec::new();
        for (path, instance) in &self.instances {
            if !path.starts_with(&instance_path) {
                continue;
            }
            let Some(definition) = temp.subpatches.get(&instance.key) else {
                continue;
            };
            for module in &definition.modules {
                if let Some(id) = self.ids.get(&path.child(module.id)) {
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
        if member_mode.is_some()
            && let Some(key) = self.instances.get(level).map(|i| i.key.clone())
            && let Some(definition) = self.definitions.get_mut(&key)
        {
            definition.modes.members.push(instance_path.last());
        }
        Ok(Inserted {
            instance: instance_id,
            name,
            modules: added,
            layouts,
            left_out,
            notes: flat.warnings,
        })
    }

    /// The library path the sub-patch tile `instance` follows, if it is
    /// linked.
    pub fn library_of(&self, instance: ModuleId) -> Option<&str> {
        self.definition_of(instance)?.library.as_deref()
    }

    /// True when any sub-patch in the patch follows a library file.
    pub fn has_links(&self) -> bool {
        self.definitions.values().any(|d| d.library.is_some())
    }

    /// Unlink: makes the linked sub-patch tile `instance` a plain copy
    /// (sub-patches.md, Linked library items). Returns its name. Refused
    /// for a tile that is not a linked sub-patch.
    pub fn unlink(&mut self, instance: ModuleId) -> Result<String, String> {
        let key = self
            .ids
            .path_of(instance)
            .and_then(|path| self.instances.get(path))
            .map(|i| i.key.clone())
            .ok_or_else(|| "select a sub-patch to unlink".to_string())?;
        let definition = self
            .definitions
            .get_mut(&key)
            .expect("every instance has its definition");
        if definition.library.take().is_none() {
            return Err(format!("{} does not follow the library", definition.metadata.name));
        }
        Ok(definition.metadata.name.clone())
    }

    /// Takes the link off the definition `key`, for the first edit inside
    /// a linked sub-patch. Returns its name, or `None` when it was not
    /// linked.
    pub fn unlink_key(&mut self, key: &str) -> Option<String> {
        let definition = self.definitions.get_mut(key)?;
        definition.library.take()?;
        Some(definition.metadata.name.clone())
    }

    /// What each linked definition and everything nested in it would be
    /// saved as, by key: the text an edit inside changes, and the refresh
    /// on load compares against the library file. Empty with no link, at
    /// no cost.
    pub fn linked_fingerprints(&self, graph: &Graph, layout: impl Fn(u32) -> Layout) -> BTreeMap<String, String> {
        if !self.has_links() {
            return BTreeMap::new();
        }
        let patch = self.unflatten(graph, layout);
        patch
            .subpatches
            .iter()
            .filter(|(_, definition)| definition.library.is_some())
            .map(|(key, definition)| (key.clone(), crate::library::fingerprint(definition, &patch.subpatches)))
            .collect()
    }

    /// The sub-patch tile `instance` as a library file: its definition at
    /// the top level, with the definitions nested in it (sub-patches.md,
    /// Library). `None` for a tile that is not a sub-patch.
    pub fn definition_file(&self, graph: &Graph, layout: impl Fn(u32) -> Layout, instance: ModuleId) -> Option<Patch> {
        let key = &self.instances.get(self.ids.path_of(instance)?)?.key;
        let patch = self.unflatten(graph, layout);
        let definition = patch.subpatches.get(key)?;
        Some(Patch::from_definition(definition, &patch.subpatches))
    }

    /// The definition key of the instance shown at `level`. `None` at the
    /// top.
    pub fn key_at(&self, level: &ModulePath) -> Option<&str> {
        self.instances.get(level).map(|i| i.key.as_str())
    }

    /// The keys of the definitions the one at `key` uses, through any
    /// depth, as a file written from it would carry them.
    pub fn nested_keys(&self, graph: &Graph, key: &str) -> Vec<String> {
        let patch = self.unflatten(graph, |_| Layout::default());
        patch
            .subpatches
            .get(key)
            .map(|definition| reachable_keys(definition, &patch.subpatches))
            .unwrap_or_default()
    }
}

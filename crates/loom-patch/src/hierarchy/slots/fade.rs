//! Builds and retires the transient wiring for slot crossfades.

use super::*;

impl Hierarchy {
    /// What `sync` wires while slots fade (sub-patches.md, How a swap
    /// crossfades): each slot input to the leaving part too, each part's
    /// role output to its fade, and what the slot's output fed onto the
    /// fade's `out`.
    pub(in crate::hierarchy) fn fade_wiring(&self, graph: &Graph) -> FadeWiring {
        let mut wiring = FadeWiring::default();
        for (slot_path, state) in &self.slots {
            let Some(fading) = &state.fading else { continue };
            let own = self.slot_fade_wiring(graph, slot_path, state, fading);
            // Every other stack copy is wired as copy 0 is, each end moved
            // into that copy by path; an end outside the stack feeds every
            // copy and stays (ADR-0044).
            for copy in &fading.copies {
                let fade_k = |fade: u32| -> Option<u32> {
                    let (role_out, _) = fading.fades.iter().find(|(_, f)| f.0 == fade)?;
                    copy.fades.iter().find(|(r, _)| r == role_out).map(|(_, f)| f.0)
                };
                let end_k = |end: &End| -> Option<End> {
                    let id = fade_k(end.0)
                        .map(ModuleId)
                        .or_else(|| self.in_copy(ModuleId(end.0), &copy.level))?;
                    Some((id.0, end.1.clone()))
                };
                for (from, to) in &own.add {
                    if let (Some(from), Some(to)) = (end_k(from), end_k(to)) {
                        wiring.add.push((from, to));
                    }
                }
                for (from, to) in &own.reroute {
                    if let (Some(from), Some(to)) = (end_k(from), end_k(to)) {
                        wiring.reroute.push((from, to));
                    }
                }
                // Out of an empty thru slot, a copy's `b` and the inputs
                // its fade takes over are its own thru ends, recorded at
                // the swap: a path cannot name its port on a consumer
                // outside the stack (ADR-0046). Copy 0's own wiring adds no
                // `b` for it to move.
                if state.part.is_none() {
                    self.thru_ends_wiring(graph, &copy.thru_source, &copy.thru_dests, &copy.fades, &mut wiring);
                }
            }
            wiring.add.extend(own.add);
            wiring.reroute.extend(own.reroute);
            wiring.retarget.extend(own.retarget);
            wiring.replace.extend(own.replace);
        }
        wiring
    }

    /// The fade wiring of one slot in copy 0.
    fn slot_fade_wiring(
        &self,
        graph: &Graph,
        slot_path: &ModulePath,
        state: &SlotState,
        fading: &Fading,
    ) -> FadeWiring {
        let mut wiring = FadeWiring::default();
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let slot_lid = slot_path.last();
        // What feeds the slot's role input `name`, at the flat level.
        let fed = |name: &str| -> Option<(ModuleId, String)> {
            let link = self.link_into(&level, &(slot_lid, name.to_string()))?;
            self.source_behind(graph, &level, &link.from)
        };
        for (role_out, fade) in &fading.fades {
            // `a`: the leaving part, or the thru signal a part fades in
            // from (sub-patches.md, Empty slots).
            let a = match fading.leaving {
                Some(leaving) => self.part_output(graph, &level, leaving, role_out),
                None => fed(loom_core::THRU_IN),
            };
            if let Some((l, l_out)) = a {
                wiring.add.push(((l.0, l_out), (fade.0, "a".to_string())));
            }
            match state.part {
                Some(part) => {
                    if let Some((p, p_out)) = self.part_output(graph, &level, part, role_out) {
                        wiring.add.push(((p.0, p_out.clone()), (fade.0, "b".to_string())));
                        wiring.reroute.push(((p.0, p_out), (fade.0, "out".to_string())));
                    }
                }
                None => {
                    // A part fading out to the thru: `b` is the thru
                    // signal, and the fade takes over only the inputs
                    // the slot's output fed, by destination, so other
                    // inputs the thru source feeds are untouched. With
                    // nothing on `in`, the part fades to 0.
                    if self.in_stack(&level) {
                        // Inside a stack, copy 0 takes its own ends from
                        // the graph at the swap, as every copy does
                        // (ADR-0046).
                        let own = [(role_out.clone(), *fade)];
                        self.thru_ends_wiring(graph, &fading.thru_source, &fading.thru_dests, &own, &mut wiring);
                        continue;
                    }
                    if let Some((x, x_out)) = fed(loom_core::THRU_IN) {
                        wiring.add.push(((x.0, x_out), (fade.0, "b".to_string())));
                    }
                    let dests: Vec<End> = self
                        .links_from(&level, &(slot_lid, role_out.clone()))
                        .map(|l| l.to)
                        .collect();
                    for dest in dests {
                        for (to, input) in self.inputs_behind(graph, &level, &dest) {
                            // A stack downstream runs this input in every
                            // copy, each fed from the same source, so the
                            // fade feeds them all (ADR-0044).
                            for copy in std::iter::once(to).chain(self.copies_of(to).iter().map(|(c, _)| *c)) {
                                wiring
                                    .retarget
                                    .push(((copy.0, input.clone()), (fade.0, "out".to_string())));
                            }
                        }
                    }
                }
            }
        }
        // The leaving part keeps its inputs while it fades.
        if let Some(leaving) = fading.leaving {
            for role_in in state.role.inputs {
                let Some(src) = fed(role_in.name) else { continue };
                for dest in self.part_inputs(graph, &level, leaving, role_in.name) {
                    wiring.add.push(((src.0.0, src.1.clone()), (dest.0.0, dest.1)));
                }
            }
        }
        wiring
    }

    /// The fade wiring of one stack copy, copy 0 or another, whose part fades out to its thru
    /// signal, from the ends `source` and `dests` recorded at the swap
    /// (ADR-0046): the thru source feeds the copy's fade's `b` while its
    /// module is still there, and each recorded input moves onto the
    /// fade's `out` only while its thru cable is still wanted.
    fn thru_ends_wiring(
        &self,
        graph: &Graph,
        source: &Option<End>,
        dests: &[End],
        fades: &[(String, ModuleId)],
        wiring: &mut FadeWiring,
    ) {
        let there = source.clone().filter(|end| graph.node(ModuleId(end.0)).is_some());
        for (role_out, fade) in fades {
            if role_out != loom_core::THRU_OUT {
                continue;
            }
            if let Some(there) = &there {
                wiring.add.push((there.clone(), (fade.0, "b".to_string())));
            }
            for dest in dests {
                wiring
                    .replace
                    .push((source.clone(), dest.clone(), (fade.0, "out".to_string())));
            }
        }
    }

    /// The thru ends of the leaving part `lid` of the slot at `level`, in
    /// the copy whose level is `level_k`, read from the graph before the
    /// swap empties the slot (ADR-0046): the output that feeds the part's
    /// `in` there, and every input outside the part that its role output
    /// feeds.
    pub(in crate::hierarchy::slots) fn copy_thru_ends(
        &self,
        graph: &Graph,
        level: &ModulePath,
        lid: u32,
        level_k: &ModulePath,
    ) -> ThruEnds {
        let inside = level_k.child(lid);
        let under_part = |module: ModuleId| self.ids.path_of(module).is_some_and(|p| p.starts_with(&inside));
        let source = self
            .part_inputs(graph, level, lid, loom_core::THRU_IN)
            .into_iter()
            .find_map(|(id, own)| {
                let id = self.in_copy(id, level_k)?;
                let port = graph.node(id)?.input_index(&own)?;
                let input = InputRef { module: id, port };
                let from = graph.source_of(input)?;
                Some(cable_ends(graph, from, input).0)
            });
        let dests = self
            .part_output(graph, level, lid, loom_core::THRU_OUT)
            .and_then(|(id, own)| {
                let id = self.in_copy(id, level_k)?;
                let port = graph.node(id)?.output_index(&own)?;
                Some(OutputRef { module: id, port })
            })
            .map(|output| {
                graph
                    .cables()
                    .filter(|(from, to)| *from == output && !under_part(to.module))
                    .map(|(from, to)| cable_ends(graph, from, to).1)
                    .collect()
            })
            .unwrap_or_default();
        (source, dests)
    }

    /// The slot's level `level` in every stack copy past the first, at any
    /// nesting depth: copy 0's level with each copy's steps put in after
    /// its stack's instance (ADR-0040, Copies in the editor). Empty outside
    /// a stack of more than one copy.
    pub fn copy_levels(&self, level: &ModulePath) -> Vec<ModulePath> {
        let mut partial = vec![ModulePath(Vec::new())];
        for n in 1..=level.0.len() {
            let step = level.0[n - 1];
            for path in &mut partial {
                path.0.push(step);
            }
            let copies = self
                .modes_at(&level.prefix(n))
                .filter(|m| m.mode == crate::modes::Mode::Stack)
                .map_or(1, |m| m.copy_count());
            if copies > 1 {
                let more: Vec<ModulePath> = partial
                    .iter()
                    .flat_map(|path| (1..copies).map(move |k| path.copy(k)))
                    .collect();
                partial.extend(more);
            }
        }
        partial.retain(|path| path != level);
        partial
    }

    /// The flat module in the copy whose level is `level_k` that stands for
    /// copy 0's module `id`: its path with the copy's steps put in where
    /// both run inside the same stacks. A module outside those stacks, which
    /// feeds every copy, is itself.
    fn in_copy(&self, id: ModuleId, level_k: &ModulePath) -> Option<ModuleId> {
        let path = self.ids.path_of(id)?;
        let mut out = Vec::new();
        let mut i = 0;
        for &step in &level_k.0 {
            if step >= crate::flatten::COPY_STEP {
                // A copy step goes in only after a stack both paths share.
                if i > 0 {
                    out.push(step);
                }
                continue;
            }
            if path.0.get(i) == Some(&step) {
                out.push(step);
                i += 1;
            } else {
                break;
            }
        }
        out.extend_from_slice(&path.0[i..]);
        self.ids.get(&ModulePath(out))
    }

    /// Adds, in every other copy of the stacks the slot at `slot_path` is
    /// in, the same fades as `fades` (copy 0's), at the same steps under
    /// that copy's slot path and with the same laws (ADR-0044), into an
    /// empty thru slot as well as out of one (ADR-0046). With a leaving
    /// part, a copy whose leaving part is missing is skipped. On an error
    /// part-way, the caller rolls back by comparing modules, as for
    /// [`Hierarchy::add_fades`].
    pub(in crate::hierarchy::slots) fn add_copy_fades(
        &mut self,
        controller: &mut Controller,
        slot_path: &ModulePath,
        leaving: Option<u32>,
        fades: &[(String, ModuleId)],
    ) -> Result<Vec<CopyFade>, String> {
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let mut made = Vec::new();
        for level_k in self.copy_levels(&level) {
            if let Some(leaving) = leaving
                && self.ids.get(&level_k.child(leaving)).is_none()
                && !self.ids.paths().any(|(p, _)| p.starts_with(&level_k.child(leaving)))
            {
                continue;
            }
            let slot_k = level_k.child(slot_path.last());
            let mut copy_fades = Vec::new();
            for (role_out, fade) in fades {
                let step = self
                    .ids
                    .path_of(*fade)
                    .map(ModulePath::last)
                    .ok_or("a fade has no path")?;
                let law = controller
                    .graph()
                    .node(*fade)
                    // Copy 0's law, by name, so another parameter cannot change it.
                    .and_then(|n| Some(n.params[n.descriptor.param_index("law")?]))
                    .unwrap_or(0.0);
                let id = self.ids.id_of(&slot_k.child(step)).map_err(|e| e.to_string())?;
                controller
                    .add_module_with_id(id, FADE_KIND)
                    .map_err(|e| e.to_string())?;
                copy_fades.push((role_out.clone(), id));
                controller.set_param_named(id, "law", law).map_err(|e| e.to_string())?;
            }
            made.push(CopyFade {
                level: level_k,
                fades: copy_fades,
                thru_source: None,
                thru_dests: Vec::new(),
            });
        }
        Ok(made)
    }

    /// Every module that exists only while a slot fades, in every copy:
    /// each leaving part and everything under it, and each swap fade.
    /// `sync` leaves them, and the assignments onto them, alone
    /// (ADR-0044).
    pub fn fading_ids(&self) -> BTreeSet<ModuleId> {
        let mut ids = BTreeSet::new();
        for (path, state) in &self.slots {
            let Some(fading) = &state.fading else { continue };
            let level = path.parent().unwrap_or_else(ModulePath::root);
            let mut levels = vec![level];
            levels.extend(fading.copies.iter().map(|c| c.level.clone()));
            if let Some(lid) = fading.leaving {
                for level in &levels {
                    let leaving = level.child(lid);
                    ids.extend(
                        self.ids
                            .paths()
                            .filter(|(p, _)| p.starts_with(&leaving))
                            .map(|(_, id)| id),
                    );
                }
            }
            ids.extend(fading.fades.iter().map(|(_, id)| *id));
            ids.extend(fading.copies.iter().flat_map(|c| c.fades.iter().map(|(_, id)| *id)));
        }
        ids
    }

    /// Adds one swap fade per `(role output, law)` under the slot at
    /// `slot_path`, with its law set (sub-patches.md, How a swap
    /// crossfades). Returns them as `(role output, flat id)`. On an error
    /// part-way, the fades already added stay in the controller: the
    /// caller works on a candidate hierarchy and rolls back by comparing
    /// the graph's modules with what it held before (`swap_now`).
    pub(in crate::hierarchy::slots) fn add_fades(
        &mut self,
        controller: &mut Controller,
        slot_path: &ModulePath,
        outputs: Vec<(String, Law)>,
    ) -> Result<Vec<(String, ModuleId)>, String> {
        let mut made = Vec::new();
        for (role_out, law) in outputs {
            let k = self.next_level_id(slot_path);
            let fade_path = slot_path.child(k);
            let fade_id = self.ids.id_of(&fade_path).map_err(|e| e.to_string())?;
            controller
                .add_module_with_id(fade_id, FADE_KIND)
                .map_err(|e| e.to_string())?;
            made.push((role_out, fade_id));
            controller
                .set_param_named(fade_id, "law", fade_law_value(law))
                .map_err(|e| e.to_string())?;
        }
        Ok(made)
    }

    /// True when a swap into or out of the empty slot at `slot_path` fades
    /// from or to its thru signal: a thru role, inside any number of
    /// stacks, each copy from or to its own thru signal (ADR-0046;
    /// sub-patches.md, Empty slots). Any other role changes as a cable
    /// change, which live-patching rule 9 ramps over 5 ms.
    pub(in crate::hierarchy::slots) fn fades_with_thru(&self, slot_path: &ModulePath) -> bool {
        self.slots.get(slot_path).is_some_and(|s| s.role.thru)
    }

    /// The thru output of the slot at `slot_path` that `part` (a level id)
    /// has, for a fade between the part and the thru signal: Linear, since
    /// the dry signal and the signal through the part are nearly the same.
    pub(in crate::hierarchy::slots) fn thru_fade_outputs(
        &self,
        graph: &Graph,
        slot_path: &ModulePath,
        part: u32,
    ) -> Vec<(String, Law)> {
        let level = slot_path.parent().unwrap_or_else(ModulePath::root);
        let out = loom_core::THRU_OUT;
        if self.part_output(graph, &level, part, out).is_some() {
            vec![(out.to_string(), Law::Linear)]
        } else {
            Vec::new()
        }
    }
}

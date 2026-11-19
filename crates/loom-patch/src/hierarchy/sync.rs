//! Reconciles the editable hierarchy with the engine graph.

use super::*;

impl Hierarchy {
    /// Makes the controller's cables the ones the file would load: the
    /// hierarchy is written out, flattened with the flattener itself (which
    /// also refuses what a file could not hold), and the difference in
    /// cables is applied. It also puts the live assignments on each
    /// parameter in the order the file loads them (flattening rule 11),
    /// giving a new id only to one that must move, so the sum plays the
    /// same after save and reload (sub-patches.md, Editor model), and gives
    /// each exposed parameter the range MIDI sweeps on it (Exposing).
    /// Returns the assignments that got a new id, as (old, new), so what
    /// keeps an id can follow. The caller commits.
    ///
    /// # Errors
    ///
    /// Invalid patches and shortages for the batch of new DSP modules
    /// are refused before changing the graph. If the controller refuses
    /// a later operation, the caller must take back the uncommitted edit.
    pub fn sync(&mut self, controller: &mut Controller) -> Result<Vec<(AssignmentId, AssignmentId)>, PatchError> {
        self.sync_without(controller, &[])
    }

    /// [`Hierarchy::sync`] with `skip` left out of the file, as if those
    /// modules were gone: a delete checks and settles the cables first,
    /// while the modules are still in the graph. Cables into and out of
    /// them are disconnected. Errors follow the rules of [`Hierarchy::sync`].
    pub fn sync_without(
        &mut self,
        controller: &mut Controller,
        skip: &[ModuleId],
    ) -> Result<Vec<(AssignmentId, AssignmentId)>, PatchError> {
        // The modules the caller is removing: a fade never takes over an
        // input of one (ADR-0046), and one with no path is given none.
        let going: BTreeSet<ModuleId> = skip.iter().copied().collect();
        let mut skip = going.clone();
        skip.extend(self.transient_ids());
        self.refresh_member_entries();
        for (id, _) in controller.graph().nodes() {
            if self.ids.path_of(id).is_none() && !going.contains(&id) {
                // Refused only when another path owns the id, which a
                // module the controller holds never has.
                let _ = self.ids.reserve(ModulePath::top(id.0), id);
            }
        }
        // Do not write an old implied cable as direct when its link was
        // just pulled. Dissolve explicitly promotes the joins it keeps.
        let links_only = self.unflatten_with(controller.graph(), |_| Layout::default(), None, &skip);
        let mut links_scratch = self.ids.clone();
        let links_flat = links_only.flatten(controller.registry(), &mut links_scratch)?;
        let mut implied: BTreeSet<(End, End)> = links_flat
            .cables
            .iter()
            .map(|cable| (cable.from.clone(), cable.to.clone()))
            .collect();
        let suppressed = self.implied.union(&implied).cloned().collect();
        let patch = self.unflatten_with(controller.graph(), |_| Layout::default(), Some(&suppressed), &skip);
        // On the links' table, so implied modules have the same ids.
        let mut scratch = links_scratch.clone();
        let flat = patch.flatten(controller.registry(), &mut scratch)?;
        // Stack copies and the modules a mode adds are the flattener's, with
        // every cable on them (ADR-0040).
        let implied_ids: BTreeSet<u32> = flat
            .modules
            .iter()
            .filter(|m| m.path.is_implied())
            .map(|m| m.id.0)
            .collect();
        implied.extend(
            flat.cables
                .iter()
                .filter(|c| implied_ids.contains(&c.from.0) || implied_ids.contains(&c.to.0))
                .map(|cable| (cable.from.clone(), cable.to.clone())),
        );
        self.sync_implied_modules(controller, &flat, &scratch)?;
        self.implied = implied;
        let mut desired: BTreeSet<(End, End)> = flat
            .cables
            .iter()
            .map(|cable| (cable.from.clone(), cable.to.clone()))
            .collect();
        debug_assert_eq!(
            self.implied,
            desired.difference(&self.direct_cables(&patch)).cloned().collect(),
            "the links imply the same cables both ways"
        );
        // While a slot fades, both parts run and the fades sit between the
        // new part and what the slot's output fed (sub-patches.md, How a
        // swap crossfades).
        let pulled = self.pulled_fade_inputs(controller.graph());
        let wiring = self.fade_wiring(controller.graph());
        for (part_out, fade_out) in &wiring.reroute {
            let moved: Vec<(End, End)> = desired
                .iter()
                .filter(|(from, to)| from == part_out && to.0 != fade_out.0)
                .cloned()
                .collect();
            for (from, to) in moved {
                desired.remove(&(from, to.clone()));
                desired.insert((fade_out.clone(), to));
            }
        }
        for (input, fade_out) in &wiring.retarget {
            desired.retain(|(_, to)| to != input);
            desired.insert((fade_out.clone(), input.clone()));
        }
        // A stack copy's fade takes over an input only while its thru cable
        // is still wanted, so an edit during the fade wins (ADR-0046).
        for (thru, input, fade_out) in &wiring.replace {
            if pulled.contains(input) {
                continue;
            }
            let take = match thru {
                Some(thru) => desired.remove(&(thru.clone(), input.clone())),
                None => {
                    input_ref(controller.graph(), input)
                        .and_then(|input| controller.graph().source_of(input))
                        .is_some()
                        && !going.contains(&ModuleId(input.0))
                        && !desired.iter().any(|(_, to)| to == input)
                }
            };
            if take {
                desired.insert((fade_out.clone(), input.clone()));
            }
        }
        desired.extend(wiring.add);
        let current = graph_cables(controller.graph());
        for (_, to) in current.difference(&desired) {
            let input = input_ref(controller.graph(), to);
            if let Some(input) = input {
                controller.disconnect(input)?;
            }
        }
        for (from, to) in desired.difference(&current) {
            controller.connect_named(ModuleId(from.0), &from.1, ModuleId(to.0), &to.1)?;
        }
        // While a slot fades in a stack, a wanted assignment from a copy's
        // new part comes from that copy's fade (ADR-0044).
        let through_fade: BTreeMap<End, End> = wiring.reroute.iter().cloned().collect();
        self.sync_implied_links(controller, &flat, &through_fade)?;
        let renamed = order_assignments(controller, &flat.modulation, &through_fade).map_err(PatchError::Engine)?;
        // The range MIDI sweeps follows the surfaces: a range that changed
        // or went away reaches the engine with the caller's commit.
        controller
            .set_limits(flat.limits_in(controller.graph()))
            .map_err(PatchError::Engine)?;
        self.forget_fade_inputs(&pulled);
        self.pulled.clear();
        self.note_flat(&flat);
        self.synced_revision = SyncMark(Some(controller.revision()));
        Ok(renamed)
    }
}

/// Puts the live assignments on each parameter in the order `canonical`
/// lists them, which is the order the file loads them: each instance's
/// inner ones first, then the level's own (flattening rule 11). The engine
/// sums assignments on one parameter in id order, so an assignment whose
/// id is below the one before it in that order gets a fresh id; every
/// other one keeps its id. Two assignments from one source onto one
/// parameter, which only a hand-written file makes, are matched to the
/// file's entries in id order, so a settled order stays settled. A live
/// assignment the file does not hold is left alone. Only the graph
/// changes, so a full command queue cannot stop this part-way; the
/// caller's commit carries it to the engine. Returns the ids that changed,
/// as (old, new).
fn order_assignments(
    controller: &mut Controller,
    canonical: &[PatchModulation],
    through_fade: &BTreeMap<End, End>,
) -> Result<Vec<(AssignmentId, AssignmentId)>, loom_core::LoomError> {
    let graph = controller.graph();
    let mut live: BTreeMap<(End, End), Vec<AssignmentId>> = BTreeMap::new();
    for (id, a) in graph.assignments() {
        let source = (a.source.module.0, crate::port_name(graph, a.source));
        let target = (a.target.0, crate::param_name(graph, a.target, a.param));
        live.entry((source, target)).or_default().push(id);
    }
    let mut groups: BTreeMap<End, Vec<AssignmentId>> = BTreeMap::new();
    for entry in canonical {
        let source = through_fade.get(&entry.source).unwrap_or(&entry.source);
        let ids = live.get_mut(&(source.clone(), entry.target.clone()));
        if let Some(id) = ids.and_then(|ids| (!ids.is_empty()).then(|| ids.remove(0))) {
            groups.entry(entry.target.clone()).or_default().push(id);
        }
    }
    let mut renamed = Vec::new();
    for ids in groups.into_values() {
        let mut last: Option<AssignmentId> = None;
        for id in ids {
            let placed = match last {
                Some(last) if id <= last => {
                    let fresh = controller.renumber_assignment(id)?;
                    renamed.push((id, fresh));
                    fresh
                }
                _ => id,
            };
            last = Some(placed);
        }
    }
    Ok(renamed)
}

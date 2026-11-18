//! Looks up the visible graph and follows connections through boundaries.

use super::*;

impl Hierarchy {
    /// The id table.
    pub fn ids(&self) -> &FlatIds {
        &self.ids
    }

    /// True when the patch has no sub-patch.
    pub fn is_flat(&self) -> bool {
        self.instances.is_empty() && self.slots.is_empty()
    }

    /// The name a tile shows: a definition's name for an instance, a port's
    /// name for a port. `None` for a module or an id no tile has.
    pub fn tile_name(&self, id: ModuleId) -> Option<String> {
        let path = self.ids.path_of(id)?;
        if let Some(definition) = self.definition_at(path) {
            return Some(definition.metadata.name.clone());
        }
        if let Some(slot) = self.slots.get(path) {
            return Some(slot.name());
        }
        self.ports.get(path).map(|port| port.name.clone())
    }

    /// The level nearest to `level` that still exists: `level` itself, or
    /// the longest prefix of it that is the top or an instance.
    pub fn nearest_level(&self, level: &ModulePath) -> ModulePath {
        (0..=level.0.len())
            .rev()
            .map(|n| level.prefix(n))
            .find(|prefix| prefix.is_root() || self.instances.contains_key(prefix))
            .unwrap_or_else(ModulePath::root)
    }

    /// The path of a tile: the table's, or a top-level path for a graph
    /// module the table never saw.
    pub fn path_of(&self, id: ModuleId) -> ModulePath {
        self.ids.path_of(id).cloned().unwrap_or_else(|| ModulePath::top(id.0))
    }

    /// The level a tile is shown on.
    pub fn level_of(&self, id: ModuleId) -> ModulePath {
        self.path_of(id).parent().unwrap_or_else(ModulePath::root)
    }

    /// What a flat id stands for, or `None` for an id no live tile has.
    pub fn tile(&self, graph: &Graph, id: ModuleId) -> Option<Tile> {
        if self.is_hidden(id) {
            return None;
        }
        self.shown_tile(graph, id)
    }

    /// What tile `id` shows, for an id already known not to be hidden.
    pub(super) fn shown_tile(&self, graph: &Graph, id: ModuleId) -> Option<Tile> {
        if graph.node(id).is_some() {
            return Some(Tile::Module);
        }
        let path = self.ids.path_of(id)?;
        if let Some(instance) = self.instances.get(path) {
            return Some(Tile::Instance(instance.key.clone()));
        }
        if let Some(slot) = self.slots.get(path) {
            return Some(Tile::Slot(slot.role.kind()));
        }
        self.ports
            .get(path)
            .map(|port| Tile::Port(port.direction, port.name.clone()))
    }

    /// The definition of an instance tile.
    pub fn definition_of(&self, instance: ModuleId) -> Option<&Definition> {
        let path = self.ids.path_of(instance)?;
        self.definition_at(path)
    }

    /// The definition shown at `level`, or `None` at the top.
    pub fn definition_at(&self, level: &ModulePath) -> Option<&Definition> {
        let instance = self.instances.get(level)?;
        self.definitions.get(&instance.key)
    }

    /// Authors of the sub-patches this patch carries, including parts a
    /// slot holds for later. The caller removes repeated and top-level names.
    pub fn part_authors(&self) -> impl Iterator<Item = &str> {
        let leaving = self.leaving_paths();
        let live = self
            .instances
            .iter()
            .filter(move |(path, _)| !leaving.covers(path))
            .filter_map(|(_, instance)| self.definitions.get(&instance.key))
            .flat_map(|definition| &definition.metadata.authors);
        let held = self
            .definitions
            .values()
            .filter_map(|definition| definition.stored.as_ref())
            .flat_map(|part| std::iter::once(&part.definition).chain(part.nested.values()))
            .flat_map(|definition| &definition.metadata.authors);
        live.chain(held).map(String::as_str)
    }

    /// The names along a level's path, outermost first, for the breadcrumb.
    pub fn path_names(&self, level: &ModulePath) -> Vec<String> {
        (1..=level.0.len())
            .filter_map(|n| {
                let prefix = ModulePath(level.0[..n].to_vec());
                self.level_name(&prefix)
            })
            .collect()
    }

    /// The instance and port tiles of a level, in level-id order, each with
    /// what it is.
    pub fn structural_tiles(&self, level: &ModulePath) -> Vec<(ModuleId, Tile)> {
        let mut tiles: Vec<(u32, ModuleId, Tile)> = Vec::new();
        for (path, instance) in &self.instances {
            if path.parent().as_ref() == Some(level)
                && let Some(id) = self.ids.get(path)
            {
                tiles.push((path.last(), id, Tile::Instance(instance.key.clone())));
            }
        }
        for (path, port) in &self.ports {
            if path.parent().as_ref() == Some(level)
                && let Some(id) = self.ids.get(path)
            {
                tiles.push((path.last(), id, Tile::Port(port.direction, port.name.clone())));
            }
        }
        for (path, slot) in &self.slots {
            if path.parent().as_ref() == Some(level)
                && let Some(id) = self.ids.get(path)
            {
                tiles.push((path.last(), id, Tile::Slot(slot.role.kind())));
            }
        }
        tiles.sort_by_key(|(lid, _, _)| *lid);
        tiles.into_iter().map(|(_, id, tile)| (id, tile)).collect()
    }

    /// The ports of an instance's tile, inputs then outputs, each in
    /// level-id order: the port tile's flat id and its name.
    pub fn instance_ports(&self, instance: ModuleId) -> (PortList, PortList) {
        let Some(path) = self.ids.path_of(instance) else {
            return (Vec::new(), Vec::new());
        };
        // A layer or select: its members' ports, by name, in the order they
        // first appear (sub-patches.md, Members are nested instances).
        if let Some(modes) = self.modes_at(path).filter(|m| m.mode.has_members()) {
            let (mut inputs, mut outputs): (PortList, PortList) = (Vec::new(), Vec::new());
            for lid in &modes.members {
                let Some(member) = self.ids.get(&path.child(*lid)) else {
                    continue;
                };
                let (member_inputs, member_outputs) = self.instance_ports(member);
                for (list, more) in [(&mut inputs, member_inputs), (&mut outputs, member_outputs)] {
                    for (id, name) in more {
                        if !list.iter().any(|(_, n)| *n == name) {
                            list.push((id, name));
                        }
                    }
                }
            }
            return (inputs, outputs);
        }
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for (port_path, port) in &self.ports {
            if port_path.parent().as_ref() != Some(path) {
                continue;
            }
            let Some(id) = self.ids.get(port_path) else { continue };
            match port.direction {
                Direction::Input => inputs.push((port_path.last(), id, port.name.clone())),
                Direction::Output => outputs.push((port_path.last(), id, port.name.clone())),
            }
        }
        inputs.sort_by_key(|(lid, _, _)| *lid);
        outputs.sort_by_key(|(lid, _, _)| *lid);
        let strip = |list: Vec<(u32, ModuleId, String)>| list.into_iter().map(|(_, id, name)| (id, name)).collect();
        (strip(inputs), strip(outputs))
    }

    /// The flat id of the tile with level id `lid` at `level`. At the top
    /// level the two are the same number, so a module the table never saw
    /// is found too.
    pub fn id_at(&self, level: &ModulePath, lid: u32) -> Option<ModuleId> {
        self.ids
            .get(&level.child(lid))
            .or_else(|| level.is_root().then_some(ModuleId(lid)))
    }

    /// The cables a level shows, in its ids: the graph's cables between two
    /// of its modules (those the links do not imply), then the links.
    pub fn level_cables(&self, graph: &Graph, level: &ModulePath) -> Vec<LevelCable> {
        let mut cables = Vec::new();
        let hidden = self.hidden();
        for (from, to) in graph.cables() {
            if hidden.contains(from.module) || hidden.contains(to.module) {
                continue;
            }
            let (from_end, to_end) = cable_ends(graph, from, to);
            if self.implied.contains(&(from_end.clone(), to_end.clone())) {
                continue;
            }
            let (from_path, to_path) = (self.path_of(from.module), self.path_of(to.module));
            if from_path.parent().as_ref() == Some(level) && to_path.parent().as_ref() == Some(level) {
                cables.push(LevelCable {
                    from: (from_path.last(), from_end.1),
                    to: (to_path.last(), to_end.1),
                    link: false,
                });
            }
        }
        for link in self.links_at(level) {
            cables.push(LevelCable {
                from: link.from.clone(),
                to: link.to.clone(),
                link: true,
            });
        }
        cables
    }

    /// True when the graph holds `from` to `to` only because a link
    /// implies it.
    pub fn is_implied(&self, graph: &Graph, from: OutputRef, to: InputRef) -> bool {
        self.implied.contains(&cable_ends(graph, from, to))
    }

    /// The flat output that stands behind an end of `level`: a module's
    /// output itself, what is behind an instance's output port, or what
    /// feeds a Patch input from outside. `None` when nothing does.
    pub fn source_behind(&self, graph: &Graph, level: &ModulePath, end: &End) -> Option<(ModuleId, String)> {
        // A loop, not recursion, so a long chain of pass-through ports or
        // empty thru slots cannot overflow the stack.
        let mut seen: BTreeSet<(ModulePath, End)> = BTreeSet::new();
        let (mut level, mut end) = (level.clone(), end.clone());
        loop {
            if !seen.insert((level.clone(), end.clone())) {
                return None;
            }
            match self.source_step(graph, &level, &end)? {
                Step::Found(found) => return Some(found),
                Step::Next(next_level, next_end) => (level, end) = (next_level, next_end),
            }
        }
    }

    /// One step of [`Hierarchy::source_behind`]: the flat output the end is,
    /// the end one link further back, or `None` when nothing stands behind
    /// it. Each end has at most one step back, which is what lets
    /// [`behind::Behind`] give every end on a chain the chain's answer.
    pub(crate) fn source_step(&self, graph: &Graph, level: &ModulePath, end: &End) -> Option<Step> {
        #[cfg(any(test, feature = "test-follows"))]
        behind::test_support::follow();
        let path = level.child(end.0);
        if let Some(port) = self.ports.get(&path) {
            if port.direction != Direction::Input || end.1 != "out" {
                return None;
            }
            // What feeds the instance's port, one level up.
            let parent = level.parent()?;
            let link = self.link_into(&parent, &(level.last(), port.name.clone()))?;
            return Some(Step::Next(parent, link.from.clone()));
        }
        if self.instances.contains_key(&path) {
            // A mode's sum or select stands behind the port.
            if let Some(added) = self.added_output(&path, &end.1) {
                return Some(Step::Found((added, "out".to_string())));
            }
            // A layer of one member: the member's port.
            if let Some(modes) = self.modes_at(&path).filter(|m| m.mode.has_members()) {
                let member = *modes.members.first()?;
                return Some(Step::Next(path, (member, end.1.clone())));
            }
            let port_path = self.port_path(&path, Direction::Output, &end.1)?;
            let link = self.link_into(&path, &(port_path.last(), "in".to_string()))?;
            return Some(Step::Next(path, link.from.clone()));
        }
        if let Some(slot) = self.slots.get(&path) {
            // A slot's role output: the part's own output behind it.
            slot.role.output(&end.1)?;
            let Some(part) = slot.part else {
                // An empty thru slot passes on what feeds its `in`
                // (ADR-0043); any other empty slot has nothing.
                if slot.role.thru && end.1 == loom_core::THRU_OUT {
                    let link = self.link_into(level, &(end.0, loom_core::THRU_IN.to_string()))?;
                    return Some(Step::Next(level.clone(), link.from.clone()));
                }
                return None;
            };
            let own = if self.instances.contains_key(&level.child(part)) {
                end.1.clone()
            } else {
                slot.own_output(&end.1)
            };
            return Some(Step::Next(level.clone(), (part, own)));
        }
        let id = self.ids.get(&path)?;
        let node = graph.node(id)?;
        node.output_index(&end.1)?;
        Some(Step::Found((id, end.1.clone())))
    }

    /// The flat inputs that stand behind an input end of `level`: a
    /// module's input itself, every inner input an instance's port feeds,
    /// what a Patch output feeds outside, or what an empty thru slot's
    /// `out` feeds.
    pub fn inputs_behind(&self, graph: &Graph, level: &ModulePath, end: &End) -> Vec<(ModuleId, String)> {
        // A work stack, not recursion, so a long chain cannot overflow the
        // stack; an end met twice is followed once.
        let mut found = Vec::new();
        let mut seen: BTreeSet<(ModulePath, End)> = BTreeSet::new();
        let mut work = vec![(level.clone(), end.clone())];
        while let Some((level, end)) = work.pop() {
            if !seen.insert((level.clone(), end.clone())) {
                continue;
            }
            match self.input_step(graph, &level, &end) {
                InputStep::Found(input) => found.push(input),
                InputStep::Next(next) => work.extend(next.into_iter().rev()),
                InputStep::Nothing => {}
            }
        }
        found
    }

    /// One step of [`Hierarchy::inputs_behind`]: the flat input the end is,
    /// the ends one link further on, in order, or nothing.
    pub(crate) fn input_step(&self, graph: &Graph, level: &ModulePath, end: &End) -> InputStep {
        #[cfg(any(test, feature = "test-follows"))]
        behind::test_support::follow();
        let path = level.child(end.0);
        let next = |at: &ModulePath, from: &End| -> InputStep {
            InputStep::Next(self.links_from(at, from).map(|l| (at.clone(), l.to.clone())).collect())
        };
        if let Some(port) = self.ports.get(&path) {
            if port.direction != Direction::Output || end.1 != "in" {
                return InputStep::Nothing;
            }
            let Some(parent) = level.parent() else {
                return InputStep::Nothing;
            };
            return next(&parent, &(level.last(), port.name.clone()));
        }
        if self.instances.contains_key(&path) {
            // A layer or select: every member's port of that name.
            if let Some(modes) = self.modes_at(&path).filter(|m| m.mode.has_members()) {
                return InputStep::Next(
                    modes
                        .members
                        .iter()
                        .map(|m| (path.clone(), (*m, end.1.clone())))
                        .collect(),
                );
            }
            let Some(port_path) = self.port_path(&path, Direction::Input, &end.1) else {
                return InputStep::Nothing;
            };
            return next(&path, &(port_path.last(), "out".to_string()));
        }
        if let Some(slot) = self.slots.get(&path) {
            if slot.role.input(&end.1).is_none() {
                return InputStep::Nothing;
            }
            let Some(part) = slot.part else {
                // An empty thru slot passes its `in` on to what its `out`
                // feeds (ADR-0043).
                if slot.role.thru && end.1 == loom_core::THRU_IN {
                    return next(level, &(end.0, loom_core::THRU_OUT.to_string()));
                }
                return InputStep::Nothing;
            };
            let own = if self.instances.contains_key(&level.child(part)) {
                end.1.clone()
            } else {
                slot.own_input(&end.1)
            };
            return InputStep::Next(vec![(level.clone(), (part, own))]);
        }
        match self.ids.get(&path) {
            Some(id) if graph.node(id).is_some_and(|n| n.input_index(&end.1).is_some()) => {
                InputStep::Found((id, end.1.clone()))
            }
            _ => InputStep::Nothing,
        }
    }

    /// The path of the port tile named `name` facing `direction` inside
    /// the instance at `instance`.
    fn port_path(&self, instance: &ModulePath, direction: Direction, name: &str) -> Option<ModulePath> {
        self.ports
            .iter()
            .find(|(path, port)| {
                path.parent().as_ref() == Some(instance) && port.direction == direction && port.name == name
            })
            .map(|(path, _)| path.clone())
    }

    /// The links drawn on `level`.
    pub(super) fn links_at<'a>(&'a self, level: &ModulePath) -> impl Iterator<Item = &'a Link> + 'a {
        self.links.at_level(level)
    }

    /// The one link into `to` on `level`, by range.
    pub(super) fn link_into(&self, level: &ModulePath, to: &End) -> Option<&Link> {
        self.links.linked_into(level, to)
    }

    /// The links out of `from` on `level`, by range.
    pub(super) fn links_from<'a>(&'a self, level: &ModulePath, from: &End) -> impl Iterator<Item = Link> + 'a {
        self.links.linked_from(level, from)
    }

    /// The name of the output port of `instance` behind which the flat
    /// output `source` stands.
    pub fn port_for_output(&self, graph: &Graph, instance: &ModulePath, source: &(ModuleId, String)) -> Option<String> {
        let parent = instance.parent()?;
        self.ports
            .iter()
            .filter(|(path, port)| path.parent().as_ref() == Some(instance) && port.direction == Direction::Output)
            .find(|(_, port)| {
                self.source_behind(graph, &parent, &(instance.last(), port.name.clone()))
                    .as_ref()
                    == Some(source)
            })
            .map(|(_, port)| port.name.clone())
    }

    /// True when an end of `level` is a module of the graph.
    pub(super) fn is_module_end(&self, graph: &Graph, level: &ModulePath, end: &End) -> bool {
        self.id_at(level, end.0).is_some_and(|id| graph.node(id).is_some())
    }
}

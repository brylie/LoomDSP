//! Display values and probes: one group of [`Controller`]'s methods.

use super::*;

/// # Display values and probes
///
/// What the engine shows back: display values, live modulated values, and
/// probes for meters and scopes (`docs/design/telemetry.md`).
impl Controller {
    /// Display value `index` of a module, as of the last block the engine
    /// ran, for a light on its panel (ADR-0013). `None` if the module does
    /// not exist or has no such value. A module the engine has not run yet
    /// reads 0. Reading a trigger value clears it (see [`DisplayReader::read`]).
    pub fn display(&self, module: ModuleId, index: usize) -> Option<f32> {
        self.display_source(module, index)
            .map(|source| self.display_reader().read(source))
    }

    /// The live value of a modulated parameter: the last frame of the
    /// latest block, as a position from 0 to 1, before any module input
    /// adds to it. `None` when the parameter is not modulated, has no cell
    /// (see [`CompileReport::without_live_value`]), or the engine has not
    /// swapped in the schedule that gives it one yet. Never waits.
    pub fn modulated(&self, module: ModuleId, param: usize) -> Option<f32> {
        self.live.modulated(module, param)
    }

    /// Where display value `index` of a module lives, for reading it with a
    /// [`DisplayReader`] on another thread. `None` if the module does not
    /// exist or has no such value.
    pub fn display_source(&self, module: ModuleId, index: usize) -> Option<DisplaySource> {
        let node = self.graph.nodes.get(&module)?;
        let slot = node.slot?;
        let spec = node.descriptor.displays.get(index)?;
        Some(DisplaySource::new(slot, module.0, index, spec.kind))
    }

    /// A handle that reads display values from any thread.
    pub fn display_reader(&self) -> DisplayReader {
        self.live.display_reader()
    }

    /// Starts watching an output port. The engine sends its telemetry from
    /// the next [`Controller::commit`] on (FR-VIS-04).
    ///
    /// Probes are view state: they are not part of the graph and are not
    /// saved in patches. Removing the module removes its probes.
    pub fn add_probe(&mut self, output: OutputRef, kind: ProbeKind) -> Result<ProbeId, LoomError> {
        self.check_output(output)?;
        let id = self.probes.add(output, kind);
        self.dirty = true;
        Ok(id)
    }

    /// Stops watching a port, from the next [`Controller::commit`] on.
    /// Telemetry already in the queues may still arrive.
    pub fn remove_probe(&mut self, id: ProbeId) -> Result<(), LoomError> {
        self.probes.remove(id).ok_or(LoomError::UnknownProbe(id))?;
        self.dirty = true;
        Ok(())
    }

    /// The port and kind of a probe.
    pub fn probe(&self, id: ProbeId) -> Option<(OutputRef, ProbeKind)> {
        self.probes.get(id)
    }

    /// Every probe, sorted by ID.
    pub fn probes(&self) -> impl Iterator<Item = (ProbeId, OutputRef, ProbeKind)> + '_ {
        self.probes.iter()
    }
}

/// The ports the UI watches, by probe id (FR-VIS-04). View state: not part
/// of the graph, and not saved.
pub(super) struct Probes {
    watched: BTreeMap<ProbeId, (OutputRef, ProbeKind)>,
    /// The id the next probe gets.
    next: u32,
}

impl Probes {
    /// No probes yet.
    pub(super) fn new() -> Self {
        Self {
            watched: BTreeMap::new(),
            next: 1,
        }
    }

    /// Watches `output`, and returns the new probe's id.
    pub(super) fn add(&mut self, output: OutputRef, kind: ProbeKind) -> ProbeId {
        let id = ProbeId(self.next);
        self.next += 1;
        self.watched.insert(id, (output, kind));
        id
    }

    /// Stops a probe. Returns what it watched, or `None` for an unknown id.
    pub(super) fn remove(&mut self, id: ProbeId) -> Option<(OutputRef, ProbeKind)> {
        self.watched.remove(&id)
    }

    /// Stops every probe on a module's outputs, for a removed module.
    pub(super) fn remove_on(&mut self, module: ModuleId) {
        self.watched.retain(|_, (from, _)| from.module != module);
    }

    /// The port and kind of a probe.
    pub(super) fn get(&self, id: ProbeId) -> Option<(OutputRef, ProbeKind)> {
        self.watched.get(&id).copied()
    }

    /// Every probe, sorted by id.
    pub(super) fn iter(&self) -> impl Iterator<Item = (ProbeId, OutputRef, ProbeKind)> + '_ {
        self.watched.iter().map(|(&id, &(from, kind))| (id, from, kind))
    }

    /// Every probe, as the compiler takes them.
    pub(super) fn as_map(&self) -> &BTreeMap<ProbeId, (OutputRef, ProbeKind)> {
        &self.watched
    }
}

/// Values the engine shows back: display values for lights, and live
/// values of modulated parameters, with the cell each one has in the live
/// table (`docs/design/modulation.md`, Live value). The tables are shared
/// with the engine; the cells are assigned here, at each commit.
pub(super) struct LiveValues {
    displays: Arc<DisplayTable>,
    live: Arc<LiveTable>,
    cells: LiveCells,
}

impl LiveValues {
    /// Reads the engine's `displays` and `live` tables. No parameter has a
    /// live cell yet.
    pub(super) fn new(displays: Arc<DisplayTable>, live: Arc<LiveTable>, cells: usize) -> Self {
        Self {
            displays,
            live,
            cells: LiveCells::new(cells),
        }
    }

    /// The live value of parameter `param` of `module`, if it has a cell
    /// and the engine has written it.
    pub(super) fn modulated(&self, module: ModuleId, param: usize) -> Option<f32> {
        let key = live_key(module, param);
        self.live.read(self.cells.cell(key)?, key)
    }

    /// A handle that reads display values from any thread.
    pub(super) fn display_reader(&self) -> DisplayReader {
        DisplayReader::new(self.displays.clone())
    }

    /// Gives cells to the modulated parameters in `keys`, keeping the ones
    /// they had. Returns the cells taken back, for the engine to release,
    /// and how many parameters got no cell because the table is full.
    pub(super) fn assign(&mut self, keys: &[u64]) -> (Vec<u32>, usize) {
        self.cells.assign(keys)
    }

    /// The cell of the parameter with `key`, if it has one.
    pub(super) fn cell(&self, key: u64) -> Option<u32> {
        self.cells.cell(key)
    }
}

/// The live-table key of a parameter, as the compiler builds it for fills.
fn live_key(module: ModuleId, param: usize) -> u64 {
    (u64::from(module.0) << 32) | param as u64
}

#[cfg(test)]
mod tests;

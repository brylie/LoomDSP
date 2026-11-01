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
        let key = (u64::from(module.0) << 32) | param as u64;
        self.live.read(self.live_cells.cell(key)?, key)
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
        DisplayReader::new(self.displays.clone())
    }

    /// Starts watching an output port. The engine sends its telemetry from
    /// the next [`Controller::commit`] on (FR-VIS-04).
    ///
    /// Probes are view state: they are not part of the graph and are not
    /// saved in patches. Removing the module removes its probes.
    pub fn add_probe(&mut self, output: OutputRef, kind: ProbeKind) -> Result<ProbeId, LoomError> {
        self.check_output(output)?;
        let id = ProbeId(self.next_probe);
        self.next_probe += 1;
        self.probes.insert(id, (output, kind));
        self.dirty = true;
        Ok(id)
    }

    /// Stops watching a port, from the next [`Controller::commit`] on.
    /// Telemetry already in the queues may still arrive.
    pub fn remove_probe(&mut self, id: ProbeId) -> Result<(), LoomError> {
        self.probes.remove(&id).ok_or(LoomError::UnknownProbe(id))?;
        self.dirty = true;
        Ok(())
    }

    /// The port and kind of a probe.
    pub fn probe(&self, id: ProbeId) -> Option<(OutputRef, ProbeKind)> {
        self.probes.get(&id).copied()
    }

    /// Every probe, sorted by ID.
    pub fn probes(&self) -> impl Iterator<Item = (ProbeId, OutputRef, ProbeKind)> + '_ {
        self.probes.iter().map(|(&id, &(from, kind))| (id, from, kind))
    }
}

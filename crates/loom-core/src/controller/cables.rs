//! Cables: one group of [`Controller`]'s methods.

use super::*;

/// # Cables
///
/// Connecting and disconnecting ports, and finding ports by name.
/// Cable edits take effect at the next [`commit`](Controller::commit).
impl Controller {
    /// Connects an output to an input. Any cable already on that input is
    /// replaced and returned.
    pub fn connect(&mut self, from: OutputRef, to: InputRef) -> Result<Option<OutputRef>, LoomError> {
        self.check_output(from)?;
        self.check_input(to)?;
        self.dirty = true;
        Ok(self.graph.cables.insert(to, from))
    }

    /// Connects ports by name.
    pub fn connect_named(
        &mut self,
        from: ModuleId,
        output: &str,
        to: ModuleId,
        input: &str,
    ) -> Result<Option<OutputRef>, LoomError> {
        let from = self.output_ref(from, output)?;
        let to = self.input_ref(to, input)?;
        self.connect(from, to)
    }

    /// Removes the cable on an input. Returns where it came from.
    pub fn disconnect(&mut self, to: InputRef) -> Result<Option<OutputRef>, LoomError> {
        self.check_input(to)?;
        let removed = self.graph.cables.remove(&to);
        if removed.is_some() {
            self.dirty = true;
        }
        Ok(removed)
    }

    /// Resolves an output port by name.
    pub fn output_ref(&self, module: ModuleId, name: &str) -> Result<OutputRef, LoomError> {
        let node = self.node(module)?;
        let port = node
            .unknown
            .as_ref()
            .map_or_else(
                || node.descriptor.output_index(name),
                |unknown| unknown.outputs.iter().position(|port| port == name),
            )
            .ok_or_else(|| LoomError::NoSuchPort {
                module,
                direction: "output",
                port: name.to_string(),
            })?;
        Ok(OutputRef { module, port })
    }

    /// Resolves an input port by name.
    pub fn input_ref(&self, module: ModuleId, name: &str) -> Result<InputRef, LoomError> {
        let node = self.node(module)?;
        let port = node
            .unknown
            .as_ref()
            .map_or_else(
                || node.descriptor.input_index(name),
                |unknown| unknown.inputs.iter().position(|port| port == name),
            )
            .ok_or_else(|| LoomError::NoSuchPort {
                module,
                direction: "input",
                port: name.to_string(),
            })?;
        Ok(InputRef { module, port })
    }
}

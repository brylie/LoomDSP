//! Modulation assignments: one group of [`Controller`]'s methods.

use super::*;

/// # Modulation assignments
///
/// Adding, removing, and changing modulation assignments
/// (`docs/design/modulation.md`). Adding, removing, or retargeting one takes
/// effect at the next [`commit`](Controller::commit); depth, polarity, and the
/// enabled flag take effect at the next buffer.
impl Controller {
    /// Assigns an output to a parameter. Depth is clamped to -1..1.
    /// Takes effect on the next [`Controller::commit`].
    ///
    /// The new assignment follows the source output's polarity and starts
    /// enabled, with the given depth.
    pub fn add_assignment(
        &mut self,
        source: OutputRef,
        target: ModuleId,
        param: usize,
        depth: f32,
    ) -> Result<AssignmentId, LoomError> {
        self.check_output(source)?;
        self.check_target(target, param)?;
        let id = AssignmentId(self.next_assignment);
        self.next_assignment += 1;
        self.graph.assignments.insert(
            id,
            Assignment {
                source,
                target,
                param,
                depth: clamp_depth(depth),
                polarity: None,
                enabled: true,
            },
        );
        self.dirty = true;
        Ok(id)
    }

    /// Puts an assignment back under its old id, for undo. The id must be
    /// free. Assignments on one parameter are summed in id order, so a
    /// restored assignment must keep its place in that order.
    /// Takes effect on the next [`Controller::commit`].
    pub fn restore_assignment(&mut self, id: AssignmentId, assignment: Assignment) -> Result<(), LoomError> {
        if self.graph.assignments.contains_key(&id) {
            return Err(LoomError::DuplicateAssignmentId(id));
        }
        self.check_output(assignment.source)?;
        self.check_target(assignment.target, assignment.param)?;
        self.next_assignment = self.next_assignment.max(id.0 + 1);
        self.graph.assignments.insert(
            id,
            Assignment {
                depth: clamp_depth(assignment.depth),
                ..assignment
            },
        );
        self.dirty = true;
        Ok(())
    }

    /// Assigns an output to a parameter, both named.
    pub fn add_assignment_named(
        &mut self,
        source: ModuleId,
        output: &str,
        target: ModuleId,
        param: &str,
        depth: f32,
    ) -> Result<AssignmentId, LoomError> {
        let source = self.output_ref(source, output)?;
        let param = self.param_index(target, param)?;
        self.add_assignment(source, target, param, depth)
    }

    /// Removes one modulation assignment. Takes effect on the next commit.
    pub fn remove_assignment(&mut self, id: AssignmentId) -> Result<Assignment, LoomError> {
        let removed = self
            .graph
            .assignments
            .remove(&id)
            .ok_or(LoomError::UnknownAssignment(id))?;
        self.dirty = true;
        Ok(removed)
    }

    /// Changes an assignment's endpoints without changing its id or place
    /// in the application order. The new graph takes effect on commit.
    pub fn retarget_assignment(
        &mut self,
        id: AssignmentId,
        source: OutputRef,
        target: ModuleId,
        param: usize,
    ) -> Result<(), LoomError> {
        self.graph.assignment(id).ok_or(LoomError::UnknownAssignment(id))?;
        self.check_output(source)?;
        self.check_target(target, param)?;
        let assignment = self.graph.assignments.get_mut(&id).expect("checked assignment");
        if (assignment.source, assignment.target, assignment.param) != (source, target, param) {
            assignment.source = source;
            assignment.target = target;
            assignment.param = param;
            self.dirty = true;
        }
        Ok(())
    }

    /// Sets an assignment's depth, clamped to -1..1. Takes effect at the next
    /// buffer, without recompiling.
    pub fn set_assignment_depth(&mut self, id: AssignmentId, depth: f32) -> Result<f32, LoomError> {
        let depth = clamp_depth(depth);
        self.update_assignment(id, |a| a.depth = depth)?;
        Ok(depth)
    }

    /// Forces an assignment's polarity, or `None` to follow the source output.
    /// Takes effect at the next buffer, without recompiling.
    pub fn set_assignment_polarity(&mut self, id: AssignmentId, polarity: Option<Polarity>) -> Result<(), LoomError> {
        self.update_assignment(id, |a| a.polarity = polarity)
    }

    /// Turns an assignment on or off. The scheduling edge stays, so this does
    /// not recompile. Takes effect at the next buffer.
    pub fn set_assignment_enabled(&mut self, id: AssignmentId, enabled: bool) -> Result<(), LoomError> {
        self.update_assignment(id, |a| a.enabled = enabled)
    }

    /// Sends the changed assignment first, then writes the graph. A full
    /// queue then changes nothing, so the graph and the engine never
    /// disagree, as in [`Controller::set_params`].
    fn update_assignment(&mut self, id: AssignmentId, change: impl FnOnce(&mut Assignment)) -> Result<(), LoomError> {
        let mut assignment = *self
            .graph
            .assignments
            .get(&id)
            .ok_or(LoomError::UnknownAssignment(id))?;
        change(&mut assignment);
        self.send(Command::UpdateAssignment {
            id,
            depth: assignment.depth,
            enabled: assignment.enabled,
            polarity: assignment.polarity,
        })?;
        self.graph.assignments.insert(id, assignment);
        Ok(())
    }
}

/// Clamps a modulation depth to -1..1. NaN becomes 0, which adds nothing.
fn clamp_depth(depth: f32) -> f32 {
    if depth.is_nan() { 0.0 } else { depth.clamp(-1.0, 1.0) }
}

//! Works out which cables carry stereo (ADR-0020).
//!
//! Runs on the control thread when the graph compiles, so it may allocate.
//! Spec: `docs/design/stereo.md`, rules 6 to 9.
//!
//! A module runs as two copies, one per channel, when it has no `Stereo`
//! port and a stereo cable reaches one of its `PerChannel` inputs. A
//! `PerChannel` output is stereo when its module runs as two copies, and a
//! `Stereo` output always is. Inside a feedback loop these depend on each
//! other, so the widths are found with a work queue: a module goes from
//! mono to stereo at most once, so it is queued at most once, and each
//! cable is followed at most once. Starting from mono means a loop is
//! stereo only if stereo enters it.

use crate::descriptor::{Channels, ModuleDescriptor, ModuleRole};
use crate::graph::{Graph, InputRef, ModuleId, OutputRef};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// How many channels a cable carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Width {
    /// One channel.
    #[default]
    Mono,
    /// Two channels, left and right.
    Stereo,
}

/// The widths of one graph.
#[derive(Debug, Default)]
pub(crate) struct Widths {
    /// Modules that run as two copies.
    pub per_channel: BTreeSet<ModuleId>,
    /// Outputs that carry stereo.
    pub stereo_outputs: BTreeSet<OutputRef>,
    /// How many modules the queue handled. Tests use it to check that each
    /// module is handled at most once.
    pub visited: usize,
}

/// True when the engine may run this module as two copies. Only modules the
/// engine runs per channel qualify (the audio output has no outputs to pass
/// stereo on), and a module that handles stereo itself is never copied.
fn can_copy(descriptor: &ModuleDescriptor) -> bool {
    matches!(descriptor.role, ModuleRole::Dsp(_) | ModuleRole::UnitDelay)
        && !descriptor
            .inputs
            .iter()
            .chain(descriptor.outputs)
            .any(|port| port.channels == Channels::Stereo)
}

fn output_width(descriptor: &ModuleDescriptor, port: usize, copied: bool) -> Width {
    match descriptor.outputs.get(port).map(|spec| spec.channels) {
        Some(Channels::Stereo) => Width::Stereo,
        Some(Channels::PerChannel) if copied => Width::Stereo,
        _ => Width::Mono,
    }
}

/// Finds the width of every output in `graph`. Modulation assignments are
/// not followed: they never make a module run per channel (rule 17).
pub(crate) fn infer(graph: &Graph) -> Widths {
    // Cables by the module they leave, so the queue finds them directly.
    let mut leaving: BTreeMap<ModuleId, Vec<(usize, InputRef)>> = BTreeMap::new();
    for (from, to) in graph.cables() {
        leaving.entry(from.module).or_default().push((from.port, to));
    }

    let mut widths = Widths::default();
    let mut queue: VecDeque<ModuleId> = graph
        .nodes()
        .filter(|(_, node)| {
            node.descriptor
                .outputs
                .iter()
                .any(|port| port.channels == Channels::Stereo)
        })
        .map(|(id, _)| id)
        .collect();

    while let Some(id) = queue.pop_front() {
        widths.visited += 1;
        let Some(node) = graph.node(id) else { continue };
        let copied = widths.per_channel.contains(&id);
        for &(port, to) in leaving.get(&id).into_iter().flatten() {
            if output_width(node.descriptor, port, copied) == Width::Mono {
                continue;
            }
            let Some(target) = graph.node(to.module) else { continue };
            let widens = target
                .descriptor
                .inputs
                .get(to.port)
                .is_some_and(|spec| spec.channels == Channels::PerChannel);
            if widens && can_copy(target.descriptor) && widths.per_channel.insert(to.module) {
                queue.push_back(to.module);
            }
        }
    }

    for (id, node) in graph.nodes() {
        let copied = widths.per_channel.contains(&id);
        for port in 0..node.descriptor.outputs.len() {
            if output_width(node.descriptor, port, copied) == Width::Stereo {
                widths.stereo_outputs.insert(OutputRef { module: id, port });
            }
        }
    }
    widths
}

#[cfg(test)]
mod tests;

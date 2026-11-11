//! Voice and stereo width inference (ADR-0020 and ADR-0038).
//!
//! A control-thread work queue propagates increasing widths to a fixed point.
//! Modulation assignments never widen a module. Spec: `docs/design/polyphony.md`.

use crate::descriptor::{Channels, ModuleDescriptor, ModuleRole};
use crate::graph::{Graph, InputRef, ModuleId, OutputRef};
use crate::names::ModuleNames;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Voice count and stereo side count of an output cable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Width {
    /// Number of voices, from 1 to 16.
    pub voices: usize,
    /// One for mono, two for stereo inside each voice.
    pub sides: usize,
}

impl Default for Width {
    fn default() -> Self {
        Self::MONO
    }
}

impl Width {
    /// One mono voice.
    pub const MONO: Self = Self { voices: 1, sides: 1 };
    /// One stereo voice.
    pub const STEREO: Self = Self { voices: 1, sides: 2 };
}

/// Widths and copy counts of one graph.
#[derive(Debug, Default)]
pub(crate) struct Widths {
    /// Modules copied per stereo side, within each voice.
    pub per_channel: BTreeSet<ModuleId>,
    /// Outputs with two sides per voice.
    pub stereo_outputs: BTreeSet<OutputRef>,
    /// Voice copies per module; bundle-aware modules have one.
    pub voices: BTreeMap<ModuleId, usize>,
    /// Width of every output.
    pub outputs: BTreeMap<OutputRef, Width>,
    /// Number of modules handled by the work queue.
    pub visited: usize,
}

/// Bundle-aware modules read every voice and side in one instance.
fn bundles(descriptor: &ModuleDescriptor) -> bool {
    descriptor.inputs.iter().any(|port| port.channels == Channels::Voices)
}

/// Only ordinary DSP and delay modules can be copied per stereo side.
fn can_copy(descriptor: &ModuleDescriptor) -> bool {
    matches!(descriptor.role, ModuleRole::Dsp(_) | ModuleRole::UnitDelay)
        && !bundles(descriptor)
        && !descriptor
            .inputs
            .iter()
            .chain(descriptor.outputs)
            .any(|port| port.channels == Channels::Stereo)
}

/// Computes output widths from this module's current input and copy widths.
fn outputs(graph: &Graph, widths: &Widths, id: ModuleId) -> Vec<Width> {
    let node = graph.node(id).expect("queued modules exist");
    let declared = node
        .descriptor
        .param_index("voices")
        .map_or(1, |i| node.params[i].round().clamp(1.0, 16.0) as usize);
    let bundle_sides = if bundles(node.descriptor) {
        node.descriptor
            .inputs
            .iter()
            .enumerate()
            .filter(|(_, port)| port.channels == Channels::Voices)
            .filter_map(|(port, _)| graph.source_of(InputRef { module: id, port }))
            .map(|from| widths.outputs.get(&from).copied().unwrap_or_default().sides)
            .max()
            .unwrap_or(1)
    } else {
        1
    };
    (0..node.output_count())
        .map(|index| {
            let channels = node
                .descriptor
                .outputs
                .get(index)
                .map_or(Channels::PerChannel, |port| port.channels);
            Width {
                voices: if channels == Channels::Voices {
                    declared
                } else {
                    widths.voices[&id]
                },
                sides: if channels == Channels::Stereo
                    || widths.per_channel.contains(&id)
                    || channels == Channels::PerChannel && bundle_sides == 2
                {
                    2
                } else {
                    1
                },
            }
        })
        .collect()
}

/// Finds every output's width. Mono graphs retain the existing schedule shape.
pub(crate) fn infer(graph: &Graph) -> Widths {
    let mut leaving: BTreeMap<ModuleId, Vec<(usize, InputRef)>> = BTreeMap::new();
    for (from, to) in graph.cables() {
        leaving.entry(from.module).or_default().push((from.port, to));
    }
    let mut widths = Widths::default();
    for (id, node) in graph.nodes() {
        widths.voices.insert(id, 1);
        for port in 0..node.output_count() {
            widths.outputs.insert(OutputRef { module: id, port }, Width::MONO);
        }
    }
    let mut queue: VecDeque<_> = graph
        .nodes()
        .filter(|(_, node)| {
            node.descriptor
                .outputs
                .iter()
                .any(|port| matches!(port.channels, Channels::Stereo | Channels::Voices))
        })
        .map(|(id, _)| id)
        .collect();
    let mut queued: BTreeSet<_> = queue.iter().copied().collect();
    while let Some(id) = queue.pop_front() {
        queued.remove(&id);
        widths.visited += 1;
        for (port, width) in outputs(graph, &widths, id).into_iter().enumerate() {
            widths.outputs.insert(OutputRef { module: id, port }, width);
        }
        for &(port, to) in leaving.get(&id).into_iter().flatten() {
            let from_width = widths.outputs[&OutputRef { module: id, port }];
            let Some(target) = graph.node(to.module) else { continue };
            let Some(spec) = target.descriptor.inputs.get(to.port) else {
                continue;
            };
            let mut changed = false;
            if !bundles(target.descriptor)
                && !matches!(target.descriptor.role, ModuleRole::AudioOutput)
                && from_width.voices > widths.voices[&to.module]
            {
                widths.voices.insert(to.module, from_width.voices);
                changed = true;
            }
            if spec.channels == Channels::PerChannel && from_width.sides == 2 && can_copy(target.descriptor) {
                changed |= widths.per_channel.insert(to.module);
            }
            // A consumer such as Voice merge owns its sides and emits a stereo
            // result when its bundle input is stereo, without stereo copies.
            if spec.channels == Channels::Voices && from_width.sides == 2 {
                changed |= outputs(graph, &widths, to.module)
                    .iter()
                    .enumerate()
                    .any(|(port, width)| {
                        widths.outputs.get(&OutputRef {
                            module: to.module,
                            port,
                        }) != Some(width)
                    });
            }
            if changed && queued.insert(to.module) {
                queue.push_back(to.module);
            }
        }
    }
    for (&output, width) in &widths.outputs {
        if width.sides == 2 {
            widths.stereo_outputs.insert(output);
        }
    }
    widths
}

#[cfg(test)]
mod tests;

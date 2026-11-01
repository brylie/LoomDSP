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
mod tests {
    use super::*;
    use crate::descriptor::{Panel, PortDefault, PortSpec};
    use crate::signal::SignalType;

    const fn port(name: &'static str, signal: SignalType, channels: Channels) -> PortSpec {
        PortSpec {
            name,
            label: name,
            signal,
            description: "A test port.",
            default: PortDefault::Value(0.0),
            range: crate::PortRange::Signal,
            channels,
        }
    }

    struct Silent;

    impl crate::Module for Silent {
        fn process(&mut self, _: &crate::ProcessContext<'_>, _: &mut crate::PortIo<'_>) {}
    }

    macro_rules! descriptor {
        ($id:literal, $role:expr, $inputs:expr, $outputs:expr) => {
            ModuleDescriptor {
                id: $id,
                version: 1,
                name: $id,
                category: "Test",
                family: crate::Family::Interface,
                description: "",
                inputs: $inputs,
                outputs: $outputs,
                params: &[],
                role: $role,
                panel: Panel::EMPTY,
                displays: &[],
            }
        };
    }

    const DSP: ModuleRole = ModuleRole::Dsp(|_| Box::new(Silent));

    /// Makes stereo, like Join.
    static SOURCE: ModuleDescriptor = descriptor!(
        "test.stereo-source",
        DSP,
        &[],
        &[port("out", SignalType::Audio, Channels::Stereo)]
    );
    /// A mono source, like an LFO.
    static LFO: ModuleDescriptor = descriptor!(
        "test.lfo",
        DSP,
        &[],
        &[port("out", SignalType::Modulation, Channels::PerChannel)]
    );
    /// Audio in, a cutoff that downmixes, audio out.
    static FILTER: ModuleDescriptor = descriptor!(
        "test.filter",
        DSP,
        &[
            port("in", SignalType::Audio, Channels::PerChannel),
            port("cutoff", SignalType::Pitch, Channels::Downmix),
        ],
        &[port("out", SignalType::Audio, Channels::PerChannel)]
    );
    /// Two audio inputs, one output.
    static MIXER: ModuleDescriptor = descriptor!(
        "test.mixer",
        DSP,
        &[
            port("a", SignalType::Audio, Channels::PerChannel),
            port("b", SignalType::Audio, Channels::PerChannel),
        ],
        &[port("out", SignalType::Audio, Channels::PerChannel)]
    );
    static DELAY: ModuleDescriptor = descriptor!(
        "test.delay1",
        ModuleRole::UnitDelay,
        &[port("in", SignalType::Audio, Channels::PerChannel)],
        &[port("out", SignalType::Audio, Channels::PerChannel)]
    );
    /// Handles stereo itself, like a stereo reverb.
    static WIDE: ModuleDescriptor = descriptor!(
        "test.wide",
        DSP,
        &[port("in", SignalType::Audio, Channels::Stereo)],
        &[port("out", SignalType::Audio, Channels::Stereo)]
    );
    static OUT: ModuleDescriptor = descriptor!(
        "test.out",
        ModuleRole::AudioOutput,
        &[port("in", SignalType::Audio, Channels::Stereo)],
        &[]
    );

    fn add(graph: &mut Graph, id: u32, descriptor: &'static ModuleDescriptor) -> ModuleId {
        let id = ModuleId(id);
        graph.nodes.insert(
            id,
            crate::Node {
                descriptor,
                unknown: None,
                params: Vec::new(),
                slot: matches!(descriptor.role, ModuleRole::Dsp(_)).then_some(id.0 as usize),
                ranges: Vec::new(),
                // Distinct from every left slot, for graphs that compile.
                copy_slot: matches!(descriptor.role, ModuleRole::Dsp(_)).then_some(id.0 as usize + 100_000),
            },
        );
        id
    }

    fn cable(graph: &mut Graph, from: ModuleId, to: ModuleId, input: usize) {
        graph.cables.insert(
            InputRef {
                module: to,
                port: input,
            },
            OutputRef { module: from, port: 0 },
        );
    }

    fn out(module: ModuleId) -> OutputRef {
        OutputRef { module, port: 0 }
    }

    #[test]
    fn a_patch_without_a_stereo_source_is_all_mono() {
        let mut graph = Graph::default();
        let lfo = add(&mut graph, 1, &LFO);
        let filter = add(&mut graph, 2, &FILTER);
        let mixer = add(&mut graph, 3, &MIXER);
        let output = add(&mut graph, 4, &OUT);
        cable(&mut graph, lfo, filter, 1);
        cable(&mut graph, lfo, mixer, 0);
        cable(&mut graph, filter, mixer, 1);
        cable(&mut graph, mixer, output, 0);
        let widths = infer(&graph);
        assert!(widths.per_channel.is_empty());
        assert!(widths.stereo_outputs.is_empty());
        assert_eq!(widths.visited, 0);
    }

    #[test]
    fn stereo_spreads_through_per_channel_inputs() {
        let mut graph = Graph::default();
        let source = add(&mut graph, 1, &SOURCE);
        let first = add(&mut graph, 2, &FILTER);
        let second = add(&mut graph, 3, &FILTER);
        let lfo = add(&mut graph, 4, &LFO);
        let output = add(&mut graph, 5, &OUT);
        cable(&mut graph, source, first, 0);
        cable(&mut graph, first, second, 0);
        cable(&mut graph, lfo, second, 1);
        cable(&mut graph, second, output, 0);
        let widths = infer(&graph);
        assert_eq!(widths.per_channel, BTreeSet::from([first, second]));
        assert_eq!(
            widths.stereo_outputs,
            BTreeSet::from([out(source), out(first), out(second)])
        );
    }

    #[test]
    fn a_mono_input_on_a_copied_module_does_not_change_its_source() {
        let mut graph = Graph::default();
        let source = add(&mut graph, 1, &SOURCE);
        let lfo = add(&mut graph, 2, &LFO);
        let mixer = add(&mut graph, 3, &MIXER);
        cable(&mut graph, source, mixer, 0);
        cable(&mut graph, lfo, mixer, 1);
        let widths = infer(&graph);
        assert_eq!(widths.per_channel, BTreeSet::from([mixer]));
        assert!(!widths.stereo_outputs.contains(&out(lfo)));
    }

    #[test]
    fn a_downmix_input_does_not_copy_its_module() {
        let mut graph = Graph::default();
        let source = add(&mut graph, 1, &SOURCE);
        let filter = add(&mut graph, 2, &FILTER);
        cable(&mut graph, source, filter, 1);
        let widths = infer(&graph);
        assert!(widths.per_channel.is_empty());
        assert_eq!(widths.stereo_outputs, BTreeSet::from([out(source)]));
    }

    #[test]
    fn modules_that_handle_stereo_themselves_are_never_copied() {
        let mut graph = Graph::default();
        let source = add(&mut graph, 1, &SOURCE);
        let wide = add(&mut graph, 2, &WIDE);
        let lfo = add(&mut graph, 3, &LFO);
        let mono_fed = add(&mut graph, 4, &WIDE);
        let output = add(&mut graph, 5, &OUT);
        cable(&mut graph, source, wide, 0);
        cable(&mut graph, lfo, mono_fed, 0);
        cable(&mut graph, wide, output, 0);
        let widths = infer(&graph);
        assert!(widths.per_channel.is_empty());
        // A `Stereo` output is stereo whatever feeds the module.
        assert_eq!(
            widths.stereo_outputs,
            BTreeSet::from([out(source), out(wide), out(mono_fed)])
        );
    }

    /// mixer.a <- delay <- filter <- mixer: a loop through `PerChannel` inputs.
    fn audio_loop(graph: &mut Graph) -> [ModuleId; 3] {
        let mixer = add(graph, 10, &MIXER);
        let filter = add(graph, 11, &FILTER);
        let delay = add(graph, 12, &DELAY);
        cable(graph, mixer, filter, 0);
        cable(graph, filter, delay, 0);
        cable(graph, delay, mixer, 0);
        [mixer, filter, delay]
    }

    #[test]
    fn a_loop_that_no_stereo_enters_stays_mono() {
        let mut graph = Graph::default();
        let [mixer, ..] = audio_loop(&mut graph);
        let lfo = add(&mut graph, 1, &LFO);
        cable(&mut graph, lfo, mixer, 1);
        let widths = infer(&graph);
        assert!(widths.per_channel.is_empty());
        assert!(widths.stereo_outputs.is_empty());
    }

    #[test]
    fn stereo_into_a_per_channel_input_makes_the_whole_loop_stereo() {
        let mut graph = Graph::default();
        let [mixer, filter, delay] = audio_loop(&mut graph);
        let source = add(&mut graph, 1, &SOURCE);
        cable(&mut graph, source, mixer, 1);
        let widths = infer(&graph);
        assert_eq!(widths.per_channel, BTreeSet::from([mixer, filter, delay]));
        assert_eq!(
            widths.stereo_outputs,
            BTreeSet::from([out(source), out(mixer), out(filter), out(delay)])
        );
        assert_eq!(widths.visited, 4);
    }

    #[test]
    fn stereo_only_into_a_downmix_input_leaves_the_loop_mono() {
        let mut graph = Graph::default();
        let [_, filter, _] = audio_loop(&mut graph);
        let source = add(&mut graph, 1, &SOURCE);
        cable(&mut graph, source, filter, 1);
        let widths = infer(&graph);
        assert!(widths.per_channel.is_empty());
        assert_eq!(widths.stereo_outputs, BTreeSet::from([out(source)]));
    }

    #[test]
    fn each_module_is_handled_at_most_once() {
        // A long chain whose IDs run against the signal, so a pass in ID
        // order would move stereo forward one module per pass.
        const LENGTH: u32 = 500;
        let mut graph = Graph::default();
        let source = add(&mut graph, LENGTH + 1, &SOURCE);
        let mut previous = source;
        for id in (1..=LENGTH).rev() {
            let filter = add(&mut graph, id, &FILTER);
            cable(&mut graph, previous, filter, 0);
            previous = filter;
        }
        let widths = infer(&graph);
        assert_eq!(widths.per_channel.len(), LENGTH as usize);
        assert_eq!(widths.visited, LENGTH as usize + 1);
    }

    #[test]
    fn the_compile_report_lists_the_widths() {
        let mut graph = Graph::default();
        let source = add(&mut graph, 1, &SOURCE);
        let filter = add(&mut graph, 2, &FILTER);
        let lfo = add(&mut graph, 3, &LFO);
        cable(&mut graph, source, filter, 0);
        cable(&mut graph, lfo, filter, 1);
        let report = crate::compile::compile(&graph, &BTreeMap::new(), &BTreeMap::new(), 48_000.0, 64).1;
        assert_eq!(report.width(out(filter)), Width::Stereo);
        assert_eq!(report.width(out(lfo)), Width::Mono);
        assert!(report.runs_per_channel(filter));
        assert!(!report.runs_per_channel(lfo));
    }
}

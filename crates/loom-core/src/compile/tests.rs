/// A new right tap first inherits its mono tap, then its mono source.
#[test]
fn new_right_taps_inherit_mono_history() {
    use super::{RIGHT_TAP, Schedule, Tap};
    let source = [0.25, 0.75];
    let tap = |key, source, state| Tap {
        key,
        source,
        input: std::ptr::null(),
        output: std::ptr::null_mut(),
        state,
    };
    let mut old = Schedule::empty();
    old.taps.push(tap(3, 8, 0.5));
    old.sources.push((8, source.as_ptr()));
    let mut next = Schedule::empty();
    next.taps.push(tap(3 | RIGHT_TAP, 8 | RIGHT_TAP, 0.0));
    next.taps.push(tap(4 | RIGHT_TAP, 8 | RIGHT_TAP, 0.0));
    next.take_tap_state_from(&old, source.len());
    assert_eq!(next.taps[0].state, 0.5);
    assert_eq!(next.taps[1].state, 0.75);

    // A matching right tap wins over the mono tap and mono source.
    old.taps.push(tap(3 | RIGHT_TAP, 8 | RIGHT_TAP, -0.5));
    next.take_tap_state_from(&old, source.len());
    assert_eq!(next.taps[0].state, -0.5);

    // A previously stereo source wins over its left channel for a new tap.
    let right_source = [-0.25, -0.75];
    old.sources.push((8 | RIGHT_TAP, right_source.as_ptr()));
    next.take_tap_state_from(&old, source.len());
    assert_eq!(next.taps[1].state, -0.75);
}
use super::*;

/// A port for test descriptors.
const fn port(name: &'static str, signal: crate::SignalType) -> crate::PortSpec {
    crate::PortSpec {
        name,
        label: name,
        signal,
        description: "A test port.",
        default: crate::PortDefault::Value(0.0),
        range: crate::PortRange::Signal,
        channels: crate::Channels::PerChannel,
    }
}

#[test]
fn tarjan_finds_cycles_in_topological_order() {
    // 0 -> 1 -> 2 -> 1, 2 -> 3
    let succ = vec![vec![1], vec![2], vec![1, 3], vec![]];
    let mut sccs = tarjan(&succ);
    sccs.reverse();
    assert_eq!(sccs, vec![vec![0], vec![1, 2], vec![3]]);
}

struct Silent;

impl crate::Module for Silent {
    fn process(&mut self, _: &crate::ProcessContext<'_>, _: &mut crate::PortIo<'_>) {}
}

static FOLLOWER: crate::ModuleDescriptor = crate::ModuleDescriptor {
    id: "test.follower",
    version: 1,
    name: "Follower",
    category: "Test",
    family: crate::Family::Interface,
    description: "Inputs with every kind of default.",
    inputs: &[
        port("bar", crate::SignalType::Phase).with_default(PortDefault::BarPhase),
        port("bar2", crate::SignalType::Phase).with_default(PortDefault::BarPhase),
        port("half", crate::SignalType::Modulation).with_default(PortDefault::Value(0.5)),
        port("zero", crate::SignalType::Modulation),
    ],
    outputs: &[port("out", crate::SignalType::Modulation)],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Silent)),
    panel: crate::Panel::EMPTY,
    displays: &[],
};

fn followers(count: u32) -> Graph {
    let mut graph = Graph::default();
    for i in 0..count {
        graph.nodes.insert(
            ModuleId(i + 1),
            crate::Node {
                descriptor: &FOLLOWER,
                unknown: None,
                params: Vec::new(),
                slot: Some(i as usize),
                ranges: Vec::new(),
                copy_slot: None,
            },
        );
    }
    graph
}

/// A probe on output 0 of each module in `modules`, so they count as
/// shown and are scheduled.
fn probes_on(modules: impl IntoIterator<Item = u32>) -> BTreeMap<ProbeId, (OutputRef, ProbeKind)> {
    modules
        .into_iter()
        .map(|m| {
            let from = OutputRef {
                module: ModuleId(m),
                port: 0,
            };
            (ProbeId(m), (from, ProbeKind::Meter))
        })
        .collect()
}

fn input_ptr(schedule: &Schedule, op: usize, port: usize) -> *const f32 {
    schedule.input_ptrs[schedule.ops[op].inputs.0 as usize + port]
}

#[test]
fn inputs_that_follow_the_same_default_share_one_buffer() {
    let schedule = compile(&followers(100), &probes_on(1..=100), &BTreeMap::new(), 48_000.0, 64).0;
    // 100 outputs, one bar phase buffer, one buffer holding 0.5.
    assert_eq!(schedule._buffers.len(), 102);
    let bar = schedule.transport.bar_phase.expect("bar phase is used") as *const f32;
    assert!(schedule.transport.beat_phase.is_none());
    assert!(schedule.transport.playing.is_none());
    let half = input_ptr(&schedule, 0, 2);
    // SAFETY: `half` points at a schedule buffer of 64 floats.
    assert_eq!(unsafe { std::slice::from_raw_parts(half, 64) }, &[0.5; 64]);
    for op in 0..100 {
        assert_eq!(input_ptr(&schedule, op, 0), bar);
        assert_eq!(input_ptr(&schedule, op, 1), bar);
        assert_eq!(input_ptr(&schedule, op, 2), half);
        assert_eq!(input_ptr(&schedule, op, 3), schedule._zero.as_ptr());
        assert_eq!(schedule.ops[op].connected, 0);
    }
}

#[test]
fn a_patch_without_stereo_compiles_as_before() {
    // 1 -> 2 -> 3, and 3 feeds itself, so one delay is inserted.
    let mut graph = followers(3);
    let cable = |graph: &mut Graph, from: u32, to: u32| {
        graph.cables.insert(
            InputRef {
                module: ModuleId(to),
                port: 3,
            },
            OutputRef {
                module: ModuleId(from),
                port: 0,
            },
        );
    };
    cable(&mut graph, 1, 2);
    cable(&mut graph, 2, 3);
    cable(&mut graph, 3, 3);
    let (schedule, report) = compile(&graph, &probes_on(1..=3), &BTreeMap::new(), 48_000.0, 64);
    assert!(report.per_channel.is_empty());
    assert!(report.stereo_outputs.is_empty());
    // One operation per module, one tap for the one delay, and buffers
    // for 3 outputs, the bar phase, the 0.5 default, and the tap.
    assert_eq!(schedule.ops.len(), 3);
    assert!(schedule.ops.iter().all(|op| op.channel == 0));
    assert_eq!(schedule.taps.len(), 1);
    assert_eq!(schedule._buffers.len(), 6);
    assert!(schedule.downmixes.is_empty());
    assert!(schedule.right_ptrs.is_empty());
    assert!(schedule.copies.is_empty());
}

/// A mono cable into a stereo input shares one buffer without a copy.
#[test]
fn mono_cable_into_stereo_input_reuses_its_buffer() {
    static STEREO_INPUT: crate::ModuleDescriptor = crate::ModuleDescriptor {
        id: "test.stereo-input",
        version: 1,
        name: "Stereo input",
        category: "Test",
        family: crate::Family::Interface,
        description: "A stereo input for testing.",
        inputs: &[port("in", crate::SignalType::Audio).with_channels(Channels::Stereo)],
        outputs: &[port("out", crate::SignalType::Audio)],
        params: &[],
        role: ModuleRole::Dsp(|_| Box::new(Silent)),
        panel: crate::Panel::EMPTY,
        displays: &[],
    };
    let mut graph = followers(2);
    graph.nodes.get_mut(&ModuleId(2)).unwrap().descriptor = &STEREO_INPUT;
    graph.cables.insert(
        InputRef {
            module: ModuleId(2),
            port: 0,
        },
        OutputRef {
            module: ModuleId(1),
            port: 0,
        },
    );
    let (schedule, report) = compile(&graph, &probes_on([2]), &BTreeMap::new(), 48_000.0, 64);
    assert!(report.per_channel.is_empty() && report.stereo_outputs.is_empty());
    assert_eq!(schedule._buffers.len(), 4, "two outputs and two defaults");
    assert!(schedule.downmixes.is_empty());
    assert!(
        schedule.right_ptrs.is_empty(),
        "both sides use the ordinary input pointer"
    );
    assert!(schedule.copies.is_empty());
    let source = schedule.ops.iter().find(|op| op.slot == 0).unwrap();
    let destination = schedule.ops.iter().find(|op| op.slot == 1).unwrap();
    assert_eq!(
        schedule.output_ptrs[source.outputs.0 as usize] as *const f32,
        schedule.input_ptrs[destination.inputs.0 as usize]
    );
}

#[test]
fn downmix_kinds_follow_the_spec_cases() {
    assert_eq!(DownmixKind::of(crate::SignalType::Audio), DownmixKind::Mean);
    assert_eq!(DownmixKind::of(crate::SignalType::Pitch), DownmixKind::Mean);
    assert_eq!(DownmixKind::of(crate::SignalType::Modulation), DownmixKind::Mean);
    assert_eq!(DownmixKind::of(crate::SignalType::Gate), DownmixKind::Max);
    assert_eq!(DownmixKind::of(crate::SignalType::Phase), DownmixKind::Phase);
    assert_eq!(DownmixKind::Mean.apply(0.5, 0.5), 0.5);
    assert_eq!(DownmixKind::Mean.apply(0.5, -0.5), 0.0);
    assert_eq!(DownmixKind::Max.apply(1.0, 0.0), 1.0);
    assert_eq!(DownmixKind::Phase.apply(0.2, 0.4), 0.3);
    assert_eq!(DownmixKind::Phase.apply(0.25, 0.75), 0.0);
    assert_eq!(DownmixKind::Phase.apply(0.75, 0.25), 0.5);
    let across = DownmixKind::Phase.apply(0.9, 0.1);
    assert_eq!(across, 0.0);
}

#[test]
fn a_cable_replaces_the_default() {
    let mut graph = followers(2);
    let from = OutputRef {
        module: ModuleId(2),
        port: 0,
    };
    let to = InputRef {
        module: ModuleId(1),
        port: 0,
    };
    graph.cables.insert(to, from);
    let schedule = compile(&graph, &probes_on([1]), &BTreeMap::new(), 48_000.0, 64).0;
    // Module 2 runs first; module 1 reads module 2's output.
    let source = schedule.output_ptrs[schedule.ops[0].outputs.0 as usize] as *const f32;
    assert_eq!(input_ptr(&schedule, 1, 0), source);
    assert_eq!(schedule.ops[1].connected, 1);
    // Its second bar input and module 2's still follow the transport.
    let bar = schedule.transport.bar_phase.expect("still used") as *const f32;
    assert_eq!(input_ptr(&schedule, 1, 1), bar);
    assert_eq!(input_ptr(&schedule, 0, 0), bar);
}

/// `test.lit`: a follower with one display value.
static LIT: crate::ModuleDescriptor = crate::ModuleDescriptor {
    id: "test.lit",
    displays: &[crate::DisplaySpec {
        name: "lit",
        label: "Lit",
        description: "A test light.",
        kind: crate::DisplayKind::Level,
    }],
    ..FOLLOWER
};

/// `test.with-param`: a follower with one parameter to modulate.
static WITH_PARAM: crate::ModuleDescriptor = crate::ModuleDescriptor {
    id: "test.with-param",
    params: &[ParamSpec {
        name: "p",
        label: "p",
        description: "A test parameter.",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
        options: crate::descriptor::ParamOptions::DEFAULT,
    }],
    ..FOLLOWER
};

/// Adds `descriptor` to `graph` as module `id`, in slot `id`.
fn add(graph: &mut Graph, id: u32, descriptor: &'static crate::ModuleDescriptor) {
    let node = crate::Node {
        descriptor,
        unknown: None,
        params: descriptor.default_params(),
        slot: Some(id as usize),
        copy_slot: None,
        ranges: vec![None; descriptor.params.len()],
    };
    graph.nodes.insert(ModuleId(id), node);
}

/// Plugs output 0 of `from` into input `port` of `to`.
fn cable(graph: &mut Graph, from: u32, to: u32, port: usize) {
    let from = OutputRef {
        module: ModuleId(from),
        port: 0,
    };
    let to = InputRef {
        module: ModuleId(to),
        port,
    };
    graph.cables.insert(to, from);
}

/// The slots the schedule runs, in order, loop members included.
fn scheduled_slots(schedule: &Schedule) -> Vec<u32> {
    let mut slots = Vec::new();
    for step in &schedule.steps {
        match *step {
            Step::Module(op) => slots.push(schedule.ops[op as usize].slot),
            Step::Loop { ops, .. } => {
                for &op in &schedule.loop_ops[ops.0 as usize..ops.1 as usize] {
                    slots.push(schedule.ops[op as usize].slot);
                }
            }
            Step::Tap(_) => {}
        }
    }
    slots
}

/// A chain that leads to nothing the user hears or sees is left out of
/// the schedule and listed in the report (FR-GRAPH-09). A module with
/// a probe, and everything that feeds it, runs.
#[test]
fn modules_that_reach_nothing_are_skipped() {
    let mut graph = followers(4);
    // 2 -> 1 is probed. 4 -> 3 leads nowhere.
    cable(&mut graph, 2, 1, 0);
    cable(&mut graph, 4, 3, 0);
    let (schedule, report) = compile(&graph, &probes_on([1]), &BTreeMap::new(), 48_000.0, 64);
    assert_eq!(scheduled_slots(&schedule), vec![1, 0]);
    assert_eq!(report.skipped, BTreeSet::from([ModuleId(3), ModuleId(4)]));
    assert_eq!(report.modules, 4);

    let (schedule, report) = compile(&graph, &BTreeMap::new(), &BTreeMap::new(), 48_000.0, 64);
    assert!(schedule.steps.is_empty());
    assert_eq!(report.skipped.len(), 4);
    // Skipped modules read no transport, so the engine fills nothing.
    assert!(schedule.transport.bar_phase.is_none());
}

/// A module with display values runs with no cable, so its lights stay
/// true, and so does the module feeding it.
#[test]
fn a_module_with_lights_runs_unconnected() {
    let mut graph = followers(1);
    add(&mut graph, 2, &LIT);
    cable(&mut graph, 1, 2, 0);
    let (schedule, report) = compile(&graph, &BTreeMap::new(), &BTreeMap::new(), 48_000.0, 64);
    assert_eq!(scheduled_slots(&schedule), vec![0, 2]);
    assert!(report.skipped.is_empty());
}

/// A modulated parameter shows a live dot on its knob, so its module
/// runs, and so does the source that modulates it.
#[test]
fn a_modulated_module_runs_unconnected() {
    let mut graph = followers(2);
    add(&mut graph, 3, &WITH_PARAM);
    let assignment = crate::graph::Assignment {
        source: OutputRef {
            module: ModuleId(1),
            port: 0,
        },
        target: ModuleId(3),
        param: 0,
        depth: 0.5,
        polarity: None,
        enabled: true,
    };
    graph.assignments.insert(AssignmentId(1), assignment);
    let (schedule, report) = compile(&graph, &BTreeMap::new(), &BTreeMap::new(), 48_000.0, 64);
    assert_eq!(scheduled_slots(&schedule), vec![0, 3]);
    assert_eq!(schedule.fills.len(), 1);
    assert_eq!(report.skipped, BTreeSet::from([ModuleId(2)]));
}

/// A feedback loop that reaches nothing is still reported, with its
/// inserted delay, so the canvas can show it. Its tap keeps the held
/// sample while the loop is not scheduled.
#[test]
fn a_skipped_loop_is_reported_but_not_run() {
    let mut graph = followers(3);
    // 1 <-> 2 is a loop with no unit delay. 3 is probed and alone.
    cable(&mut graph, 1, 2, 0);
    cable(&mut graph, 2, 1, 0);
    let (schedule, report) = compile(&graph, &probes_on([3]), &BTreeMap::new(), 48_000.0, 64);
    assert_eq!(scheduled_slots(&schedule), vec![2]);
    assert_eq!(schedule.taps.len(), 1);
    assert_eq!(report.loops.len(), 1);
    assert_eq!(report.loops[0].inserted_delays.len(), 1);
    assert_eq!(report.skipped, BTreeSet::from([ModuleId(1), ModuleId(2)]));

    // Probed, the same loop runs one sample at a time with its delay.
    let (schedule, report) = compile(&graph, &probes_on([1]), &BTreeMap::new(), 48_000.0, 64);
    assert_eq!(scheduled_slots(&schedule), vec![0, 1]);
    assert_eq!(schedule.taps.len(), 1);
    assert_eq!(report.skipped, BTreeSet::from([ModuleId(3)]));

    // A live loop's held sample survives a skipped schedule and is
    // still there when the loop becomes live again.
    let mut live = schedule;
    live.taps[0].state = 0.75;
    let (mut skipped, _) = compile(&graph, &probes_on([3]), &BTreeMap::new(), 48_000.0, 64);
    skipped.take_tap_state_from(&live, 64);
    assert_eq!(skipped.taps[0].state, 0.75);
    let (mut reconnected, _) = compile(&graph, &probes_on([1]), &BTreeMap::new(), 48_000.0, 64);
    reconnected.take_tap_state_from(&skipped, 64);
    assert_eq!(reconnected.taps[0].state, 0.75);
}

/// A schedule with one smoothed fill per key, stored in the given order
/// (execution order, not key order). Each fill's smoother targets `target(key)`.
fn schedule_with_fills(keys: &[u64], target: impl Fn(u64) -> f32) -> Schedule {
    static SPEC: ParamSpec = ParamSpec {
        name: "p",
        label: "p",
        description: "A test parameter.",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
        options: crate::descriptor::ParamOptions::DEFAULT,
    };
    let mut schedule = Schedule::empty();
    for &key in keys {
        let mut smooth = Smoothed::new(0.0, 48_000.0, 0.01);
        smooth.set_target(target(key));
        schedule.fills.push(ParamFill {
            key,
            slot: (key >> 32) as u32,
            param: key as u16,
            spec: SPEC,
            base: 0.75,
            smooth: Some(smooth),
            buffer: std::ptr::null_mut(),
            sources: Vec::new(),
            live: crate::live::NO_CELL,
        });
    }
    schedule.fill_order = (0..keys.len() as u32).collect();
    schedule.fill_order.sort_by_key(|&i| schedule.fills[i as usize].key);
    schedule
}

/// A range change keeps the glide going from the value it had reached,
/// at that value's place in the new range (control-ranges.md).
#[test]
fn a_range_change_keeps_the_gliding_value() {
    let old = schedule_with_fills(&[1 << 32], |_| 1.0);
    let mut old = old;
    // Part way: position 0.4 of 0 to 1 is the value 0.4.
    old.fills[0].smooth.as_mut().unwrap().reset(0.4);
    let mut new = schedule_with_fills(&[1 << 32], |_| 1.0);
    new.fills[0].spec.min = -1.0;
    new.fills[0].spec.max = 1.0;
    new.take_smooth_state_from(&old, |_, _| None);
    let smooth = new.fills[0].smooth.unwrap();
    // 0.4 in -1 to 1 is position 0.7. The target is the base, 0.75.
    assert!((smooth.current() - 0.7).abs() < 1e-6, "{}", smooth.current());
    assert!((smooth.target() - 0.875).abs() < 1e-6, "{}", smooth.target());
}

#[test]
fn smooth_state_carries_over_by_key_across_many_shuffled_fills() {
    const N: u64 = 2_000;
    // 1_999 is prime, so this stride visits every index in a scrambled order.
    let shuffled = |keep: fn(u64) -> bool| -> Vec<u64> {
        (0..N)
            .map(|i| (i * 1_999) % N)
            .filter(|&i| keep(i))
            .map(|i| ((i / 4) << 32) | (i % 4))
            .collect()
    };
    let old = schedule_with_fills(&shuffled(|i| i % 3 != 0), |key| 0.25 + (key >> 32) as f32 / 1_000.0);
    let mut new = schedule_with_fills(&shuffled(|i| i % 2 == 0), |_| 0.5);
    let calls = std::cell::Cell::new(0);
    new.take_smooth_state_from(&old, |slot, _| {
        calls.set(calls.get() + 1);
        (slot % 2 == 0).then_some(0.1)
    });

    // The reference: the quadratic scan this merge replaced.
    let mut expected_new = 0;
    for fill in &new.fills {
        let smooth = fill.smooth.as_ref().unwrap();
        match old.fills.iter().find(|previous| previous.key == fill.key) {
            Some(previous) => {
                assert_eq!(smooth.target(), previous.smooth.as_ref().unwrap().target());
                assert_eq!(smooth.current(), previous.smooth.as_ref().unwrap().current());
            }
            None => {
                expected_new += 1;
                if fill.slot % 2 == 0 {
                    assert!((smooth.current() - 0.1).abs() < 1e-6);
                    assert_eq!(smooth.target(), fill.spec.normalize(0.75));
                } else {
                    assert_eq!(smooth.target(), 0.5, "no glide: left as built");
                }
            }
        }
    }
    assert_eq!(calls.get(), expected_new);
    assert!(expected_new > 0 && expected_new < new.fills.len());
}

use super::*;
use crate::descriptor::{ModuleDescriptor, ParamOptions};
use crate::graph::{Assignment, UnknownModule};

/// Signals that fade and signals that switch, by type (rule 9).
#[test]
fn audio_modulation_and_pitch_fade_and_gates_and_phases_switch() {
    assert!(fades(SignalType::Audio));
    assert!(fades(SignalType::Modulation));
    assert!(fades(SignalType::Pitch));
    assert!(!fades(SignalType::Gate));
    assert!(!fades(SignalType::Phase));
}

/// A module that does nothing, for the test descriptors.
struct Silent;

impl crate::Module for Silent {
    fn process(&mut self, _: &crate::ProcessContext<'_>, _: &mut crate::PortIo<'_>) {}
}

/// A port of `signal` for the test descriptors.
const fn port(name: &'static str, signal: SignalType) -> crate::PortSpec {
    crate::PortSpec {
        name,
        label: name,
        signal,
        description: "A test port.",
        default: PortDefault::Value(0.0),
        range: crate::PortRange::Signal,
        channels: Channels::PerChannel,
    }
}

/// A continuous parameter for the test descriptors.
const fn param(name: &'static str, default: f32) -> ParamSpec {
    ParamSpec {
        name,
        label: name,
        description: "A test parameter.",
        min: 0.0,
        max: 1.0,
        default,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
}

/// A test module type with these ports and parameters.
const fn module(
    id: &'static str,
    inputs: &'static [crate::PortSpec],
    outputs: &'static [crate::PortSpec],
    params: &'static [ParamSpec],
) -> ModuleDescriptor {
    ModuleDescriptor {
        id,
        version: 1,
        name: id,
        category: "Test",
        family: crate::Family::Interface,
        description: "A test module.",
        inputs,
        outputs,
        params,
        role: ModuleRole::Dsp(|_| Box::new(Silent)),
        panel: crate::Panel::EMPTY,
        displays: &[],
        roles: &[],
    }
}

/// Makes a phase, like the Phase module.
static SOURCE: ModuleDescriptor = module("test.source", &[], &[port("phase", SignalType::Phase)], &[]);
/// Moves a phase, like Phase Shift.
static SHIFTER: ModuleDescriptor = module(
    "test.shifter",
    &[port("phase", SignalType::Phase)],
    &[port("phase", SignalType::Phase)],
    &[],
);
/// Two phases in, one out, for a loop through phase cables.
static MERGE: ModuleDescriptor = module(
    "test.merge",
    &[port("a", SignalType::Phase), port("b", SignalType::Phase)],
    &[port("phase", SignalType::Phase)],
    &[],
);
/// Makes a sound from a phase, like Sine.
static OSC: ModuleDescriptor = module(
    "test.osc",
    &[port("phase", SignalType::Phase)],
    &[port("out", SignalType::Audio)],
    &[],
);
/// An audio input, a gate input, and a continuous parameter.
static READER: ModuleDescriptor = module(
    "test.reader",
    &[port("in", SignalType::Audio), port("gate", SignalType::Gate)],
    &[port("out", SignalType::Audio)],
    &[param("level", 0.5)],
);
/// The Sum the editor adds, by its ID.
static SUM: ModuleDescriptor = module(
    SUM_MODULE,
    &[port("in1", SignalType::Audio), port("in2", SignalType::Audio)],
    &[port("out", SignalType::Audio)],
    &[param(SUM_GAIN_PARAM, 1.0)],
);

/// A cable in a test graph: (module, output) to (module, input).
type Cable = ((u32, usize), (u32, usize));

/// A graph of `modules`, each in its own slot, and `cables`.
fn graph(modules: &[(u32, &'static ModuleDescriptor)], cables: &[Cable]) -> Graph {
    let mut graph = Graph::default();
    for &(id, descriptor) in modules {
        graph
            .nodes
            .insert(ModuleId(id), Node::new(descriptor, Some(id as usize)));
    }
    for &((from, from_port), (to, to_port)) in cables {
        graph.cables.insert(
            InputRef {
                module: ModuleId(to),
                port: to_port,
            },
            OutputRef {
                module: ModuleId(from),
                port: from_port,
            },
        );
    }
    graph
}

/// Input `port` of module `module`.
fn input(module: u32, port: usize) -> InputRef {
    InputRef {
        module: ModuleId(module),
        port,
    }
}

/// The fade plan for a swap from `before` to `after`.
fn plan(before: &Graph, after: &Graph) -> FadePlan {
    let none = BTreeMap::new();
    let (_, _, wiring) = compile_with_fades(
        before,
        (&BTreeMap::new(), &BTreeMap::new(), &none),
        48_000.0,
        64,
        Fades::default(),
    );
    let mut buffers = Buffers::default();
    let layout = Layout::new(after, &mut buffers);
    let compiled = compile_assignments(&layout);
    FadePlan::new(&wiring, &layout, &compiled, &[], &BTreeSet::new(), 240)
}

/// The modules of the direct case: two phases (1, 2), an oscillator (3),
/// and a reader (4) of its sound.
const DIRECT: [(u32, &ModuleDescriptor); 4] = [(1, &SOURCE), (2, &SOURCE), (3, &OSC), (4, &READER)];

/// A phase cable plugged into an oscillator, moved, or pulled: the input
/// its sound feeds fades, though the phase input itself switches. A new
/// module the sound feeds has nothing to hold, so it does not fade.
#[test]
fn a_direct_phase_replug_fades_what_the_oscillator_feeds() {
    let sound = ((3, 0), (4, 0));
    let cases = [
        ("plugged", vec![sound], vec![((1, 0), (3, 0)), sound]),
        ("moved", vec![((1, 0), (3, 0)), sound], vec![((2, 0), (3, 0)), sound]),
        ("pulled", vec![((1, 0), (3, 0)), sound], vec![sound]),
    ];
    for (name, before, after) in cases {
        let plan = plan(&graph(&DIRECT, &before), &graph(&DIRECT, &after));
        assert_eq!(plan.fresh, BTreeSet::from([input(4, 0)]), "{name}");
    }

    // A new reader of the oscillator, in the same swap.
    let before = graph(&DIRECT, &[((1, 0), (3, 0)), sound]);
    let mut modules = DIRECT.to_vec();
    modules.push((5, &READER));
    let after = graph(&modules, &[((2, 0), (3, 0)), sound, ((3, 0), (5, 0))]);
    assert_eq!(plan(&before, &after).fresh, BTreeSet::from([input(4, 0)]));

    // No phase change, no walk: a cable change elsewhere fades only itself.
    let after = graph(&DIRECT, &[((1, 0), (3, 0)), sound, ((2, 0), (4, 1))]);
    assert!(plan(&before, &after).fresh.is_empty(), "a gate input switches");
}

/// The modules of the chain case: two phases (1, 2), a shifter (3), an
/// oscillator (4), and a reader (5).
const CHAIN: [(u32, &ModuleDescriptor); 5] = [(1, &SOURCE), (2, &SOURCE), (3, &SHIFTER), (4, &OSC), (5, &READER)];

/// Phase into a shifter into an oscillator: replugging the shifter's phase
/// fades what the oscillator feeds, because the walk follows the phase
/// cable on. A gate input the oscillator feeds is not added: it switches.
/// An assignment whose source is the oscillator ramps as a fresh one.
#[test]
fn the_walk_follows_phase_cables_and_stops_at_a_gate() {
    let rest = [((3, 0), (4, 0)), ((4, 0), (5, 0)), ((4, 0), (5, 1))];
    let before = graph(&CHAIN, &[[((1, 0), (3, 0))].as_slice(), &rest].concat());
    let mut after = graph(&CHAIN, &[[((2, 0), (3, 0))].as_slice(), &rest].concat());
    let level = READER.param_index("level").unwrap();
    let assignment = Assignment {
        source: OutputRef {
            module: ModuleId(4),
            port: 0,
        },
        target: ModuleId(5),
        param: level,
        depth: 0.5,
        polarity: None,
        enabled: true,
    };
    let mut before = before;
    before.assignments.insert(AssignmentId(1), assignment);
    after.assignments.insert(AssignmentId(1), assignment);
    let plan = plan(&before, &after);
    assert_eq!(plan.fresh, BTreeSet::from([input(5, 0)]));
    assert_eq!(plan.live.get(&(ModuleId(5), level, AssignmentId(1))), Some(&true));
}

/// A loop through phase cables ends: each module is visited once.
#[test]
fn a_loop_through_phase_cables_ends() {
    let modules = [
        (1, &SOURCE),
        (2, &SOURCE),
        (3, &MERGE),
        (4, &MERGE),
        (5, &OSC),
        (6, &READER),
    ];
    let rest = [((4, 0), (3, 1)), ((3, 0), (4, 0)), ((4, 0), (5, 0)), ((5, 0), (6, 0))];
    let before = graph(&modules, &[[((1, 0), (3, 0))].as_slice(), &rest].concat());
    let after = graph(&modules, &[[((2, 0), (3, 0))].as_slice(), &rest].concat());
    assert_eq!(plan(&before, &after).fresh, BTreeSet::from([input(6, 0)]));
}

/// An exact rewire never seeds the walk: a phase cable moved onto a new
/// Sum at gain 1 that plays the old source alone changes nothing heard.
#[test]
fn an_exact_rewire_adds_nothing() {
    let before = graph(&DIRECT, &[((1, 0), (3, 0)), ((3, 0), (4, 0))]);
    let mut modules = DIRECT.to_vec();
    modules.push((7, &SUM));
    let after = graph(&modules, &[((1, 0), (7, 0)), ((7, 0), (3, 0)), ((3, 0), (4, 0))]);
    let plan = plan(&before, &after);
    assert!(plan.fresh.is_empty(), "{:?}", plan.fresh);
    assert!(plan.live.is_empty());
}

/// A phase through a Sum the editor added carries a phase, so the walk
/// goes on through it to the oscillator after it.
#[test]
fn the_walk_goes_on_through_a_mixer_that_carries_a_phase() {
    let mut modules = DIRECT.to_vec();
    modules.push((7, &SUM));
    let rest = [((7, 0), (3, 0)), ((3, 0), (4, 0))];
    let before = graph(&modules, &[[((1, 0), (7, 0))].as_slice(), &rest].concat());
    let after = graph(&modules, &[[((2, 0), (7, 0))].as_slice(), &rest].concat());
    assert_eq!(plan(&before, &after).fresh, BTreeSet::from([input(4, 0)]));
}

/// A phase through a Sum the editor added, not new in this swap, into an
/// audio input: what the cable carries past the mixer decides, as for a
/// cable change (rule 9, The mixers the editor adds). It carries a phase,
/// so the reader's input switches, and the walk stops there.
#[test]
fn a_phase_through_a_mixer_into_an_audio_input_switches() {
    let modules = [(1, &SOURCE), (2, &SOURCE), (3, &SHIFTER), (5, &READER), (7, &SUM)];
    let rest = [((3, 0), (7, 0)), ((7, 0), (5, 0))];
    let before = graph(&modules, &[[((1, 0), (3, 0))].as_slice(), &rest].concat());
    let after = graph(&modules, &[[((2, 0), (3, 0))].as_slice(), &rest].concat());
    let plan = plan(&before, &after);
    assert!(plan.fresh.is_empty(), "the reader's input switches: {:?}", plan.fresh);
}

/// An unknown module downstream of a replugged phase: its inputs switch,
/// the walk stops there and never reads past its placeholder's empty
/// ports, and a known reader of the same sound still fades.
#[test]
fn an_unknown_module_downstream_switches() {
    let build = |phase: u32| {
        let rest = [
            ((3, 0), (4, 0)),
            ((4, 0), (5, 0)),
            ((3, 0), (9, 0)),
            ((4, 0), (9, 1)),
            ((9, 0), (10, 0)),
        ];
        let mut graph = graph(&CHAIN, &[[((phase, 0), (3, 0))].as_slice(), &rest].concat());
        let mut node = Node::new(UnknownModule::descriptor(), Some(9));
        node.unknown = Some(UnknownModule {
            kind: "test.gone@1".into(),
            params: BTreeMap::new(),
            ranges: BTreeMap::new(),
            inputs: vec!["phase".into(), "in".into()],
            outputs: vec!["out".into()],
            parameters: Vec::new(),
        });
        graph.nodes.insert(ModuleId(9), node);
        graph.nodes.insert(ModuleId(10), Node::new(&READER, Some(10)));
        graph
    };
    let plan = plan(&build(1), &build(2));
    assert_eq!(plan.fresh, BTreeSet::from([input(5, 0)]));
}

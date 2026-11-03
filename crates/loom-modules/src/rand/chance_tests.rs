use super::*;
use loom_core::ModuleConfig;
use loom_core::testing::run;

/// The eight outputs of one render: `a[lane]` and `b[lane]`.
struct Lanes {
    a: Vec<Vec<f32>>,
    b: Vec<Vec<f32>>,
}

/// A module with these parameters, by name.
fn chance(params: &[(&str, f32)]) -> Box<dyn Module> {
    let ModuleRole::Dsp(create) = CHANCE.role else {
        unreachable!()
    };
    let mut m = create(&ModuleConfig {
        sample_rate: 48_000.0,
        max_block: 64,
    });
    for (name, value) in params {
        m.set_param(CHANCE.param_index(name).unwrap(), *value);
    }
    m
}

/// Renders `frames` frames in blocks of `block`. `inputs` holds the lane
/// triggers then the reset; a `None` has no cable.
fn render(m: &mut dyn Module, inputs: [Option<&[f32]>; 5], frames: usize, block: usize) -> Lanes {
    let mut outs = vec![vec![0.0; frames]; 8];
    let zeros = vec![0.0; frames];
    for start in (0..frames).step_by(block) {
        let end = (start + block).min(frames);
        let slices: Vec<&[f32]> = inputs
            .iter()
            .map(|input| &input.unwrap_or(&zeros)[start..end])
            .collect();
        let connected: Vec<bool> = inputs.iter().map(Option::is_some).collect();
        let mut parts: Vec<&mut [f32]> = outs.iter_mut().map(|out| &mut out[start..end]).collect();
        run(m, 48_000.0, &slices, &connected, &mut parts, &[]);
    }
    let b = outs.split_off(4);
    Lanes { a: outs, b }
}

/// A clock of one-sample triggers, every `period` frames.
fn clock(frames: usize, period: usize) -> Vec<f32> {
    (0..frames).map(|i| if i % period == 0 { 1.0 } else { 0.0 }).collect()
}

/// How many frames of an output are high.
fn fired(out: &[f32]) -> usize {
    out.iter().filter(|&&v| v > 0.5).count()
}

/// Chance 0 never sends to A, and 1 always does, so B gets the rest.
#[test]
fn chance_zero_never_picks_a_and_one_always_does() {
    let trigger = clock(10_000, 2);
    let mut m = chance(&[("chance1", 0.0), ("chance2", 1.0), ("chance3", 0.0), ("chance4", 1.0)]);
    let out = render(m.as_mut(), [Some(&trigger), None, None, None, None], 10_000, 64);
    assert_eq!(fired(&out.a[0]), 0);
    assert_eq!(fired(&out.b[0]), 5_000);
    assert_eq!(fired(&out.a[1]), 5_000);
    assert_eq!(fired(&out.b[1]), 0);
}

/// Over 10,000 triggers at 0.3, between 28% and 32% go to A, on every lane
/// and for several seeds.
#[test]
fn the_share_sent_to_a_follows_the_chance() {
    let trigger = clock(20_000, 2);
    for seed in [1.0, 2.0, 3.0, 65535.0] {
        let mut m = chance(&[
            ("seed", seed),
            ("chance1", 0.3),
            ("chance2", 0.3),
            ("chance3", 0.3),
            ("chance4", 0.3),
        ]);
        let out = render(m.as_mut(), [Some(&trigger), None, None, None, None], 20_000, 64);
        for (lane, a) in out.a.iter().enumerate() {
            let share = fired(a) as f32 / 10_000.0;
            assert!((0.28..=0.32).contains(&share), "seed {seed}, lane {lane}: {share}");
        }
    }
}

/// A gate of 100 frames chosen for A gives 100 frames on A and none on B,
/// and the output carries the input's value.
#[test]
fn a_chosen_gate_keeps_its_length_and_value() {
    let mut gate = vec![0.0; 300];
    gate[50..150].fill(0.8);
    let mut m = chance(&[("chance1", 1.0)]);
    let out = render(m.as_mut(), [Some(&gate), None, None, None, None], 300, 64);
    assert_eq!(out.a[0], gate);
    assert_eq!(fired(&out.b[0]), 0);
}

/// One clock on lane 1 drives lanes 2 to 4 when they have no cable, and each
/// lane makes its own choice.
#[test]
fn a_lane_with_no_cable_follows_the_lane_above() {
    let trigger = clock(4_000, 2);
    let mut m = chance(&[]);
    let out = render(m.as_mut(), [Some(&trigger), None, None, None, None], 4_000, 64);
    for lane in 0..4 {
        assert_eq!(fired(&out.a[lane]) + fired(&out.b[lane]), 2_000, "lane {lane}");
    }
    assert_ne!(out.a[0], out.a[1], "lanes choose on their own");
    assert_ne!(out.a[1], out.a[2], "lanes choose on their own");
}

/// Lane 1 with no cable gets nothing, even when lane 2 has one.
#[test]
fn lane_one_with_no_cable_gets_nothing() {
    let trigger = clock(1_000, 2);
    let mut m = chance(&[("chance1", 1.0)]);
    let out = render(m.as_mut(), [None, Some(&trigger), None, None, None], 1_000, 64);
    assert_eq!(fired(&out.a[0]) + fired(&out.b[0]), 0);
    assert_eq!(fired(&out.a[1]) + fired(&out.b[1]), 500);
}

/// With the same clock cabled to lane 3, its choices are the same whether or
/// not lane 2 has its own cable, because each lane has its own stream.
#[test]
fn patching_another_lane_does_not_change_a_lanes_choices() {
    let trigger = clock(4_000, 2);
    let other = clock(4_000, 7);
    let lane_three = |lane2: Option<&[f32]>| {
        let mut m = chance(&[]);
        render(
            m.as_mut(),
            [Some(&trigger), lane2, Some(&trigger), None, None],
            4_000,
            64,
        )
        .a[2]
            .clone()
    };
    assert_eq!(lane_three(None), lane_three(Some(&other)));
}

/// The same seed gives the same choices at any block size, and another seed
/// gives other choices.
#[test]
fn choices_depend_on_the_seed_and_not_the_block_size() {
    let trigger = clock(4_000, 3);
    let choices = |seed: f32, block: usize| {
        let mut m = chance(&[("seed", seed)]);
        render(m.as_mut(), [Some(&trigger), None, None, None, None], 4_000, block).a[0].clone()
    };
    let first = choices(1.0, 1);
    assert_eq!(first, choices(1.0, 7));
    assert_eq!(first, choices(1.0, 64));
    assert_ne!(first, choices(2.0, 64));
}

/// A reset restarts the choices from the seed. On the same frame as a
/// trigger it comes first, so the trigger gets the first choice.
#[test]
fn a_reset_replays_the_choices() {
    let trigger = clock(2_000, 2);
    let mut reset = vec![0.0; 2_000];
    reset[1_000] = 1.0;
    let mut m = chance(&[]);
    let out = render(m.as_mut(), [Some(&trigger), None, None, None, Some(&reset)], 2_000, 64);
    assert_eq!(out.a[0][..1_000], out.a[0][1_000..]);
}

/// A reset while a gate is held keeps its choice: it is no new edge.
#[test]
fn a_reset_does_not_cut_a_held_gate() {
    let gate = vec![1.0; 100];
    let mut reset = vec![0.0; 100];
    reset[50] = 1.0;
    let mut m = chance(&[("chance1", 1.0)]);
    let out = render(m.as_mut(), [Some(&gate), None, None, None, Some(&reset)], 100, 64);
    assert_eq!(fired(&out.a[0]), 100);
}

/// Different seeds, channels, and streams start from different states, also
/// for the low seeds a player tries first.
#[test]
fn nearby_seeds_start_far_apart() {
    let first = |seed: u32, channel: usize, number: u32| stream(seed, channel, number).draw();
    let values = [first(1, 0, 0), first(2, 0, 0), first(1, 1, 0), first(1, 0, 1)];
    for (i, a) in values.iter().enumerate() {
        for b in &values[i + 1..] {
            assert!((a - b).abs() > 0.01, "{values:?}");
        }
    }
}

/// A right-hand stereo copy mixes its channel into the seed, so it does not
/// make the left side's choices (M2 modules, rule 1).
#[test]
fn a_right_hand_copy_makes_other_choices() {
    use loom_core::testing::run_with_transport_and_channel;
    let trigger = clock(1_000, 2);
    let zeros = vec![0.0; 1_000];
    let side = |channel: usize| {
        let mut m = chance(&[]);
        let mut outs = vec![vec![0.0; 1_000]; 8];
        let mut parts: Vec<&mut [f32]> = outs.iter_mut().map(Vec::as_mut_slice).collect();
        run_with_transport_and_channel(
            m.as_mut(),
            48_000.0,
            loom_core::TransportInfo::default(),
            &[&trigger, &zeros, &zeros, &zeros, &zeros],
            &[true, false, false, false, false],
            &mut parts,
            &[],
            channel,
        );
        outs.swap_remove(0)
    };
    assert_ne!(side(0), side(1));
}

/// State copies only between instances of the same module.
#[test]
fn another_module_gives_no_state_to_copy() {
    let ModuleRole::Dsp(create) = SAMPLE_HOLD.role else {
        unreachable!()
    };
    let other = create(&ModuleConfig {
        sample_rate: 48_000.0,
        max_block: 64,
    });
    assert!(!chance(&[]).copy_state_from(other.as_ref()));
}

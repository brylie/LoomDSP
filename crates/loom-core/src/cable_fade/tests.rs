use super::*;

/// 5 ms at 48 kHz is 240 frames; the length is never 0, whatever the rate.
#[test]
fn the_fade_is_5_ms_and_never_empty() {
    assert_eq!(fade_frames(48_000.0), 240);
    assert_eq!(fade_frames(44_100.0), 221);
    assert_eq!(fade_frames(0.0), 1);
    assert_eq!(fade_frames(f32::NAN), 1);
    assert_eq!(fade_frames(f32::INFINITY), u32::MAX);
}

/// Frame 0 is the held value exactly, frame k is the straight line, and
/// from frame N on the signal passes through bit for bit.
#[test]
fn the_ramp_holds_then_moves_in_a_straight_line() {
    let length = 4;
    let mut ramp = Ramp::new(1.0, length);
    let signal = [0.0, 0.0, 0.0, 0.0, 0.3, -0.7];
    let mut out = [9.0; 6];
    ramp.run(&signal, &mut out);
    assert_eq!(out, [1.0, 0.75, 0.5, 0.25, 0.3, -0.7]);
    assert!(ramp.is_done());
}

/// The ramp reaches a moving signal, not a fixed one.
#[test]
fn the_ramp_follows_a_moving_signal() {
    let mut ramp = Ramp::new(0.0, 2);
    assert_eq!(ramp.next(0.8), 0.0);
    assert_eq!(ramp.next(0.6), 0.3);
    assert_eq!(ramp.next(0.4), 0.4);
}

/// Running in blocks of any size gives the same samples as running in one.
#[test]
fn the_ramp_is_the_same_at_every_block_size() {
    let signal: Vec<f32> = (0..500).map(|n| (n as f32 * 0.031).sin()).collect();
    let mut whole = vec![0.0; signal.len()];
    Ramp::new(0.9, 240).run(&signal, &mut whole);
    for block in [1, 77, 512] {
        let mut ramp = Ramp::new(0.9, 240);
        let mut out = vec![0.0; signal.len()];
        for (target, out) in signal.chunks(block).zip(out.chunks_mut(block)) {
            ramp.run(target, out);
        }
        assert_eq!(out, whole, "block size {block}");
    }
}

/// A held value that is NaN or infinite does not fade: the input switches.
#[test]
fn a_broken_held_value_switches() {
    for held in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut ramp = Ramp::new(held, 240);
        assert!(ramp.is_done());
        assert_eq!(ramp.next(0.25), 0.25);
    }
}

/// Fades between the largest floats never give a value that is not finite.
#[test]
fn a_ramp_between_extreme_values_stays_finite() {
    for (held, signal) in [
        (f32::MAX, -f32::MAX),
        (-f32::MAX, f32::MAX),
        (f32::MAX, f32::MAX),
        (-f32::MAX, -f32::MAX),
    ] {
        let mut ramp = Ramp::new(held, 240);
        for _ in 0..241 {
            let value = ramp.next(signal);
            assert!(value.is_finite(), "{held} to {signal}: {value}");
        }
    }
}

/// The position stops at the end, so a fade never starts again, however
/// long the engine runs.
#[test]
fn the_position_never_wraps() {
    let mut ramp = Ramp::new(0.5, 240);
    ramp.advance(u32::MAX - 1);
    ramp.advance(u32::MAX);
    assert!(ramp.is_done());
    assert_eq!(ramp.next(0.1), 0.1);
}

/// A carried ramp goes on exactly where the old one was.
#[test]
fn a_carried_ramp_continues() {
    let mut old = Ramp::new(1.0, 4);
    old.advance(2);
    let mut new = Ramp::finished(4);
    new.continue_from(&old);
    assert_eq!(new.next(0.0), 0.5);
    assert_eq!(new.next(0.0), 0.25);
    assert!(new.next(0.0) == 0.0 && new.is_done());
}

/// Fades read their targets into their own buffers over a range.
#[test]
fn running_a_fade_fills_its_buffer_from_its_target() {
    let target = [0.0f32; 8];
    let mut buffer = [9.0f32; 8];
    let mut fades = [InputFade::new(
        input_key(1, 0, 0),
        (true, None),
        4,
        target.as_ptr(),
        buffer.as_mut_ptr(),
    )];
    fades[0].ramp = Ramp::new(1.0, 4);
    run(&mut fades, 0, 3);
    run(&mut fades, 3, 5);
    assert_eq!(buffer, [1.0, 0.75, 0.5, 0.25, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(fades[0].buffer(), buffer.as_ptr());
}

/// Keys sort by module, then port, then channel.
#[test]
fn input_keys_sort_by_module_port_and_channel() {
    assert!(input_key(1, 5, 9) < input_key(2, 0, 0));
    assert!(input_key(1, 0, 9) < input_key(1, 1, 0));
    assert_eq!(key_channel(input_key(3, 4, 5)), 5);
}

/// The mixer list names the four mixers the editor adds.
#[test]
fn the_mixers_are_the_four_the_editor_adds() {
    assert_eq!(MIXERS, [SUM_MODULE, SPREAD_SUM_MODULE, SELECT_MODULE, SWAP_FADE_MODULE]);
}

/// When the old schedule never ran, a fresh fade reads no memory: a fixed
/// default gives its value, and anything else waits for a lower channel or
/// starts from 0 (rule 9, two swaps before one block).
#[test]
fn a_schedule_that_never_ran_is_read_without_touching_memory() {
    use crate::compile::Read;
    let mut old = Schedule::empty();
    old.reads.push(Read {
        key: input_key(1, 0, 0),
        ptr: std::ptr::null(),
        constant: Some(0.6),
    });
    old.reads.push(Read {
        key: input_key(1, 1, 0),
        ptr: std::ptr::null(),
        constant: None,
    });
    assert_eq!(last_read(&old, input_key(1, 0, 0), 0), Some(0.6));
    assert_eq!(last_read(&old, input_key(1, 1, 0), 0), None);
    assert_eq!(last_read(&old, input_key(2, 0, 0), 0), None);
}

/// A right side starts from its own voice's left side, a new copy of a
/// module from copy 0, and a new voice that is summed from nothing.
#[test]
fn a_fade_with_nothing_to_read_takes_the_right_start() {
    assert_eq!(fallback(1, false), Some(0));
    assert_eq!(fallback(5, true), Some(4));
    assert_eq!(fallback(4, true), Some(0));
    assert_eq!(fallback(4, false), None);
    assert_eq!(fallback(0, true), None);
}

/// A contribution ramp from a value gives the same samples as a value
/// ramp, so a swap's fresh ramp sounds as it did before setting ramps.
#[test]
fn a_contribution_ramp_from_a_value_matches_the_value_ramp() {
    let mut value = Ramp::new(0.4, 240);
    let mut contribution = ContributionRamp::from_value(0.4, 240);
    for n in 0..300 {
        let x = (n as f32 * 0.07).sin();
        let new = 0.3 * x;
        assert_eq!(contribution.next(x, new), value.next(new), "frame {n}");
    }
    assert!(contribution.is_done());
}

/// A held value that is NaN or infinite gives a finished ramp: the
/// contribution switches (rule 9, The ramp).
#[test]
fn a_broken_held_contribution_switches() {
    for held in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let ramp = ContributionRamp::from_value(held, 240);
        assert!(ramp.is_done());
        assert_eq!(ramp.value(0.5, 0.25), 0.25);
    }
}

/// A ramp from a held value never reads the source for its old side, so a
/// NaN source cannot reach the held value.
#[test]
fn a_ramp_from_a_value_does_not_read_the_source() {
    let mut ramp = ContributionRamp::from_value(0.5, 4);
    assert!(!ramp.reads_source());
    assert_eq!(ramp.next(f32::NAN, 0.0), 0.5);
    assert_eq!(ramp.next(f32::NAN, 0.0), 0.375);
    let blended = ContributionRamp::from_map((0.5, 0.0), 4);
    assert!(blended.reads_source());
}

/// A setting ramp blends `old(x)` into `new(x)` on the source's own sample,
/// and ends on `new` exactly.
#[test]
fn a_setting_ramp_blends_two_maps_of_the_moving_source() {
    let length = 4;
    let mut ramp = ContributionRamp::from_map((0.5, 0.0), length);
    for k in 0..6 {
        let x = [0.2f32, -0.6, 0.9, 0.1, -0.3, 0.7][k];
        let (old, new) = (0.5 * x, 0.25 * x);
        let t = (k as f32 / length as f32).min(1.0);
        let expected = old * (1.0 - t) + new * t;
        assert!((ramp.next(x, new) - expected).abs() <= 1e-7, "frame {k}");
    }
    assert!(ramp.is_done());
    assert_eq!(ramp.next(0.3, 0.075), 0.075, "the new setting exactly");
}

/// A change during a blend starts from where the blend is: the map it
/// plays at its current position.
#[test]
fn a_restarted_ramp_starts_from_the_running_blend() {
    let mut ramp = ContributionRamp::from_map((1.0, 0.0), 4);
    ramp.next(0.0, 0.0);
    ramp.next(0.0, 0.0);
    // Half way from `x` to the setting `(0, 0.5)`.
    assert_eq!(ramp.map_toward((0.0, 0.5)), (0.5, 0.25));
    let before = ramp.value(0.6, 0.5);
    ramp.restart_toward((0.0, 0.5));
    assert!((ramp.value(0.6, 0.0) - before).abs() <= 1e-7, "no step");
    let mut done = ContributionRamp::finished(4);
    done.restart_toward((0.2, 0.1));
    assert_eq!(
        done.map_toward((0.0, 0.0)),
        (0.2, 0.1),
        "an ended ramp starts from the setting"
    );
}

/// Each row of the setting table in the spec, and the map gives what
/// `depth · remap(x)` gives.
#[test]
fn settings_map_the_source_sample_in_a_straight_line() {
    use crate::compile::AssignmentSettings;
    use crate::descriptor::Polarity;
    let set = |depth: f32, enabled: bool, polarity: Option<Polarity>| AssignmentSettings {
        depth,
        enabled,
        polarity,
    };
    let bipolar = runtime(1, 0.5, Polarity::Bipolar);
    let unipolar = runtime(1, 0.5, Polarity::Unipolar);
    assert_eq!(bipolar.map(set(0.5, false, None)), (0.0, 0.0));
    assert_eq!(bipolar.map(set(0.0, true, None)), (0.0, 0.0));
    assert_eq!(bipolar.map(set(0.5, true, None)), (0.5, 0.0));
    assert_eq!(bipolar.map(set(0.5, true, Some(Polarity::Bipolar))), (0.5, 0.0));
    assert_eq!(bipolar.map(set(0.5, true, Some(Polarity::Unipolar))), (0.25, 0.25));
    assert_eq!(unipolar.map(set(0.5, true, Some(Polarity::Bipolar))), (1.0, -0.5));
    let mut silent = runtime(1, 0.5, Polarity::Bipolar);
    silent.silent_source = true;
    assert_eq!(silent.map(set(0.5, true, None)), (0.0, 0.0));
    for source in [Polarity::Bipolar, Polarity::Unipolar] {
        for polarity in [None, Some(Polarity::Unipolar), Some(Polarity::Bipolar)] {
            let mut runtime = runtime(1, 0.5, source);
            runtime.polarity = polarity;
            let (g, h) = runtime.map(runtime.settings());
            for x in [-1.0f32, -0.3, 0.0, 0.8] {
                assert!((g * x + h - runtime.contribution(x)).abs() <= 1e-7);
            }
        }
    }
}

/// A runtime with id `id`, depth, and the source's polarity, settled.
fn runtime(id: u32, depth: f32, source_polarity: crate::descriptor::Polarity) -> crate::compile::AssignmentRuntime {
    let settings = crate::compile::AssignmentSettings {
        depth,
        enabled: true,
        polarity: None,
    };
    crate::compile::AssignmentRuntime {
        id: crate::graph::AssignmentId(id),
        depth,
        enabled: true,
        polarity: None,
        source_polarity,
        silent_source: false,
        played: settings,
        changed: false,
    }
}

/// A fill with key `key` and the given sources, reading nothing.
fn fill(key: u64, sources: Vec<ModSource>) -> crate::compile::ParamFill {
    crate::compile::ParamFill {
        key,
        slot: 0,
        param: 0,
        spec: crate::descriptor::ParamSpec {
            name: "p",
            label: "P",
            description: "A test parameter.",
            min: -1.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: crate::descriptor::ParamOptions::DEFAULT,
        },
        base: 0.0,
        smooth: None,
        buffer: std::ptr::null_mut(),
        sources,
        live: crate::live::NO_CELL,
    }
}

/// A live source of assignment `id` on runtime `state`, with a ramp slot.
fn live(id: u32, state: u32, ramp: ContributionRamp, last: f32) -> ModSource {
    ModSource::Live {
        id: crate::graph::AssignmentId(id),
        state,
        buffer: std::ptr::null(),
        ramp: Some(ramp),
        last,
        planned: false,
        fresh: false,
    }
}

/// The ramp and last value of the first source of fill `i`.
fn state_of(schedule: &Schedule, i: usize) -> (ContributionRamp, f32) {
    match &schedule.fills[i].sources[0] {
        ModSource::Live {
            ramp: Some(ramp), last, ..
        } => (*ramp, *last),
        _ => panic!("a live source with a slot"),
    }
}

/// A schedule with these fills, sorted by key, and runtimes.
fn schedule(fills: Vec<crate::compile::ParamFill>, assignments: Vec<crate::compile::AssignmentRuntime>) -> Schedule {
    let mut schedule = Schedule::empty();
    let mut order: Vec<u32> = (0..fills.len() as u32).collect();
    order.sort_by_key(|&i| fills[i as usize].key);
    schedule.fills = fills;
    schedule.fill_order = order;
    schedule.assignments = assignments;
    schedule
}

/// A swap whose old source names a runtime index past the old table, or a
/// fill key the old schedule lacks, counts as no match, and nothing panics.
#[test]
fn a_stale_old_source_counts_as_no_match() {
    use crate::descriptor::Polarity;
    let running = ContributionRamp::from_map((0.5, 0.1), 240);
    // Runtime 7 does not exist in the old schedule.
    let old = schedule(
        vec![fill(1, vec![live(1, 7, running, 0.3)])],
        vec![runtime(1, 0.5, Polarity::Bipolar)],
    );
    let mut new = schedule(
        vec![
            fill(1, vec![live(1, 0, ContributionRamp::finished(240), 0.0)]),
            fill(2, vec![live(1, 0, ContributionRamp::finished(240), 0.0)]),
        ],
        vec![runtime(1, 0.25, Polarity::Bipolar)],
    );
    take_contributions_from(&mut new, &old, 64);
    for i in 0..2 {
        let (ramp, last) = state_of(&new, i);
        assert!(ramp.is_done(), "fill {i}");
        assert_eq!(last, 0.0, "fill {i}");
    }
}

/// A right side and a new voice with no old match take their fallback
/// fill's `last`, `g0`, `h0`, and position, so every side blends together.
#[test]
fn new_sides_and_voices_take_their_fallback_fill_s_ramp() {
    use crate::descriptor::Polarity;
    let mut running = ContributionRamp::from_map((0.5, 0.1), 240);
    running.next(0.0, 0.0);
    let left = 4 << 32;
    let right = left | (1 << 31);
    let voice = left | (2 << 24);
    let old = schedule(
        vec![fill(left, vec![live(1, 0, running, 0.3)])],
        vec![runtime(1, 0.5, Polarity::Bipolar)],
    );
    let mut new = schedule(
        vec![
            fill(right, vec![live(1, 0, ContributionRamp::finished(240), 0.0)]),
            fill(voice, vec![live(1, 0, ContributionRamp::finished(240), 0.0)]),
            fill(left, vec![live(1, 0, ContributionRamp::finished(240), 0.0)]),
        ],
        vec![runtime(1, 0.5, Polarity::Bipolar)],
    );
    take_contributions_from(&mut new, &old, 64);
    let left_state = state_of(&new, 2);
    assert_eq!(left_state, (running, 0.3), "the left side carries its ramp on");
    assert_eq!(state_of(&new, 0), left_state, "the right side");
    assert_eq!(state_of(&new, 1), left_state, "a new voice");
}

/// At a swap, a source whose settings differ from what its old runtime
/// played blends from that setting, as undo brings back.
#[test]
fn a_swap_with_new_settings_blends_from_the_setting_that_played() {
    use crate::descriptor::Polarity;
    let old = schedule(
        vec![fill(1, vec![live(1, 0, ContributionRamp::finished(240), 0.3)])],
        vec![runtime(1, 0.5, Polarity::Bipolar)],
    );
    let mut new = schedule(
        vec![fill(1, vec![live(1, 0, ContributionRamp::finished(240), 0.0)])],
        vec![runtime(1, 0.25, Polarity::Bipolar)],
    );
    take_contributions_from(&mut new, &old, 64);
    let (ramp, last) = state_of(&new, 0);
    assert_eq!(ramp, ContributionRamp::from_map((0.5, 0.0), 240));
    assert_eq!(last, 0.3);
    // The same settings carry the old ramp on instead.
    let mut same = schedule(
        vec![fill(1, vec![live(1, 0, ContributionRamp::finished(240), 0.0)])],
        vec![runtime(1, 0.5, Polarity::Bipolar)],
    );
    take_contributions_from(&mut same, &old, 64);
    assert!(state_of(&same, 0).0.is_done());
}

/// One batch that changes several assignments starts a ramp on each of
/// their sources and on no other; settings sent back to what plays start
/// none; and before the first block nothing starts.
#[test]
fn the_pass_after_a_batch_starts_ramps_only_where_settings_changed() {
    use crate::descriptor::Polarity;
    let build = || {
        schedule(
            vec![
                fill(1, vec![live(1, 0, ContributionRamp::finished(240), 0.0)]),
                fill(2, vec![live(2, 1, ContributionRamp::finished(240), 0.0)]),
                fill(3, vec![live(3, 2, ContributionRamp::finished(240), 0.0)]),
                fill(4, vec![live(1, 0, ContributionRamp::finished(240), 0.0)]),
            ],
            vec![
                runtime(1, 0.5, Polarity::Bipolar),
                runtime(2, 0.5, Polarity::Bipolar),
                runtime(3, 0.5, Polarity::Bipolar),
            ],
        )
    };
    let mut schedule = build();
    for (index, depth) in [(0, 0.25), (1, 0.75)] {
        schedule.assignments[index].depth = depth;
        schedule.assignments[index].changed = true;
    }
    // Changed and changed back: no ramp.
    schedule.assignments[2].changed = true;
    schedule.assignments_changed = true;
    start_setting_ramps(&mut schedule, true);
    for (i, ramps) in [(0, true), (1, true), (2, false), (3, true)] {
        let (ramp, _) = state_of(&schedule, i);
        assert_eq!(!ramp.is_done(), ramps, "fill {i}");
    }
    assert_eq!(state_of(&schedule, 0).0, ContributionRamp::from_map((0.5, 0.0), 240));
    assert!(
        schedule
            .assignments
            .iter()
            .all(|a| !a.changed && a.played == a.settings())
    );
    assert!(!schedule.assignments_changed);

    let mut before = build();
    before.assignments[0].depth = 0.25;
    before.assignments[0].changed = true;
    before.assignments_changed = true;
    start_setting_ramps(&mut before, false);
    assert!(
        state_of(&before, 0).0.is_done(),
        "nothing starts before the first block"
    );
    assert_eq!(before.assignments[0].played.depth, 0.25);
}

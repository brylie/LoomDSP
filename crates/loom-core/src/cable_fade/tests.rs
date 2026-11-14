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
        true,
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

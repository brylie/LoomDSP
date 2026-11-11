use super::*;

/// Three outputs with room for an offset of 1 and 2 frames, and a right buffer
/// for output 1, handed to `check`.
fn with_three_outputs(check: impl FnOnce(&mut PortIo<'_>)) -> [[f32; 3]; 4] {
    let mut buffers = [[0.0f32; 3]; 4];
    let [first, second, third, right] = &mut buffers;
    let outputs = [first.as_mut_ptr(), second.as_mut_ptr(), third.as_mut_ptr()];
    let right = [RightPtr {
        port: 1,
        output: true,
        ptr: right.as_mut_ptr(),
    }];
    // SAFETY: each buffer holds `offset + frames` = 3 floats and outlives `io`,
    // and all four are distinct.
    let mut io = unsafe { PortIo::new(&[], &outputs, &[], &right, &[], 0, 1, 2) };
    check(&mut io);
    buffers
}

/// `split_outputs` hands back each port's own buffers, at the call's offset, so
/// a module can hold them all across its frame loop.
#[test]
fn split_outputs_hands_back_each_ports_buffers() {
    let buffers = with_three_outputs(|io| {
        let ([third, first], [[left, right]]) = io.split_outputs([2, 0], [1]);
        assert_eq!([first.len(), third.len(), left.len(), right.len()], [2; 4]);
        first.fill(1.0);
        left.fill(2.0);
        third.fill(3.0);
        right.fill(4.0);
    });
    assert_eq!(
        buffers,
        [[0.0, 1.0, 1.0], [0.0, 2.0, 2.0], [0.0, 3.0, 3.0], [0.0, 4.0, 4.0]]
    );
}

/// `stereo_output` is the same pair that `split_outputs` gives.
#[test]
fn stereo_output_gives_left_then_right() {
    let buffers = with_three_outputs(|io| {
        let [left, right] = io.stereo_output(1);
        left.fill(2.0);
        right.fill(4.0);
    });
    assert_eq!(buffers[1], [0.0, 2.0, 2.0]);
    assert_eq!(buffers[3], [0.0, 4.0, 4.0]);
}

/// Naming a port twice would give two overlapping mutable slices.
#[test]
#[should_panic(expected = "port 0 twice")]
fn split_outputs_refuses_a_mono_port_twice() {
    with_three_outputs(|io| {
        io.split_outputs([0, 0], []);
    });
}

/// The same holds across the two lists.
#[test]
#[should_panic(expected = "port 1 twice")]
fn split_outputs_refuses_a_port_in_both_lists() {
    with_three_outputs(|io| {
        io.split_outputs([1], [1]);
    });
}

/// A port with no right buffer cannot be stereo.
#[test]
#[should_panic(expected = "declared Channels::Stereo")]
fn split_outputs_refuses_a_stereo_request_on_a_mono_port() {
    with_three_outputs(|io| {
        io.split_outputs([], [0]);
    });
}

/// In a debug build, a right buffer that is the left one fails instead of
/// aliasing silently. The check runs before any slice is made.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "are one buffer")]
fn stereo_output_refuses_an_aliased_right_buffer() {
    let mut buffer = [0.0f32; 2];
    let outputs = [buffer.as_mut_ptr()];
    let right = [RightPtr {
        port: 0,
        output: true,
        ptr: buffer.as_mut_ptr(),
    }];
    // SAFETY: the buffer holds `offset + frames` = 2 floats and outlives `io`.
    // The right pointer breaks the aliasing rule on purpose, and the check
    // under test panics before anything reads or writes through it.
    let mut io = unsafe { PortIo::new(&[], &outputs, &[], &right, &[], 0, 0, 2) };
    io.stereo_output(0);
}

/// `Param` gives the modulated sample where a parameter has a buffer and
/// the base everywhere else, also after `split` and with an offset.
#[test]
fn param_falls_back_to_the_base() {
    let modulated = [0.0, 0.1, 0.2, 0.3];
    let mut out = [0.0f32; 4];
    let outputs = [out.as_mut_ptr()];
    let params = [(1u16, modulated.as_ptr())];
    // SAFETY: each buffer holds `offset + frames` = 4 floats and outlives
    // `io`; the one output does not alias the parameter buffer.
    let mut io = unsafe { PortIo::new(&[], &outputs, &params, &[], &[], 0, 1, 3) };
    let (plain, swept) = (io.param(0), io.param(1));
    assert!(!plain.is_modulated());
    assert!(swept.is_modulated());
    assert_eq!(io.param_at_or(1, 0, 9.0), 0.1);
    assert_eq!(io.param_at_or(0, 0, 9.0), 9.0);
    let ([], [out]) = io.split::<0, 1>();
    for (i, out) in out.iter_mut().enumerate() {
        *out = plain.at_or(i, 5.0) + swept.at_or(i, 100.0);
    }
    assert_eq!(out, &[5.1, 5.2, 5.3]);
    assert_eq!(swept.map_or(2, 0u8, |v| (v * 10.0).round() as u8), 3);
    assert_eq!(plain.map_or(2, 7u8, |_| 0), 7);
}

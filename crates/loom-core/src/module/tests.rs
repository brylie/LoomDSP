use super::*;

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
    let mut io = unsafe { PortIo::new(&[], &outputs, &params, &[], 0, 1, 3) };
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

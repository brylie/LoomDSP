use super::*;

#[test]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn denormals_flush_to_zero_inside_guard() {
    let tiny = std::hint::black_box(f32::MIN_POSITIVE);
    {
        let _guard = ScopedFlushDenormals::new();
        let result = std::hint::black_box(tiny) / std::hint::black_box(4.0f32);
        assert_eq!(result, 0.0);
    }
    let result = std::hint::black_box(tiny) / std::hint::black_box(4.0f32);
    assert!(result > 0.0, "mode must be restored after the guard drops");
}

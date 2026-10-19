//! Smoke test for `loom-app --screenshot`: it renders an example patch to a
//! PNG file and exits with status 0. Skipped where no GPU adapter is found.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Runs the app with these arguments and returns the decoded image as
/// (width, height, RGBA pixels), or `None` when there is no GPU.
fn screenshot(name: &str, args: &[&str]) -> Option<(u32, u32, Vec<u8>)> {
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_file(&out);
    let patch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron");
    let result = Command::new(env!("CARGO_BIN_EXE_loom-app"))
        .arg(patch)
        .arg("--screenshot")
        .arg(&out)
        .args(args)
        .output()
        .expect("loom-app runs");
    let stderr = String::from_utf8_lossy(&result.stderr);
    // Must match `screenshot::NO_GPU`.
    if stderr.contains("no GPU adapter") {
        eprintln!("skipped: {stderr}");
        return None;
    }
    assert!(result.status.success(), "loom-app failed: {stderr}");

    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&out).expect("PNG written")));
    let mut reader = decoder.read_info().expect("a PNG header");
    let mut pixels = vec![0; reader.output_buffer_size().expect("a sane size")];
    let info = reader.next_frame(&mut pixels).expect("PNG data");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    pixels.truncate(info.buffer_size());
    Some((info.width, info.height, pixels))
}

/// Number of distinct colours, to tell a drawn patch from a flat fill.
fn colours(pixels: &[u8]) -> usize {
    let mut seen: Vec<[u8; 4]> = pixels.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect();
    seen.sort_unstable();
    seen.dedup();
    seen.len()
}

#[test]
fn renders_an_example_patch() {
    let Some((width, height, pixels)) = screenshot("subtractive.png", &["--size", "320x200"]) else {
        return;
    };
    assert_eq!((width, height), (320, 200));
    // Tiles, cables, knobs, and text give many colours. A blank canvas has
    // only the background and the grid dots.
    assert!(colours(&pixels) > 50, "only {} colours", colours(&pixels));
}

#[test]
fn renders_at_a_scale_with_focus() {
    let Some((width, height, pixels)) = screenshot(
        "subtractive-focus.png",
        &[
            "--size", "160x100", "--scale", "2", "--zoom", "2", "--focus", "4:cutoff",
        ],
    ) else {
        return;
    };
    assert_eq!((width, height), (320, 200));
    assert!(colours(&pixels) > 50, "only {} colours", colours(&pixels));
}

#[test]
fn an_unknown_control_fails() {
    let result = Command::new(env!("CARGO_BIN_EXE_loom-app"))
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron"))
        .arg("--screenshot")
        .arg(PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("unused.png"))
        .args(["--focus", "4:nope"])
        .output()
        .expect("loom-app runs");
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("no control for parameter `nope`"));
}

//! Tests for `loom midi-port`, run on the built binary
//! (`docs/design/audio-qa.md`, Part 4).

use std::io::Write;
use std::process::{Command, Stdio};

/// Runs `loom midi-port` with `args`, writes `input` to its standard input,
/// and returns the exit status, stdout, and stderr.
fn midi_port(args: &[&str], input: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("midi-port")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Where virtual ports exist, the command prints the input name, plays the
/// lines, reports a bad one, and exits with status 0 at the end of input.
/// Where they do not (Windows, or Linux without the ALSA sequencer), it
/// says why and exits with status 2. Either way the run is checked, so the
/// test runs everywhere.
#[test]
fn plays_the_lines_or_says_why_it_cannot() {
    let name = format!("Loom cli test {}", std::process::id());
    let (status, stdout, stderr) = midi_port(&[&name, "--channel", "2"], "on 60\nbad\nwait 1\noff 60\non 62\n");
    if status == 2 {
        assert!(stderr.starts_with("error: "), "{stderr}");
        assert!(stdout.is_empty(), "{stdout}");
        return;
    }
    assert_eq!(status, 0, "stderr: {stderr}");
    assert!(stdout.starts_with("MIDI input: "), "{stdout}");
    assert!(stdout.contains(&name), "{stdout}");
    assert!(stderr.contains("line 2: cannot read `bad`"), "{stderr}");
}

/// A channel outside 1 to 16 is refused before any port is made.
#[test]
fn a_channel_out_of_range_is_refused() {
    let (status, _, stderr) = midi_port(&["Loom cli test", "--channel", "17"], "");
    assert_eq!(status, 2, "clap's usage error: {stderr}");
    assert!(stderr.contains("--channel"), "{stderr}");
}

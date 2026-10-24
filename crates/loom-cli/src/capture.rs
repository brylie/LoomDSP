//! `loom capture`: records an input device, such as the input side of a
//! loopback device, to a WAV file (`docs/design/audio-qa.md`, Part 4).
//! `loom analyze` then checks what really reached the device.

use loom_host_desktop::{AudioOptions, Capture, CaptureReport};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

/// The options of `loom capture`.
#[derive(clap::Args)]
pub struct Options {
    /// Audio input device (part of the name), for example a loopback
    /// device. `loom devices` lists the inputs.
    #[arg(long)]
    device: String,
    /// WAV file to write: 32-bit float, stereo, at the device's rate.
    #[arg(short, long)]
    output: PathBuf,
    /// How long to record, in seconds.
    #[arg(long)]
    seconds: f64,
    /// Sample rate in Hz. Default: device default.
    #[arg(long)]
    sample_rate: Option<u32>,
}

/// Exit status of a broken run: no device, a write error, dropped
/// frames, or a stream error. `loom analyze` uses 1 for a failed check.
const BROKEN: u8 = 2;

/// Runs `loom capture`. Exit status 0 when every frame the device
/// delivered was written, and 2 otherwise.
pub fn run(options: &Options) -> ExitCode {
    match capture(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(BROKEN)
        }
    }
}

fn capture(options: &Options) -> Result<(), String> {
    let length = length(options.seconds)?;
    let audio = AudioOptions {
        device: Some(options.device.clone()),
        exact: false,
        sample_rate: options.sample_rate,
        buffer_size: None,
    };
    let capture = Capture::start(&audio, &options.output).map_err(|e| e.to_string())?;
    // Scripts wait for this line before they start playing.
    eprintln!("Capturing \"{}\" at {} Hz", capture.name(), capture.sample_rate());
    wait(length);
    let report = capture.stop().map_err(|e| e.to_string())?;
    eprintln!(
        "Wrote {} frames to {}",
        report.frames - report.dropped,
        options.output.display()
    );
    check(&report)
}

/// The capture's length from `--seconds`: a positive number that a
/// `Duration` can hold.
fn length(seconds: f64) -> Result<Duration, String> {
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err(format!("--seconds must be a positive number, not {seconds}"));
    }
    Duration::try_from_secs_f64(seconds).map_err(|_| format!("--seconds is too large: {seconds}"))
}

/// Sleeps for `length`, in short steps.
fn wait(length: Duration) {
    let started = Instant::now();
    loop {
        let elapsed = started.elapsed();
        if elapsed >= length {
            break;
        }
        std::thread::sleep(length.saturating_sub(elapsed).min(Duration::from_millis(50)));
    }
}

/// Fails a capture that dropped frames or whose stream reported errors:
/// its file is not what the device delivered.
fn check(report: &CaptureReport) -> Result<(), String> {
    if report.dropped > 0 {
        return Err(format!(
            "{} frames were dropped, so the file has jumps the device never played",
            report.dropped
        ));
    }
    if report.stream_errors > 0 {
        return Err(format!(
            "the input stream reported {} errors, so the file may end early or have gaps",
            report.stream_errors
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `--seconds` must be a positive number a `Duration` can hold.
    #[test]
    fn length_takes_positive_numbers_only() {
        assert_eq!(length(1.5), Ok(Duration::from_millis(1500)));
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(length(bad).unwrap_err().contains("positive"), "{bad}");
        }
        assert!(length(1e300).unwrap_err().contains("too large"));
    }

    /// The wait lasts at least the length asked for.
    #[test]
    fn wait_lasts_the_length() {
        let started = Instant::now();
        wait(Duration::from_millis(20));
        assert!(started.elapsed() >= Duration::from_millis(20));
    }

    /// A clean capture passes; dropped frames and stream errors each fail
    /// it, with the count in the message.
    #[test]
    fn check_fails_dropped_frames_and_stream_errors() {
        let clean = CaptureReport {
            frames: 100,
            dropped: 0,
            stream_errors: 0,
        };
        assert_eq!(check(&clean), Ok(()));
        let dropped = CaptureReport { dropped: 3, ..clean };
        assert!(check(&dropped).unwrap_err().contains("3 frames were dropped"));
        let errors = CaptureReport {
            stream_errors: 2,
            ..clean
        };
        assert!(check(&errors).unwrap_err().contains("reported 2 errors"));
    }
}

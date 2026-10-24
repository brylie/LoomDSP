//! `loom capture`: records an input device, such as the input side of a
//! loopback device, to a WAV file (`docs/design/audio-qa.md`, Part 4).
//! `loom analyze` then checks what really reached the device.

use loom_host_desktop::{AudioOptions, Capture};
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

/// Exit status of a broken run: no device, a write error, or dropped
/// frames. `loom analyze` uses 1 for a failed check.
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
    if !options.seconds.is_finite() || options.seconds <= 0.0 {
        return Err(format!("--seconds must be a positive number, not {}", options.seconds));
    }
    let audio = AudioOptions {
        device: Some(options.device.clone()),
        exact: false,
        sample_rate: options.sample_rate,
        buffer_size: None,
    };
    let capture = Capture::start(&audio, &options.output).map_err(|e| e.to_string())?;
    // Scripts wait for this line before they start playing.
    eprintln!("Capturing \"{}\" at {} Hz", capture.name(), capture.sample_rate());
    let started = Instant::now();
    let length = Duration::from_secs_f64(options.seconds);
    while started.elapsed() < length {
        std::thread::sleep((length - started.elapsed()).min(Duration::from_millis(50)));
    }
    let report = capture.stop().map_err(|e| e.to_string())?;
    eprintln!(
        "Wrote {} frames to {}",
        report.frames - report.dropped,
        options.output.display()
    );
    if report.dropped > 0 {
        return Err(format!(
            "{} frames were dropped, so the file has jumps the device never played",
            report.dropped
        ));
    }
    Ok(())
}

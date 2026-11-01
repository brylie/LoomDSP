//! `loom analyze`: measurements on a WAV file, with optional checks
//! (`docs/design/audio-qa.md`, `loom analyze`). It only measures; the limits a
//! script passes decide what fails.

use loom_analysis::{self as analysis, Analysis};
use std::path::PathBuf;
use std::process::ExitCode;

/// The options of `loom analyze`.
#[derive(clap::Args)]
pub struct Options {
    /// WAV file: 16-bit, 24-bit, or 32-bit float.
    file: PathBuf,
    /// Measure from this time, in seconds (for example after a fade-in).
    #[arg(long, value_name = "SECONDS")]
    from: Option<f64>,
    /// Measure up to this time, in seconds.
    #[arg(long, value_name = "SECONDS")]
    to: Option<f64>,
    /// Print JSON instead of text, for scripts and agents.
    #[arg(long)]
    json: bool,
    /// Fail (exit status 1) if any channel's peak is over this.
    #[arg(long, value_name = "X")]
    max_peak: Option<f32>,
    /// Fail if any step between neighbouring samples is over this.
    #[arg(long, value_name = "X")]
    max_step: Option<f32>,
    /// Fail if the DC of any 100 ms window is over this in size.
    #[arg(long, value_name = "X")]
    max_dc: Option<f64>,
}

/// Exit status when the file cannot be read: a broken run, not a failed
/// check.
const UNREADABLE: u8 = 2;

/// Runs `loom analyze`. Exit status 0 when every check passes, 1 when one
/// fails, and 2 when the file cannot be read or an option is not usable.
pub fn run(options: &Options) -> ExitCode {
    if let Err(e) = validate(options) {
        eprintln!("error: {e}");
        return ExitCode::from(UNREADABLE);
    }
    let audio = match analysis::read_wav(&options.file) {
        Ok(audio) => audio,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(UNREADABLE);
        }
    };
    let result = analysis::analyze(&audio, options.from, options.to);
    if options.json {
        match serde_json::to_string_pretty(&result) {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::from(UNREADABLE);
            }
        }
    } else {
        print_text(&result);
    }
    let failures = failures(&result, options);
    for failure in &failures {
        eprintln!("FAIL {failure}");
    }
    if failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Refuses times and limits that would make a check meaningless: NaN or
/// infinite values, and `--from` after `--to`. A NaN limit, for example,
/// would pass every check, because every comparison with NaN is false.
fn validate(options: &Options) -> Result<(), String> {
    let finite = [
        ("--from", options.from),
        ("--to", options.to),
        ("--max-peak", options.max_peak.map(f64::from)),
        ("--max-step", options.max_step.map(f64::from)),
        ("--max-dc", options.max_dc),
    ];
    for (flag, value) in finite {
        if let Some(value) = value
            && !value.is_finite()
        {
            return Err(format!("{flag} must be a finite number, not {value}"));
        }
    }
    if let (Some(from), Some(to)) = (options.from, options.to)
        && from > to
    {
        return Err(format!("--from {from} is after --to {to}"));
    }
    Ok(())
}

/// One line per failed check, naming the check, the channel, and the value.
/// NaN and infinite samples fail every check given, as in the module test
/// harness (module-test-harness.md, rule 7): broken audio must not pass
/// because the other measurements leave those samples out.
fn failures(result: &Analysis, options: &Options) -> Vec<String> {
    let mut out = Vec::new();
    let checks = [
        options.max_peak.map(|x| format!("--max-peak {x}")),
        options.max_step.map(|x| format!("--max-step {x}")),
        options.max_dc.map(|x| format!("--max-dc {x}")),
    ];
    for (i, c) in result.channels.iter().enumerate() {
        if c.non_finite > 0 {
            for check in checks.iter().flatten() {
                out.push(format!(
                    "{check}: channel {} has {} NaN or infinite samples",
                    i + 1,
                    c.non_finite
                ));
            }
        }
        if let Some(limit) = options.max_peak
            && c.peak > limit
        {
            out.push(format!(
                "--max-peak {limit}: channel {} peak {} at {:.4} s",
                i + 1,
                c.peak,
                c.peak_at
            ));
        }
        if let Some(limit) = options.max_step
            && c.max_step > limit
        {
            out.push(format!(
                "--max-step {limit}: channel {} step {} at {:.4} s",
                i + 1,
                c.max_step,
                c.max_step_at
            ));
        }
        if let Some(limit) = options.max_dc
            && c.dc_window.abs() > limit
        {
            out.push(format!("--max-dc {limit}: channel {} DC {}", i + 1, c.dc_window));
        }
    }
    out
}

/// The measurements as text, one block per channel.
fn print_text(result: &Analysis) {
    println!(
        "{} frames ({:.3} s) at {} Hz, from {:.3} s. Ceiling {:.4}.",
        result.frames, result.seconds, result.sample_rate, result.from_seconds, result.ceiling
    );
    for (i, c) in result.channels.iter().enumerate() {
        println!("Channel {}:", i + 1);
        println!("  first sample   {:+.6}", c.first);
        println!(
            "  peak           {:.6} ({:.2} dBFS) at {:.4} s",
            c.peak, c.peak_dbfs, c.peak_at
        );
        println!("  over ceiling   {}", c.over_ceiling);
        println!("  at full scale  {}", c.full_scale);
        println!("  not finite     {}", c.non_finite);
        println!("  largest step   {:.6} at {:.4} s", c.max_step, c.max_step_at);
        println!(
            "  DC             {:+.6} (largest 100 ms window {:+.6})",
            c.dc, c.dc_window
        );
        println!("  RMS            {:.6} ({:.2} dBFS)", c.rms, c.rms_dbfs);
    }
}

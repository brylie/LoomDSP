//! The lifetime and status reporting of the live player.

use super::{AudioArgs, CLEAR_LINE, Live, PatchArgs, Result, format_stats, start_live};
use std::io::{BufRead, IsTerminal};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Starts playback and fades the stream on Enter or a termination signal.
pub(super) fn run(source: &PatchArgs, args: &AudioArgs, note: Option<u8>) -> Result<ExitCode> {
    let stop = stop_on_signal()?;
    let live = start_live(&source.patch, args, note, source.library.as_deref())?;
    let terminal = std::io::stderr().is_terminal();
    let reader = stop.clone();
    std::thread::spawn(move || {
        if stop_line(std::io::stdin().lock()) {
            reader.store(true, Ordering::Relaxed);
        }
    });

    play_until_stop(live, &stop, terminal);
    Ok(ExitCode::SUCCESS)
}

/// Reports live problems and stops the stream after its fade.
fn play_until_stop(mut live: Live, stop: &AtomicBool, terminal: bool) {
    log(format_args!(
        "playing on {} at {} Hz",
        live.audio.device_name(),
        live.audio.sample_rate()
    ));
    if terminal {
        println!("Press Enter or Ctrl+C to stop.");
    }
    let mut counts = (0, 0);
    let mut progress = (0, Instant::now());
    let mut stalled = false;
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(250));
        live.controller.collect_garbage();
        let stats = live.stats.snapshot();
        let now = Instant::now();
        if stats.callbacks != progress.0 {
            progress = (stats.callbacks, now);
        }
        let waiting = now.duration_since(progress.1) >= Duration::from_secs(2);
        if waiting != stalled {
            log(if waiting {
                "audio callbacks stopped"
            } else {
                "audio callbacks resumed"
            });
            stalled = waiting;
        }
        if counts != (stats.overruns, stats.stream_errors) {
            log(format_args!(
                "audio overruns: {}, stream errors: {}",
                stats.overruns, stats.stream_errors
            ));
            counts = (stats.overruns, stats.stream_errors);
        }
        if terminal {
            eprint!("{CLEAR_LINE}{}", format_stats(&stats));
        }
    }
    if terminal {
        eprintln!();
    }
    let fade = live.controller.fade_out();
    let silent = live.audio.stop(loom_host_desktop::STOP_LIMIT);
    if let Err(error) = fade {
        log(format_args!("could not request the stop fade: {error}"));
    } else if !silent {
        log("audio stopped before the fade completed");
    }
    log("playback stopped");
}

/// Installs a process-wide stop flag for Ctrl+C and Unix termination.
fn stop_on_signal() -> Result<Arc<AtomicBool>> {
    let stop = Arc::new(AtomicBool::new(false));
    let signal = stop.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
    Ok(stop)
}

/// True for a line from a pipe or terminal. End of input keeps playback going.
fn stop_line(mut input: impl BufRead) -> bool {
    let mut line = Vec::new();
    input.read_until(b'\n', &mut line).is_ok() && line.ends_with(b"\n")
}

/// Log entries are readable when stderr is redirected to a run log.
fn log(message: impl std::fmt::Display) {
    eprintln!("{} {message}", jiff::Timestamp::now());
}

#[cfg(test)]
mod tests;

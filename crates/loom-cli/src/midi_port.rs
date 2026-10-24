//! `loom midi-port`: a virtual MIDI controller played from standard input
//! (`docs/design/audio-qa.md`, Part 4). A person types the lines in a
//! terminal, or a script pipes them in.

use loom_host_desktop::{HostError, VirtualController};
use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::time::Duration;

/// The options of `loom midi-port`.
#[derive(clap::Args)]
pub struct Options {
    /// Name of the virtual MIDI port. The app lists it as a MIDI input.
    name: String,
    /// MIDI channel, 1 to 16.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=16))]
    channel: u8,
}

/// Exit status when the port cannot be created: virtual ports need macOS
/// or Linux with the ALSA sequencer.
const NO_PORT: u8 = 2;

/// Velocity of a note on with none given.
const DEFAULT_VELOCITY: u8 = 100;

/// One line of input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    On { note: u8, velocity: u8 },
    Off { note: u8 },
    Wait(Duration),
}

/// What plays the steps: the virtual port, or a fake in tests.
trait Keys {
    fn note_on(&mut self, channel: u8, note: u8, velocity: u8) -> Result<(), HostError>;
    fn note_off(&mut self, channel: u8, note: u8) -> Result<(), HostError>;
    fn release_all(&mut self) -> Result<(), HostError>;
}

impl Keys for VirtualController {
    fn note_on(&mut self, channel: u8, note: u8, velocity: u8) -> Result<(), HostError> {
        VirtualController::note_on(self, channel, note, velocity)
    }

    fn note_off(&mut self, channel: u8, note: u8) -> Result<(), HostError> {
        VirtualController::note_off(self, channel, note)
    }

    fn release_all(&mut self) -> Result<(), HostError> {
        VirtualController::release_all(self)
    }
}

/// Runs `loom midi-port`. Exit status 0 at the end of input, 1 if sending
/// or reading the input fails, and 2 when the port cannot be created.
pub fn run(options: &Options) -> ExitCode {
    let mut port = match VirtualController::create(&options.name) {
        Ok(port) => port,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(NO_PORT);
        }
    };
    match port.input_name() {
        Ok(name) => println!("MIDI input: {name}"),
        Err(e) => eprintln!("warning: {e}"),
    }
    let _ = std::io::stdout().flush();
    let stdin = std::io::stdin();
    match play(stdin.lock(), &mut port, options.channel - 1, &mut std::io::stderr()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Plays every line of `input` on `keys`, on `channel` (0 to 15), then
/// releases the notes still held. A line that cannot be read, as a step or
/// as text, is reported on `errors` with its line number, and skipped. An
/// error reading the input itself stops the run, after the release, so a
/// script does not pass with steps left unplayed.
fn play(input: impl BufRead, keys: &mut impl Keys, channel: u8, errors: &mut impl Write) -> Result<(), String> {
    let mut failed = None;
    for (index, line) in input.lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                let _ = writeln!(errors, "line {}: {e}", index + 1);
                continue;
            }
            Err(e) => {
                failed = Some(format!("reading line {}: {e}", index + 1));
                break;
            }
        };
        let sent = match parse(&line) {
            Ok(None) => Ok(()),
            Ok(Some(Step::On { note, velocity })) => keys.note_on(channel, note, velocity),
            Ok(Some(Step::Off { note })) => keys.note_off(channel, note),
            Ok(Some(Step::Wait(time))) => {
                std::thread::sleep(time);
                Ok(())
            }
            Err(e) => {
                let _ = writeln!(errors, "line {}: {e}", index + 1);
                Ok(())
            }
        };
        sent.map_err(|e| e.to_string())?;
    }
    keys.release_all().map_err(|e| e.to_string())?;
    failed.map_or(Ok(()), Err)
}

/// Reads one line: `on NOTE [VELOCITY]`, `off NOTE`, `wait MS`, or nothing
/// (empty, or a `#` comment).
fn parse(line: &str) -> Result<Option<Step>, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let words: Vec<&str> = line.split_whitespace().collect();
    let step = match words.as_slice() {
        ["on", note] => Step::On {
            note: number(note, "note", 0, 127)?,
            velocity: DEFAULT_VELOCITY,
        },
        ["on", note, velocity] => Step::On {
            note: number(note, "note", 0, 127)?,
            velocity: number(velocity, "velocity", 1, 127)?,
        },
        ["off", note] => Step::Off {
            note: number(note, "note", 0, 127)?,
        },
        ["wait", ms] => Step::Wait(Duration::from_millis(
            ms.parse()
                .map_err(|_| format!("`{ms}` is not a number of milliseconds"))?,
        )),
        _ => {
            return Err(format!(
                "cannot read `{line}`: use `on NOTE [VELOCITY]`, `off NOTE`, or `wait MS`"
            ));
        }
    };
    Ok(Some(step))
}

/// A whole number from `min` to `max`.
fn number(word: &str, what: &str, min: u8, max: u8) -> Result<u8, String> {
    word.parse()
        .ok()
        .filter(|n| (min..=max).contains(n))
        .ok_or_else(|| format!("{what} `{word}` is not a whole number from {min} to {max}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keys that write down what they were asked to play.
    #[derive(Default)]
    struct Log(Vec<String>);

    impl Keys for Log {
        fn note_on(&mut self, channel: u8, note: u8, velocity: u8) -> Result<(), HostError> {
            self.0.push(format!("on {channel} {note} {velocity}"));
            Ok(())
        }

        fn note_off(&mut self, channel: u8, note: u8) -> Result<(), HostError> {
            self.0.push(format!("off {channel} {note}"));
            Ok(())
        }

        fn release_all(&mut self) -> Result<(), HostError> {
            self.0.push("release".into());
            Ok(())
        }
    }

    /// Every line in the table of the spec is read.
    #[test]
    fn the_lines_are_read() {
        assert_eq!(
            parse("on 60"),
            Ok(Some(Step::On {
                note: 60,
                velocity: 100
            }))
        );
        assert_eq!(parse("  on 0 127 "), Ok(Some(Step::On { note: 0, velocity: 127 })));
        assert_eq!(parse("off 127"), Ok(Some(Step::Off { note: 127 })));
        assert_eq!(parse("wait 250"), Ok(Some(Step::Wait(Duration::from_millis(250)))));
        assert_eq!(parse(""), Ok(None));
        assert_eq!(parse("# hold middle C"), Ok(None));
    }

    /// Notes and velocities out of range, and unknown words, are errors.
    #[test]
    fn bad_lines_are_errors() {
        for line in [
            "on 128",
            "on 60 0",
            "on 60 128",
            "off",
            "off -1",
            "wait soon",
            "play 60",
            "on 60 100 1",
        ] {
            assert!(parse(line).is_err(), "{line}");
        }
    }

    /// A bad line is reported with its number and skipped, and the notes
    /// still held are released at the end of input.
    #[test]
    fn bad_lines_are_skipped_and_notes_released_at_the_end() {
        let mut keys = Log::default();
        let mut errors = Vec::new();
        play(
            "on 60\nplay 61\noff 60\non 62 90\n".as_bytes(),
            &mut keys,
            3,
            &mut errors,
        )
        .unwrap();
        assert_eq!(keys.0, ["on 3 60 100", "off 3 60", "on 3 62 90", "release"]);
        let errors = String::from_utf8(errors).unwrap();
        assert!(errors.starts_with("line 2: "), "{errors}");
    }

    /// A line that is not UTF-8 text is reported and skipped, not taken as
    /// the end of input.
    #[test]
    fn a_line_that_is_not_text_is_skipped() {
        let mut keys = Log::default();
        let mut errors = Vec::new();
        play(&b"on 60\n\xFF\noff 60\n"[..], &mut keys, 0, &mut errors).unwrap();
        assert_eq!(keys.0, ["on 0 60 100", "off 0 60", "release"]);
        let errors = String::from_utf8(errors).unwrap();
        assert!(errors.starts_with("line 2: "), "{errors}");
    }
}

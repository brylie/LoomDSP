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

use super::*;

/// A readout writes a value by its display range: a number with its unit, a
/// note name with cents, or a choice name. It never fails on a bad value.
#[test]
fn a_readout_writes_its_value_by_format() {
    let number = DisplayRange {
        min: -50.0,
        max: 50.0,
        unit: "cents",
        format: DisplayFormat::Number { decimals: 1 },
    };
    assert_eq!(number.text(12.54), "12.5 cents");
    // Around silence the text does not flip between "-0.0" and "0.0".
    assert_eq!(number.text(-0.04), "0.0 cents");
    assert_eq!(number.text(-0.0), "0.0 cents");
    assert_eq!(number.text(-0.06), "-0.1 cents");
    let plain = DisplayRange {
        unit: "",
        format: DisplayFormat::Number { decimals: 0 },
        ..number
    };
    assert_eq!(plain.text(7.0), "7");

    let note = DisplayRange {
        format: DisplayFormat::Note,
        ..plain
    };
    assert_eq!(note.text(0.0), "C4", "0.0 is middle C");
    assert_eq!(note.text(0.075), "A4", "9 semitones at 0.1 per octave");
    assert_eq!(note.text(-0.1), "C3", "an octave down");
    assert_eq!(note.text(1.0 / 1200.0), "C4 +10¢");
    assert_eq!(note.text(-1.0 / 1200.0), "C4 -10¢");
    assert_eq!(note.text(0.1 / 12.0), "C#4");

    let choices = DisplayRange {
        min: 1.0,
        format: DisplayFormat::Choices(&["One", "Two"]),
        ..plain
    };
    assert_eq!(choices.text(1.0), "One");
    assert_eq!(choices.text(2.0), "Two");
    assert_eq!(choices.text(99.0), "Two", "past the end names the last");
    assert_eq!(choices.text(-99.0), "One");

    for range in [number, note, choices] {
        assert_eq!(range.text(f32::NAN), "--");
        assert_eq!(range.text(f32::INFINITY), "--");
    }
}

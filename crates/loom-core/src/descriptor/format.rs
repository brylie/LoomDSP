//! Text for numbers, notes, and gains, shared by parameters and displays.

/// Names joined as a sentence lists them: "A or B", "A, B, or C".
pub(super) fn list_text(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_string(),
        [first, second] => format!("{first} or {second}"),
        [rest @ .., last] => format!("{}, or {last}", rest.join(", ")),
    }
}

/// A number as help text shows it: no trailing zeros, and 0 for -0, which
/// reads as 0 to people.
pub fn number_text(value: f32) -> String {
    if value == 0.0 {
        "0".to_string()
    } else {
        value.to_string()
    }
}

/// A gain in decibels. A gain of 0 gives negative infinity.
pub(super) fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.log10()
}

/// The gain of a value in decibels.
pub(super) fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// A gain as a player reads it: "−6.0 dB", "0.0 dB", "+6.0 dB", or "−∞ dB".
/// It uses the true minus sign, and [`ParamSpec::parse_value_text`] reads
/// everything it writes.
pub(super) fn decibel_text(gain: f32) -> String {
    if gain <= 0.0 {
        return "−∞ dB".to_string();
    }
    let db = (gain_to_db(gain) * 10.0).round() / 10.0;
    if db > 0.0 {
        format!("+{db:.1} dB")
    } else if db < 0.0 {
        format!("−{:.1} dB", -db)
    } else {
        // Rounding can leave -0.0, and "−0.0 dB" means nothing to a player.
        "0.0 dB".to_string()
    }
}

/// A typed gain in decibels: what [`decibel_text`] writes, an ASCII minus,
/// or "-inf". A plain number is in dB too.
pub(super) fn parse_decibels(text: &str) -> Option<f32> {
    let text = text.trim().replace(',', ".").replace('−', "-").to_lowercase();
    let number = text.strip_suffix("db").unwrap_or(&text).trim_end();
    if matches!(number, "-inf" | "-∞") {
        return Some(0.0);
    }
    // Rust reads "NaN" and "inf" as numbers. A player never means them.
    number.parse::<f32>().ok().filter(|db| db.is_finite()).map(db_to_gain)
}

/// A pitch as a note name and cents. Pitch is 0.1 per octave, with 0.0 as
/// middle C (ADR-0007), which is C4, or note 60 in MIDI.
pub(super) fn note_text(pitch: f32) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let semitones = 60.0 + f64::from(pitch) * 120.0;
    let nearest = semitones.round();
    let cents = ((semitones - nearest) * 100.0).round() as i32;
    let note = nearest as i64;
    let name = NAMES[note.rem_euclid(12) as usize];
    let octave = note.div_euclid(12) - 1;
    if cents == 0 {
        format!("{name}{octave}")
    } else {
        format!("{name}{octave} {cents:+}¢")
    }
}

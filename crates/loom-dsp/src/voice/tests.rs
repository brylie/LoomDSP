//! Allocation rules, checked without a graph or a MIDI source.

use super::*;

/// A source note whose identity and pitch are easy to distinguish.
fn note(id: u64) -> Note {
    Note {
        id: NoteId(id),
        pitch: id as f32,
        velocity: 0.8,
        pressure: 0.2,
        timbre: 0.3,
    }
}

/// Starts a new sample with no connected level input.
fn frame(allocator: &mut Allocator) {
    allocator.begin_frame(None);
}

/// Free voices are used in order, and a ninth note retriggers the oldest.
#[test]
fn oldest_stealing_has_exactly_one_low_sample() {
    let mut a = Allocator::new(8);
    for id in 0..8 {
        frame(&mut a);
        a.note_on(note(id));
        assert_eq!(a.voices_for(NoteId(id)).collect::<Vec<_>>(), [id as usize]);
    }
    frame(&mut a);
    a.note_on(note(8));
    assert!(!a.voices()[0].gate);
    assert_eq!(a.voices()[0].note, Some(note(8)));
    assert!(a.voices()[1..].iter().all(|voice| voice.gate));
    frame(&mut a);
    assert!(a.voices()[0].gate);
}

/// Released voices retain values and the one released first is reused first.
#[test]
fn free_longest_wins_and_release_values_hold() {
    let mut a = Allocator::new(2);
    frame(&mut a);
    a.note_on(note(1));
    a.note_on(note(2));
    frame(&mut a);
    a.note_off(NoteId(2));
    a.note_off(NoteId(1));
    assert!(!a.voices()[0].gate);
    assert_eq!(a.voices()[0].note, Some(note(1)));
    a.update_note(Note { pitch: 99.0, ..note(1) });
    assert_eq!(a.voices()[0].note, Some(note(1)));
    frame(&mut a);
    a.note_on(note(3));
    assert_eq!(a.voices_for(NoteId(3)).collect::<Vec<_>>(), [1]);
    assert!(a.voices()[1].gate);
}

/// Quietest reads current levels; no cable falls back to oldest, as do ties.
#[test]
fn quietest_uses_levels_or_oldest() {
    for levels in [
        None,
        Some([0.9, 0.1]),
        Some([0.2, 0.2]),
        Some([f32::NAN, f32::INFINITY]),
    ] {
        let mut a = Allocator::new(2);
        a.configure(2, Mode::Quietest, 1);
        frame(&mut a);
        a.note_on(note(1));
        a.note_on(note(2));
        a.begin_frame(levels.as_ref().map(|levels| &levels[..]));
        a.note_on(note(3));
        let expected = usize::from(levels == Some([0.9, 0.1]));
        assert_eq!(a.voices_for(NoteId(3)).collect::<Vec<_>>(), [expected]);
    }
}

/// Last-note priority keeps the original mono velocity on a return to a held key.
#[test]
fn legato_returns_without_retriggering() {
    let mut a = Allocator::new(1);
    frame(&mut a);
    a.note_on(note(1));
    frame(&mut a);
    a.note_on(Note {
        velocity: 0.4,
        ..note(2)
    });
    assert!(a.voices()[0].gate);
    frame(&mut a);
    a.note_off(NoteId(2));
    assert!(a.voices()[0].gate);
    assert_eq!(a.voices()[0].note.unwrap().pitch, 1.0);
    assert_eq!(a.voices()[0].note.unwrap().velocity, 0.4);
    a.note_off(NoteId(99));
    assert!(a.voices()[0].gate);
    a.note_off(NoteId(1));
    assert!(!a.voices()[0].gate);
}

/// Only note-ons retrigger in mono Retrigger, not returns caused by note-offs.
#[test]
fn retrigger_returns_without_another_edge() {
    let mut a = Allocator::new(1);
    a.configure(1, Mode::Retrigger, 1);
    frame(&mut a);
    a.note_on(note(1));
    assert!(a.voices()[0].gate);
    frame(&mut a);
    a.note_on(note(2));
    assert!(!a.voices()[0].gate);
    frame(&mut a);
    assert!(a.voices()[0].gate);
    a.note_off(NoteId(2));
    assert!(a.voices()[0].gate);
}

/// A repeated held identity stays on its own voice, with a new gate edge.
#[test]
fn repeated_identity_retriggers_its_own_voice() {
    let mut a = Allocator::new(3);
    frame(&mut a);
    a.note_on(note(1));
    a.note_on(note(2));
    frame(&mut a);
    a.note_on(Note {
        velocity: 0.5,
        ..note(1)
    });
    assert_eq!(a.voices_for(NoteId(1)).collect::<Vec<_>>(), [0]);
    assert!(!a.voices()[0].gate);
    assert!(a.voices()[1].gate);
    assert!(!a.voices()[2].held);
    frame(&mut a);
    assert!(a.voices()[0].gate);
}

/// Fresh on/off gives a pulse; off/on gives a retrigger even with unused voices.
#[test]
fn same_frame_event_order_controls_the_edge() {
    for capacity in [1, 3] {
        let mut a = Allocator::new(capacity);
        frame(&mut a);
        a.note_on(note(1));
        a.note_off(NoteId(1));
        assert!(a.voices()[0].gate);
        assert!(!a.voices()[0].held);
        frame(&mut a);
        assert!(!a.voices()[0].gate);
        a.note_on(note(1));
        frame(&mut a);
        let i = a.voices_for(NoteId(1)).next().unwrap();
        a.note_off(NoteId(1));
        a.note_on(note(1));
        assert!(!a.voices()[i].gate);
        assert_eq!(a.voices_for(NoteId(1)).collect::<Vec<_>>(), [i]);
        frame(&mut a);
        assert!(a.voices()[i].gate);
    }
}

/// Same-pitch notes on different caller identities remain independent.
#[test]
fn identity_is_separate_from_pitch() {
    let mut a = Allocator::new(2);
    frame(&mut a);
    a.note_on(note(1));
    a.note_on(Note { pitch: 1.0, ..note(2) });
    frame(&mut a);
    a.note_off(NoteId(1));
    assert!(!a.voices()[0].gate);
    assert!(a.voices()[1].gate);
}

/// Continuous source expression follows every voice of a unison group.
#[test]
fn unison_expression_and_release_follow_the_note() {
    let mut a = Allocator::new(4);
    a.configure(4, Mode::Oldest, 2);
    frame(&mut a);
    a.note_on(note(1));
    let changed = Note {
        pressure: 0.9,
        pitch: 1.2,
        ..note(1)
    };
    a.update_note(changed);
    assert_eq!(a.voices_for(NoteId(1)).collect::<Vec<_>>(), [0, 1]);
    assert!(a.voices()[..2].iter().all(|voice| voice.note == Some(changed)));
    frame(&mut a);
    a.note_off(NoteId(1));
    a.update_note(note(1));
    assert!(
        a.voices()[..2]
            .iter()
            .all(|voice| !voice.gate && voice.note == Some(changed))
    );
}

/// A quiet group's loudest voice decides its level; stealing releases whole groups.
#[test]
fn quietest_steals_whole_groups_and_keeps_remainders_silent() {
    let mut a = Allocator::new(5);
    a.configure(5, Mode::Quietest, 2);
    frame(&mut a);
    a.note_on(note(1));
    a.note_on(note(2));
    a.begin_frame(Some(&[0.1, 0.9, 0.3, 0.3, 0.0]));
    a.note_on(note(3));
    assert_eq!(a.voices_for(NoteId(1)).count(), 2);
    assert_eq!(a.voices_for(NoteId(2)).count(), 0);
    assert_eq!(a.voices_for(NoteId(3)).count(), 2);
    let remainder = a.voices().iter().find(|voice| !voice.held).unwrap();
    assert!(!remainder.gate);
}

/// Width changes preserve surviving notes and reset removed or added voices.
#[test]
fn capacity_changes_forget_removed_notes() {
    let mut a = Allocator::new(4);
    frame(&mut a);
    for id in 1..=4 {
        a.note_on(note(id));
    }
    a.configure(2, Mode::Oldest, 1);
    assert_eq!(a.voices_for(NoteId(1)).count(), 1);
    assert_eq!(a.voices_for(NoteId(3)).count(), 0);
    a.configure(4, Mode::Oldest, 1);
    assert_eq!(a.voices()[2..], [Voice::default(); 2]);
    a.configure(1, Mode::Oldest, 99);
    frame(&mut a);
    a.note_on(note(5));
    assert!(a.voices()[0].gate, "Oldest at one voice resolves to Legato");
    a.note_off(NoteId(5));
    assert_eq!(a.voices()[0].note.unwrap().id, NoteId(1));
    a.configure(0, Mode::Legato, 0);
    assert_eq!(a.voices().len(), 1);
    a.configure(99, Mode::Legato, 1);
    assert_eq!(a.voices().len(), MAX_VOICES);
    assert_eq!(Mode::Quietest.for_capacity(1), Mode::Legato);
    assert_eq!(Mode::Retrigger.for_capacity(2), Mode::Oldest);
}

/// A bounded mono history evicts the oldest key and handles re-presses once.
#[test]
fn mono_history_is_bounded_and_unique() {
    let mut a = Allocator::new(1);
    for id in 0..17 {
        frame(&mut a);
        a.note_on(note(id));
    }
    assert_eq!(a.held_len, MAX_VOICES);
    a.note_off(NoteId(0));
    a.note_on(note(16));
    assert_eq!(a.held_len, MAX_VOICES);
    for id in (1..=16).rev() {
        frame(&mut a);
        a.note_off(NoteId(id));
    }
    assert!(!a.voices()[0].gate);
}

/// Reset clears notes, release tails, ordering, and pending edges, retaining policy.
#[test]
fn reset_and_cloned_state_are_deterministic() {
    let mut a = Allocator::new(2);
    frame(&mut a);
    a.note_on(note(1));
    a.note_on(note(2));
    a.note_on(note(3));
    let mut b = a.clone();
    frame(&mut a);
    frame(&mut b);
    assert_eq!(a.voices(), b.voices());
    a.reset();
    assert_eq!(a.voices(), [Voice::default(); 2]);
    frame(&mut a);
    a.note_on(note(1));
    assert!(a.voices()[0].gate);
}

/// A chord source sends only changed notes, so shared pitches never lose their gates.
#[test]
fn chord_change_keeps_shared_pitches() {
    let mut a = Allocator::new(4);
    frame(&mut a);
    for id in [60, 64, 67] {
        a.note_on(note(id));
    }
    frame(&mut a);
    a.note_on(note(69));
    assert!(a.voices()[..3].iter().all(|voice| voice.gate));
    assert_eq!(a.voices_for(NoteId(69)).collect::<Vec<_>>(), [3]);
    frame(&mut a);
    a.note_off(NoteId(64));
    assert!(a.voices()[0].gate && a.voices()[2].gate);
}

/// A script shaped like a note cycle lands on identical samples at every block size.
fn cycle(block: usize) -> Vec<Vec<Voice>> {
    let mut a = Allocator::new(2);
    let mut rendered = Vec::new();
    for base in (0..1024).step_by(block) {
        for i in base..(base + block).min(1024) {
            frame(&mut a);
            match i {
                0 => {
                    a.note_on(note(1));
                    a.note_on(note(2));
                }
                76 => a.note_on(note(3)),
                77 => a.note_off(NoteId(2)),
                511 => {
                    a.note_on(note(4));
                    a.note_off(NoteId(4));
                }
                512 => {
                    a.note_off(NoteId(3));
                    a.note_on(note(3));
                }
                _ => {}
            }
            rendered.push(a.voices().to_vec());
        }
    }
    rendered
}

/// Allocation and gate edges do not depend on the caller's buffer boundaries.
#[test]
fn scripted_cycle_is_block_invariant_and_repeatable() {
    let expected = cycle(1);
    assert_eq!(cycle(77), expected);
    assert_eq!(cycle(512), expected);
    assert_eq!(cycle(1), expected);
    assert!(!expected[76][0].gate);
    assert!(expected[77][0].gate);
}

//! Fixed-capacity note allocation, independent of MIDI and the graph.
//!
//! Call [`Allocator::begin_frame`] once per sample, apply that sample's events
//! in arrival order, then read [`Allocator::voices`]. A source can use
//! [`Allocator::voices_for`] to keep expression following its input signals.
//! Spec: `docs/design/polyphony.md`, rules 18 to 27. Background:
//! `docs/learn/voices-and-polyphony.md`.

/// Maximum number of voices, and of remembered held mono notes.
pub const MAX_VOICES: usize = 16;

/// A caller's note identity. The allocator gives this value no MIDI meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteId(pub u64);

/// The values a source associates with a note. Pitch uses the source's units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    /// Identity used for note-offs, expression, and repeated note-ons.
    pub id: NoteId,
    /// Pitch, including any expression the source applies.
    pub pitch: f32,
    /// Note-on strength.
    pub velocity: f32,
    /// Pressure expression.
    pub pressure: f32,
    /// Timbre expression.
    pub timbre: f32,
}

/// Stealing above one voice, or gate articulation at one voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Steal the note that started first.
    #[default]
    Oldest,
    /// Steal the quietest note, falling back to Oldest without levels.
    Quietest,
    /// Keep the mono gate high while any remembered note is held.
    Legato,
    /// Lower the mono gate for one sample on a new note-on.
    Retrigger,
}

impl Mode {
    /// Resolves a mode for the current capacity without changing its stored value.
    #[inline]
    pub fn for_capacity(self, capacity: usize) -> Self {
        match (capacity <= 1, self) {
            (true, Self::Oldest | Self::Quietest) => Self::Legato,
            (false, Self::Legato | Self::Retrigger) => Self::Oldest,
            _ => self,
        }
    }
}

/// One voice's current output and retained release-tail values.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Voice {
    /// Last assigned note, retained after its gate falls. `None` before first use.
    pub note: Option<Note>,
    /// Whether the source still holds this note, including a retrigger's low sample.
    pub held: bool,
    /// Gate value for the current sample.
    pub gate: bool,
    started: u64,
    freed: u64,
    released_frame: u64,
    on_frame: u64,
    /// Last frame's emitted gate, before this frame's pending edges apply.
    was_high: bool,
    /// Both flags together mean a released note's delayed one-sample pulse.
    rise_next: bool,
    fall_next: bool,
}

/// A bounded allocator. Construction, edits, and processing allocate no memory.
#[derive(Debug, Clone)]
pub struct Allocator {
    voices: [Voice; MAX_VOICES],
    capacity: usize,
    mode: Mode,
    unison: usize,
    held: [Option<Note>; MAX_VOICES],
    held_len: usize,
    frame: u64,
    serial: u64,
    levels: Option<[f32; MAX_VOICES]>,
}

impl Default for Allocator {
    #[inline]
    fn default() -> Self {
        Self::new(1)
    }
}

impl Allocator {
    /// Starts silent. Capacity is clamped to 1 through 16.
    #[inline]
    pub fn new(capacity: usize) -> Self {
        Self {
            voices: [Voice::default(); MAX_VOICES],
            capacity: capacity.clamp(1, MAX_VOICES),
            mode: Mode::Legato,
            unison: 1,
            held: [None; MAX_VOICES],
            held_len: 0,
            frame: 0,
            serial: 0,
            levels: None,
        }
    }

    /// Updates capacity and policy. Removed voices and their notes are forgotten.
    /// New voices start silent. Unison is clamped to 1 through capacity.
    #[inline]
    pub fn configure(&mut self, capacity: usize, mode: Mode, unison: usize) {
        let capacity = capacity.clamp(1, MAX_VOICES);
        if capacity != self.capacity {
            let first = capacity.min(self.capacity);
            self.voices[first..].fill(Voice::default());
            self.held.fill(None);
            self.held_len = 0;
            if capacity == 1 && self.voices[0].held {
                self.held[0] = self.voices[0].note;
                self.held_len = 1;
            }
        }
        self.capacity = capacity;
        self.mode = mode;
        self.unison = unison.clamp(1, capacity);
    }

    /// Starts the next sample and completes pending gate edges. `levels` is
    /// absent for an unconnected level input; shorter slices are padded with 0.
    #[inline]
    pub fn begin_frame(&mut self, levels: Option<&[f32]>) {
        self.frame = self.frame.saturating_add(1);
        for voice in &mut self.voices[..self.capacity] {
            voice.was_high = voice.gate;
            if voice.rise_next && voice.fall_next {
                // A retriggered note that ended on its own frame still gets
                // a high sample now, followed by its fall on the next frame.
                voice.gate = true;
                voice.rise_next = false;
            } else {
                if voice.fall_next {
                    voice.gate = false;
                } else if voice.rise_next && voice.held {
                    voice.gate = true;
                }
                voice.rise_next = false;
                voice.fall_next = false;
            }
        }
        self.levels = levels.map(|levels| {
            let mut result = [0.0; MAX_VOICES];
            for (out, &value) in result.iter_mut().zip(levels) {
                // Non-finite levels cannot poison a stealing comparison.
                *out = if value.is_finite() { value } else { 0.0 };
            }
            result
        });
    }

    /// Current outputs, in voice order. A caller reads them after this frame's events.
    #[inline]
    pub fn voices(&self) -> &[Voice] {
        &self.voices[..self.capacity]
    }

    /// Zero-based indices of every held voice belonging to `id`.
    #[inline]
    pub fn voices_for(&self, id: NoteId) -> impl Iterator<Item = usize> + '_ {
        self.voices()
            .iter()
            .enumerate()
            .filter_map(move |(i, voice)| (voice.held && voice.note.is_some_and(|note| note.id == id)).then_some(i))
    }

    /// Updates a held note's values without changing its gate or allocation order.
    /// Released voices keep their values, even if the same input is reused.
    #[inline]
    pub fn update_note(&mut self, note: Note) {
        for held in self.held[..self.held_len].iter_mut().flatten() {
            if held.id == note.id {
                *held = note;
            }
        }
        for voice in &mut self.voices[..self.capacity] {
            if voice.held && voice.note.is_some_and(|held| held.id == note.id) {
                voice.note = Some(note);
            }
        }
    }

    /// Starts a note, or retriggers its held voices. In mono Legato, note-ons
    /// instead keep a high gate and move the pitch (last-note priority).
    #[inline]
    pub fn note_on(&mut self, note: Note) {
        self.serial = self.serial.saturating_add(1);
        if self.capacity == 1 {
            self.press_mono(note);
            return;
        }
        let mut found = false;
        for voice in &mut self.voices[..self.capacity] {
            if (voice.held || voice.released_frame == self.frame && self.frame != 0)
                && voice.note.is_some_and(|held| held.id == note.id)
            {
                Self::start(voice, note, self.frame, self.serial, true);
                found = true;
            }
        }
        if found {
            return;
        }
        let mut stolen = [false; MAX_VOICES];
        while self.voices().iter().filter(|voice| !voice.held).count() < self.unison {
            let victim = self.victim();
            let id = self.voices[victim].note.map(|held| held.id);
            for (i, voice) in self.voices[..self.capacity].iter_mut().enumerate() {
                if voice.held && voice.note.map(|held| held.id) == id {
                    voice.held = false;
                    voice.freed = self.serial;
                    stolen[i] = true;
                }
            }
        }
        for _ in 0..self.unison {
            if let Some(i) = (0..self.capacity)
                .filter(|&i| !self.voices[i].held)
                .min_by_key(|&i| (self.voices[i].freed, i))
            {
                let voice = &self.voices[i];
                let retrigger =
                    stolen[i] || voice.gate || voice.was_high || voice.released_frame == self.frame && self.frame != 0;
                Self::start(&mut self.voices[i], note, self.frame, self.serial, retrigger);
            }
        }
        // Whole-group stealing can leave an unused remainder. Its gate must fall.
        for (i, voice) in self.voices[..self.capacity].iter_mut().enumerate() {
            if stolen[i] && !voice.held {
                voice.gate = false;
                voice.rise_next = false;
                voice.fall_next = false;
            }
        }
    }

    /// Releases all voices of a note. A fresh note-on followed by its note-off
    /// on this sample keeps a one-sample gate. A retrigger/off on one frame
    /// keeps its low sample, then emits the pulse on the next frame.
    #[inline]
    pub fn note_off(&mut self, id: NoteId) {
        self.serial = self.serial.saturating_add(1);
        if self.capacity == 1 {
            self.release_mono(id);
            return;
        }
        for voice in &mut self.voices[..self.capacity] {
            if voice.held && voice.note.is_some_and(|note| note.id == id) {
                Self::release(voice, self.frame, self.serial);
            }
        }
    }

    /// Clears every note and pending edge, keeping capacity and policy.
    #[inline]
    pub fn reset(&mut self) {
        self.voices.fill(Voice::default());
        self.held.fill(None);
        self.held_len = 0;
        self.frame = 0;
        self.serial = 0;
        self.levels = None;
    }

    /// Changes a voice's note and schedules any retrigger edge.
    fn start(voice: &mut Voice, note: Note, frame: u64, serial: u64, retrigger: bool) {
        *voice = Voice {
            note: Some(note),
            held: true,
            gate: !retrigger,
            started: serial,
            on_frame: frame,
            released_frame: 0,
            rise_next: retrigger,
            ..*voice
        };
        voice.fall_next = false;
    }

    /// Releases a voice, retaining either a fresh or a delayed one-sample pulse.
    fn release(voice: &mut Voice, frame: u64, serial: u64) {
        voice.held = false;
        voice.freed = serial;
        voice.released_frame = frame;
        if voice.rise_next && voice.on_frame == frame {
            // Keep the pending rise, then fall one frame after that rise.
            voice.fall_next = true;
        } else {
            voice.fall_next = voice.gate && voice.on_frame == frame;
            voice.gate = voice.fall_next;
            voice.rise_next = false;
        }
    }

    /// Removes one remembered mono note, returning whether it was held.
    fn forget(&mut self, id: NoteId) -> bool {
        let Some(i) = self.held[..self.held_len]
            .iter()
            .position(|held| held.is_some_and(|note| note.id == id))
        else {
            return false;
        };
        self.held.copy_within(i + 1..self.held_len, i);
        self.held_len -= 1;
        self.held[self.held_len] = None;
        true
    }

    /// Last-note priority with a bounded history, as the original mono input used.
    fn press_mono(&mut self, note: Note) {
        self.forget(note.id);
        if self.held_len == MAX_VOICES {
            self.held.copy_within(1.., 0);
            self.held_len -= 1;
        }
        self.held[self.held_len] = Some(note);
        self.held_len += 1;
        let retrigger = self.voices[0].held && self.mode.for_capacity(1) == Mode::Retrigger
            || self.voices[0].rise_next
            || !self.voices[0].held
                && (self.voices[0].gate
                    || self.voices[0].was_high
                    || self.voices[0].released_frame == self.frame && self.frame != 0);
        Self::start(&mut self.voices[0], note, self.frame, self.serial, retrigger);
    }

    /// Returning to a held key is a note-off, so it does not restart the gate.
    fn release_mono(&mut self, id: NoteId) {
        if !self.forget(id) {
            return;
        }
        if self.held_len == 0 {
            Self::release(&mut self.voices[0], self.frame, self.serial);
        } else if self.voices[0].note.is_some_and(|note| note.id == id) {
            let velocity = self.voices[0].note.map_or(0.0, |note| note.velocity);
            self.voices[0].note = self.held[self.held_len - 1].map(|note| Note { velocity, ..note });
        }
    }

    /// Selects a held note group. Maximum member level protects its loudest side.
    fn victim(&self) -> usize {
        let quietest = self.mode.for_capacity(self.capacity) == Mode::Quietest;
        (0..self.capacity)
            .filter(|&i| self.voices[i].held)
            .min_by(|&a, &b| {
                let level = |i: usize| {
                    self.voices()
                        .iter()
                        .enumerate()
                        .filter(|(_, voice)| {
                            voice.held && voice.note.map(|note| note.id) == self.voices[i].note.map(|note| note.id)
                        })
                        .map(|(j, _)| self.levels.as_ref().map_or(0.0, |levels| levels[j]))
                        .fold(f32::NEG_INFINITY, f32::max)
                };
                let by_level = if quietest {
                    level(a).total_cmp(&level(b))
                } else {
                    std::cmp::Ordering::Equal
                };
                by_level
                    .then_with(|| self.voices[a].started.cmp(&self.voices[b].started))
                    .then(a.cmp(&b))
            })
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests;

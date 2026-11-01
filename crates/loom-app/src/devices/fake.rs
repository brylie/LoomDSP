use super::*;
use loom_host_desktop::MidiInputState;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// What happened, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Opened(String),
    MidiOpened(String),
    MidiClosed(String),
}

#[derive(Default)]
pub struct Fake {
    pub outputs: RefCell<Outputs>,
    /// Available fake input devices.
    pub audio_inputs: RefCell<Inputs>,
    /// The producer held by the open fake input.
    pub input_writer: RefCell<Option<loom_core::audio_input::InputWriter>>,
    /// Number of input streams opened.
    pub input_opens: std::cell::Cell<usize>,
    /// Number of input streams closed.
    pub input_closes: std::cell::Cell<usize>,
    /// Simulates a refusal to open the input.
    pub input_error: RefCell<Option<String>>,
    /// Simulates a permission denied after the stream opened.
    pub input_permission_denied: std::cell::Cell<bool>,
    /// Devices that fail to open.
    pub broken: RefCell<Vec<String>>,
    /// Devices that open, then fail to start.
    pub wont_start: RefCell<Vec<String>>,
    /// The active running streams' callback stats, so tests can emit lost errors.
    pub active_stats: RefCell<Vec<(String, Arc<loom_host_desktop::CallbackStats>)>>,
    /// The MIDI inputs plugged in now, as (name, port identity). A
    /// test plugs and unplugs them with [`Fake::plug`] and
    /// [`Fake::unplug`].
    pub midi: RefCell<Vec<(String, String)>>,
    /// Counts identities, so a port plugged in again gets a new one.
    pub next_id: std::cell::Cell<u32>,
    /// MIDI inputs, by listed name, that fail to open.
    pub broken_midi: RefCell<Vec<String>>,
    /// True while listing the MIDI inputs fails.
    pub midi_list_fails: std::cell::Cell<bool>,
    pub log: RefCell<Vec<Event>>,
    /// The open MIDI inputs, so a test can play notes on them.
    pub ports: RefCell<Vec<(String, Rc<RefCell<MidiInputState>>)>>,
    /// With a tape, every device is a stepped output on it: it plays
    /// only when the test steps the tape, and keeps every sample
    /// (audio-qa.md, Stepped test output). Without one, devices run on a timer.
    pub tape: Option<loom_host_desktop::Tape>,
    /// With a text, MIDI goes through the real `midir` inputs instead
    /// of `midi`, but only those whose names contain the text, so a
    /// test with virtual ports never opens a real controller
    /// (audio-qa.md, Virtual MIDI controllers).
    pub real_midi: Option<String>,
    /// Devices that start, make [`STALL_AFTER`] callbacks, and then
    /// stop calling back with no error, as a macOS device did after a
    /// rate change. They play on [`Fake::stall_tape`].
    pub stalls: Vec<String>,
    /// The tape the stalling devices play on. Tests leave it alone.
    pub stall_tape: loom_host_desktop::Tape,
}

/// Callbacks a stalling device makes before it stops.
pub const STALL_AFTER: usize = 4;

/// A device that offers 44.1 to 48 kHz, buffers of 32 to 1024 frames,
/// and defaults to 48 kHz.
pub fn device(name: &str) -> OutputInfo {
    OutputInfo {
        name: name.to_string(),
        default_rate: 48_000,
        rates: vec![(44_100, 48_000)],
        buffer: Some((32, 1024)),
    }
}

impl Fake {
    /// Devices by name, the first the system default, and MIDI inputs.
    pub fn new(devices: &[OutputInfo], midi: &[&str]) -> Self {
        Self {
            outputs: RefCell::new(Outputs {
                default: devices.first().map(|d| d.name.clone()),
                devices: devices.to_vec(),
            }),
            midi: RefCell::new(midi.iter().map(|s| (s.to_string(), format!("id-{s}"))).collect()),
            ..Self::default()
        }
    }

    /// Dynamically removes a device from the output list.
    pub fn remove_device(&self, name: &str) {
        let mut outputs = self.outputs.borrow_mut();
        outputs.devices.retain(|d| d.name != name);
        if outputs.default.as_deref() == Some(name) {
            outputs.default = outputs.devices.first().map(|d| d.name.clone());
        }
    }

    /// Dynamically adds a device to the output list.
    pub fn add_device(&self, info: OutputInfo) {
        let mut outputs = self.outputs.borrow_mut();
        if !outputs.devices.iter().any(|d| d.name == info.name) {
            outputs.devices.push(info);
        }
        if outputs.default.is_none() {
            outputs.default = outputs.devices.first().map(|d| d.name.clone());
        }
    }

    /// Emits a device loss error for the running output on `name`.
    pub fn emit_lost(&self, name: &str) {
        for (dev_name, stats) in self.active_stats.borrow().iter() {
            if dev_name == name {
                stats.mark_dead();
            }
        }
    }

    /// Plugs in an input with a new port identity, at the end of the
    /// list.
    pub fn plug(&self, name: &str) {
        let id = self.next_id.get() + 1;
        self.next_id.set(id);
        self.midi.borrow_mut().push((name.to_string(), format!("{name}#{id}")));
    }

    /// Unplugs the last input listed with this name. Its connection, if
    /// open, stays until the app closes it, as a real one does.
    pub fn unplug(&self, name: &str) {
        let mut midi = self.midi.borrow_mut();
        if let Some(index) = midi.iter().rposition(|(n, _)| n == name) {
            midi.remove(index);
        }
    }

    /// How many inputs are open, for leak checks.
    pub fn open_count(&self) -> usize {
        self.ports.borrow().len()
    }

    pub fn opened(&self) -> Vec<String> {
        self.log
            .borrow()
            .iter()
            .filter_map(|e| match e {
                Event::Opened(name) => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    /// Plays raw MIDI bytes on an open input.
    pub fn play(&self, input: &str, bytes: &[u8]) {
        let ports = self.ports.borrow();
        let (_, state) = ports.iter().find(|(name, _)| name == input).expect("the input is open");
        state.borrow_mut().receive(bytes);
    }

    pub fn is_open(&self, input: &str) -> bool {
        self.ports.borrow().iter().any(|(name, _)| name == input)
    }
}

struct Port {
    name: String,
    id: String,
    input: MidiInputId,
    state: Rc<RefCell<MidiInputState>>,
    host: Rc<Fake>,
}

/// A fake input stream whose writer is controlled by the test.
struct AudioPort {
    host: Rc<Fake>,
    name: String,
    status: loom_core::audio_input::InputStatus,
}

impl InputPort for AudioPort {
    fn name(&self) -> &str {
        &self.name
    }
    fn is_live(&self) -> bool {
        self.status.is_live()
    }
    fn status(&self) -> loom_core::audio_input::InputStatus {
        self.status.clone()
    }
}

impl Drop for AudioPort {
    fn drop(&mut self) {
        self.host.input_writer.borrow_mut().take();
        self.host.input_closes.set(self.host.input_closes.get() + 1);
    }
}

impl MidiPort for Port {
    fn name(&self) -> &str {
        &self.name
    }

    fn port_id(&self) -> &str {
        &self.id
    }

    fn input(&self) -> MidiInputId {
        self.input
    }

    fn close(self: Box<Self>) {
        self.host.ports.borrow_mut().retain(|(name, _)| *name != self.name);
        self.state.borrow_mut().release_all();
        self.host.log.borrow_mut().push(Event::MidiClosed(self.name.clone()));
    }
}

/// The host `App::new` uses in tests: the real audio outputs, but no
/// MIDI inputs, so a controller plugged into the machine cannot change
/// a result. Opening a MIDI port through it is a test bug.
pub struct NoMidi;

impl Host for NoMidi {
    fn outputs(&self) -> Result<Outputs, String> {
        System.outputs()
    }

    fn open(&self, options: &AudioOptions) -> Result<AudioOutput, String> {
        System.open(options)
    }

    #[allow(clippy::result_large_err)]
    #[allow(clippy::result_large_err)]
    fn start(
        &self,
        output: AudioOutput,
        engine: loom_core::Engine,
    ) -> Result<RunningAudio, (loom_core::Engine, String)> {
        System.start(output, engine)
    }

    fn midi_list(&self) -> Result<Vec<MidiInputInfo>, String> {
        Ok(Vec::new())
    }

    fn open_midi(&self, name: &str, _midi: SharedMidi, _input: MidiInputId) -> Result<Box<dyn MidiPort>, String> {
        panic!("a test opened the MIDI port `{name}`; use fake::Shared with its own ports");
    }
}

/// The fake behind an `Rc`, so its MIDI ports can reach it.
pub struct Shared(pub Rc<Fake>);

impl Host for Shared {
    fn outputs(&self) -> Result<Outputs, String> {
        Ok(self.0.outputs.borrow().clone())
    }

    fn audio_inputs(&self) -> Result<Inputs, String> {
        Ok(self.0.audio_inputs.borrow().clone())
    }

    fn audio_input_permission_denied(&self) -> bool {
        self.0.input_permission_denied.get()
    }

    fn open_audio_input(
        &self,
        options: &AudioOptions,
        buffer: usize,
    ) -> Result<(Box<dyn InputPort>, loom_core::audio_input::InputReader), String> {
        if let Some(error) = &*self.0.input_error.borrow() {
            return Err(error.clone());
        }
        let inputs = self.0.audio_inputs.borrow();
        let name = options
            .device
            .as_ref()
            .and_then(|wanted| {
                inputs
                    .devices
                    .iter()
                    .find(|name| {
                        if options.exact {
                            *name == wanted
                        } else {
                            name.to_lowercase().contains(&wanted.to_lowercase())
                        }
                    })
                    .cloned()
            })
            .or_else(|| options.device.is_none().then(|| inputs.default.clone()).flatten())
            .ok_or("no audio input device available")?;
        let (writer, reader) = loom_core::audio_input::input_ring(buffer, 2);
        let status = writer.status();
        *self.0.input_writer.borrow_mut() = Some(writer);
        self.0.input_opens.set(self.0.input_opens.get() + 1);
        Ok((
            Box::new(AudioPort {
                host: self.0.clone(),
                name,
                status,
            }),
            reader,
        ))
    }

    fn open(&self, options: &AudioOptions) -> Result<AudioOutput, String> {
        let fake = &self.0;
        let outputs = fake.outputs.borrow();
        let name = options
            .device
            .clone()
            .or_else(|| outputs.default.clone())
            .ok_or("no device")?;
        let info = outputs
            .devices
            .iter()
            .find(|d| d.name == name)
            .ok_or_else(|| format!("no audio output device found matching `{name}`"))?;
        if fake.broken.borrow().contains(&name) {
            return Err("the device is busy".to_string());
        }
        let rate = options.sample_rate.unwrap_or(info.default_rate);
        if !offers_rate(info, rate) {
            return Err(format!("audio device `{name}` does not support {rate} Hz"));
        }
        fake.log.borrow_mut().push(Event::Opened(name.clone()));
        let block = options.buffer_size.unwrap_or(256);
        Ok(match &fake.tape {
            _ if fake.stalls.contains(&name) => AudioOutput::stepped(rate, block, &fake.stall_tape),
            Some(tape) => AudioOutput::stepped(rate, block, tape),
            None => AudioOutput::clock(rate, block),
        }
        .named(name))
    }

    #[allow(clippy::result_large_err)]
    fn start(
        &self,
        output: AudioOutput,
        engine: loom_core::Engine,
    ) -> Result<RunningAudio, (loom_core::Engine, String)> {
        if self.0.wont_start.borrow().iter().any(|name| name == output.name()) {
            return Err((engine, "the stream did not start".to_string()));
        }
        let stalls = self.0.stalls.iter().any(|name| name == output.name());
        let block = output.buffer_size().unwrap_or(256) as usize;
        let name = output.name().to_string();
        let running = match output.start(engine) {
            Ok(r) => r,
            Err(e) => return Err((e.engine, e.error.to_string())),
        };
        self.0.active_stats.borrow_mut().push((name, running.stats().clone()));
        if stalls {
            self.0.stall_tape.step(block * STALL_AFTER);
        }
        Ok(running)
    }

    fn midi_list(&self) -> Result<Vec<MidiInputInfo>, String> {
        match &self.0.real_midi {
            Some(only) => Ok(System
                .midi_list()?
                .into_iter()
                .filter(|i| i.name.contains(only))
                .collect()),
            None if self.0.midi_list_fails.get() => Err("the MIDI service is not running".to_string()),
            None => {
                let (names, ids) = self.0.midi.borrow().iter().cloned().unzip();
                Ok(loom_host_desktop::input_infos(names, ids))
            }
        }
    }

    fn open_midi(&self, name: &str, midi: SharedMidi, input: MidiInputId) -> Result<Box<dyn MidiPort>, String> {
        if let Some(only) = &self.0.real_midi {
            assert!(name.contains(only), "a test opens only its own MIDI ports");
            return System.open_midi(name, midi, input);
        }
        if self.0.broken_midi.borrow().iter().any(|broken| broken == name) {
            return Err(format!("{name}: the port is busy"));
        }
        let Some(info) = self.midi_list()?.into_iter().find(|i| i.name == name) else {
            return Err(format!("no MIDI input port found matching `{name}`"));
        };
        let state = Rc::new(RefCell::new(MidiInputState::for_input(midi, input)));
        self.0.ports.borrow_mut().push((name.to_string(), state.clone()));
        self.0.log.borrow_mut().push(Event::MidiOpened(name.to_string()));
        Ok(Box::new(Port {
            name: name.to_string(),
            id: info.id,
            input,
            state,
            host: self.0.clone(),
        }))
    }
}

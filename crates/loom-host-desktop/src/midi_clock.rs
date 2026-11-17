//! MIDI output connections and sample-to-playback scheduling.

use crate::{HostError, unique_names};
use loom_core::{ClockEvent, ClockMessage, ClockReader, Engine};
use midir::{MidiOutput, MidiOutputPort};
use rtrb::{Consumer, Producer, RingBuffer};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// The clock outputs enabled for this rig and any send failures.
#[derive(Clone, Default)]
pub struct ClockPorts(Arc<Ports>);

/// Control and worker data. The audio callback never needs this lock.
#[derive(Default)]
struct Ports {
    enabled: Mutex<Vec<String>>,
    failures: AtomicU64,
}

impl ClockPorts {
    /// Replaces the list of output names enabled for clock.
    pub fn set_enabled(&self, names: &[String]) {
        *self.0.enabled.lock().unwrap_or_else(|err| err.into_inner()) = names.to_vec();
    }

    /// Timing and device send failures since the rig started.
    pub fn failures(&self) -> u64 {
        self.0.failures.load(Ordering::Relaxed)
    }
}

/// Lists the system's MIDI outputs using the same naming scheme as inputs.
pub fn midi_clock_outputs() -> Result<Vec<String>, HostError> {
    discover().map(|(_, ports)| ports.into_iter().map(|(name, _)| name).collect())
}

/// Lists MIDI outputs together with the port handles needed to open them.
fn discover() -> Result<(MidiOutput, Vec<(String, MidiOutputPort)>), HostError> {
    let output = MidiOutput::new("LoomDSP clock").map_err(|error| HostError::Midi(error.to_string()))?;
    let ports = output.ports();
    let names: Vec<_> = ports
        .iter()
        .map(|port| output.port_name(port).unwrap_or_else(|_| "<unnamed port>".into()))
        .collect();
    Ok((output, unique_names(&names).into_iter().zip(ports).collect()))
}

/// The audio callback's time mapping for the first frame of one render.
#[derive(Clone, Copy)]
pub(crate) struct Anchor {
    pub frame: u64,
    pub playback: Instant,
}

/// The worker's lifetime is tied to the audio stream.
pub(crate) struct ClockWorker {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl ClockWorker {
    /// Connects the audio callback to a worker without sharing a lock.
    pub fn spawn(engine: &mut Engine, ports: ClockPorts) -> Option<(Self, Producer<Anchor>)> {
        let (send, receive) = RingBuffer::new(1024);
        let reader = engine.attach_clock_output();
        reader.set_enabled(false);
        let sample_rate = f64::from(engine.config().sample_rate);
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let worker = thread::Builder::new()
            .name("loom-midi-clock".into())
            .spawn(move || {
                Worker::new(reader, receive, sample_rate, ports).run(&done);
            })
            .ok()?;
        Some((
            Self {
                stop,
                worker: Some(worker),
            },
            send,
        ))
    }
}

impl Drop for ClockWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

/// A connected output and its synchronization state.
struct Connection {
    name: String,
    id: String,
    send_message: Box<dyn FnMut(ClockMessage) -> bool>,
    needs_state: bool,
}

impl Connection {
    /// Sends one message on the worker thread.
    fn send(&mut self, message: ClockMessage) -> bool {
        (self.send_message)(message)
    }

    /// Restores the receiver's position and play state.
    fn sync(&mut self, position: f64, playing: bool) -> bool {
        self.needs_state = false;
        if !playing {
            return self.send(ClockMessage::Stop) && self.send(ClockMessage::at_position(position));
        }
        if position == 0.0 {
            return self.send(ClockMessage::Start);
        }
        self.send(ClockMessage::at_position(position)) && self.send(ClockMessage::Continue)
    }
}

/// The worker owns discovery, open connections, and pending deadlines.
struct Worker {
    reader: ClockReader,
    incoming: Consumer<Anchor>,
    anchors: VecDeque<Anchor>,
    sample_rate: f64,
    ports: ClockPorts,
    connections: Vec<Connection>,
    pending: Option<ClockEvent>,
    dropped: u64,
    next_scan: Instant,
}

impl Worker {
    /// Prepares the worker before polling.
    fn new(reader: ClockReader, incoming: Consumer<Anchor>, sample_rate: f64, ports: ClockPorts) -> Self {
        Self {
            reader,
            incoming,
            anchors: VecDeque::new(),
            sample_rate,
            ports,
            connections: Vec::new(),
            pending: None,
            dropped: 0,
            next_scan: Instant::now(),
        }
    }

    /// Refreshes endpoints, processes deadlines, and releases ports on shutdown.
    fn run(&mut self, stop: &AtomicBool) {
        while !stop.load(Ordering::Relaxed) {
            let now = Instant::now();
            if now >= self.next_scan {
                self.scan();
                self.next_scan = now + Duration::from_millis(250);
            }
            while let Ok(anchor) = self.incoming.pop() {
                if self.anchors.len() == 1024 {
                    self.anchors.pop_front();
                }
                self.anchors.push_back(anchor);
            }
            let wait = self.dispatch(Instant::now);
            let max_wait = if self.connections.is_empty() { 25 } else { 1 };
            thread::park_timeout(if self.connections.is_empty() {
                Duration::from_millis(max_wait)
            } else {
                wait.min(Duration::from_millis(max_wait))
            });
        }
        for connection in &mut self.connections {
            connection.send(ClockMessage::Stop);
        }
    }

    /// Opens enabled outputs and removes ports that vanished or were disabled.
    fn scan(&mut self) {
        let wanted = self
            .ports
            .0
            .enabled
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        self.connections.retain_mut(|connection| {
            let keep = wanted.contains(&connection.name);
            if !keep {
                connection.send(ClockMessage::Stop);
            }
            keep
        });
        if wanted.is_empty() {
            self.reader.set_enabled(false);
            self.pending = None;
            while self.reader.pop().is_some() {}
            return;
        }
        let Ok((_, ports)) = discover() else { return };
        self.connections.retain(|connection| {
            ports
                .iter()
                .any(|(name, port)| *name == connection.name && port.id() == connection.id)
        });
        for (name, port) in ports {
            if !wanted.contains(&name) || self.connections.iter().any(|connection| connection.name == name) {
                continue;
            }
            let id = port.id();
            if let Ok(output) = MidiOutput::new("LoomDSP clock")
                && let Ok(mut connection) = output.connect(&port, "loomdsp-clock")
            {
                self.connections.push(Connection {
                    name,
                    id,
                    send_message: Box::new(move |message| message.encode(|bytes| connection.send(bytes)).is_ok()),
                    needs_state: true,
                });
            }
        }
        self.reader.set_enabled(!self.connections.is_empty());
    }

    /// Handles pending frames in order. Timing faults discard the overdue backlog.
    fn dispatch(&mut self, now: impl Fn() -> Instant) -> Duration {
        let idle = Duration::from_millis(1);
        let dropped = self.reader.dropped();
        if dropped != self.dropped {
            self.dropped = dropped;
            self.recover();
        }
        loop {
            let Some(event) = self.pending.or_else(|| self.reader.pop()) else {
                let (position, playing) = self.reader.state();
                if !playing {
                    self.connections
                        .retain_mut(|connection| !connection.needs_state || connection.sync(position, false));
                }
                return idle;
            };
            while self.anchors.get(1).is_some_and(|anchor| anchor.frame <= event.frame) {
                self.anchors.pop_front();
            }
            let Some(anchor) = self.anchors.front() else {
                self.pending = Some(event);
                return idle;
            };
            let due = anchor.playback
                + Duration::from_secs_f64(event.frame.saturating_sub(anchor.frame) as f64 / self.sample_rate);
            let now = now();
            if due > now {
                self.pending = Some(event);
                return due.duration_since(now);
            }
            self.pending = None;
            let lateness = now.duration_since(due);
            let interval = Duration::from_secs_f64(event.pulse_seconds);
            if !self.connections.is_empty() && event.message == ClockMessage::Pulse && lateness >= interval {
                self.recover();
                return idle;
            }
            let failures = &self.ports.0.failures;
            self.connections.retain_mut(|connection| {
                let sent = if connection.needs_state {
                    match event.message {
                        ClockMessage::Pulse => connection.sync(event.position, true) && connection.send(event.message),
                        ClockMessage::Stop => connection.sync(event.position, false),
                        _ => true,
                    }
                } else {
                    connection.send(event.message)
                };
                if !sent {
                    failures.fetch_add(1, Ordering::Relaxed);
                }
                sent
            });
        }
    }

    /// Drops outstanding messages and stops receivers before the next synchronization.
    fn recover(&mut self) {
        self.pending = None;
        while self.reader.pop().is_some() {}
        if !self.connections.is_empty() {
            self.ports.0.failures.fetch_add(1, Ordering::Relaxed);
        }
        self.connections.retain_mut(|connection| {
            connection.needs_state = true;
            connection.send(ClockMessage::Stop)
        });
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests;

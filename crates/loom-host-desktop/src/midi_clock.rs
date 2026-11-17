//! MIDI output connections and sample-to-playback scheduling.

use crate::{HostError, unique_names};
use loom_core::{ClockEvent, ClockMessage, ClockReader, Engine};
use midir::{MidiOutput, MidiOutputPort};
use rtrb::{Consumer, Producer, RingBuffer};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, Weak};
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
    routing_error: Mutex<Option<String>>,
}

impl ClockPorts {
    /// Replaces the list of output names enabled for clock.
    pub fn set_enabled(&self, names: &[String]) {
        *self.0.enabled.lock().unwrap_or_else(|err| err.into_inner()) = names.to_vec();
    }

    /// The current discovery or connection error, if the worker is retrying one.
    pub fn routing_error(&self) -> Option<String> {
        self.0
            .routing_error
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    /// Timing and device send failures since the rig started.
    pub fn failures(&self) -> u64 {
        self.0.failures.load(Ordering::Relaxed)
    }
}

/// Lists the system's MIDI outputs using the same naming scheme as inputs.
pub fn midi_clock_outputs() -> Result<Vec<String>, HostError> {
    discover().map(|ports| ports.into_iter().map(|(endpoint, _)| endpoint.name).collect())
}

/// Lists MIDI outputs together with the port handles needed to open them.
fn discover() -> Result<Vec<(Endpoint, MidiOutputPort)>, HostError> {
    let output = MidiOutput::new("LoomDSP clock").map_err(|error| HostError::Midi(error.to_string()))?;
    let ports = output.ports();
    let names: Vec<_> = ports
        .iter()
        .map(|port| output.port_name(port).unwrap_or_else(|_| "<unnamed port>".into()))
        .collect();
    Ok(unique_names(&names)
        .into_iter()
        .zip(ports)
        .map(|(name, port)| (Endpoint { name, id: port.id() }, port))
        .collect())
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
                thread::scope(|scope| {
                    let (updates, routes) = mpsc::sync_channel(1);
                    let scanner = thread::Builder::new()
                        .name("loom-midi-ports".into())
                        .spawn_scoped(scope, || {
                            scan_ports(&done, &ports, updates);
                        });
                    if scanner.is_err() {
                        ports.0.failures.fetch_add(1, Ordering::Relaxed);
                    }
                    Worker::new(reader, receive, sample_rate, ports.clone()).run(&done, &routes);
                    if let Ok(scanner) = scanner {
                        scanner.thread().unpark();
                    }
                });
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

/// A name from Preferences and the current identity supplied by the MIDI service.
#[derive(Clone, PartialEq, Eq)]
struct Endpoint {
    name: String,
    id: String,
}

/// The routes discovered and opened away from the deadline thread.
#[derive(Default)]
struct Scan {
    wanted: Vec<String>,
    /// Absent when the MIDI service could not list ports. Keep live routes then.
    available: Option<Vec<Endpoint>>,
    opened: Vec<Connection>,
    error: Option<String>,
}

/// Discovers and opens outputs without holding up MIDI deadlines.
fn scan_ports(stop: &AtomicBool, ports: &ClockPorts, updates: SyncSender<Scan>) {
    let mut known = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        let wanted = ports.0.enabled.lock().unwrap_or_else(|err| err.into_inner()).clone();
        let mut scan = prepare_scan(wanted, &mut known);
        *ports.0.routing_error.lock().unwrap_or_else(|err| err.into_inner()) = scan.error.take();
        // At most one update queues. The next scan retries handles that
        // were dropped because the worker still had an update waiting.
        match updates.try_send(scan) {
            Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
            Err(mpsc::TrySendError::Disconnected(_)) => break,
        }
        if !stop.load(Ordering::Relaxed) {
            thread::park_timeout(Duration::from_millis(250));
        }
    }
}

/// Opens newly seen outputs and retries ones the scheduler lost.
fn prepare_scan(wanted: Vec<String>, known: &mut Vec<(Endpoint, Weak<()>)>) -> Scan {
    known.retain(|(_, lifetime)| lifetime.strong_count() != 0);
    let mut scan = Scan {
        wanted,
        ..Scan::default()
    };
    if scan.wanted.is_empty() {
        return scan;
    }
    let ports = match discover() {
        Ok(ports) => ports,
        Err(error) => {
            scan.error = Some(format!("Could not list MIDI clock outputs: {error}"));
            return scan;
        }
    };
    let mut available = Vec::new();
    for (endpoint, port) in ports {
        if !scan.wanted.contains(&endpoint.name) {
            continue;
        }
        available.push(endpoint.clone());
        if known.iter().any(|(live, _)| *live == endpoint) {
            continue;
        }
        let mut connection = match MidiOutput::new("LoomDSP clock")
            .map_err(|error| error.to_string())
            .and_then(|output| {
                output
                    .connect(&port, "loomdsp-clock")
                    .map_err(|error| error.to_string())
            }) {
            Ok(connection) => connection,
            Err(error) => {
                scan.error
                    .get_or_insert_with(|| format!("Could not open MIDI clock output \"{}\": {error}", endpoint.name));
                continue;
            }
        };
        let lifetime = Arc::new(());
        known.push((endpoint.clone(), Arc::downgrade(&lifetime)));
        scan.opened.push(Connection {
            endpoint,
            send_message: Box::new(move |message| message.encode(|bytes| connection.send(bytes)).is_ok()),
            needs_state: true,
            _lifetime: lifetime,
        });
    }
    scan.available = Some(available);
    scan
}

/// A connected output and its synchronization state.
struct Connection {
    endpoint: Endpoint,
    send_message: Box<dyn FnMut(ClockMessage) -> bool + Send>,
    needs_state: bool,
    /// Lets the scanner detect a dropped connection and retry the same endpoint.
    _lifetime: Arc<()>,
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
        }
    }

    /// Applies prepared routes, processes deadlines, and stops outputs on shutdown.
    fn run(&mut self, stop: &AtomicBool, routes: &Receiver<Scan>) {
        while !stop.load(Ordering::Relaxed) {
            if let Ok(scan) = routes.try_recv() {
                self.apply_scan(scan);
            }
            while let Ok(anchor) = self.incoming.pop() {
                if self.anchors.len() == 1024 {
                    self.anchors.pop_front();
                }
                self.anchors.push_back(anchor);
            }
            let wait = self.dispatch(Instant::now);
            thread::park_timeout(if self.connections.is_empty() {
                Duration::from_millis(25)
            } else {
                wait.min(Duration::from_millis(1))
            });
        }
        for connection in &mut self.connections {
            connection.send(ClockMessage::Stop);
        }
    }

    /// Installs already-opened routes without calling the MIDI discovery API.
    fn apply_scan(&mut self, scan: Scan) {
        self.connections.retain_mut(|connection| {
            let keep = scan.wanted.contains(&connection.endpoint.name)
                && scan
                    .available
                    .as_ref()
                    .is_none_or(|available| available.contains(&connection.endpoint));
            if !keep {
                connection.send(ClockMessage::Stop);
            }
            keep
        });
        self.connections.extend(scan.opened);
        self.reader.set_enabled(!self.connections.is_empty());
        if self.connections.is_empty() {
            self.pending = None;
            while self.reader.pop().is_some() {}
        }
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
            let interval = event.pulse_period;
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

#[cfg(test)]
mod tests;

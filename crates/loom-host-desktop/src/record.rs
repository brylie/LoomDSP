//! Recording what the engine plays to a WAV file.
//!
//! The audio thread may not do file I/O (NFR-RT-01), so it pushes samples
//! into a lock-free ring buffer, and a writer thread drains the ring into
//! the file. When the ring is full, samples are dropped and counted, never
//! waited for. The writer updates the WAV header every second, so the file
//! can be read even if the process is killed.
//!
//! One recording can span many streams (`docs/design/audio-qa.md`, Part
//! 2): each stream gets its own ring from [`Recorder::tap`], and the writer
//! drains the rings in the order they were made. Nothing is written between
//! streams.

use crate::HostError;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How much audio each ring holds, in seconds. The writer drains it every
/// [`DRAIN`], so this leaves room for a slow disk.
const RING_SECONDS: usize = 2;
/// How often the writer drains the ring.
const DRAIN: Duration = Duration::from_millis(20);
/// How often the writer updates the WAV header.
const FLUSH: Duration = Duration::from_secs(1);

fn record_error(error: impl std::fmt::Display) -> HostError {
    HostError::Record(error.to_string())
}

/// The audio thread's end: pushes stereo frames into the ring.
pub(crate) struct Tap {
    producer: rtrb::Producer<f32>,
    dropped: Arc<AtomicU64>,
}

impl Tap {
    /// Pushes interleaved stereo frames. Real-time safe: never allocates
    /// or waits. Frames that do not fit are counted.
    pub(crate) fn push(&mut self, left: &[f32], right: &[f32]) {
        for (l, r) in left.iter().zip(right) {
            if self.producer.slots() < 2 {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            // Two free slots were just checked, and only this thread pushes.
            let _ = self.producer.push(*l);
            let _ = self.producer.push(*r);
        }
    }
}

/// A WAV file being written, for as many streams as play while it is open.
/// Dropping it writes what is left and closes the file.
pub struct Recorder {
    sample_rate: u32,
    /// Sends the writer the ring of each new stream.
    streams: Sender<rtrb::Consumer<f32>>,
    done: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    /// The first write error. The recording stops there; what was written
    /// before stays readable.
    error: Arc<Mutex<Option<String>>>,
    thread: Option<JoinHandle<()>>,
}

impl Recorder {
    /// Creates the WAV file (32-bit float, stereo) and starts the writer.
    /// Every stream recorded into it must run at `sample_rate`.
    pub fn start(path: &Path, sample_rate: u32) -> Result<Self, HostError> {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(path, spec).map_err(record_error)?;
        let (streams, receiver) = mpsc::channel();
        let done = Arc::new(AtomicBool::new(false));
        let error = Arc::new(Mutex::new(None));
        let thread_done = done.clone();
        let thread_error = error.clone();
        let thread = std::thread::Builder::new()
            .name("loom-record".to_string())
            .spawn(move || {
                // A write error (a full disk) ends the recording. Later
                // frames no longer fit the ring and count as dropped.
                if let Err(e) = write(&mut writer, &receiver, &thread_done).and_then(|()| writer.finalize())
                    && let Ok(mut slot) = thread_error.lock()
                {
                    *slot = Some(e.to_string());
                }
            })
            .map_err(record_error)?;
        Ok(Self {
            sample_rate,
            streams,
            done,
            dropped: Arc::new(AtomicU64::new(0)),
            error,
            thread: Some(thread),
        })
    }

    /// The sample rate of the file.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// A ring for one more stream. Its frames are written after those of
    /// every earlier tap.
    pub(crate) fn tap(&self) -> Tap {
        let capacity = self.sample_rate as usize * 2 * RING_SECONDS;
        let (producer, consumer) = rtrb::RingBuffer::<f32>::new(capacity.max(2));
        // The writer ends only when this recorder drops, so it is listening.
        let _ = self.streams.send(consumer);
        Tap {
            producer,
            dropped: self.dropped.clone(),
        }
    }

    /// Frames dropped because a ring was full.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// The write error that stopped the recording, if one did.
    pub fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|e| e.clone())
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Writes every ready sample of `consumer`.
fn drain(
    writer: &mut hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    consumer: &mut rtrb::Consumer<f32>,
) -> Result<(), hound::Error> {
    let ready = consumer.slots();
    if let Ok(chunk) = consumer.read_chunk(ready) {
        let (a, b) = chunk.as_slices();
        for &sample in a.iter().chain(b) {
            writer.write_sample(sample)?;
        }
        chunk.commit_all();
    }
    Ok(())
}

/// The writer loop: drains the current stream's ring until that stream has
/// ended and its ring is empty, then moves to the next. When the recorder
/// drops, writes what every ring still holds.
fn write(
    writer: &mut hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    streams: &Receiver<rtrb::Consumer<f32>>,
    done: &AtomicBool,
) -> Result<(), hound::Error> {
    let mut current: Option<rtrb::Consumer<f32>> = None;
    let mut flushed = Instant::now();
    loop {
        // Read the flags before draining, so nothing pushed before them is
        // left behind.
        let stop = done.load(Ordering::Acquire);
        if current.is_none() {
            current = streams.try_recv().ok();
        }
        if let Some(consumer) = &mut current {
            let ended = consumer.is_abandoned();
            drain(writer, consumer)?;
            if ended {
                // Its stream stopped, and all it pushed is written.
                current = None;
                continue;
            }
        }
        if stop {
            // Streams still open when the recorder closes: write what they
            // pushed so far, in order.
            while let Ok(mut consumer) = streams.try_recv() {
                drain(writer, &mut consumer)?;
            }
            return Ok(());
        }
        if flushed.elapsed() >= FLUSH {
            writer.flush()?;
            flushed = Instant::now();
        }
        std::thread::sleep(DRAIN);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One stream's frames are written as pushed.
    #[test]
    fn writes_what_was_pushed() {
        let dir = std::env::temp_dir().join(format!("loom-record-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.wav");
        let recorder = Recorder::start(&path, 48_000).unwrap();
        let mut tap = recorder.tap();
        let left: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
        let right: Vec<f32> = left.iter().map(|x| -x).collect();
        tap.push(&left, &right);
        drop(tap);
        assert_eq!(recorder.dropped(), 0);
        drop(recorder);

        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().sample_rate, 48_000);
        let samples: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).collect();
        assert_eq!(samples.len(), 2000);
        assert_eq!(samples[2 * 500], left[500]);
        assert_eq!(samples[2 * 500 + 1], right[500]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Streams follow each other in the file in the order their taps were
    /// made, with nothing between them, even when the second stream pushes
    /// before the first has ended.
    #[test]
    fn streams_are_written_in_order_with_nothing_between() {
        let dir = std::env::temp_dir().join(format!("loom-record-streams-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.wav");
        let recorder = Recorder::start(&path, 48_000).unwrap();
        let mut first = recorder.tap();
        let mut second = recorder.tap();
        second.push(&[2.0; 300], &[2.0; 300]);
        first.push(&[1.0; 200], &[1.0; 200]);
        drop(first);
        drop(second);
        let mut third = recorder.tap();
        third.push(&[3.0; 100], &[3.0; 100]);
        // The third stream is still open when the recording closes.
        drop(recorder);
        drop(third);

        let mut reader = hound::WavReader::open(&path).unwrap();
        let left: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).step_by(2).collect();
        let expected: Vec<f32> = [vec![1.0; 200], vec![2.0; 300], vec![3.0; 100]].concat();
        assert_eq!(left, expected);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A full ring drops frames and counts them.
    #[test]
    fn a_full_ring_drops_and_counts() {
        let dir = std::env::temp_dir().join(format!("loom-record-full-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 10 Hz: the ring holds 2 seconds, 20 frames.
        let recorder = Recorder::start(&dir.join("out.wav"), 10).unwrap();
        let mut tap = recorder.tap();
        // Hold the writer back by pushing faster than it drains.
        tap.push(&[0.0; 100], &[0.0; 100]);
        assert!(recorder.dropped() > 0);
        drop(tap);
        drop(recorder);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

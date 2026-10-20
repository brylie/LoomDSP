//! Recording what the engine plays to a WAV file.
//!
//! The audio thread may not do file I/O (NFR-RT-01), so it pushes samples
//! into a lock-free ring buffer, and a writer thread drains the ring into
//! the file. When the ring is full, samples are dropped and counted, never
//! waited for. The writer updates the WAV header every second, so the file
//! can be read even if the process is killed.

use crate::HostError;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How much audio the ring holds, in seconds. The writer drains it every
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

/// The writer thread. Dropping it writes what is left and closes the file.
pub(crate) struct Recording {
    done: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl Recording {
    /// Creates the WAV file (32-bit float, stereo) and starts the writer.
    pub(crate) fn start(path: &Path, sample_rate: u32) -> Result<(Self, Tap), HostError> {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(path, spec).map_err(record_error)?;
        let capacity = sample_rate as usize * 2 * RING_SECONDS;
        let (producer, mut consumer) = rtrb::RingBuffer::<f32>::new(capacity);
        let done = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicU64::new(0));
        let thread_done = done.clone();
        let thread = std::thread::Builder::new()
            .name("loom-record".to_string())
            .spawn(move || {
                let mut flushed = Instant::now();
                loop {
                    // Read the flag before draining, so nothing pushed before
                    // the stop is left behind.
                    let stop = thread_done.load(Ordering::Acquire) || consumer.is_abandoned();
                    let ready = consumer.slots();
                    if let Ok(chunk) = consumer.read_chunk(ready) {
                        let (a, b) = chunk.as_slices();
                        for &sample in a.iter().chain(b) {
                            // A write error (disk full) ends the recording;
                            // the samples so far stay readable.
                            if writer.write_sample(sample).is_err() {
                                return;
                            }
                        }
                        chunk.commit_all();
                    }
                    if stop {
                        let _ = writer.finalize();
                        return;
                    }
                    if flushed.elapsed() >= FLUSH {
                        let _ = writer.flush();
                        flushed = Instant::now();
                    }
                    std::thread::sleep(DRAIN);
                }
            })
            .map_err(record_error)?;
        let tap = Tap {
            producer,
            dropped: dropped.clone(),
        };
        Ok((
            Self {
                done,
                dropped,
                thread: Some(thread),
            },
            tap,
        ))
    }

    /// Frames dropped because the ring was full.
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_what_was_pushed() {
        let dir = std::env::temp_dir().join(format!("loom-record-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.wav");
        let (recording, mut tap) = Recording::start(&path, 48_000).unwrap();
        let left: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
        let right: Vec<f32> = left.iter().map(|x| -x).collect();
        tap.push(&left, &right);
        drop(tap);
        assert_eq!(recording.dropped(), 0);
        drop(recording);

        let mut reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().sample_rate, 48_000);
        let samples: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).collect();
        assert_eq!(samples.len(), 2000);
        assert_eq!(samples[2 * 500], left[500]);
        assert_eq!(samples[2 * 500 + 1], right[500]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_full_ring_drops_and_counts() {
        let dir = std::env::temp_dir().join(format!("loom-record-full-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 10 Hz: the ring holds 2 seconds, 20 frames.
        let (recording, mut tap) = Recording::start(&dir.join("out.wav"), 10).unwrap();
        // Hold the writer back by pushing faster than it drains.
        tap.push(&[0.0; 100], &[0.0; 100]);
        assert!(recording.dropped() > 0);
        drop(tap);
        drop(recording);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

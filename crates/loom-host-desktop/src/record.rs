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
use std::io::{Seek, Write};
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
            self.push_frame(*l, *r);
        }
    }

    /// Pushes the first two channels of interleaved frames with `channels`
    /// channels each, as from an input device. A mono frame goes to both
    /// sides. Real-time safe, like [`Tap::push`].
    pub(crate) fn push_interleaved(&mut self, data: &[f32], channels: usize) {
        for frame in data.chunks_exact(channels.max(1)) {
            let left = frame[0];
            self.push_frame(left, frame.get(1).copied().unwrap_or(left));
        }
    }

    /// Pushes one stereo frame, or counts it as dropped if it does not fit.
    fn push_frame(&mut self, left: f32, right: f32) {
        if self.producer.slots() < 2 {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        // Two free slots were just checked, and only this thread pushes.
        let _ = self.producer.push(left);
        let _ = self.producer.push(right);
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
        let file = std::fs::File::create(path).map_err(record_error)?;
        Self::start_with(std::io::BufWriter::new(file), sample_rate)
    }

    /// Starts a recording into `out`, which must be at offset 0. [`start`]
    /// passes a file; tests pass a writer that fails, as a full disk does.
    ///
    /// [`start`]: Recorder::start
    fn start_with<W>(out: W, sample_rate: u32) -> Result<Self, HostError>
    where
        W: Write + Seek + Send + 'static,
    {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::new(out, spec).map_err(record_error)?;
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

impl Recorder {
    /// Writes what is left, closes the file, and returns the write error
    /// that stopped the recording, if one did. Dropping the recorder does
    /// the same without the error.
    pub fn close(mut self) -> Option<String> {
        self.finish();
        self.error()
    }

    /// Tells the writer to write what is left, and waits for it.
    fn finish(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Writes every ready whole frame of `consumer`. The audio thread pushes
/// a frame's two samples one after the other, so the writer can find half
/// a frame in the ring. That half waits for the next drain: hound refuses
/// to write a header for a file that ends in half a frame.
fn drain<W: Write + Seek>(
    writer: &mut hound::WavWriter<W>,
    consumer: &mut rtrb::Consumer<f32>,
) -> Result<(), hound::Error> {
    let ready = consumer.slots() & !1;
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
fn write<W: Write + Seek>(
    writer: &mut hound::WavWriter<W>,
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

/// A folder for one test, removed when dropped, even if the test fails.
#[cfg(test)]
pub(crate) struct TestDir(pub(crate) std::path::PathBuf);

#[cfg(test)]
impl TestDir {
    /// An empty folder named after the test and the process.
    pub(crate) fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("loom-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

#[cfg(test)]
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One stream's frames are written as pushed.
    #[test]
    fn writes_what_was_pushed() {
        let dir = TestDir::new("record-test");
        let path = dir.0.join("out.wav");
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
    }

    /// Streams follow each other in the file in the order their taps were
    /// made, with nothing between them, even when the second stream pushes
    /// before the first has ended.
    #[test]
    fn streams_are_written_in_order_with_nothing_between() {
        let dir = TestDir::new("record-streams");
        let path = dir.0.join("out.wav");
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
    }

    /// Interleaved input frames keep their first two channels, and a mono
    /// input is written to both sides. This is the writer `loom capture`
    /// uses, tested here with a fake input in place of a device.
    #[test]
    fn interleaved_input_keeps_the_first_two_channels() {
        let dir = TestDir::new("record-interleaved");
        let path = dir.0.join("out.wav");
        let recorder = Recorder::start(&path, 48_000).unwrap();
        let mut quad = recorder.tap();
        quad.push_interleaved(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0], 4);
        drop(quad);
        let mut mono = recorder.tap();
        mono.push_interleaved(&[9.0, 10.0], 1);
        drop(mono);
        assert_eq!(recorder.dropped(), 0);
        drop(recorder);

        let mut reader = hound::WavReader::open(&path).unwrap();
        let samples: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).collect();
        assert_eq!(samples, [1.0, 2.0, 5.0, 6.0, 9.0, 9.0, 10.0, 10.0]);
    }

    /// Closing a recording that wrote without error reports no error, and
    /// leaves a readable file with what was pushed.
    #[test]
    fn close_reports_no_error_after_a_clean_recording() {
        let dir = TestDir::new("record-close");
        let path = dir.0.join("out.wav");
        let recorder = Recorder::start(&path, 48_000).unwrap();
        let mut tap = recorder.tap();
        tap.push(&[0.25; 10], &[0.5; 10]);
        drop(tap);
        assert_eq!(recorder.close(), None);
        assert_eq!(hound::WavReader::open(&path).unwrap().len(), 20);
    }

    /// A writer that keeps what it is given in memory and fails once it
    /// holds `limit` bytes, as a full disk does.
    struct FullDisk {
        bytes: Arc<Mutex<std::io::Cursor<Vec<u8>>>>,
        limit: u64,
    }

    impl Write for FullDisk {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut bytes = self.bytes.lock().unwrap();
            if bytes.position() + buf.len() as u64 > self.limit {
                return Err(std::io::Error::new(std::io::ErrorKind::StorageFull, "the disk is full"));
            }
            bytes.write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Seek for FullDisk {
        fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
            self.bytes.lock().unwrap().seek(to)
        }
    }

    /// A write error stops the recording and is reported by
    /// [`Recorder::error`] and [`Recorder::close`]. The samples pushed
    /// before it are written (#237).
    #[test]
    fn a_failing_writer_reports_its_error() {
        let bytes = Arc::new(Mutex::new(std::io::Cursor::new(Vec::new())));
        // Room for the header and a few hundred samples.
        let limit = 4096;
        let disk = FullDisk {
            bytes: bytes.clone(),
            limit,
        };
        let recorder = Recorder::start_with(disk, 48_000).unwrap();
        assert_eq!(recorder.error(), None);
        let mut tap = recorder.tap();
        let left: Vec<f32> = (0..2000).map(|i| i as f32).collect();
        let right: Vec<f32> = left.iter().map(|x| -x).collect();
        tap.push(&left, &right);
        drop(tap);

        let waited = Instant::now();
        while recorder.error().is_none() {
            assert!(waited.elapsed() < Duration::from_secs(10), "the writer failed");
            std::thread::sleep(Duration::from_millis(1));
        }
        let error = recorder.close().expect("close reports the error");
        assert!(error.contains("the disk is full"), "{error}");

        // What reached the disk before the error is the start of the stream.
        let bytes = bytes.lock().unwrap().get_ref().clone();
        assert!(bytes.len() as u64 <= limit);
        let data = bytes.windows(4).position(|w| w == b"data").unwrap() + 8;
        let written: Vec<f32> = bytes[data..]
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        assert!(written.len() > 100, "{} samples written", written.len());
        let pushed = left.iter().zip(&right).flat_map(|(l, r)| [*l, *r]);
        assert!(written.iter().copied().eq(pushed.take(written.len())));
    }

    /// A full ring drops frames and counts them.
    #[test]
    fn a_full_ring_drops_and_counts() {
        let dir = TestDir::new("record-full");
        // 10 Hz: the ring holds 2 seconds, 20 frames.
        let recorder = Recorder::start(&dir.0.join("out.wav"), 10).unwrap();
        let mut tap = recorder.tap();
        // Hold the writer back by pushing faster than it drains.
        tap.push(&[0.0; 100], &[0.0; 100]);
        assert!(recorder.dropped() > 0);
        drop(tap);
        drop(recorder);
    }
}

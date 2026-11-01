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
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
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

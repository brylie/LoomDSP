use super::*;
use crate::record::TestDir;

/// The sink writes what a fake input delivers, counts its frames and
/// errors, and reports them when it finishes.
#[test]
fn the_sink_writes_and_counts_what_the_input_delivers() {
    let dir = TestDir::new("capture-sink");
    let path = dir.0.join("capture.wav");
    let sink = Sink::start(&path, 48_000).unwrap();
    let mut input = sink.input(2);
    let mut error = sink.error();
    input(&[0.1, 0.2, 0.3, 0.4]);
    input(&[0.5, 0.6]);
    error();
    drop(input);
    let report = sink.finish().unwrap();
    assert_eq!(
        report,
        CaptureReport {
            frames: 3,
            dropped: 0,
            stream_errors: 1
        }
    );

    let mut reader = hound::WavReader::open(&path).unwrap();
    assert_eq!(reader.spec().sample_rate, 48_000);
    let samples: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).collect();
    assert_eq!(samples, [0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);
}

/// Frames that do not fit the ring are reported as dropped, and still
/// counted as delivered.
#[test]
fn the_sink_reports_dropped_frames() {
    let dir = TestDir::new("capture-sink-full");
    // 10 Hz: the ring holds 2 seconds, 20 frames.
    let sink = Sink::start(&dir.0.join("capture.wav"), 10).unwrap();
    let mut input = sink.input(1);
    input(&[0.0; 100]);
    drop(input);
    let report = sink.finish().unwrap();
    assert_eq!(report.frames, 100);
    assert!(report.dropped > 0, "{report:?}");
}

/// A file that cannot be created is an error before any device opens.
#[test]
fn an_unwritable_file_is_an_error() {
    let dir = TestDir::new("capture-sink-bad");
    let path = dir.0.join("no such folder").join("capture.wav");
    assert!(matches!(Sink::start(&path, 48_000), Err(HostError::Record(_))));
}

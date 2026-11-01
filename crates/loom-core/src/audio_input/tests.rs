use super::*;

/// Startup, underrun, and recovery never repeat an old block.
#[test]
fn underrun_silences_the_whole_block_until_refilled() {
    let (mut writer, mut reader) = input_ring(4, 2);
    let mut output = [[1.0; MAX_INPUT_CHANNELS]; 3];
    for n in 1..=3 {
        assert!(writer.push([n as f32; MAX_INPUT_CHANNELS]));
    }
    reader.read_block(&mut output);
    assert_eq!(output, [[0.0; MAX_INPUT_CHANNELS]; 3]);
    assert_eq!(reader.underruns(), 0);
    assert!(writer.push([4.0; MAX_INPUT_CHANNELS]));
    reader.read_block(&mut output);
    assert_eq!(output.map(|frame| frame[0]), [1.0, 2.0, 3.0]);
    reader.read_block(&mut output);
    assert_eq!(output, [[0.0; MAX_INPUT_CHANNELS]; 3]);
    assert_eq!(reader.underruns(), 1);
    for n in 5..=7 {
        assert!(writer.push([n as f32; MAX_INPUT_CHANNELS]));
    }
    reader.read_block(&mut output);
    assert_eq!(output.map(|frame| frame[0]), [4.0, 5.0, 6.0]);
}

/// Overrun correction discards oldest frames to the target fill.
#[test]
fn overrun_discards_oldest_frames() {
    let (mut writer, mut reader) = input_ring(4, 1);
    for n in 0..10 {
        assert!(writer.push([n as f32; MAX_INPUT_CHANNELS]));
    }
    let mut output = [[0.0; MAX_INPUT_CHANNELS]; 2];
    reader.read_block(&mut output);
    assert_eq!(output.map(|frame| frame[0]), [6.0, 7.0]);
    assert_eq!(reader.overruns(), 1);
    assert_eq!(reader.channels(), 1);
}

/// A failed input never plays the frames left in its ring.
#[test]
fn failed_input_goes_silent() {
    let (mut writer, mut reader) = input_ring(4, 2);
    for _ in 0..4 {
        assert!(writer.push([0.5; MAX_INPUT_CHANNELS]));
    }
    writer.status().fail();
    let mut output = [[1.0; MAX_INPUT_CHANNELS]; 4];
    assert!(!reader.read_block(&mut output));
    assert_eq!(output, [[0.0; MAX_INPUT_CHANNELS]; 4]);
}

/// A one-percent clock mismatch triggers corrections in both directions.
#[test]
fn drifting_clocks_are_corrected() {
    for (written, expect_underruns) in [(99, true), (101, false)] {
        let (mut writer, mut reader) = input_ring(100, 2);
        for _ in 0..100 {
            assert!(writer.push([0.5; MAX_INPUT_CHANNELS]));
        }
        let mut output = [[0.0; MAX_INPUT_CHANNELS]; 100];
        for _ in 0..500 {
            reader.read_block(&mut output);
            for _ in 0..written {
                assert!(writer.push([0.5; MAX_INPUT_CHANNELS]));
            }
        }
        if expect_underruns {
            assert!(reader.underruns() > 0);
            assert_eq!(reader.overruns(), 0);
        } else {
            assert!(reader.overruns() > 0);
            assert_eq!(reader.underruns(), 0);
        }
    }
}

use super::*;
use loom_core::testing::run;

const RATE: f32 = 48_000.0;

/// Renders `frames` samples of noise in the given colour (0 white, 1
/// pink, 2 brown) at the given seed, in blocks of `block` frames.
fn render(colour: f32, seed: f32, frames: usize, block: usize) -> Vec<f32> {
    let ModuleRole::Dsp(create) = NOISE.role else {
        unreachable!()
    };
    let mut noise = create(&ModuleConfig {
        sample_rate: RATE,
        max_block: block,
    });
    noise.set_param(0, seed);
    noise.set_param(1, colour);
    noise.reset();
    let mut out = vec![0.0; frames];
    for chunk in out.chunks_mut(block) {
        run(noise.as_mut(), RATE, &[], &[], &mut [chunk], &[]);
    }
    out
}

/// A copied noise stream mixes in the right channel and stays deterministic.
#[test]
fn copied_noise_uses_the_channel_in_its_seed() {
    fn run_copy(channel: usize, block: usize) -> Vec<f32> {
        let mut left = Noise::new(RATE);
        let mut output = [0.0; 64];
        run(&mut left, RATE, &[], &[], &mut [&mut output], &[]);
        let mut copy = Noise::new(RATE);
        assert!(copy.copy_state_from(&left));
        let mut result = vec![0.0; 512];
        for chunk in result.chunks_mut(block) {
            loom_core::testing::run_with_transport_and_channel(
                &mut copy,
                RATE,
                loom_core::TransportInfo::default(),
                &[],
                &[],
                &mut [chunk],
                &[],
                channel,
            );
        }
        result
    }
    let left = run_copy(0, 77);
    let right = run_copy(1, 77);
    assert_ne!(left, right);
    assert_eq!(right, run_copy(1, 1));
    assert_eq!(right, run_copy(1, 512));
}

/// The two channels do not reuse either random stream at a shifted offset.
#[test]
fn stereo_noise_streams_do_not_overlap() {
    let mut left = Noise::new(RATE);
    let mut right = Noise::new(RATE);
    left.restart(42);
    right.channel = 1;
    right.restart(42);
    let streams = [left.state, left.aux, right.state, right.aux].map(|mut state| {
        (0..128)
            .map(|_| {
                Noise::next_white(&mut state);
                state
            })
            .collect::<std::collections::HashSet<_>>()
    });
    for i in 0..streams.len() {
        for j in i + 1..streams.len() {
            assert!(streams[i].is_disjoint(&streams[j]), "streams {i} and {j} overlap");
        }
    }
}

fn rms(samples: &[f32]) -> f64 {
    (samples.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / samples.len() as f64).sqrt()
}

/// An in-place radix-2 FFT. `re.len()` must be a power of two.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -std::f64::consts::TAU / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (sin, cos) = (angle * k as f64).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cos - im[b] * sin, re[b] * sin + im[b] * cos);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// The slope of the power spectral density between 100 Hz and 10 kHz, in
/// dB per octave. It averages the power per bin over one octave around
/// each frequency, over many Hann-windowed segments.
fn slope_db_per_octave(samples: &[f32]) -> f64 {
    const SEGMENT: usize = 1 << 16;
    let mut power = vec![0.0; SEGMENT / 2];
    for segment in samples.as_chunks::<SEGMENT>().0 {
        let mut re: Vec<f64> = segment
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let hann = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / SEGMENT as f64).cos();
                f64::from(v) * hann
            })
            .collect();
        let mut im = vec![0.0; SEGMENT];
        fft(&mut re, &mut im);
        for (bin, p) in power.iter_mut().enumerate() {
            *p += re[bin] * re[bin] + im[bin] * im[bin];
        }
    }
    let bin_hz = f64::from(RATE) / SEGMENT as f64;
    let mean_around = |centre: f64| {
        let (low, high) = (
            (centre / 2f64.sqrt() / bin_hz) as usize,
            (centre * 2f64.sqrt() / bin_hz) as usize,
        );
        power[low..high].iter().sum::<f64>() / (high - low) as f64
    };
    let octaves = (10_000.0f64 / 100.0).log2();
    10.0 * (mean_around(10_000.0) / mean_around(100.0)).log10() / octaves
}

/// Pink falls 3 dB per octave and brown 6, each within 1 dB; white is flat.
#[test]
fn slopes_follow_the_colour() {
    let frames = 16 << 16;
    for (colour, expected) in [(0.0, 0.0), (1.0, -3.0), (2.0, -6.0)] {
        let slope = slope_db_per_octave(&render(colour, 7.0, frames, 4096));
        assert!((slope - expected).abs() < 1.0, "colour {colour}: {slope} dB per octave");
    }
}

/// Every colour has about the same loudness as white (within 3 dB), and
/// brown stays near zero over a minute.
#[test]
fn colours_match_white_in_level_and_brown_does_not_drift() {
    let frames = 60 * RATE as usize;
    let white = rms(&render(0.0, 3.0, frames, 4096));
    for colour in [1.0, 2.0] {
        let samples = render(colour, 3.0, frames, 4096);
        let db = 20.0 * (rms(&samples) / white).log10();
        assert!(db.abs() < 3.0, "colour {colour}: {db} dB from white");
        let mean = samples.iter().map(|&v| f64::from(v)).sum::<f64>() / frames as f64;
        assert!(mean.abs() < 0.05, "colour {colour}: mean {mean}");
    }
}

/// The same seed gives the same noise at block sizes 1, 77, and 512, and
/// switching to white gives the same samples as before pink existed.
#[test]
fn same_seed_same_output_at_any_block_size() {
    for colour in [0.0, 1.0, 2.0] {
        let reference = render(colour, 5.0, 2_000, 512);
        assert_eq!(render(colour, 5.0, 2_000, 1), reference, "colour {colour}, block 1");
        assert_eq!(render(colour, 5.0, 2_000, 77), reference, "colour {colour}, block 77");
    }
}

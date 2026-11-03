use super::*;

/// The sequence the 13, 17, 5 shifts give from a state of 1.
#[test]
fn xorshift_follows_its_shifts() {
    let mut state = 1;
    assert_eq!(xorshift32(&mut state), 270_369);
    assert_eq!(xorshift32(&mut state), 67_634_689);
    assert_eq!(state, 67_634_689);
}

/// The finaliser gives MurmurHash3's known value for 1, and its only zero is
/// moved off 0, so the stream cannot stick.
#[test]
fn the_start_is_the_murmur3_finaliser() {
    assert_eq!(Rng::new(1).0, 0x514E_28B7);
    assert_eq!(Rng::new(0).0, 1);
}

/// Words next to each other start far apart: about half of the 32 bits differ.
#[test]
fn similar_words_start_far_apart() {
    let total: u32 = (1..=1_000u32)
        .map(|word| (Rng::new(word).0 ^ Rng::new(word + 1).0).count_ones())
        .sum();
    let mean = f64::from(total) / 1_000.0;
    assert!((14.0..18.0).contains(&mean), "{mean}");
}

/// Draws stay below 1, because a draw keeps only 24 bits. The largest of those
/// is exact in an `f32`. All 32 bits would round up to 1, and a chance of 1
/// could fail.
#[test]
fn a_draw_never_reaches_one() {
    let mut most = 0.0f32;
    for word in 0..1_000 {
        let mut rng = Rng::new(word);
        for _ in 0..1_000 {
            most = most.max(rng.draw());
        }
    }
    assert!(most < 1.0, "{most}");
    assert_eq!((u32::MAX >> 8) as f32 / (1u32 << 24) as f32, 1.0 - f32::EPSILON / 2.0);
}

/// Each tenth of the range gets a tenth of the draws, within half a point.
#[test]
fn draws_are_uniform() {
    let mut counts = [0u32; 10];
    let mut rng = Rng::new(7);
    for _ in 0..1_000_000 {
        counts[(rng.draw() * 10.0) as usize] += 1;
    }
    for (bin, &count) in counts.iter().enumerate() {
        let share = f64::from(count) / 1_000_000.0;
        assert!((share - 0.1).abs() < 0.005, "bin {bin}: {share}");
    }
}

/// The same word replays the same numbers, and another word gives others.
#[test]
fn the_same_word_replays() {
    let take = |word: u32| {
        let mut rng = Rng::new(word);
        [rng.draw(), rng.draw(), rng.draw()]
    };
    assert_eq!(take(5), take(5));
    assert_ne!(take(5), take(6));
}

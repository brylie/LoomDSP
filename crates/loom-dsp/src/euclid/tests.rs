use super::*;

/// The pattern as text: `x` for a hit.
fn shown(steps: u32, hits: u32, rotation: u32) -> String {
    let pattern = Pattern::new(steps, hits, rotation);
    (0..steps)
        .map(|step| if pattern.is_hit(step) { 'x' } else { '.' })
        .collect()
}

/// The gaps between hits, going round the pattern once.
fn gaps(text: &str) -> Vec<usize> {
    let hits: Vec<usize> = text.match_indices('x').map(|(index, _)| index).collect();
    let mut gaps: Vec<usize> = hits.windows(2).map(|pair| pair[1] - pair[0]).collect();
    gaps.push(hits[0] + text.len() - hits[hits.len() - 1]);
    gaps
}

/// True when `a` is `b` turned round by some number of places.
fn same_up_to_rotation(a: &str, b: &str) -> bool {
    a.len() == b.len() && format!("{a}{a}").contains(b)
}

/// 3 in 8 is the fixed form the spec gives.
#[test]
fn three_in_eight_is_exact() {
    assert_eq!(shown(8, 3, 0), "x..x..x.");
}

/// 5 in 8 and 5 in 16 match Toussaint's published patterns, up to rotation.
#[test]
fn published_patterns_match_up_to_rotation() {
    assert!(same_up_to_rotation(&shown(8, 5, 0), "x.xx.xx."), "{}", shown(8, 5, 0));
    assert!(
        same_up_to_rotation(&shown(16, 5, 0), "x..x..x..x..x..."),
        "{}",
        shown(16, 5, 0)
    );
}

/// The defining property: the gaps between hits never differ by more than one,
/// for every pattern up to the module's largest.
#[test]
fn hits_are_spread_as_evenly_as_possible() {
    for steps in 1..=32 {
        for hits in 1..=steps {
            let text = shown(steps, hits, 0);
            assert_eq!(text.matches('x').count(), hits as usize, "{hits} in {steps}");
            let gaps = gaps(&text);
            let (least, most) = (gaps.iter().min().unwrap(), gaps.iter().max().unwrap());
            assert!(most - least <= 1, "{hits} in {steps}: {text}");
        }
    }
}

/// Rotation moves the pattern later.
#[test]
fn rotation_moves_the_pattern_later() {
    assert_eq!(shown(8, 3, 1), ".x..x..x");
    assert_eq!(shown(8, 3, 2), "x.x..x..");
}

/// A rotation at or above the steps goes round.
#[test]
fn a_large_rotation_goes_round() {
    assert_eq!(shown(8, 3, 9), shown(8, 3, 1));
    assert_eq!(shown(4, 1, 31), shown(4, 1, 3));
}

/// Hits above the steps count as the steps, and no hits is silence.
#[test]
fn hits_are_limited_to_the_steps() {
    assert_eq!(shown(4, 9, 0), "xxxx");
    assert_eq!(shown(4, 0, 0), "....");
}

/// The first hit moves with the rotation, and a silent pattern has none.
#[test]
fn the_first_hit_follows_the_rotation() {
    let first = |pattern: Pattern, steps: u32| {
        (0..steps)
            .filter(|&step| pattern.is_first_hit(step))
            .collect::<Vec<_>>()
    };
    assert_eq!(first(Pattern::new(8, 3, 0), 8), [0]);
    assert_eq!(first(Pattern::new(8, 3, 2), 8), [2]);
    assert!(first(Pattern::new(8, 0, 0), 8).is_empty());
    assert!(Pattern::new(8, 3, 2).is_hit(2));
}

/// A step past the end goes round the pattern.
#[test]
fn a_step_goes_round() {
    let pattern = Pattern::new(8, 3, 0);
    assert_eq!(pattern.is_hit(8), pattern.is_hit(0));
    assert_eq!(pattern.is_hit(11), pattern.is_hit(3));
}

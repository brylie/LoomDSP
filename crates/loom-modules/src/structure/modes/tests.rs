use super::*;
use loom_core::ModuleConfig;
use loom_core::testing::run;

const RATE: f32 = 48_000.0;
/// Samples of a select fade at [`RATE`].
const FADE: usize = 480;

/// A new module of `descriptor` at 48 kHz.
fn create(descriptor: &ModuleDescriptor) -> Box<dyn Module> {
    let ModuleRole::Dsp(create) = descriptor.role else {
        unreachable!()
    };
    create(&ModuleConfig {
        sample_rate: RATE,
        max_block: 4_800,
    })
}

/// Sixteen constant inputs: input `k` holds `k + 1`.
fn numbered(frames: usize) -> Vec<Vec<f32>> {
    (0..MOST_MEMBERS).map(|k| vec![(k + 1) as f32; frames]).collect()
}

/// The modules' kinds are the ones `loom-patch` adds when it flattens a
/// mode, and a select fade is 10 ms.
#[test]
fn the_kinds_match_the_patch_crate() {
    assert_eq!(SUM.kind(), loom_patch::modes::SUM_KIND);
    assert_eq!(SPREAD_SUM.kind(), loom_patch::modes::SPREAD_SUM_KIND);
    assert_eq!(MEMBER.kind(), loom_patch::modes::MEMBER_KIND);
    assert_eq!(SELECT.kind(), loom_patch::modes::SELECT_KIND);
    assert_eq!(MOST_MEMBERS, loom_patch::modes::MOST_MEMBERS);
    assert_eq!((MODE_FADE_SECONDS * RATE).round() as usize, FADE);
}

/// Sum adds every input and scales by its gain, set before the first frame
/// with no glide.
#[test]
fn sum_adds_its_inputs_times_the_gain() {
    let mut sum = create(&SUM);
    sum.set_param(Sum::GAIN, 0.5);
    sum.reset();
    let frames = 8;
    let inputs = numbered(frames);
    let refs: Vec<&[f32]> = inputs.iter().map(Vec::as_slice).collect();
    let mut out = vec![0.0; frames];
    run(sum.as_mut(), RATE, &refs, &[true; MOST_MEMBERS], &mut [&mut out], &[]);
    // 1 + 2 + ... + 16 = 136.
    assert!(out.iter().all(|v| *v == 68.0), "{out:?}");
}

/// A gain change glides, so a change of Copies does not step the level.
#[test]
fn a_sum_gain_change_glides() {
    let mut sum = create(&SUM);
    sum.reset();
    sum.set_param(Sum::GAIN, 0.5);
    assert!(sum.param_glide(Sum::GAIN).is_some());
    let frames = 960;
    let mut inputs = vec![vec![0.0; frames]; MOST_MEMBERS];
    inputs[0] = vec![1.0; frames];
    let refs: Vec<&[f32]> = inputs.iter().map(Vec::as_slice).collect();
    let mut out = vec![0.0; frames];
    run(sum.as_mut(), RATE, &refs, &[true; MOST_MEMBERS], &mut [&mut out], &[]);
    assert!(out[0] < 1.0 && out[0] > 0.9, "{}", out[0]);
    assert!((out[frames - 1] - 0.5).abs() < 1e-3);
    let largest = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    assert!(largest < 0.01, "{largest}");
}

/// The stereo spread sum gives copy k the pan of its offset times 1/√N;
/// inputs past the last copy are silent, and one copy sits in the centre.
#[test]
fn the_spread_sum_pans_each_copy_by_its_offset() {
    let mut sum = SpreadSum::new(RATE, 4);
    sum.set_param(SpreadSum::COPIES, 4.0);
    sum.reset();
    for k in 0..4 {
        let (left, right) = blend::pan(blend::spread_offset(k, 4));
        assert!((sum.left[k as usize].target() - 0.5 * left).abs() < 1e-7);
        assert!((sum.right[k as usize].target() - 0.5 * right).abs() < 1e-7);
    }
    assert!(sum.left[4..].iter().chain(&sum.right[4..]).all(|s| s.target() == 0.0));
    // Copy 0 is hard left, copy 3 hard right.
    assert!(sum.right[0].target().abs() < 1e-7 && sum.left[3].target().abs() < 1e-7);
    let one = SpreadSum::new(RATE, 1);
    let centre = core::f32::consts::FRAC_1_SQRT_2;
    assert!((one.left[0].target() - centre).abs() < 1e-7);
    assert!((one.right[0].target() - centre).abs() < 1e-7);
}

/// Member outputs its value, modulation included.
#[test]
fn member_outputs_its_value() {
    let mut member = create(&MEMBER);
    member.set_param(MemberValue::MEMBER, 3.0);
    let mut out = vec![0.0; 4];
    run(member.as_mut(), RATE, &[], &[], &mut [&mut out], &[]);
    assert_eq!(out, vec![3.0; 4]);
}

/// A Select with `members` members and `fade` (0 off, 1 linear, 2 equal
/// power), run over `member`.
fn select_law(members: usize, fade: f32, member: &[f32]) -> Vec<f32> {
    let mut select = create(&SELECT);
    select.set_param(Select::MEMBERS, members as f32);
    select.set_param(Select::FADE, fade);
    select.reset();
    let frames = member.len();
    let mut inputs = numbered(frames);
    inputs.push(member.to_vec());
    let refs: Vec<&[f32]> = inputs.iter().map(Vec::as_slice).collect();
    let mut out = vec![0.0; frames];
    run(
        select.as_mut(),
        RATE,
        &refs,
        &[true; MOST_MEMBERS + 1],
        &mut [&mut out],
        &[],
    );
    out
}

/// A Select with `members` members, equal power or no fade, run over
/// `member`.
fn select(members: usize, fade: bool, member: &[f32]) -> Vec<f32> {
    select_law(members, if fade { 2.0 } else { 0.0 }, member)
}

/// A linear Select fades member 3 to member 1 with gains that sum to 1.
#[test]
fn a_linear_select_fades_with_gains_that_sum_to_one() {
    let mut member = vec![3.0; 2_000];
    member[1_000..].fill(1.0);
    let out = select_law(4, 1.0, &member);
    for n in 0..FADE {
        let (from, to) = blend::linear(n as f32 / FADE as f32);
        assert!((out[1_000 + n] - (3.0 * from + to)).abs() < 1e-5, "frame {n}");
    }
}

/// A new Select starts on its member with no fade, and a change fades
/// equal power over 10 ms, then plays the new member alone.
#[test]
fn select_starts_on_its_member_and_fades_over_10_ms() {
    let mut member = vec![3.0; 2_000];
    member[1_000..].fill(1.0);
    let out = select(4, true, &member);
    assert!(out[..1_000].iter().all(|v| *v == 3.0), "starts on member 3");
    for n in 0..FADE {
        let (from, to) = blend::equal_power(n as f32 / FADE as f32);
        let want = 3.0 * from + 1.0 * to;
        assert!((out[1_000 + n] - want).abs() < 1e-5, "frame {n}");
    }
    assert!(out[1_000 + FADE..].iter().all(|v| *v == 1.0));
}

/// A Member between two whole values takes the nearest, and one past the
/// member count is held to the last.
#[test]
fn select_rounds_and_clamps_member() {
    assert_eq!(select(4, true, &[2.4; 4]), vec![2.0; 4]);
    assert_eq!(select(4, true, &[2.6; 4]), vec![3.0; 4]);
    assert_eq!(select(4, true, &[9.0; 4]), vec![4.0; 4]);
    assert_eq!(select(4, true, &[-3.0; 4]), vec![1.0; 4]);
    assert_eq!(select(4, true, &[f32::NAN; 4]), vec![1.0; 4]);
}

/// With fade off, a change switches on the frame.
#[test]
fn select_without_fade_switches_on_the_frame() {
    let mut member = vec![1.0; 8];
    member[4..].fill(2.0);
    assert_eq!(select(2, false, &member), vec![1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0]);
}

/// The output is the same at block sizes 1, 77, and 512, through a fade, a
/// reversal, and a wait.
#[test]
fn select_is_the_same_at_any_block_size() {
    let frames = 4_000;
    let member: Vec<f32> = (0..frames)
        .map(|i| match i {
            0..500 => 1.0,
            500..700 => 2.0,
            700..1_500 => 1.0,
            1_500..1_600 => 3.0,
            _ => 4.0,
        })
        .collect();
    let whole = select(4, true, &member);
    for block in [1, 77, 512] {
        let mut module = create(&SELECT);
        module.set_param(Select::MEMBERS, 4.0);
        module.reset();
        let mut out = vec![0.0; frames];
        let inputs = numbered(frames);
        for start in (0..frames).step_by(block) {
            let end = (start + block).min(frames);
            let mut refs: Vec<&[f32]> = inputs.iter().map(|v| &v[start..end]).collect();
            refs.push(&member[start..end]);
            run(
                module.as_mut(),
                RATE,
                &refs,
                &[true; MOST_MEMBERS + 1],
                &mut [&mut out[start..end]],
                &[],
            );
        }
        assert_eq!(out, whole, "block {block}");
    }
}

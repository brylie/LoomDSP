use super::*;

/// sRGB channel to linear light.
fn linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear light to an sRGB channel, clamped to 0 to 1.
fn encode(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// A colour as seen with a colour-vision deficiency, by the matrices of
/// Machado, Oliveira, and Fernandes (IEEE TVCG 2009) at severity 1,
/// applied in linear light. `None` is normal vision.
fn simulate(colour: Color, matrix: Option<[[f32; 3]; 3]>) -> Color {
    let Some(m) = matrix else { return colour };
    let rgb = [linear(colour.r), linear(colour.g), linear(colour.b)];
    let row = |r: [f32; 3]| encode(r[0] * rgb[0] + r[1] * rgb[1] + r[2] * rgb[2]);
    Color::from_rgb(row(m[0]), row(m[1]), row(m[2]))
}

/// CIE L*a*b* of an sRGB colour, D65 white.
fn lab(colour: Color) -> [f32; 3] {
    let (r, g, b) = (linear(colour.r), linear(colour.g), linear(colour.b));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.950_47;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.088_83;
    let f = |t: f32| {
        if t > 0.008_856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    [116.0 * f(y) - 16.0, 500.0 * (f(x) - f(y)), 200.0 * (f(y) - f(z))]
}

/// CIE76 colour difference.
fn delta_e(a: Color, b: Color) -> f32 {
    let (a, b) = (lab(a), lab(b));
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Normal vision, then protanopia, deuteranopia, and tritanopia.
const VISION: [(&str, Option<[[f32; 3]; 3]>); 4] = [
    ("normal", None),
    (
        "protanopia",
        Some([
            [0.152_286, 1.052_583, -0.204_868],
            [0.114_503, 0.786_281, 0.099_216],
            [-0.003_882, -0.048_116, 1.051_998],
        ]),
    ),
    (
        "deuteranopia",
        Some([
            [0.367_322, 0.860_646, -0.227_968],
            [0.280_085, 0.672_501, 0.047_413],
            [-0.011_820, 0.042_940, 0.968_881],
        ]),
    ),
    (
        "tritanopia",
        Some([
            [1.255_528, -0.076_749, -0.178_779],
            [-0.078_411, 0.930_809, 0.147_602],
            [0.004_733, 0.691_367, 0.303_900],
        ]),
    ),
];

/// Family colours take light text and tints dark text, each at 4.5:1
/// or more (visual-system.md, Module families).
#[test]
fn swatches_meet_text_contrast() {
    let p = Palette::default();
    let swatches = p.swatches();
    for (i, colour) in swatches.iter().enumerate() {
        let text = if i < 8 { p.text } else { p.background };
        let ratio = contrast_ratio(text, *colour);
        assert!(ratio >= MIN_TEXT_CONTRAST, "swatch {i}: {ratio:.2}:1");
    }
}

/// Every pair of family colours stays at least ΔE 14 apart in normal
/// vision and three simulated colour-vision deficiencies, and no swatch
/// is within ΔE 15 of a reserved colour (ADR-0029).
#[test]
fn family_colours_stay_apart_for_colour_blind_viewers() {
    let p = Palette::default();
    for (name, matrix) in VISION {
        for (i, a) in p.families.iter().enumerate() {
            for (j, b) in p.families.iter().enumerate().skip(i + 1) {
                let d = delta_e(simulate(*a, matrix), simulate(*b, matrix));
                assert!(d >= 14.0, "{name}: families {i} and {j} are ΔE {d:.1} apart");
            }
        }
    }
    for (i, swatch) in p.swatches().iter().enumerate() {
        for reserved in p.reserved() {
            let d = delta_e(*swatch, reserved);
            assert!(d >= 15.0, "swatch {i} is ΔE {d:.1} from a reserved colour");
        }
    }
}

/// The colour difference matches a reference value: pure black to pure
/// white is ΔE 100 in CIE76.
#[test]
fn delta_e_matches_a_reference_value() {
    assert!((delta_e(Color::BLACK, Color::WHITE) - 100.0).abs() < 0.1);
}

/// Black on white is 21:1, a colour on itself is 1:1, and the order does
/// not matter. #767676 on white is the well-known 4.54:1 grey.
#[test]
fn contrast_ratio_matches_reference_values() {
    assert!((contrast_ratio(Color::BLACK, Color::WHITE) - 21.0).abs() < 1e-3);
    assert!((contrast_ratio(Color::WHITE, Color::BLACK) - 21.0).abs() < 1e-3);
    assert!((contrast_ratio(Color::WHITE, Color::WHITE) - 1.0).abs() < 1e-6);
    let grey = Color::from_rgb8(0x76, 0x76, 0x76);
    assert!((contrast_ratio(grey, Color::WHITE) - 4.54).abs() < 0.01);
}

/// Every text colour the canvas draws meets 4.5:1 on the fill under it.
#[test]
fn canvas_text_meets_wcag_minimum_contrast() {
    let p = Palette::default();
    let pairs = [
        ("title text on tile", p.text, p.tile),
        ("port label text on tile", p.text_dim, p.tile),
        ("option and value text on tile", p.text, p.tile),
        ("badge number on badge fill", p.background, p.modulation),
    ];
    for (name, text, fill) in pairs {
        let ratio = contrast_ratio(text, fill);
        assert!(
            ratio >= MIN_TEXT_CONTRAST,
            "{name}: {ratio:.2}:1 is below {MIN_TEXT_CONTRAST}:1"
        );
    }
}

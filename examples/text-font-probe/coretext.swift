// Measures the probe's six labels with macOS's own text stack, CoreText,
// in the system interface font, as a native macOS app would set them.
// It answers which candidate's widths match the platform: the research
// note (docs/research/2026-11-text-crate-choice.md) compares them.
//
// Usage: swift coretext.swift
//
// The font is AppKit's system font (`NSFont.systemFont`), which is what a
// native control uses. Asking CoreText for `.SFNS-Regular` by name gives
// Times New Roman instead, so the script does not.

import AppKit
import CoreText

/// The labels and sizes of the Rust probe (src/main.rs).
let labels = ["Cutoff", "Resonance", "Oscillator", "800 Hz", "Envelope", "Attack 12 ms"]
let sizes: [CGFloat] = [9, 11, 13]

/// The typographic width of `text` set in `font` on one line.
func width(_ text: String, _ font: CTFont) -> Double {
    let attributes: [NSAttributedString.Key: Any] = [.font: font]
    let line = CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: attributes))
    return CTLineGetTypographicBounds(line, nil, nil, nil)
}

let version = ProcessInfo.processInfo.operatingSystemVersion
print("macOS \(version.majorVersion).\(version.minorVersion).\(version.patchVersion)")
print("path                 px  label             width  face")
for size in sizes {
    let system = NSFont.systemFont(ofSize: size) as CTFont
    for label in labels {
        for (path, font) in [("CoreText system font", system)] {
            let face = CTFontCopyPostScriptName(font) as String
            let measured = String(format: "%8.3f", width(label, font))
            let row = path.padding(toLength: 20, withPad: " ", startingAt: 0)
            let text = label.padding(toLength: 14, withPad: " ", startingAt: 0)
            print("\(row) \(String(format: "%4.0f", Double(size)))  \(text) \(measured)  \(face)")
        }
    }
}

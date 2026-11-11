// Prints what the macOS accessibility API reports for every slider in a
// running program's windows: the facts AccessKit's macOS adapter gives
// macOS, with no screen reader involved.
//
// Usage: swift inspect.swift PID
//
// It reads another program's accessibility tree, so macOS asks for the
// Accessibility permission for the terminal (or other program) that runs
// it: System Settings, Privacy & Security, Accessibility. Grant it
// yourself. Without it the script says so and exits with status 2.

import ApplicationServices
import Foundation

/// The attributes printed for each slider, in order.
let names = [
    "AXRole", "AXSubrole", "AXRoleDescription", "AXTitle", "AXDescription", "AXHelp",
    "AXValue", "AXValueDescription", "AXMinValue", "AXMaxValue",
]

/// What reading one attribute gave. A failed read is kept apart from an
/// absent attribute: "the element has no such attribute" is a finding, and
/// "the read did not work" is not.
enum Read {
    /// The attribute's value.
    case value(CFTypeRef)
    /// The element does not have the attribute, or it has no value.
    case absent
    /// The read failed for another reason, such as the other program not
    /// answering. The code is the `AXError` raw value.
    case failed(Int32)
}

/// Reads one attribute of an element.
func read(_ element: AXUIElement, _ name: String) -> Read {
    var value: CFTypeRef?
    let result = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    switch result {
    case .success:
        if let value = value { return .value(value) }
        return .absent
    case .attributeUnsupported, .noValue:
        return .absent
    default:
        return .failed(result.rawValue)
    }
}

/// How many reads failed while walking the tree. A failed read there can
/// hide a slider, so the count is reported at the end.
var failedReads = 0

/// The value of one attribute, or nil when the element does not have it
/// or the read failed. A failed read is counted and named on standard
/// error.
func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    switch read(element, name) {
    case .value(let value):
        return value
    case .absent:
        return nil
    case .failed(let code):
        failedReads += 1
        FileHandle.standardError.write("read of \(name) failed (AXError \(code))\n".data(using: .utf8)!)
        return nil
    }
}

/// What a read gave, as text.
func describe(_ read: Read) -> String {
    switch read {
    case .value(let value):
        return describe(value)
    case .absent:
        return "(absent)"
    case .failed(let code):
        return "(read failed, AXError \(code): not evidence that it is absent)"
    }
}

/// An attribute value as text, with its type, so a number and a string
/// that look alike can be told apart. The type is read from Core
/// Foundation, because these values do not all bridge to Swift types.
func describe(_ value: CFTypeRef) -> String {
    let type = CFGetTypeID(value)
    if type == CFStringGetTypeID() {
        let text = unsafeBitCast(value, to: CFString.self) as String
        return text.isEmpty ? "\"\" (empty string)" : "\"\(text)\" (string)"
    }
    if type == CFNumberGetTypeID() {
        return "\(unsafeBitCast(value, to: NSNumber.self)) (number)"
    }
    if type == CFBooleanGetTypeID() {
        return CFBooleanGetValue(unsafeBitCast(value, to: CFBoolean.self)) ? "true (boolean)" : "false (boolean)"
    }
    return (CFCopyTypeIDDescription(type) as String?) ?? "(another type)"
}

/// The string value of an attribute, or nil when it is absent or not a
/// string.
func string(_ element: AXUIElement, _ name: String) -> String? {
    guard let value = attribute(element, name), CFGetTypeID(value) == CFStringGetTypeID() else { return nil }
    return unsafeBitCast(value, to: CFString.self) as String
}

/// The elements in an attribute that holds a list of elements.
func elements(_ element: AXUIElement, _ name: String) -> [AXUIElement] {
    guard let value = attribute(element, name), CFGetTypeID(value) == CFArrayGetTypeID() else { return [] }
    let array = unsafeBitCast(value, to: CFArray.self)
    var found: [AXUIElement] = []
    for index in 0..<CFArrayGetCount(array) {
        guard let item = CFArrayGetValueAtIndex(array, index) else { continue }
        let object = Unmanaged<CFTypeRef>.fromOpaque(item).takeUnretainedValue()
        if CFGetTypeID(object) == AXUIElementGetTypeID() {
            found.append(unsafeBitCast(object, to: AXUIElement.self))
        }
    }
    return found
}

/// How deep the search goes. An accessibility tree can refer back to
/// itself, so the search must stop somewhere.
let maxDepth = 12

/// Prints every slider under `element`, depth first, down to `maxDepth`.
func visit(_ element: AXUIElement, _ found: inout Int, _ depth: Int = 0) {
    if depth > maxDepth { return }
    if showTree {
        let indent = String(repeating: "  ", count: depth)
        print("\(indent)\(string(element, "AXRole") ?? "(no role)") \(describe(read(element, "AXTitle")))")
    }
    if string(element, "AXRole") == "AXSlider" {
        found += 1
        print("slider \(found)")
        for name in names {
            print("  \(name.padding(toLength: 20, withPad: " ", startingAt: 0))\(describe(read(element, name)))")
        }
        var actions: CFArray?
        if AXUIElementCopyActionNames(element, &actions) == .success, let actions = actions {
            var listed: [String] = []
            for index in 0..<CFArrayGetCount(actions) {
                if let item = CFArrayGetValueAtIndex(actions, index) {
                    listed.append(Unmanaged<CFString>.fromOpaque(item).takeUnretainedValue() as String)
                }
            }
            print("  actions             \(listed.joined(separator: ", "))")
        }
        var settable: DarwinBoolean = false
        if AXUIElementIsAttributeSettable(element, "AXValue" as CFString, &settable) == .success {
            print("  AXValue settable    \(settable.boolValue)")
        }
    }
    for child in elements(element, "AXChildren") {
        visit(child, &found, depth + 1)
    }
}

let arguments = CommandLine.arguments.dropFirst().filter { $0 != "--tree" }
/// With `--tree`, every element's role and title is printed on the way
/// down, to see where the sliders should be.
let showTree = CommandLine.arguments.contains("--tree")
guard arguments.count == 1, let pid = Int32(arguments[arguments.startIndex]) else {
    FileHandle.standardError.write("usage: inspect [--tree] PID\n".data(using: .utf8)!)
    exit(64)
}
guard AXIsProcessTrusted() else {
    FileHandle.standardError.write(
        "This program is not allowed to read other programs' accessibility trees.\nGrant the Accessibility permission to the terminal that runs it, then run it again.\n"
            .data(using: .utf8)!)
    exit(2)
}
setvbuf(stdout, nil, _IONBF, 0)
let version = ProcessInfo.processInfo.operatingSystemVersion
print("macOS \(version.majorVersion).\(version.minorVersion).\(version.patchVersion)")
var found = 0
for window in elements(AXUIElementCreateApplication(pid), "AXWindows") {
    visit(window, &found)
}
if failedReads > 0 {
    print("\(failedReads) reads failed while walking the tree, so the list above may be incomplete. Run it again.")
}
if found == 0 {
    print("No slider found. Is the window open, and has assistive technology asked for its tree?")
}

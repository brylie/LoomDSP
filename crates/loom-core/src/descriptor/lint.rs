//! Checks that every port, parameter, and display value has a label and a description.

use super::ModuleDescriptor;

/// A port, parameter, or display value with an empty label or description.
///
/// Every built-in module is checked at compile time with
/// [`assert_documented`]. The `loom-test` harness fails on one, and the
/// registry loads the module but keeps a note ([`Registry::notes`](crate::Registry::notes)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Undocumented {
    /// Whether it is an input, an output, a parameter, or a display value.
    pub kind: SpecKind,
    /// Its stable name.
    pub name: &'static str,
    /// Which text is empty.
    pub missing: MissingText,
}

/// Whether a spec is an input, an output, a parameter, or a display value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecKind {
    /// An input port.
    Input,
    /// An output port.
    Output,
    /// A parameter.
    Parameter,
    /// A display value, for a light.
    Display,
}

impl SpecKind {
    /// "input", "output", "parameter", or "display value".
    pub const fn as_str(self) -> &'static str {
        match self {
            SpecKind::Input => "input",
            SpecKind::Output => "output",
            SpecKind::Parameter => "parameter",
            SpecKind::Display => "display value",
        }
    }
}

/// Which text of a port, parameter, or display value is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingText {
    /// The `label` field.
    Label,
    /// The `description` field.
    Description,
}

impl MissingText {
    /// "label" or "description".
    pub const fn as_str(self) -> &'static str {
        match self {
            MissingText::Label => "label",
            MissingText::Description => "description",
        }
    }
}

impl std::fmt::Display for Undocumented {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} `{}` has no {}",
            self.kind.as_str(),
            self.name,
            self.missing.as_str()
        )
    }
}

/// One text field of a port, parameter, or display value, for the checks.
struct Text {
    undocumented: Undocumented,
    value: &'static str,
}

impl ModuleDescriptor {
    /// The first port, parameter, or display value with an empty (or blank)
    /// label or description, in the order inputs, outputs, parameters,
    /// display values.
    pub const fn first_undocumented(&self) -> Option<Undocumented> {
        let mut index = 0;
        while let Some(text) = self.text(index) {
            if is_blank(text.value) {
                return Some(text.undocumented);
            }
            index += 1;
        }
        None
    }

    /// Every port, parameter, and display value with an empty (or blank)
    /// label or description, in the order inputs, outputs, parameters,
    /// display values.
    pub fn undocumented(&self) -> Vec<Undocumented> {
        (0..)
            .map_while(|index| self.text(index))
            .filter(|text| is_blank(text.value))
            .map(|text| text.undocumented)
            .collect()
    }

    /// Text field number `index`, counting the label and then the
    /// description of each input, output, parameter, and display value. One list for both
    /// checks above, so they cannot disagree.
    const fn text(&self, index: usize) -> Option<Text> {
        let (port, field) = (index / 2, index % 2);
        let (kind, name, label, description) = if port < self.inputs.len() {
            let spec = &self.inputs[port];
            (SpecKind::Input, spec.name, spec.label, spec.description)
        } else if port < self.inputs.len() + self.outputs.len() {
            let spec = &self.outputs[port - self.inputs.len()];
            (SpecKind::Output, spec.name, spec.label, spec.description)
        } else if port < self.inputs.len() + self.outputs.len() + self.params.len() {
            let spec = &self.params[port - self.inputs.len() - self.outputs.len()];
            (SpecKind::Parameter, spec.name, spec.label, spec.description)
        } else if port < self.inputs.len() + self.outputs.len() + self.params.len() + self.displays.len() {
            let spec = &self.displays[port - self.inputs.len() - self.outputs.len() - self.params.len()];
            (SpecKind::Display, spec.name, spec.label, spec.description)
        } else {
            return None;
        };
        let (missing, value) = if field == 0 {
            (MissingText::Label, label)
        } else {
            (MissingText::Description, description)
        };
        Some(Text {
            undocumented: Undocumented { kind, name, missing },
            value,
        })
    }
}

pub(super) const fn is_blank(text: &str) -> bool {
    text.trim_ascii().is_empty()
}

/// Panics if any port, parameter, or display value of `modules` has an
/// empty label or description. Call it in a `const` item, so a missing text is a compile
/// error:
///
/// ```
/// # static ALL: &[&loom_core::ModuleDescriptor] = &[];
/// const _: () = loom_core::assert_documented(ALL);
/// ```
pub const fn assert_documented(modules: &[&ModuleDescriptor]) {
    let mut i = 0;
    while i < modules.len() {
        let module = modules[i];
        if let Some(missing) = module.first_undocumented() {
            // A `const` cannot format, so the message is copied into a
            // fixed buffer by hand.
            let message = ConstMessage::new()
                .push(module.id)
                .push(": ")
                .push(missing.kind.as_str())
                .push(" `")
                .push(missing.name)
                .push("` has no ")
                .push(missing.missing.as_str())
                .push(". Every port, parameter, and display value needs a short label and a one-sentence description.");
            panic!("{}", message.as_str(module.id));
        }
        i += 1;
    }
}

/// A message built in a `const fn`, cut at 256 bytes.
struct ConstMessage {
    bytes: [u8; 256],
    len: usize,
}

impl ConstMessage {
    const fn new() -> Self {
        Self {
            bytes: [0; 256],
            len: 0,
        }
    }

    const fn push(mut self, text: &str) -> Self {
        let text = text.as_bytes();
        let mut i = 0;
        while i < text.len() && self.len < self.bytes.len() {
            self.bytes[self.len] = text[i];
            self.len += 1;
            i += 1;
        }
        self
    }

    /// The message, or `fallback` if the cut split a character.
    const fn as_str<'a>(&'a self, fallback: &'a str) -> &'a str {
        match std::str::from_utf8(self.bytes.split_at(self.len).0) {
            Ok(text) => text,
            Err(_) => fallback,
        }
    }
}

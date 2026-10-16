//! The set of module types available to patches.

use crate::descriptor::{ModuleDescriptor, ModuleRole, ParamMapping, PortDefault};
use crate::error::LoomError;
use std::collections::BTreeMap;

/// A parsed `"id@version"` module kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleKind {
    /// Module ID, for example `"loom.osc.sine"`.
    pub id: String,
    /// Version number.
    pub version: u32,
}

impl ModuleKind {
    /// Parses `"id@version"`. A missing version means version 1.
    pub fn parse(kind: &str) -> Result<Self, LoomError> {
        match kind.rsplit_once('@') {
            Some((id, version)) => {
                let version = version.parse().map_err(|_| LoomError::InvalidKind(kind.to_string()))?;
                if id.is_empty() {
                    return Err(LoomError::InvalidKind(kind.to_string()));
                }
                Ok(Self {
                    id: id.to_string(),
                    version,
                })
            }
            None if !kind.is_empty() => Ok(Self {
                id: kind.to_string(),
                version: 1,
            }),
            None => Err(LoomError::InvalidKind(kind.to_string())),
        }
    }
}

/// Module types by stable ID.
#[derive(Debug, Default)]
pub struct Registry {
    modules: BTreeMap<&'static str, &'static ModuleDescriptor>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a module type. Fails on duplicate IDs or invalid descriptors.
    pub fn register(&mut self, descriptor: &'static ModuleDescriptor) -> Result<(), LoomError> {
        let invalid = |reason: &str| LoomError::InvalidDescriptor {
            id: descriptor.id.to_string(),
            reason: reason.to_string(),
        };
        if descriptor.inputs.len() > 64 {
            return Err(invalid("more than 64 inputs"));
        }
        if descriptor
            .inputs
            .iter()
            .any(|port| matches!(port.default, PortDefault::Value(value) if !value.is_finite()))
        {
            return Err(invalid("an input default must be a finite number"));
        }
        for param in descriptor.params {
            if param.mapping == ParamMapping::Logarithmic {
                if param.integer {
                    return Err(invalid("a whole-number parameter cannot be logarithmic"));
                }
                if param.min.is_nan() || param.min <= 0.0 {
                    return Err(invalid("a logarithmic parameter needs a minimum above 0"));
                }
            }
        }
        descriptor.check_panel().map_err(|reason| invalid(&reason))?;
        match descriptor.role {
            ModuleRole::UnitDelay if descriptor.inputs.len() != 1 || descriptor.outputs.len() != 1 => {
                return Err(invalid("a unit delay needs exactly one input and one output"));
            }
            ModuleRole::AudioOutput if descriptor.inputs.len() != 2 || !descriptor.outputs.is_empty() => {
                return Err(invalid("an audio output needs two inputs and no outputs"));
            }
            _ => {}
        }
        if self.modules.insert(descriptor.id, descriptor).is_some() {
            return Err(invalid("duplicate module ID"));
        }
        Ok(())
    }

    /// Looks up a module by ID.
    pub fn get(&self, id: &str) -> Option<&'static ModuleDescriptor> {
        self.modules.get(id).copied()
    }

    /// Resolves an `"id@version"` kind string.
    ///
    /// A patch saved with an older version still resolves (migrations are
    /// the patch loader's job). A newer version than the registry knows is
    /// an error.
    pub fn resolve(&self, kind: &str) -> Result<&'static ModuleDescriptor, LoomError> {
        let parsed = ModuleKind::parse(kind)?;
        let descriptor = self
            .get(&parsed.id)
            .ok_or_else(|| LoomError::UnknownModuleKind(kind.to_string()))?;
        if parsed.version > descriptor.version {
            return Err(LoomError::UnsupportedModuleVersion {
                kind: kind.to_string(),
                supported: descriptor.version,
            });
        }
        Ok(descriptor)
    }

    /// All registered modules, sorted by ID.
    pub fn iter(&self) -> impl Iterator<Item = &'static ModuleDescriptor> + '_ {
        self.modules.values().copied()
    }

    /// Number of registered modules.
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// True if no modules are registered.
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kinds() {
        assert_eq!(
            ModuleKind::parse("loom.osc.sine@2").unwrap(),
            ModuleKind {
                id: "loom.osc.sine".into(),
                version: 2
            }
        );
        assert_eq!(ModuleKind::parse("loom.osc.sine").unwrap().version, 1);
        assert!(ModuleKind::parse("@1").is_err());
        assert!(ModuleKind::parse("x@y").is_err());
    }

    #[test]
    fn refuses_input_defaults_that_are_not_finite() {
        static NAN_DEFAULT: ModuleDescriptor = ModuleDescriptor {
            id: "test.nan",
            version: 1,
            name: "NaN",
            category: "Test",
            description: "",
            inputs: &[
                crate::PortSpec::new("in", crate::SignalType::Modulation).with_default(PortDefault::Value(f32::NAN))
            ],
            outputs: &[],
            params: &[],
            role: ModuleRole::UnitDelay,
            panel: crate::Panel::EMPTY,
            displays: &[],
        };
        let error = Registry::new().register(&NAN_DEFAULT).unwrap_err();
        assert!(error.to_string().contains("finite"), "{error}");
    }

    /// A DSP module with the given parameters, for registry checks.
    macro_rules! with_params {
        ($($param:expr),*) => {{
            static DESCRIPTOR: ModuleDescriptor = ModuleDescriptor {
                id: "test.params",
                version: 1,
                name: "Params",
                category: "Test",
                description: "",
                inputs: &[],
                outputs: &[],
                params: &[$($param),*],
                role: ModuleRole::Dsp(|_| unreachable!("never created")),
                panel: crate::Panel::EMPTY,
                displays: &[],
            };
            &DESCRIPTOR
        }};
    }

    #[test]
    fn refuses_impossible_logarithmic_parameters() {
        use crate::ParamSpec;
        let whole = with_params!(ParamSpec::new("count", 1.0, 100.0, 1.0, "").integer().logarithmic());
        let error = Registry::new().register(whole).unwrap_err();
        assert!(error.to_string().contains("whole-number"), "{error}");
        let from_zero = with_params!(ParamSpec::new("time", 0.0, 10.0, 1.0, "s").logarithmic());
        let error = Registry::new().register(from_zero).unwrap_err();
        assert!(error.to_string().contains("minimum above 0"), "{error}");
        let fine = with_params!(ParamSpec::new("time", 0.001, 10.0, 1.0, "s").logarithmic());
        assert!(Registry::new().register(fine).is_ok());
    }

    /// A DSP module with two parameters, one display value, and a panel.
    macro_rules! with_panel {
        ($width:expr, $height:expr, [$($widget:expr),*]) => {{
            use crate::{ParamSpec, Widget};
            static DESCRIPTOR: ModuleDescriptor = ModuleDescriptor {
                id: "test.panel",
                version: 1,
                name: "Panel",
                category: "Test",
                description: "",
                inputs: &[],
                outputs: &[crate::PortSpec::new("out", crate::SignalType::Audio)],
                params: &[
                    ParamSpec::new("level", 0.0, 1.0, 0.5, ""),
                    ParamSpec::new("on", 0.0, 1.0, 0.0, "").integer(),
                    ParamSpec::new("slope", 0.0, 3.0, 0.0, "").integer(),
                ],
                role: ModuleRole::Dsp(|_| unreachable!("never created")),
                panel: crate::Panel::new($width, $height, &[$($widget),*]),
                displays: &["busy"],
            };
            Registry::new().register(&DESCRIPTOR).map_err(|error| error.to_string())
        }};
    }

    #[test]
    fn accepts_a_panel_that_fits() {
        let result = with_panel!(
            12,
            6,
            [
                Widget::knob(0, 0, "level"),
                Widget::toggle(3, 0, "on"),
                Widget::radio(6, 0, "slope", &["6", "12", "18", "24"]),
                Widget::light(0, 3, "busy"),
                Widget::meter(9, 0, "out")
            ]
        );
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn refuses_panels_that_do_not_fit() {
        let refused = [
            (
                with_panel!(3, 3, [Widget::knob(0, 0, "volume")]),
                "`volume` (Knob): no such parameter",
            ),
            (with_panel!(3, 2, [Widget::toggle(0, 0, "level")]), "a toggle needs"),
            (
                with_panel!(3, 4, [Widget::radio(0, 0, "slope", &["6", "12", "24"])]),
                "radio buttons need",
            ),
            (
                with_panel!(3, 2, [Widget::light(0, 0, "idle")]),
                "no such display value",
            ),
            (
                with_panel!(3, 6, [Widget::meter(0, 0, "wet")]),
                "`wet` (Meter): no such output",
            ),
            (with_panel!(3, 2, [Widget::knob(0, 0, "level")]), "outside the panel"),
            (
                with_panel!(6, 3, [Widget::knob(0, 0, "level"), Widget::toggle(2, 1, "on")]),
                "`on` (Toggle): overlaps `level`",
            ),
        ];
        for (result, expected) in refused {
            let error = result.unwrap_err();
            assert!(error.contains(expected), "{error}");
        }
    }

    #[test]
    fn refuses_too_many_display_values() {
        static DESCRIPTOR: ModuleDescriptor = ModuleDescriptor {
            id: "test.displays",
            version: 1,
            name: "Displays",
            category: "Test",
            description: "",
            inputs: &[],
            outputs: &[],
            params: &[],
            role: ModuleRole::Dsp(|_| unreachable!("never created")),
            panel: crate::Panel::EMPTY,
            displays: &["a", "b", "c", "d", "e"],
        };
        let error = Registry::new().register(&DESCRIPTOR).unwrap_err();
        assert!(error.to_string().contains("more than 4 display values"), "{error}");
    }
}

//! Roles: the contracts that let a slot swap one part for another
//! (ADR-0017; `docs/design/sub-patches.md`, Roles; FR-SDK-08).
//!
//! A role names the ports and the core parameters a part promises to have.
//! A module declares the roles it fills in its descriptor, mapping each
//! role name to one of its own where they differ; a sub-patch declares them
//! in its definition, by name alone. The registry checks a module's
//! declarations when the module is registered, and `loom-patch` checks a
//! definition's with [`RoleDescriptor::mismatches`] when it is saved,
//! inserted, or placed in a slot.
//!
//! Each port and parameter is required or optional. A part must have every
//! required one, of the role's kind: a port's kind is its signal type, a
//! parameter's its unit and whether it is a whole number. Minimum and
//! maximum may differ, because a value that carries across is clamped to
//! the new part's range.

use crate::descriptor::{ModuleDescriptor, ParamSpec, PortSpec};
use crate::signal::SignalType;
use std::fmt;

pub mod roles;

/// A port a role promises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RolePort {
    /// The name a slot's port has, and the name a part maps to its own.
    pub name: &'static str,
    /// Short display name on the slot's tile.
    pub label: &'static str,
    /// What the port carries, in one sentence.
    pub description: &'static str,
    /// The kind a part's port must have.
    pub signal: SignalType,
    /// True when every part must have it. An optional port a part lacks
    /// connects to nothing while that part is in.
    pub required: bool,
}

/// A core parameter a role promises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleParam {
    /// The name a value carries across by.
    pub name: &'static str,
    /// Short display name.
    pub label: &'static str,
    /// What the parameter does, in one sentence.
    pub description: &'static str,
    /// The unit a part's parameter must have, such as `"Hz"` or `""`.
    pub unit: &'static str,
    /// True when a part's parameter must be a whole number, false when it
    /// must not be.
    pub whole: bool,
    /// True when every part must have it. An optional parameter carries
    /// across only when both parts have it.
    pub required: bool,
}

/// A role: a namespaced id with a version, and the ports and parameters
/// a part promises. Data in the SDK, like a [`ModuleDescriptor`].
#[derive(Debug)]
pub struct RoleDescriptor {
    /// Stable, namespaced id, such as `"loom.role.filter"`.
    pub id: &'static str,
    /// Version. A new version only adds optional ports and parameters, so
    /// every part that filled the old one still fits.
    pub version: u32,
    /// Display name, such as "Filter".
    pub name: &'static str,
    /// One sentence on what a part of this role does.
    pub description: &'static str,
    /// Inputs.
    pub inputs: &'static [RolePort],
    /// Outputs.
    pub outputs: &'static [RolePort],
    /// Core parameters.
    pub params: &'static [RoleParam],
}

impl PartialEq for RoleDescriptor {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

/// What a part lacks for a role, or has in the wrong kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleMismatch {
    /// `"input"`, `"output"`, `"parameter"`, or `"exposed"` for a
    /// definition's exposed item.
    pub what: &'static str,
    /// The role's name for it.
    pub name: String,
    /// Why, when the part has something of that name: its kind.
    pub reason: Option<String>,
}

impl fmt::Display for RoleMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.reason {
            None => write!(f, "no {} `{}`", self.what, self.name),
            Some(reason) => write!(f, "{} `{}` {}", self.what, self.name, reason),
        }
    }
}

/// The kind of a part's parameter, as a role compares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamKind {
    /// Its unit.
    pub unit: &'static str,
    /// True for a whole number.
    pub whole: bool,
}

impl ParamKind {
    /// The kind of a parameter spec.
    pub fn of(spec: &ParamSpec) -> Self {
        Self {
            unit: spec.unit,
            whole: spec.options.integer,
        }
    }
}

impl RoleDescriptor {
    /// The `"id@version"` string used in patch files.
    pub fn kind(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }

    /// The input named `name`.
    pub fn input(&self, name: &str) -> Option<&RolePort> {
        self.inputs.iter().find(|p| p.name == name)
    }

    /// The output named `name`.
    pub fn output(&self, name: &str) -> Option<&RolePort> {
        self.outputs.iter().find(|p| p.name == name)
    }

    /// The parameter named `name`.
    pub fn param(&self, name: &str) -> Option<&RoleParam> {
        self.params.iter().find(|p| p.name == name)
    }

    /// What a part lacks for this role. `input`, `output`, and `param` say
    /// what the part has under a role name: the kind of its port or
    /// parameter, or `None`. `param_word` is how a missing parameter is
    /// named: `"parameter"` for a module, `"exposed"` for a definition.
    /// Empty when the part fits.
    pub fn mismatches(
        &self,
        input: impl Fn(&str) -> Option<SignalType>,
        output: impl Fn(&str) -> Option<SignalType>,
        param: impl Fn(&str) -> Option<ParamKind>,
        param_word: &'static str,
    ) -> Vec<RoleMismatch> {
        let mut found = Vec::new();
        let ports = [
            ("input", self.inputs, &input as &dyn Fn(&str) -> Option<SignalType>),
            ("output", self.outputs, &output),
        ];
        for (what, list, has) in ports {
            for port in list {
                match has(port.name) {
                    None if port.required => found.push(RoleMismatch {
                        what,
                        name: port.name.to_string(),
                        reason: None,
                    }),
                    Some(signal) if signal != port.signal => found.push(RoleMismatch {
                        what,
                        name: port.name.to_string(),
                        reason: Some(format!("carries {signal:?}, not {:?}", port.signal)),
                    }),
                    _ => {}
                }
            }
        }
        for p in self.params {
            match param(p.name) {
                None if p.required => found.push(RoleMismatch {
                    what: param_word,
                    name: p.name.to_string(),
                    reason: None,
                }),
                Some(kind) if kind.unit != p.unit => found.push(RoleMismatch {
                    what: param_word,
                    name: p.name.to_string(),
                    reason: Some(format!("is in {}, not {}", unit_text(kind.unit), unit_text(p.unit))),
                }),
                Some(kind) if kind.whole != p.whole => found.push(RoleMismatch {
                    what: param_word,
                    name: p.name.to_string(),
                    reason: Some(if p.whole {
                        "is not a whole number".to_string()
                    } else {
                        "is a whole number".to_string()
                    }),
                }),
                _ => {}
            }
        }
        found
    }
}

/// A unit for a message: `"Hz"`, or "no unit".
fn unit_text(unit: &str) -> String {
    if unit.is_empty() {
        "no unit".to_string()
    } else {
        unit.to_string()
    }
}

/// A module's declaration that it fills a role, with the role names it
/// maps to its own where they differ. Written in the module's descriptor:
///
/// ```ignore
/// roles: &[FillsRole {
///     outputs: &[("out", "lowpass")],
///     ..FillsRole::of(&loom_core::roles::FILTER)
/// }],
/// ```
#[derive(Debug, Clone, Copy)]
pub struct FillsRole {
    /// The role.
    pub role: &'static RoleDescriptor,
    /// `(role input name, own input name)`, only where they differ.
    pub inputs: &'static [(&'static str, &'static str)],
    /// `(role output name, own output name)`, only where they differ.
    pub outputs: &'static [(&'static str, &'static str)],
    /// `(role parameter name, own parameter name)`, only where they differ.
    pub params: &'static [(&'static str, &'static str)],
}

impl PartialEq for FillsRole {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.role, other.role)
            && self.inputs == other.inputs
            && self.outputs == other.outputs
            && self.params == other.params
    }
}

impl FillsRole {
    /// A declaration with every role name the same as the module's own.
    pub const fn of(role: &'static RoleDescriptor) -> Self {
        Self {
            role,
            inputs: &[],
            outputs: &[],
            params: &[],
        }
    }

    /// The module's own name for the role input `name`.
    pub fn own_input(&self, name: &str) -> String {
        mapped(self.inputs, name)
    }

    /// The module's own name for the role output `name`.
    pub fn own_output(&self, name: &str) -> String {
        mapped(self.outputs, name)
    }

    /// The module's own name for the role parameter `name`.
    pub fn own_param(&self, name: &str) -> String {
        mapped(self.params, name)
    }

    /// The role parameter that the module's own parameter `own` stands
    /// for, if any.
    pub fn role_param_of(&self, own: &str) -> Option<&'static RoleParam> {
        self.role.params.iter().find(|p| self.own_param(p.name) == own)
    }

    /// The role input that the module's own input `own` stands for.
    pub fn role_input_of(&self, own: &str) -> Option<&'static RolePort> {
        self.role.inputs.iter().find(|p| self.own_input(p.name) == own)
    }

    /// The role output that the module's own output `own` stands for.
    pub fn role_output_of(&self, own: &str) -> Option<&'static RolePort> {
        self.role.outputs.iter().find(|p| self.own_output(p.name) == own)
    }

    /// Why `module` does not fit this declaration, or `Ok` when it does: a
    /// required role port or parameter the module lacks or has in another
    /// kind, a map entry for a name the role does not have, a map to a name
    /// the module does not have, or two role names mapped onto one own
    /// name. The registry refuses the descriptor with this text (FR-SDK-08).
    pub fn check(&self, module: &ModuleDescriptor) -> Result<(), String> {
        let role = self.role.kind();
        let maps: [NameMap<'_>; 3] = [
            ("input", self.inputs, &|name| self.role.input(name).is_some(), &|name| {
                module.input_index(name).is_some()
            }),
            (
                "output",
                self.outputs,
                &|name| self.role.output(name).is_some(),
                &|name| module.output_index(name).is_some(),
            ),
            (
                "parameter",
                self.params,
                &|name| self.role.param(name).is_some(),
                &|name| module.param_index(name).is_some(),
            ),
        ];
        for (what, map, in_role, in_module) in maps {
            for (role_name, own) in map.iter() {
                if !in_role(role_name) {
                    return Err(format!("role {role} has no {what} `{role_name}` to map"));
                }
                if !in_module(own) {
                    return Err(format!(
                        "role {role} maps {what} `{role_name}` to `{own}`, which the module does not have"
                    ));
                }
            }
        }
        // Two role names onto one own name, through the map or through a
        // role name that is also a module name.
        let collisions: [(&str, Vec<String>); 3] = [
            (
                "input",
                self.role.inputs.iter().map(|p| self.own_input(p.name)).collect(),
            ),
            (
                "output",
                self.role.outputs.iter().map(|p| self.own_output(p.name)).collect(),
            ),
            (
                "parameter",
                self.role.params.iter().map(|p| self.own_param(p.name)).collect(),
            ),
        ];
        for (what, owns) in collisions {
            for (i, own) in owns.iter().enumerate() {
                if owns[..i].contains(own) {
                    return Err(format!("role {role} maps two {what}s onto `{own}`"));
                }
            }
        }
        let port_signal =
            |ports: &'static [PortSpec], name: String| ports.iter().find(|p| p.name == name).map(|p| p.signal);
        let found = self.role.mismatches(
            |name| port_signal(module.inputs, self.own_input(name)),
            |name| port_signal(module.outputs, self.own_output(name)),
            |name| {
                let own = self.own_param(name);
                module.params.iter().find(|p| p.name == own).map(ParamKind::of)
            },
            "parameter",
        );
        match found.first() {
            None => Ok(()),
            Some(RoleMismatch {
                what,
                name,
                reason: None,
            }) => Err(format!("role {role} needs {what} `{name}`; the module has none")),
            Some(mismatch) => Err(format!("role {role}: {mismatch}")),
        }
    }
}

/// One kind of name map a declaration checks: its word, its entries, and
/// whether the role and the module have a name.
type NameMap<'a> = (
    &'a str,
    &'a [(&'a str, &'a str)],
    &'a dyn Fn(&str) -> bool,
    &'a dyn Fn(&str) -> bool,
);

/// `name` mapped through `map`, or itself.
fn mapped(map: &'static [(&'static str, &'static str)], name: &str) -> String {
    map.iter()
        .find(|(role_name, _)| *role_name == name)
        .map_or(name, |(_, own)| *own)
        .to_string()
}

#[cfg(test)]
mod tests;

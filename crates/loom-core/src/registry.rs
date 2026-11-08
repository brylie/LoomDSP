//! The set of module types available to patches.

use crate::descriptor::{Channels, ModuleDescriptor, ModuleRole, ParamMapping, PortDefault};
use crate::error::LoomError;
use crate::role::RoleDescriptor;
use std::collections::{BTreeMap, BTreeSet};

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

/// Something to fix in a registered module that does not stop it from
/// loading, such as a port with no description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryNote {
    /// Module ID.
    pub module: &'static str,
    /// What to fix.
    pub message: String,
}

impl std::fmt::Display for RegistryNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.module, self.message)
    }
}

/// Module types by stable ID.
#[derive(Debug, Default)]
pub struct Registry {
    modules: BTreeMap<&'static str, &'static ModuleDescriptor>,
    /// Roles by id: the ones registered on their own and the ones a module
    /// declares (sub-patches.md, Roles).
    roles: BTreeMap<&'static str, &'static RoleDescriptor>,
    /// Modules the editor adds itself and never offers to the player, such
    /// as the swap fade. They are registered like any module, so the file
    /// and the engine know them, but the browser and the help pages leave
    /// them out.
    internal: BTreeSet<&'static str>,
    notes: Vec<RegistryNote>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a module type. Fails on duplicate IDs or invalid descriptors.
    ///
    /// A port or parameter with no label or description does not stop the
    /// module from loading: it adds a note to [`Registry::notes`]. *Why:*
    /// missing help text is worth fixing, but not worth refusing a patch.
    pub fn register(&mut self, descriptor: &'static ModuleDescriptor) -> Result<(), LoomError> {
        let invalid = |reason: &str| LoomError::InvalidDescriptor {
            id: descriptor.id.to_string(),
            reason: reason.to_string(),
        };
        if descriptor.inputs.len() > 64 {
            return Err(invalid("more than 64 inputs"));
        }
        if descriptor.inputs.iter().chain(descriptor.outputs).any(|port| {
            matches!(port.range, crate::PortRange::Span(min, max) if !min.is_finite() || !max.is_finite() || min >= max)
        }) {
            return Err(invalid("a port range must have finite, increasing bounds"));
        }
        if descriptor
            .inputs
            .iter()
            .any(|port| matches!(port.default, PortDefault::Value(value) if !value.is_finite()))
        {
            return Err(invalid("an input default must be a finite number"));
        }
        for param in descriptor.params {
            // Transient parameters (transient-params.md, rules 1 and 2): a
            // hold or a press, which starts in the safe "nothing" state.
            if param.is_transient() && !param.options.integer {
                return Err(invalid("a transient parameter must be a whole number"));
            }
            if param.is_transient() && param.default != 0.0 {
                return Err(invalid("a transient parameter must default to 0"));
            }
            if param.options.mapping == ParamMapping::Logarithmic {
                if param.options.integer {
                    return Err(invalid("a whole-number parameter cannot be logarithmic"));
                }
                if param.min.is_nan() || param.min <= 0.0 {
                    return Err(invalid("a logarithmic parameter needs a minimum above 0"));
                }
            }
            if let ParamMapping::Decibels { floor } = param.options.mapping {
                if param.options.integer {
                    return Err(invalid("a whole-number parameter cannot be in decibels"));
                }
                if param.min != 0.0 {
                    return Err(invalid("a parameter in decibels needs a minimum of 0"));
                }
                if param.max.is_nan() || param.max <= 0.0 {
                    return Err(invalid("a parameter in decibels needs a maximum above 0"));
                }
                if !param.options.ranges.is_empty() {
                    return Err(invalid("a parameter in decibels cannot have control ranges"));
                }
                if !floor.is_finite() || floor >= 20.0 * param.max.log10() {
                    return Err(invalid("a parameter in decibels needs a floor below its maximum"));
                }
            }
        }
        // Stereo rules (ADR-0020, stereo spec rule 5). An output cannot
        // downmix, and a module that handles stereo itself is never copied,
        // so a `PerChannel` input on it would never be honoured.
        if descriptor.outputs.iter().any(|port| port.channels == Channels::Downmix) {
            return Err(invalid("an output cannot be Downmix"));
        }
        let has_stereo_port = descriptor
            .inputs
            .iter()
            .chain(descriptor.outputs)
            .any(|port| port.channels == Channels::Stereo);
        if has_stereo_port
            && descriptor
                .inputs
                .iter()
                .any(|port| port.channels == Channels::PerChannel)
        {
            return Err(invalid("a module with a Stereo port needs Downmix or Stereo inputs"));
        }
        descriptor.check_panel().map_err(|reason| invalid(&reason))?;
        match descriptor.role {
            ModuleRole::UnitDelay if descriptor.inputs.len() != 1 || descriptor.outputs.len() != 1 => {
                return Err(invalid("a unit delay needs exactly one input and one output"));
            }
            ModuleRole::AudioOutput
                if descriptor.inputs.len() != 1
                    || descriptor.inputs[0].channels != Channels::Stereo
                    || !descriptor.outputs.is_empty() =>
            {
                return Err(invalid("an audio output needs one stereo input and no outputs"));
            }
            _ => {}
        }
        // The compiler binds input defaults only for DSP modules. On a unit
        // delay or an audio output, an unconnected input reads 0, so any
        // other default would be accepted and then silently ignored.
        if matches!(descriptor.role, ModuleRole::UnitDelay | ModuleRole::AudioOutput)
            && let Some(port) = descriptor
                .inputs
                .iter()
                .find(|port| port.default != PortDefault::Value(0.0))
        {
            return Err(invalid(&format!(
                "input \"{}\" has a default that the engine ignores: only DSP modules can have a non-zero or transport default",
                port.name
            )));
        }
        // Only DSP modules have a `Module` instance, so only they can read
        // parameters or publish display values. On any other role they would
        // be accepted and then silently do nothing.
        let has_params_or_displays = !descriptor.params.is_empty() || !descriptor.displays.is_empty();
        if !matches!(descriptor.role, ModuleRole::Dsp(_)) && has_params_or_displays {
            return Err(invalid("only DSP modules can have parameters or display values"));
        }
        for (i, fills) in descriptor.roles.iter().enumerate() {
            fills.check(descriptor).map_err(|reason| invalid(&reason))?;
            let same_id_elsewhere = descriptor.roles[..i]
                .iter()
                .any(|other| other.role.id == fills.role.id && !std::ptr::eq(other.role, fills.role));
            if same_id_elsewhere
                || self
                    .roles
                    .get(fills.role.id)
                    .is_some_and(|known| !std::ptr::eq(*known, fills.role))
            {
                return Err(invalid(&format!(
                    "role {} is registered twice under one id",
                    fills.role.id
                )));
            }
        }
        if self.modules.contains_key(descriptor.id) {
            return Err(invalid("duplicate module ID"));
        }
        // Only an accepted module registers its roles.
        for fills in descriptor.roles {
            self.register_role(fills.role)?;
        }
        self.modules.insert(descriptor.id, descriptor);
        self.notes
            .extend(descriptor.undocumented().into_iter().map(|missing| RegistryNote {
                module: descriptor.id,
                message: missing.to_string(),
            }));
        Ok(())
    }

    /// Adds a module the editor uses itself and never offers to the player
    /// ([`Registry::is_internal`]). Checked and looked up like any module.
    pub fn register_internal(&mut self, descriptor: &'static ModuleDescriptor) -> Result<(), LoomError> {
        self.register(descriptor)?;
        self.internal.insert(descriptor.id);
        Ok(())
    }

    /// True for a module added with [`Registry::register_internal`].
    pub fn is_internal(&self, id: &str) -> bool {
        self.internal.contains(id)
    }

    /// The registered modules a player may add: every module that is not
    /// internal, sorted by id.
    pub fn browsable(&self) -> impl Iterator<Item = &'static ModuleDescriptor> + '_ {
        self.iter().filter(|d| !self.is_internal(d.id))
    }

    /// Adds a role (sub-patches.md, Roles). A role a module declares is
    /// added when the module is. The same descriptor twice is fine; two
    /// descriptors under one id are refused.
    pub fn register_role(&mut self, role: &'static RoleDescriptor) -> Result<(), LoomError> {
        match self.roles.get(role.id) {
            Some(known) if std::ptr::eq(*known, role) => Ok(()),
            Some(_) => Err(LoomError::InvalidDescriptor {
                id: role.id.to_string(),
                reason: "duplicate role ID".to_string(),
            }),
            None => {
                self.roles.insert(role.id, role);
                Ok(())
            }
        }
    }

    /// The role with this id.
    pub fn role(&self, id: &str) -> Option<&'static RoleDescriptor> {
        self.roles.get(id).copied()
    }

    /// Resolves a role's `"id@version"` kind string, as
    /// [`Registry::resolve`] does a module's: an older version resolves, a
    /// newer one than the registry knows is an error.
    pub fn resolve_role(&self, kind: &str) -> Result<&'static RoleDescriptor, LoomError> {
        let parsed = ModuleKind::parse(kind)?;
        let role = self
            .role(&parsed.id)
            .ok_or_else(|| LoomError::UnknownRole(kind.to_string()))?;
        if parsed.version > role.version {
            return Err(LoomError::UnsupportedRoleVersion {
                kind: kind.to_string(),
                supported: role.version,
            });
        }
        Ok(role)
    }

    /// Every registered role, sorted by id.
    pub fn roles(&self) -> impl Iterator<Item = &'static RoleDescriptor> + '_ {
        self.roles.values().copied()
    }

    /// The modules that declare the role `role_id`, sorted by id, internal
    /// ones left out: what a slot's Swap menu lists.
    pub fn parts_of(&self, role_id: &str) -> Vec<&'static ModuleDescriptor> {
        self.browsable().filter(|d| d.fills(role_id).is_some()).collect()
    }

    /// Notes about registered modules, in the order they were registered.
    /// `loom check` prints them.
    pub fn notes(&self) -> &[RegistryNote] {
        &self.notes
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
mod tests;

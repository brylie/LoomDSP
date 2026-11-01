//! Values from the environment in a control script
//! (`docs/design/control-channel.md`, Script variables).
//!
//! A script names a device as `"device": "${LOOM_DEVICE}"`, so the same file
//! runs on any machine. Only string values of the parsed command are
//! searched (V2), so a value with a quote or a backslash cannot break the
//! JSON.

use serde_json::{Map, Value};

/// A variable the script uses that is unset or empty (V4).
#[derive(Debug, PartialEq, Eq)]
pub struct Unset(pub String);

impl std::fmt::Display for Unset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the environment variable {} is not set", self.0)
    }
}

/// Replaces every `${NAME}` in the string values of `command`, using `env`
/// to read a variable. Returns each name used and its value, for the
/// reply's `vars` field (V6), or the first variable that is unset or empty.
pub fn substitute(command: &mut Value, env: &impl Fn(&str) -> Option<String>) -> Result<Map<String, Value>, Unset> {
    let mut used = Map::new();
    walk(command, env, &mut used)?;
    Ok(used)
}

/// Replaces variables in `value` and everything inside it. Object keys are
/// never touched (V2).
fn walk(value: &mut Value, env: &impl Fn(&str) -> Option<String>, used: &mut Map<String, Value>) -> Result<(), Unset> {
    match value {
        Value::String(text) => {
            if let Some(new) = replace(text, env, used)? {
                *value = new;
            }
        }
        Value::Array(items) => {
            for item in items {
                walk(item, env, used)?;
            }
        }
        Value::Object(fields) => {
            for field in fields.values_mut() {
                walk(field, env, used)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

/// The new value of a string with variables in it, or `None` when it has
/// none. A string that is exactly one variable keeps the variable's type
/// (V3); otherwise the result is text.
fn replace(
    text: &str,
    env: &impl Fn(&str) -> Option<String>,
    used: &mut Map<String, Value>,
) -> Result<Option<Value>, Unset> {
    let mut read = |name: &str| {
        let value = env(name)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| Unset(name.to_string()))?;
        used.insert(name.to_string(), Value::String(value.clone()));
        Ok(value)
    };
    if let Some(name) = whole_variable(text) {
        return Ok(Some(typed(read(name)?)));
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut changed = false;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}').map(|end| &after[..end]).filter(|name| valid_name(name)) {
            Some(name) => {
                out.push_str(&read(name)?);
                rest = &after[name.len() + 1..];
                changed = true;
            }
            // Not a variable (V5): keep the `${` and go on after it.
            None => {
                out.push_str("${");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Ok(changed.then_some(Value::String(out)))
}

/// The name, when `text` is exactly `${NAME}` with a valid name.
fn whole_variable(text: &str) -> Option<&str> {
    text.strip_prefix("${")?
        .strip_suffix('}')
        .filter(|name| valid_name(name))
}

/// A variable name: `[A-Z0-9_]`, starting with a letter (V5).
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// The JSON value a whole-string variable becomes (V3): a number, or
/// `true` / `false`, if the text reads as one, and a string otherwise.
fn typed(text: String) -> Value {
    // Only what a command takes: `null`, arrays, and objects stay text, and
    // so does a value with spaces around it, which is likely a mistake.
    if text.trim() == text
        && let Ok(value @ (Value::Number(_) | Value::Bool(_))) = serde_json::from_str::<Value>(&text)
    {
        return value;
    }
    Value::String(text)
}

#[cfg(test)]
mod tests;

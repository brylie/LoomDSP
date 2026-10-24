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
mod tests {
    use super::*;
    use serde_json::json;

    /// An environment with these variables and no others.
    fn env(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string())
    }

    /// Replaces variables in `command` with `vars`.
    fn run(mut command: Value, vars: &'static [(&'static str, &'static str)]) -> Result<Value, Unset> {
        substitute(&mut command, &env(vars)).map(|_| command)
    }

    /// A whole-string variable becomes a number, a boolean, or a string.
    #[test]
    fn a_whole_string_variable_keeps_its_type() {
        let vars = &[("LOOM_RATE", "96000"), ("LOOM_ON", "true"), ("LOOM_DEVICE", "MOTU M4")];
        let command = json!({"rate": "${LOOM_RATE}", "on": "${LOOM_ON}", "device": "${LOOM_DEVICE}"});
        assert_eq!(
            run(command, vars).unwrap(),
            json!({"rate": 96000, "on": true, "device": "MOTU M4"})
        );
    }

    /// Only numbers and booleans change type: `null`, JSON arrays, a
    /// number with spaces, and a leading zero stay strings.
    #[test]
    fn only_numbers_and_booleans_change_type() {
        assert_eq!(typed("-1.5".into()), json!(-1.5));
        assert_eq!(typed("false".into()), json!(false));
        for text in ["null", "[1]", "{}", " 42 ", "096000", "True"] {
            assert_eq!(typed(text.into()), json!(text), "{text}");
        }
    }

    /// A variable inside a longer string is text, and two in one string
    /// are both replaced.
    #[test]
    fn variables_inside_a_string_are_text() {
        let vars = &[("HOME", "/home/ada"), ("LOOM_RATE", "96000")];
        let command = json!({"path": "${HOME}/out-${LOOM_RATE}.png"});
        assert_eq!(run(command, vars).unwrap(), json!({"path": "/home/ada/out-96000.png"}));
    }

    /// A value with a quote or a backslash stays one string value (V2).
    #[test]
    fn quotes_and_backslashes_stay_inside_the_value() {
        let vars = &[("LOOM_PATH", r#"C:\Users\ada\"x".png"#)];
        let command = json!({"path": "${LOOM_PATH}"});
        assert_eq!(run(command, vars).unwrap(), json!({"path": r#"C:\Users\ada\"x".png"#}));
    }

    /// `$5`, `${x}`, and an unclosed `${` are not variables (V5), and keys
    /// are never replaced (V2).
    #[test]
    fn other_dollar_signs_are_left_alone() {
        let command = json!({"text": "costs $5, ${x} and ${OPEN", "${LOOM_KEY}": 1});
        assert_eq!(run(command.clone(), &[]).unwrap(), command);
    }

    /// An unset or empty variable is refused by name (V4).
    #[test]
    fn an_unset_or_empty_variable_is_refused() {
        assert_eq!(
            run(json!({"device": "${LOOM_DEVICE}"}), &[]),
            Err(Unset("LOOM_DEVICE".to_string()))
        );
        assert_eq!(
            run(json!({"path": "a/${LOOM_DIR}/b"}), &[("LOOM_DIR", "")]),
            Err(Unset("LOOM_DIR".to_string()))
        );
    }

    /// Each variable used is returned with its value, for `vars` (V6).
    #[test]
    fn the_variables_used_are_returned() {
        let mut command = json!({"device": "${LOOM_DEVICE}", "x": 1});
        let used = substitute(&mut command, &env(&[("LOOM_DEVICE", "MOTU"), ("OTHER", "y")])).unwrap();
        assert_eq!(Value::Object(used), json!({"LOOM_DEVICE": "MOTU"}));
    }
}

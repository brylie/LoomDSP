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

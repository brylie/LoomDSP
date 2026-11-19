//! Tests for `access.rs`: section 7, rule 2, and the node each `Access`
//! gives.

use super::*;
use accesskit::{Action as KitAction, Role as KitRole, Toggled};

/// A number from 0 to 10.
fn number(value: f64) -> Value {
    Value::Number {
        value,
        min: 0.0,
        max: 10.0,
        step: None,
    }
}

/// A button with no name breaks rule 2, and so does one of only spaces.
#[test]
fn an_element_needs_a_name() {
    let button = |name: &str| Access::new(Role::Button, name).action(Action::Click);
    assert_eq!(button("").problem(false), Some(Problem::NoName));
    assert_eq!(button("  ").problem(false), Some(Problem::NoName));
    assert_eq!(button("Open").problem(false), None);
}

/// Each role that needs a value needs the right kind: a number for a
/// slider and a meter, on or off for a switch and a check box, and text for
/// a label and a status.
#[test]
fn a_role_needs_its_kind_of_value() {
    let with = |role, value: Option<Value>| {
        let access = Access::new(role, "Name").action(Action::Click);
        match value {
            Some(value) => access.value(value),
            None => access,
        }
    };
    for role in [Role::Slider, Role::Meter] {
        assert_eq!(with(role, None).problem(false), Some(Problem::NoValue), "{role:?}");
        assert_eq!(with(role, Some(Value::On(true))).problem(false), Some(Problem::NoValue));
        assert_eq!(with(role, Some(number(1.0))).problem(false), None, "{role:?}");
    }
    for role in [Role::Switch, Role::CheckBox] {
        assert_eq!(with(role, None).problem(false), Some(Problem::NoValue), "{role:?}");
        assert_eq!(with(role, Some(number(1.0))).problem(false), Some(Problem::NoValue));
        assert_eq!(with(role, Some(Value::On(false))).problem(false), None, "{role:?}");
    }
    for role in [Role::Label, Role::Status] {
        assert_eq!(with(role, None).problem(false), Some(Problem::NoValue), "{role:?}");
        assert_eq!(
            with(role, Some(Value::Text(" ".into()))).problem(false),
            Some(Problem::NoValue)
        );
        assert_eq!(with(role, Some(Value::Text("Ready".into()))).problem(false), None);
    }
}

/// A label is named by its text, so it needs no name of its own (rule 17).
#[test]
fn a_label_needs_only_its_text() {
    assert_eq!(Access::label("Cents: 12.5 cents").problem(false), None);
    assert_eq!(Access::label("").problem(false), Some(Problem::NoValue));
}

/// A status, like a label, is read by its text: the role table gives it
/// only its text, and rule 2 asks a name of interactive elements only. So
/// it needs non-empty text and no name.
#[test]
fn a_status_needs_only_its_text() {
    let status = |text: &str| Access::new(Role::Status, "").value(Value::Text(text.into()));
    assert_eq!(status("Warning: the input is late").problem(false), None);
    assert_eq!(status("").problem(false), Some(Problem::NoValue));
    assert_eq!(
        Access::new(Role::Status, "").problem(false),
        Some(Problem::NoValue),
        "no text at all"
    );
}

/// Something a user acts on must offer an action. A focus stop offers
/// Focus, so that is enough; a group a user does not act on needs none.
#[test]
fn an_operable_role_needs_an_action() {
    let button = Access::new(Role::Button, "Open");
    assert_eq!(button.problem(false), Some(Problem::NotOperable));
    assert_eq!(button.problem(true), None, "Focus is an action");
    assert_eq!(button.action(Action::Click).problem(false), None);
    assert_eq!(Access::new(Role::Group, "Toolbar").problem(false), None);
}

/// A turned-off button with no action breaks nothing, since it offers
/// nothing until it is turned on; the same button turned on does.
#[test]
fn a_turned_off_element_needs_no_action() {
    let button = |disabled| {
        Access::new(Role::Button, "Save").states(States {
            disabled,
            ..States::default()
        })
    };
    assert_eq!(button(true).problem(false), None);
    assert_eq!(button(false).problem(false), Some(Problem::NotOperable));
}

/// The node carries the role, the name, the description, and a number with
/// its range and step.
#[test]
fn a_slider_gives_its_number_and_range() {
    let access = Access::new(Role::Slider, "Cutoff")
        .value(Value::Number {
            value: 800.0,
            min: 20.0,
            max: 20_000.0,
            step: Some(1.0),
        })
        .description("800 Hz")
        .action(Action::Increment)
        .action(Action::Decrement)
        .action(Action::SetValue);
    let node = access.node(true);
    assert_eq!(node.role(), KitRole::Slider);
    assert_eq!(node.label(), Some("Cutoff"));
    assert_eq!(node.description(), Some("800 Hz"));
    assert_eq!(node.numeric_value(), Some(800.0));
    assert_eq!(node.min_numeric_value(), Some(20.0));
    assert_eq!(node.max_numeric_value(), Some(20_000.0));
    assert_eq!(node.numeric_value_step(), Some(1.0));
    for action in [
        KitAction::Increment,
        KitAction::Decrement,
        KitAction::SetValue,
        KitAction::Focus,
    ] {
        assert!(node.supports_action(action), "{action:?}");
    }
}

/// A slider with no step declares none (rule 14).
#[test]
fn a_slider_with_no_step_declares_none() {
    let node = Access::new(Role::Slider, "Gain").value(number(3.0)).node(true);
    assert_eq!(node.numeric_value_step(), None);
}

/// A switch says on or off, and text goes in the value.
#[test]
fn a_switch_is_toggled_and_text_is_a_value() {
    let on = Access::new(Role::Switch, "Mute").value(Value::On(true)).node(false);
    assert_eq!(on.toggled(), Some(Toggled::True));
    let off = Access::new(Role::Switch, "Mute").value(Value::On(false)).node(false);
    assert_eq!(off.toggled(), Some(Toggled::False));
    let label = Access::label("Ready").node(false);
    assert_eq!(label.value(), Some("Ready"));
    assert_eq!(label.label(), None, "a label has no label of its own (rule 17)");
}

/// Only a focus stop offers Focus (rule 22), and offering an action twice
/// adds it once.
#[test]
fn only_a_focus_stop_offers_focus() {
    let button = Access::new(Role::Button, "Open")
        .action(Action::Click)
        .action(Action::Click);
    assert!(!button.node(false).supports_action(KitAction::Focus));
    assert!(button.node(true).supports_action(KitAction::Focus));
    assert_eq!(button.actions, [Action::Click]);
}

/// A turned-off element offers no action: it is no focus stop, so not
/// even Focus. Its states reach the node.
#[test]
fn states_reach_the_node_and_disabled_offers_nothing() {
    let access = Access::new(Role::ComboBox, "Device")
        .action(Action::Increment)
        .action(Action::ShowContextMenu)
        .states(States {
            disabled: true,
            selected: true,
            expanded: Some(false),
            modal: true,
            current: true,
        });
    assert!(access.is_disabled());
    let node = access.node(false);
    assert!(node.is_disabled());
    assert_eq!(node.is_selected(), Some(true));
    assert_eq!(node.is_expanded(), Some(false));
    assert!(node.is_modal());
    assert_eq!(node.aria_current(), Some(accesskit::AriaCurrent::Page));
    assert!(!node.supports_action(KitAction::Increment));
    assert!(!node.supports_action(KitAction::ShowContextMenu));
    assert!(!node.supports_action(KitAction::Focus));
}

#[test]
fn every_role_and_action_maps_to_its_own() {
    let roles = [
        Role::Button,
        Role::Switch,
        Role::CheckBox,
        Role::Slider,
        Role::RadioGroup,
        Role::RadioButton,
        Role::ComboBox,
        Role::TextInput,
        Role::Label,
        Role::Group,
        Role::Dialog,
        Role::AlertDialog,
        Role::Menu,
        Role::MenuItem,
        Role::List,
        Role::ListItem,
        Role::ListBox,
        Role::ListBoxOption,
        Role::TabList,
        Role::Tab,
        Role::Status,
        Role::Image,
        Role::Meter,
        Role::Link,
        Role::Tree,
        Role::TreeItem,
    ];
    let kits: Vec<KitRole> = roles.iter().map(|role| role.kit()).collect();
    for (i, a) in kits.iter().enumerate() {
        assert!(!kits[i + 1..].contains(a), "{:?} shares {a:?}", roles[i]);
    }
    let actions = [
        Action::Click,
        Action::Increment,
        Action::Decrement,
        Action::SetValue,
        Action::ShowContextMenu,
    ];
    let kits: Vec<KitAction> = actions.iter().map(|action| action.kit()).collect();
    for (i, a) in kits.iter().enumerate() {
        assert!(!kits[i + 1..].contains(a), "{:?} shares {a:?}", actions[i]);
    }
    assert!(!kits.contains(&KitAction::Focus), "Focus is never set by hand");
}

/// Each problem says what is wrong in words.
#[test]
fn each_problem_says_what_is_wrong() {
    for problem in [Problem::NoKey, Problem::NoName, Problem::NoValue, Problem::NotOperable] {
        assert!(!problem.to_string().is_empty());
    }
    assert_eq!(Access::new(Role::Tab, "Inspector").role(), Role::Tab);
    assert_eq!(Access::new(Role::Tab, "Inspector").name(), "Inspector");
}

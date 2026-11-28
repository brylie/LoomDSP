//! The licence chooser as the runtime's panel: what a screen reader meets
//! in it, its keys, and how it gives the input back (patch-metadata.md,
//! Licence chooser, Keys). Keys go through the `Ui` harness as real
//! events, so the runtime's own routing is what is tested.

use super::{body_id, button_id, licence_id, other_id, other_text_id, question_id};
use crate::Message;
use crate::licence_chooser::Target;
use crate::ui_test::{Ui, key_press};
use accesskit::{Action, Role};
use loom_ui_core::{Key, Modifiers, UiId};

/// A window with the chooser open for the patch's licence, as patch info's
/// **Choose** opens it, on a patch with no licence yet, so nothing is
/// chosen.
fn chooser() -> Ui {
    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut().session.document.edit_info(|info| info.license = None);
    ui.send(Message::PatchInfo(crate::patch_info::Message::ChooseLicence));
    let _ = ui.redraw();
    assert!(ui.app().overlays.chooser.is_some(), "the chooser is open");
    ui
}

/// The answer `key` of question `question`.
fn answer(question: &str, key: &str) -> UiId {
    question_id(question).with(key)
}

/// One of the three buttons.
fn button(key: &str) -> UiId {
    button_id(key)
}

/// Presses `key` with `modifiers` and draws the frame after it.
fn tap(ui: &mut Ui, key: Key, modifiers: Modifiers) {
    ui.tap(key, modifiers);
    let _ = ui.redraw();
}

/// The runtime's focus.
fn focused(ui: &Ui) -> Option<UiId> {
    ui.runtime().focused().cloned()
}

/// The stops Tab meets from where the focus is, `count` times, forward or
/// back, starting with the one that has the focus.
fn walk(ui: &mut Ui, count: usize, back: bool) -> Vec<UiId> {
    let shift = if back { Modifiers::SHIFT } else { Modifiers::NONE };
    let mut met = vec![focused(ui).expect("a stop has the focus")];
    for _ in 1..count {
        tap(ui, Key::Tab, shift);
        met.push(focused(ui).expect("a stop has the focus"));
    }
    met
}

/// Opening the chooser moves the focus to the first question's first
/// answer when none is chosen, the first focusable element (the WAI-ARIA
/// modal dialog pattern), and opened on a licence it already has, to that
/// licence's answer.
#[test]
fn opening_it_focuses_the_first_question() {
    let mut ui = chooser();
    assert_eq!(focused(&ui), Some(answer("changes", "yes")));
    ui.send(Message::Licence(crate::licence_chooser::Message::Cancel));
    let _ = ui.redraw();
    ui.app_mut()
        .session
        .document
        .edit_info(|info| info.license = Some("CC-BY-ND-4.0".to_string()));
    ui.send(Message::PatchInfo(crate::patch_info::Message::ChooseLicence));
    let _ = ui.redraw();
    assert_eq!(focused(&ui), Some(answer("changes", "no")), "on the chosen answer");
}

/// A screen reader meets a dialog named by its title with no description;
/// each question is a radio group named by it, whose radios carry their
/// explanations as descriptions; **Other** is a radio with no group node
/// and its field follows it; **Use this licence** is turned off until the
/// answers give a licence.
#[test]
fn a_screen_reader_meets_its_questions_as_radio_groups() {
    let mut ui = chooser();
    let tree = ui.a11y_tree();
    let dialog = tree.node_by_id(&licence_id());
    assert_eq!(dialog.role(), Role::Dialog);
    assert_eq!(dialog.name(), Some("Choose a licence"));
    assert_eq!(dialog.description(), None);
    let changes = tree.node_by_id(&body_id().with("changes"));
    assert_eq!(changes.role(), Role::RadioGroup);
    assert_eq!(changes.name(), Some("Allow others to change it and share the result?"));
    let yes = tree.node_by_id(&answer("changes", "yes"));
    assert_eq!(yes.role(), Role::RadioButton);
    assert_eq!(
        yes.description(),
        Some("Others may change it and share their version, as long as they credit you.")
    );
    assert!(
        !tree.has_part_under(&body_id().with("commercial")),
        "the second question waits for the first"
    );
    let other = tree.node_by_id(&other_id());
    assert_eq!(other.role(), Role::RadioButton);
    assert_eq!(other.name(), Some("Other: any SPDX licence expression"));
    assert_eq!(other.position_in_set(), None, "a set of one says nothing");
    let field = tree.node_by_id(&other_text_id());
    assert_eq!(field.role(), Role::TextInput);
    assert_eq!(field.name(), Some("Other licence expression"));
    let use_it = tree.node_by_id(&button("use"));
    assert!(use_it.is_disabled(), "nothing to use yet");
    assert!(!use_it.offers(Action::Click));
    assert_eq!(tree.node_by_id(&button("clear")).name(), Some("No licence"));
}

/// The arrows choose in a question, and the second question shows once the
/// first has an answer. Tab then goes through the first question, the
/// second, **Other**, its field, Clear, Cancel, and Use, once each, and
/// wraps; Shift+Tab goes the other way. A question is met on its chosen
/// answer (the WAI-ARIA radio group pattern).
#[test]
fn tab_goes_through_every_part_once_and_wraps() {
    let mut ui = chooser();
    tap(&mut ui, Key::Down, Modifiers::NONE);
    let chooser = ui.app().overlays.chooser.as_ref().expect("open").model();
    assert!(chooser.changes[1].chosen, "Down chose the second answer");
    assert_eq!(focused(&ui), Some(answer("changes", "share-alike")));
    tap(&mut ui, Key::Tab, Modifiers::NONE);
    tap(&mut ui, Key::Space, Modifiers::NONE);
    let order = [
        answer("changes", "share-alike"),
        answer("commercial", "yes"),
        other_id(),
        other_text_id(),
        button("clear"),
        button("cancel"),
        button("use"),
    ];
    tap(&mut ui, Key::Tab, Modifiers::SHIFT);
    assert_eq!(focused(&ui), Some(order[0].clone()));
    assert_eq!(walk(&mut ui, order.len() + 1, false)[..order.len()], order, "in order");
    assert_eq!(focused(&ui), Some(order[0].clone()), "it wraps to the first question");
    tap(&mut ui, Key::Tab, Modifiers::SHIFT);
    assert_eq!(focused(&ui), Some(button("use")), "Shift+Tab wraps back to Use");
}

/// While the answers give no licence, Use is turned off and Tab skips it:
/// from Cancel, Tab wraps to the first question (ui-layer.md, section 6,
/// rule 2).
#[test]
fn tab_skips_use_while_it_is_off() {
    let mut ui = chooser();
    tap(&mut ui, Key::Tab, Modifiers::SHIFT);
    assert_eq!(focused(&ui), Some(button("cancel")), "Shift+Tab wraps to the last stop");
    tap(&mut ui, Key::Tab, Modifiers::NONE);
    assert_eq!(focused(&ui), Some(answer("changes", "yes")));
}

/// **Other** is a group of one: the arrows do not choose it, Space does.
/// Typing in its field chooses it too, and Enter there uses a valid
/// expression, which sets the patch's licence; an invalid one stays.
#[test]
fn other_is_chosen_by_space_or_typing_and_enter_uses_it() {
    let mut ui = chooser();
    ui.a11y_action(&other_id(), Action::Focus);
    assert_eq!(focused(&ui), Some(other_id()));
    tap(&mut ui, Key::Down, Modifiers::NONE);
    let model = |ui: &Ui| ui.app().overlays.chooser.as_ref().expect("open").model();
    assert!(!model(&ui).other, "Down does not choose it");
    tap(&mut ui, Key::Space, Modifiers::NONE);
    assert!(model(&ui).other, "Space does");
    tap(&mut ui, Key::Tab, Modifiers::NONE);
    assert_eq!(focused(&ui), Some(other_text_id()));
    let _ = ui.typewrite("not a licence");
    let _ = ui.redraw();
    assert_eq!(model(&ui).other_text, "not a licence");
    tap(&mut ui, Key::Enter, Modifiers::NONE);
    assert!(ui.app().overlays.chooser.is_some(), "an invalid expression is not used");
    for _ in 0.."not a licence".len() {
        tap(&mut ui, Key::Backspace, Modifiers::NONE);
    }
    let _ = ui.typewrite("MIT");
    let _ = ui.redraw();
    tap(&mut ui, Key::Enter, Modifiers::NONE);
    assert!(ui.app().overlays.chooser.is_none(), "Enter used it");
    assert_eq!(ui.app().session.document.info().license.as_deref(), Some("MIT"));
}

/// A screen reader's SetValue on the Other field types its text, through
/// the runtime's field, as the window's adapter hands it to the runtime,
/// and chooses **Other**.
#[test]
fn a_screen_reader_types_in_the_other_field() {
    let mut ui = chooser();
    let _ = ui.deliver(Message::A11y(crate::a11y::Event::Activated));
    let _ = ui.redraw();
    let node = ui.app_mut().a11y_node_of(&other_text_id());
    let _ = ui.accessibility_turn(loom_ui::AccessEvent::Action(accesskit::ActionRequest {
        action: Action::SetValue,
        target_tree: accesskit::TreeId::ROOT,
        target_node: node,
        data: Some(accesskit::ActionData::Value("CC-BY-4.0 OR MIT".into())),
    }));
    let _ = ui.redraw();
    let model = ui.app().overlays.chooser.as_ref().expect("open").model();
    assert!(model.other);
    assert_eq!(model.other_text, "CC-BY-4.0 OR MIT");
    assert!(model.ready, "a valid expression turns Use on");
}

/// Escape closes the chooser from a question, a button, and the Other
/// field, as Cancel does, and changes nothing.
#[test]
fn escape_closes_it_from_anywhere() {
    for part in [answer("changes", "yes"), button("cancel"), other_text_id()] {
        let mut ui = chooser();
        let before = ui.app().session.document.info().license.clone();
        ui.a11y_action(&part, Action::Focus);
        assert_eq!(focused(&ui), Some(part.clone()));
        tap(&mut ui, Key::Escape, Modifiers::NONE);
        assert!(ui.app().overlays.chooser.is_none(), "Escape on {part} closed it");
        assert_eq!(ui.app().session.document.info().license, before, "nothing changed");
    }
}

/// While an input method composes in the Other field, Escape and Tab are
/// the composition's: the chooser stays open and the focus stays in the
/// field (patch-metadata.md, Keys).
#[test]
fn escape_and_tab_while_composing_are_the_compositions() {
    let mut ui = chooser();
    ui.a11y_action(&other_text_id(), Action::Focus);
    let _ = ui.step(&[
        loom_ui_core::Event::Ime(loom_ui_core::ImeEvent::Enabled),
        loom_ui_core::Event::Ime(loom_ui_core::ImeEvent::Preedit {
            text: "に".to_owned(),
            cursor: Some((3, 3)),
        }),
    ]);
    let none = Modifiers::NONE;
    let _ = ui.step(&[key_press(Key::Escape, none, None, false)]);
    let _ = ui.redraw();
    assert!(ui.app().overlays.chooser.is_some(), "Escape was the composition's");
    let _ = ui.step(&[key_press(Key::Tab, none, None, false)]);
    let _ = ui.redraw();
    assert_eq!(focused(&ui), Some(other_text_id()), "Tab was the composition's");
}

/// Help opened over the chooser covers it: only help is reported, a
/// screen reader's Click on a chooser part does nothing, and closing help
/// gives the focus back to the chooser's part that had it.
#[test]
fn help_over_the_chooser_covers_it_and_gives_the_focus_back() {
    let mut ui = chooser();
    tap(&mut ui, Key::Tab, Modifiers::NONE);
    let before = focused(&ui).expect("a chooser part has the focus");
    tap(&mut ui, Key::F1, Modifiers::NONE);
    assert!(ui.app().overlays.help.is_some(), "F1 opened help");
    let tree = ui.a11y_tree();
    assert!(!tree.has_part_under(&licence_id()), "the chooser is not reported");
    assert!(tree.has_part_under(&crate::overlays::access::help_id()));
    assert!(
        ui.app().chooser_click(&button("cancel")).is_none(),
        "it refuses a click"
    );
    assert!(ui.app().chooser_stops().is_empty(), "nor is it a place to focus");
    tap(&mut ui, Key::Escape, Modifiers::NONE);
    assert!(ui.app().overlays.help.is_none());
    assert!(ui.app().overlays.chooser.is_some(), "Escape closed only help");
    assert_eq!(focused(&ui), Some(before), "the focus came back");
}

/// In a short window the box shrinks, its buttons stay in view, and the
/// body scrolls: Tab to a part out of view scrolls it into view.
#[test]
fn in_a_short_window_the_body_scrolls_to_a_focused_part() {
    let mut ui = chooser();
    ui.resize(1000.0, 360.0);
    tap(&mut ui, Key::Space, Modifiers::NONE);
    let window = ui.runtime().bounds(&licence_id()).expect("the panel");
    let cancel = ui.runtime().bounds(&button("cancel")).expect("Cancel");
    assert!(
        cancel.y + cancel.height <= window.y + window.height,
        "Cancel is in the window: {cancel:?} in {window:?}"
    );
    let body = ui.runtime().bounds(&body_id()).expect("the body");
    let field = ui.runtime().bounds(&other_text_id()).expect("the field");
    assert!(field.y > body.y + body.height, "the field starts out of view");
    // At most one lap of the stops, so a Tab order that skips the field
    // fails here instead of looping for ever.
    for _ in 0..ui.app().chooser_stops().len() {
        if focused(&ui) == Some(other_text_id()) {
            break;
        }
        tap(&mut ui, Key::Tab, Modifiers::NONE);
    }
    assert_eq!(focused(&ui), Some(other_text_id()), "Tab reached the field");
    let (offset, _) = ui.runtime().scroll_offset(&body_id()).expect("the body scrolls");
    assert!(offset > 0.0, "Tab scrolled the body");
}

/// The chooser opened for the default licence says so in its title and
/// its clearing button.
#[test]
fn the_default_chooser_says_what_it_is_for() {
    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut().overlays.chooser = Some(crate::licence_chooser::Chooser::open(Target::Default, None));
    let tree = ui.a11y_tree();
    assert_eq!(
        tree.node_by_id(&licence_id()).name(),
        Some("Default licence for new patches")
    );
    assert_eq!(tree.node_by_id(&button("clear")).name(), Some("No default"));
}

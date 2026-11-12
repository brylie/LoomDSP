use super::*;
use crate::licence_chooser::{Answer, Chooser, Target};
use loom_patch::metadata::Changes;

/// The part of `dialog` named by `id`, if it has one.
fn part<'a>(dialog: &'a Item, id: &UiId) -> Option<&'a Item> {
    dialog.walk().into_iter().find(|item| item.id == *id)
}

/// A chooser with nothing answered asks the first question, offers no
/// commercial question yet, and cannot be used.
#[test]
fn an_open_chooser_asks_its_first_question() {
    let chooser = Chooser::open(Target::Default, None);
    let dialog = items(&chooser.model());
    assert_eq!(dialog.kind, Kind::Dialog);
    assert_eq!(dialog.label.as_deref(), Some("Default licence for new patches"));
    let changes = part(&dialog, &licence_id().with("changes")).unwrap();
    assert_eq!(changes.kind, Kind::Options);
    assert_eq!(changes.children.len(), 4, "Other stands apart, with its field");
    assert!(changes.children.iter().all(|o| o.on == Some(false)));
    assert!(part(&dialog, &licence_id().with("commercial")).is_none());
    assert!(part(&dialog, &licence_id().with("result").with("open")).is_some());
    let use_it = part(&dialog, &licence_id().with("use")).unwrap();
    assert!(!use_it.enabled, "nothing to use yet");
    let clear = part(&dialog, &licence_id().with("clear")).unwrap();
    assert_eq!(clear.label.as_deref(), Some("No default"));
}

/// Answering both questions shows the second question, the licence they
/// give, and turns **Use this licence** on. Each answer's Click sends the
/// message its radio sends.
#[test]
fn both_answers_give_a_licence_to_use() {
    let mut chooser = Chooser::open(Target::Patch, None);
    let dialog = items(&chooser.model());
    let yes = part(&dialog, &licence_id().with("changes").with("yes")).unwrap();
    assert!(matches!(
        &yes.click,
        Some(AppMessage::Licence(Message::Answer(Answer::Changes(Changes::Yes))))
    ));
    let _ = chooser.update(Message::Answer(Answer::Changes(Changes::Yes)));
    let dialog = items(&chooser.model());
    let commercial = part(&dialog, &licence_id().with("commercial")).unwrap();
    assert_eq!(commercial.children.len(), 2);
    let _ = chooser.update(Message::Commercial(true));
    let dialog = items(&chooser.model());
    let chosen = part(&dialog, &licence_id().with("changes").with("yes")).unwrap();
    assert_eq!(chosen.on, Some(true));
    assert!(chosen.click.is_none(), "a chosen answer has nothing to do");
    assert!(part(&dialog, &licence_id().with("result").with("short")).is_some());
    assert!(part(&dialog, &licence_id().with("use")).unwrap().enabled);
}

/// **Other** is an option of its own beside its field. Text that is not a
/// licence expression says so, and a screen reader's text only types it,
/// since Enter there would use it.
#[test]
fn other_takes_typed_text_and_says_when_it_is_not_a_licence() {
    let mut chooser = Chooser::open(Target::Patch, None);
    for message in SetText::LicenceOther.messages("not a licence") {
        let crate::Message::Licence(message) = message else {
            panic!("a chooser message");
        };
        let _ = chooser.update(message);
    }
    let dialog = items(&chooser.model());
    assert_eq!(part(&dialog, &licence_id().with("other")).unwrap().on, Some(true));
    let field = part(&dialog, &licence_id().with("other-text")).unwrap();
    assert_eq!(field.value.as_deref(), Some("not a licence"));
    assert!(part(&dialog, &licence_id().with("result").with("error")).is_some());
    assert!(!part(&dialog, &licence_id().with("use")).unwrap().enabled);
}

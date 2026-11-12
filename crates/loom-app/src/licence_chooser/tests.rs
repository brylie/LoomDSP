use super::*;
use crate::Message as AppMessage;

/// Answers the chooser with `messages`, in order, and returns the last
/// outcome.
fn answer(chooser: &mut Chooser, messages: impl IntoIterator<Item = Message>) -> Outcome {
    let mut outcome = Outcome::Stay;
    for message in messages {
        outcome = chooser.update(message);
    }
    outcome
}

/// Acceptance: each of the seven chooser answers stores the SPDX
/// identifier in the spec's table.
#[test]
fn every_answer_gives_its_licence() {
    let cases = [
        (Answer::Changes(Changes::Yes), Some(true), "CC-BY-4.0"),
        (Answer::Changes(Changes::ShareAlike), Some(true), "CC-BY-SA-4.0"),
        (Answer::Changes(Changes::No), Some(true), "CC-BY-ND-4.0"),
        (Answer::Changes(Changes::Yes), Some(false), "CC-BY-NC-4.0"),
        (Answer::Changes(Changes::ShareAlike), Some(false), "CC-BY-NC-SA-4.0"),
        (Answer::Changes(Changes::No), Some(false), "CC-BY-NC-ND-4.0"),
        (Answer::GiveUp, None, "CC0-1.0"),
    ];
    for (first, commercial, licence) in cases {
        let mut chooser = Chooser::open(Target::Patch, None);
        let mut messages = vec![Message::Answer(first)];
        messages.extend(commercial.map(Message::Commercial));
        messages.push(Message::Use);
        assert_eq!(answer(&mut chooser, messages), Outcome::Set(Some(licence.to_string())));
        // Opening it on that licence shows the same answers.
        let reopened = Chooser::open(Target::Patch, Some(licence));
        assert_eq!(reopened.result(), Some(Ok(licence.to_string())));
    }
}

/// Rule 7: nothing is stored until both questions are answered and Use
/// is pressed. Other takes any valid SPDX expression and refuses others.
#[test]
fn the_chooser_stores_only_a_finished_choice() {
    let mut chooser = Chooser::open(Target::Patch, None);
    assert_eq!(chooser.result(), None);
    assert_eq!(answer(&mut chooser, [Message::Use]), Outcome::Stay);
    let first = Message::Answer(Answer::Changes(Changes::Yes));
    assert_eq!(answer(&mut chooser, [first, Message::Use]), Outcome::Stay);

    assert_eq!(
        answer(
            &mut chooser,
            [Message::OtherText("MIT OR Apache-2.0".into()), Message::Use]
        ),
        Outcome::Set(Some("MIT OR Apache-2.0".into()))
    );
    let mut chooser = Chooser::open(Target::Patch, Some("my own terms"));
    assert!(matches!(chooser.result(), Some(Err(_))));
    assert_eq!(answer(&mut chooser, [Message::Use]), Outcome::Stay);
    assert_eq!(answer(&mut chooser, [Message::OtherText(" ".into())]), Outcome::Stay);
    assert_eq!(chooser.result(), None);
    assert_eq!(answer(&mut chooser, [Message::Clear]), Outcome::Set(None));
    assert_eq!(answer(&mut chooser, [Message::Cancel]), Outcome::Close);
}

/// The chooser sets the patch's licence, or the default licence in the
/// preferences; Cancel and Escape change nothing.
#[test]
fn the_chooser_sets_its_target() {
    let mut app = App::new(crate::tests::app_args());
    let _ = app.send(AppMessage::PatchInfo(crate::patch_info::Message::ChooseLicence));
    let _ = app.send(AppMessage::Licence(Message::Answer(Answer::GiveUp)));
    let _ = app.send(AppMessage::Licence(Message::Use));
    assert!(app.overlays.chooser.is_none());
    assert_eq!(app.session.document.info().license.as_deref(), Some("CC0-1.0"));
    assert!(app.unsaved());

    let _ = app.send(AppMessage::PatchInfo(crate::patch_info::Message::ChooseLicence));
    let escape = iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
        modified_key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Escape),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::empty(),
        text: None,
        repeat: false,
    };
    let _ = app.send(AppMessage::Key(escape));
    assert!(app.overlays.chooser.is_none());
    assert_eq!(app.session.document.info().license.as_deref(), Some("CC0-1.0"));

    let _ = app.send(AppMessage::Preferences(crate::settings::Message::ChooseLicence));
    assert_eq!(
        app.overlays.chooser.as_ref().map(Chooser::target),
        Some(Target::Default)
    );
    let _ = app.send(AppMessage::Licence(Message::OtherText("MIT".into())));
    let _ = app.send(AppMessage::Licence(Message::Use));
    assert_eq!(app.rig.store.saved().licence.as_deref(), Some("MIT"));
    let _ = app.send(AppMessage::Preferences(crate::settings::Message::ChooseLicence));
    let _ = app.send(AppMessage::Licence(Message::Clear));
    assert_eq!(app.rig.store.saved().licence, None);
}

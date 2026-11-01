use super::*;
use std::cell::RefCell;
use std::rc::Rc;

/// A controller whose messages go into a list the test can read.
fn logged(name: &str) -> (VirtualController, Rc<RefCell<Vec<Vec<u8>>>>) {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let log = sent.clone();
    let controller = VirtualController::with_output(
        name,
        Box::new(move |bytes| {
            log.borrow_mut().push(bytes.to_vec());
            Ok(())
        }),
    );
    (controller, sent)
}

/// Notes go out as note on and note off messages, with the channel in
/// the status byte and values kept to seven bits.
#[test]
fn notes_are_sent_as_midi_messages() {
    let (mut keys, sent) = logged("Keys");
    assert_eq!(keys.name(), "Keys");
    keys.note_on(1, 60, 100).unwrap();
    keys.note_off(1, 60).unwrap();
    keys.note_on(17, 200, 255).unwrap();
    assert_eq!(
        *sent.borrow(),
        [vec![0x91, 60, 100], vec![0x81, 60, 0], vec![0x91, 72, 127]]
    );
    assert!(keys.holds_notes());
}

/// The notes still held are released by `release_all`, and again by
/// dropping the controller, so no note hangs in the app.
#[test]
fn held_notes_are_released() {
    let (mut keys, sent) = logged("Keys");
    keys.note_on(0, 60, 100).unwrap();
    keys.note_on(0, 62, 100).unwrap();
    keys.note_off(0, 62).unwrap();
    sent.borrow_mut().clear();
    keys.release_all().unwrap();
    assert_eq!(*sent.borrow(), [vec![0x80, 60, 0]]);
    assert!(!keys.holds_notes());

    keys.note_on(2, 64, 90).unwrap();
    sent.borrow_mut().clear();
    drop(keys);
    assert_eq!(*sent.borrow(), [vec![0x82, 64, 0]]);
}

/// A send that fails is an error that names the controller, and
/// changes nothing: a failed note on holds no note, and a note whose
/// note off failed is still held, so the next release tries it again.
#[test]
fn a_failed_send_names_the_controller_and_changes_nothing() {
    let fail = Rc::new(std::cell::Cell::new(false));
    let failing = fail.clone();
    let mut pad = VirtualController::with_output(
        "Pad",
        Box::new(move |_| if failing.get() { Err("gone".to_string()) } else { Ok(()) }),
    );
    fail.set(true);
    let error = pad.note_on(0, 60, 100).unwrap_err().to_string();
    assert!(error.contains("Pad: gone"), "{error}");
    assert!(!pad.holds_notes(), "a note on that failed holds nothing");

    fail.set(false);
    pad.note_on(0, 60, 100).unwrap();
    pad.note_on(0, 62, 100).unwrap();
    fail.set(true);
    assert!(pad.release_all().is_err());
    assert!(pad.holds_notes(), "the notes not released are still held");
    fail.set(false);
    pad.release_all().unwrap();
    assert!(!pad.holds_notes());
}

/// The input list shows the port by its own name on macOS, and as
/// `client:name client:port` on Linux. A longer name that starts the
/// same is another port.
#[test]
fn the_input_list_name_is_found_on_both_systems() {
    let (pad, _) = logged("Loom Pad");
    assert!(pad.is_listed_as("Loom Pad"));
    assert!(pad.is_listed_as("LoomDSP virtual:Loom Pad 128:0"));
    assert!(!pad.is_listed_as("LoomDSP virtual:Loom Pad 2 129:0"));
    assert!(!pad.is_listed_as("Other:Loom Pad 130:0"));
    assert!(!pad.is_listed_as("Loom Pad 2"));
}

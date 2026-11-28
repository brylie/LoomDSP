//! Tests for the app's dialogs (`services.rs`).

use super::choice;

/// A dialog's result names its button by label, or by place where the
/// platform reports a standard one.
#[test]
fn a_dialog_result_names_its_button() {
    use rfd::MessageDialogResult as R;
    let three = ["Restore".to_string(), "Discard".to_string(), "Not Now".to_string()];
    assert_eq!(choice(&three, R::Custom("Discard".into())), Some(1));
    assert_eq!(choice(&three, R::Custom("Not Now".into())), Some(2));
    assert_eq!(choice(&three, R::Yes), Some(0));
    assert_eq!(choice(&three, R::No), Some(1));
    assert_eq!(choice(&three, R::Cancel), None);
    let two = ["Add my name".to_string(), "Not for this patch".to_string()];
    assert_eq!(choice(&two, R::Ok), Some(0));
    assert_eq!(choice(&two, R::No), None);
    assert_eq!(choice(&two, R::Custom("Something else".into())), None);
    assert_eq!(choice(&[], R::Ok), None, "no button, no index");
}

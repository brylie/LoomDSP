//! Tests for the shortcuts that wait for the text-focus check.

use super::Shortcut;

/// The clipboard keys name their shortcuts, and no other letter does.
#[test]
fn clipboard_keys_name_their_shortcuts() {
    assert_eq!(Shortcut::clipboard('c'), Some(Shortcut::Copy));
    assert_eq!(Shortcut::clipboard('x'), Some(Shortcut::Cut));
    assert_eq!(Shortcut::clipboard('d'), Some(Shortcut::Duplicate));
    assert_eq!(Shortcut::clipboard('v'), None);
}

use super::*;

/// A check that takes only digits, with a reason like the real one.
fn digits(text: &str) -> Result<(), String> {
    if !text.is_empty() && text.chars().all(|c| c.is_ascii_digit()) {
        Ok(())
    } else {
        Err("Not a number in Hz".to_string())
    }
}

/// The value as shown commits nothing, even when it would not parse.
#[test]
fn the_shown_text_commits_nothing() {
    let draft = TypedDraft::new("440 Hz".to_string(), digits);
    assert_eq!(draft.submit(Some("440 Hz")), DraftSubmit::Unchanged);
}

/// Bad text blocks Enter and keeps its reason.
#[test]
fn bad_text_blocks_enter() {
    let draft = TypedDraft::new("abc".to_string(), digits);
    assert_eq!(draft.error(), Some("Not a number in Hz"));
    assert_eq!(draft.submit(None), DraftSubmit::Blocked);
    assert_eq!(draft.submit(Some("440 Hz")), DraftSubmit::Blocked);
}

/// Good text that differs from the baseline commits.
#[test]
fn good_text_commits() {
    let draft = TypedDraft::new("880".to_string(), digits);
    assert_eq!(draft.error(), None);
    assert_eq!(draft.submit(Some("440")), DraftSubmit::Commit);
    assert_eq!(draft.submit(None), DraftSubmit::Commit);
}

/// A recheck follows the text as it changes.
#[test]
fn a_recheck_follows_the_text() {
    let mut draft = TypedDraft::new("12".to_string(), digits);
    draft.text.push('x');
    draft.recheck(digits);
    assert_eq!(draft.submit(None), DraftSubmit::Blocked);
    draft.text.pop();
    draft.recheck(digits);
    assert_eq!(draft.submit(None), DraftSubmit::Commit);
}

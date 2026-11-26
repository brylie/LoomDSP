use super::*;
use crate::recovery::tests::TempDir;
use loom_patch::PatchError;

/// Saving replaces the file by a rename, leaves no temporary file, and
/// refuses a missing folder or a read-only target without touching it.
#[test]
fn save_replaces_the_file_and_leaves_no_temporary() {
    let dir = TempDir::new("app-patch-save");
    let path = dir.join("p.ron");
    std::fs::write(&path, "old").unwrap();
    let patch = Patch::new("Saved");
    save(&patch, &path).unwrap();
    assert_eq!(load(&path).unwrap().metadata.name, "Saved");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temporary file is left");
    // A target that cannot be written reports the error and keeps the old file.
    let missing = dir.join("no-such-folder").join("p.ron");
    assert!(matches!(save(&patch, &missing), Err(PatchFileError::Io { .. })));
    // A read-only target is refused and keeps its content.
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&path, perms.clone()).unwrap();
    assert!(matches!(
        save(&Patch::new("Other"), &path),
        Err(PatchFileError::Io { .. })
    ));
    assert_eq!(load(&path).unwrap().metadata.name, "Saved");
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&path, perms).unwrap();
}

/// Saving through a symlink writes the file it points to and keeps the link.
#[cfg(unix)]
#[test]
fn save_through_a_symlink_updates_the_target() {
    let dir = TempDir::new("app-patch-link");
    let real = dir.join("real.ron");
    let link = dir.join("link.ron");
    save(&Patch::new("First"), &real).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    save(&Patch::new("Second"), &link).unwrap();
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(load(&real).unwrap().metadata.name, "Second");
}

/// A missing file names its path, then the system's reason, as the error
/// from `loom-patch` did before the file code moved here.
#[test]
fn a_missing_file_names_its_path() {
    let dir = TempDir::new("app-missing-patch");
    let path = dir.join("no-such-patch.ron");
    let error = load(&path).unwrap_err();
    assert!(matches!(error, PatchFileError::Io { .. }));
    assert!(
        error.to_string().starts_with(&format!("{}: ", path.display())),
        "{error}"
    );
}

/// Text that is not a patch reports the parse error unchanged.
#[test]
fn bad_text_reports_the_patch_error() {
    let dir = TempDir::new("app-patch-bad");
    let path = dir.join("bad.ron");
    std::fs::write(&path, "not a patch").unwrap();
    let error = load(&path).unwrap_err();
    assert!(matches!(error, PatchFileError::Patch(PatchError::Parse(_))), "{error}");
    assert!(error.to_string().starts_with("invalid patch:"), "{error}");
}

/// A leftover temporary file with this save's name makes the save fail,
/// and the save leaves that file alone: it did not create it.
#[test]
fn a_save_never_removes_a_temporary_file_it_did_not_create() {
    let dir = TempDir::new("app-patch-stale");
    let path = dir.join("p.ron");
    let stale = dir.join(format!(".p.ron.{}.tmp", std::process::id()));
    std::fs::write(&stale, "someone else's").unwrap();
    assert!(matches!(
        save(&Patch::new("New"), &path),
        Err(PatchFileError::Io { .. })
    ));
    assert_eq!(std::fs::read_to_string(&stale).unwrap(), "someone else's");
    assert!(!path.exists(), "nothing was written");
}

/// A test build refuses to save over an example in the repository, and
/// names the test: the `Ui` harness opens copies, so this trips only when
/// something reaches a real example another way. The path is a new name in
/// the examples folder, so even a missing guard would only add a file,
/// which is removed at the end.
#[test]
#[should_panic(expected = "tried to save over an example in the repository")]
fn a_test_cannot_save_over_an_example() {
    /// Removes the file it names when dropped, so a missing guard leaves
    /// nothing behind.
    struct Remove(std::path::PathBuf);
    impl Drop for Remove {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let path = crate::ui_test::example_path("refused-by-the-guard.ron");
    let _remove = Remove(path.clone());
    let patch = loom_patch::Patch::from_ron(r#"Patch(version: 1, metadata: Metadata(name: "Guard"))"#).unwrap();
    let _ = save(&patch, &path);
}

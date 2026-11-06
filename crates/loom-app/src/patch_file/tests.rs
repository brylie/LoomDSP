use super::*;

/// Saving replaces the file by a rename, leaves no temporary file, and
/// refuses a missing folder or a read-only target without touching it.
#[test]
fn save_replaces_the_file_and_leaves_no_temporary() {
    let dir = std::env::temp_dir().join(format!("loom-app-patch-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("p.ron");
    std::fs::write(&path, "old").unwrap();
    let patch = Patch::new("Saved");
    save(&patch, &path).unwrap();
    assert_eq!(load(&path).unwrap().name, "Saved");
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
    assert_eq!(load(&path).unwrap().name, "Saved");
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&path, perms).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Saving through a symlink writes the file it points to and keeps the link.
#[cfg(unix)]
#[test]
fn save_through_a_symlink_updates_the_target() {
    let dir = std::env::temp_dir().join(format!("loom-app-patch-link-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let real = dir.join("real.ron");
    let link = dir.join("link.ron");
    save(&Patch::new("First"), &real).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    save(&Patch::new("Second"), &link).unwrap();
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(load(&real).unwrap().name, "Second");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A missing file names its path, then the system's reason, as the error
/// from `loom-patch` did before the file code moved here.
#[test]
fn a_missing_file_names_its_path() {
    let path = std::env::temp_dir().join("loom-app-no-such-patch.ron");
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
    let dir = std::env::temp_dir().join(format!("loom-app-patch-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("bad.ron");
    std::fs::write(&path, "not a patch").unwrap();
    let error = load(&path).unwrap_err();
    assert!(matches!(error, PatchFileError::Patch(PatchError::Parse(_))), "{error}");
    assert!(error.to_string().starts_with("invalid patch:"), "{error}");
    std::fs::remove_dir_all(&dir).unwrap();
}

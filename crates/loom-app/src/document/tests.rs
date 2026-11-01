use super::*;

/// A folder for one test, removed when dropped.
struct Folder(PathBuf);

impl Folder {
    /// An empty folder named after the test and the process.
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("loom-document-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// Writes an empty file `name` in the folder and returns its path.
    fn file(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, "").unwrap();
        path
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A document with no file and no notes.
fn untitled() -> Document {
    Document::new("Patch".to_string(), None, None, Vec::new())
}

/// A patch opened from the examples folder becomes an untitled copy: no
/// file, "(example)" in the title, and Save As suggests its file name.
#[test]
fn an_example_opens_as_an_untitled_copy() {
    let folder = Folder::new("example");
    let path = folder.file("drone.ron");
    let mut document = Document::new("Drone".to_string(), Some(path), None, Vec::new());
    document.set_examples(Some(folder.0.clone()));
    document.copy_if_example();
    assert_eq!(document.file(), None);
    assert_eq!(document.title_name(), "drone (example)");
    assert_eq!(document.save_start(), Some(Path::new("drone.ron")));
}

/// A file outside the examples folder stays the current file.
#[test]
fn a_plain_file_is_not_an_example() {
    let examples = Folder::new("examples-elsewhere");
    let mine = Folder::new("mine");
    let path = mine.file("song.ron");
    let mut document = Document::new("Song".to_string(), Some(path.clone()), None, Vec::new());
    document.set_examples(Some(examples.0.clone()));
    document.copy_if_example();
    assert_eq!(document.file(), Some(path.as_path()));
}

/// A restored copy whose file changed keeps no file, shows "(recovered)",
/// and saves next to the old file under a name that does not replace it.
#[test]
fn a_restore_with_a_changed_file_saves_beside_it() {
    let mut document = untitled();
    document.restore(recovery::FileState::Changed(PathBuf::from("/songs/drone.ron")));
    assert_eq!(document.file(), None);
    assert_eq!(document.title_name(), "drone (recovered)");
    assert_eq!(document.save_start(), Some(Path::new("/songs/drone (recovered).ron")));
}

/// Saving makes the path the current file and drops the copy's hints.
#[test]
fn saving_drops_the_copy_hints() {
    let folder = Folder::new("saved");
    let path = folder.file("drone.ron");
    let mut document = untitled();
    document.restore(recovery::FileState::Missing(PathBuf::from("/gone/drone.ron")));
    document.saved(path.clone(), Patch::new("Patch"));
    assert_eq!(document.file(), Some(path.as_path()));
    assert_eq!(document.save_start(), Some(path.as_path()));
    assert_eq!(document.title_name(), files::display_name(Some(&path)));
}

/// With autosave off, no copy is due and a tick does nothing.
#[test]
fn autosave_off_is_never_due() {
    let mut document = untitled();
    assert!(!document.autosave_on());
    assert!(!document.autosave_due());
    document.may_have_changed();
    assert!(!document.autosave_due());
    assert_eq!(document.autosave(Instant::now(), Patch::new("Patch")), None);
}

/// The next patch takes over the examples folder, and applies the example
/// rule to itself.
#[test]
fn the_next_patch_carries_on_the_run() {
    let folder = Folder::new("carry");
    let mut old = untitled();
    old.set_examples(Some(folder.0.clone()));
    let path = folder.file("pad.ron");
    let mut next = Document::new("Pad".to_string(), Some(path), None, Vec::new());
    next.carry_on_from(&mut old);
    assert_eq!(next.file(), None, "the example rule applied");
    assert_eq!(next.title_name(), "pad (example)");
}

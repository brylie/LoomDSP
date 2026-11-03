use super::*;
use crate::{App, FileAction, Message as AppMessage};

/// A library folder for one test, with a subfolder, a hidden file, a
/// file that does not read, and three patches.
fn library(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("loom-patches-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("live")).unwrap();
    let write = |file: &str, text: &str| std::fs::write(dir.join(file), text).unwrap();
    write(
        "drone.ron",
        r#"Patch(version: 1, name: "Low Drone", description: "Slow filter", authors: ["Ada Lovelace"], tags: ["drone", "fm"], license: "CC-BY-NC-4.0")"#,
    );
    write(
        "live/bass.ron",
        r#"Patch(version: 1, name: "Bass", authors: ["Sam Rivera"], tags: ["drone"], license: "MIT")"#,
    );
    write("pad.ron", r#"Patch(version: 1, name: "Pad")"#);
    write("broken.ron", "not a patch");
    write(".drone.ron.123.tmp", "Patch(version: 1)");
    write("notes.txt", "Patch(version: 1)");
    dir
}

/// A browser that has read every file in `dir`, as the thread would.
fn browser(dir: &Path) -> PatchBrowser {
    let mut browser = PatchBrowser::new(1, dir.to_path_buf());
    for path in patch_files(dir) {
        let row = Box::new(Row::read(&path));
        let _ = browser.update(Message::Found { open: 1, row });
    }
    browser
}

/// The names the list shows, sorted so the test does not depend on the
/// file times.
fn names(browser: &PatchBrowser) -> Vec<String> {
    let mut names: Vec<String> = browser.shown().map(Row::name).collect();
    names.sort();
    names
}

/// Your patches browser: every patch file in the library folder and its
/// subfolders is listed; a file that does not read shows its file name.
#[test]
fn the_library_folder_is_listed() {
    let dir = library("list");
    let browser = browser(&dir);
    assert_eq!(names(&browser), ["Bass", "Low Drone", "Pad", "broken.ron"]);
    let broken = browser.shown().find(|r| r.info.is_none()).unwrap();
    assert_eq!(broken.name(), "broken.ron");
    assert_eq!(browser.tags(), ["drone", "fm"]);
    // Rows for an earlier opening are dropped.
    let mut browser = browser;
    let row = Box::new(Row::read(&dir.join("pad.ron")));
    let _ = browser.update(Message::Found { open: 7, row });
    assert_eq!(names(&browser).len(), 4);
    let _ = std::fs::remove_dir_all(dir);
}

/// Acceptance: two tags show only patches with both; the author filter
/// ignores case; search and licence filters combine with them.
#[test]
fn filters_and_search_combine() {
    let dir = library("filters");
    let mut browser = browser(&dir);
    let _ = browser.update(Message::Tag("drone".into()));
    assert_eq!(names(&browser), ["Bass", "Low Drone"]);
    let _ = browser.update(Message::Tag("fm".into()));
    assert_eq!(names(&browser), ["Low Drone"]);
    let _ = browser.update(Message::Tag("fm".into()));
    let _ = browser.update(Message::Tag("drone".into()));

    let _ = browser.update(Message::Author("sam RIVERA".into()));
    assert_eq!(names(&browser), ["Bass"]);
    let _ = browser.update(Message::Author(String::new()));

    let _ = browser.update(Message::Licence(LicenceFilter::AllowsCommercialUse, true));
    assert_eq!(names(&browser), ["Bass"]);
    let _ = browser.update(Message::Licence(LicenceFilter::AllowsCommercialUse, false));
    let _ = browser.update(Message::Licence(LicenceFilter::NoLicenceStated, true));
    assert_eq!(names(&browser), ["Pad"]);
    let _ = browser.update(Message::Licence(LicenceFilter::NoLicenceStated, false));

    let _ = browser.update(Message::Search("SLOW".into()));
    assert_eq!(names(&browser), ["Low Drone"]);
    let _ = browser.update(Message::Search("nothing like it".into()));
    assert!(names(&browser).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// Keys: Down moves the highlight, Enter opens the highlighted patch, a
/// file that does not read cannot be opened, Escape closes.
#[test]
fn the_list_opens_the_highlighted_patch() {
    let dir = library("open");
    let mut browser = browser(&dir);
    let _ = browser.update(Message::Search("pad".into()));
    assert_eq!(browser.update(Message::Submit).0, Outcome::Open(dir.join("pad.ron")));
    let _ = browser.update(Message::Search(String::new()));
    let broken = browser.shown().position(|r| r.info.is_none()).unwrap();
    let _ = browser.update(Message::Hover(broken));
    assert_eq!(browser.update(Message::Submit).0, Outcome::Stay);
    let _ = browser.update(Message::Step(-10));
    assert!(browser.highlighted().is_some());
    let _ = browser.update(Message::Step(10));
    assert_eq!(
        browser.highlighted().map(|r| r.path.clone()),
        browser.shown().last().map(|r| r.path.clone())
    );
    assert_eq!(browser.update(Message::Close).0, Outcome::Close);
    let _ = std::fs::remove_dir_all(dir);
}

/// Browse opens the overlay on the library folder from the preferences;
/// picking a patch opens it in place of the current one.
#[test]
fn browse_opens_patches_in_the_app() {
    let dir = library("app");
    let mut app = App::new(crate::tests::app_args());
    let mut preferences = app.rig.store.saved().clone();
    preferences.library = Some(dir.clone());
    let _ = app.rig.store.save(preferences);
    let _ = app.update(AppMessage::File(FileAction::Browse));
    assert!(app.patches.is_some());
    for path in patch_files(&dir) {
        let row = Box::new(Row::read(&path));
        let _ = app.update(AppMessage::Patches(Message::Found {
            open: app.patches_open,
            row,
        }));
    }
    let _ = app.update(AppMessage::Patches(Message::Search("bass".into())));
    let _ = app.update(AppMessage::Patches(Message::Submit));
    assert!(app.patches.is_none());
    assert_eq!(app.document.name(), "Bass");
    assert_eq!(app.document.file(), Some(dir.join("live/bass.ron").as_path()));
    let _ = std::fs::remove_dir_all(dir);
}

/// A new player's library folder does not exist yet. Save As for a patch
/// with no file makes it and starts there, under a free name from the patch
/// name; Browse makes it too, so the browser opens on an empty library.
#[test]
fn the_library_folder_is_made_when_first_needed() {
    use crate::files::fake::{self as dialogs, Asked};
    let root = std::env::temp_dir().join(format!("loom-patches-made-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("Documents/LoomDSP/Library");
    let mut app = App::new(crate::tests::app_args());
    let mut preferences = app.rig.store.saved().clone();
    preferences.library = Some(dir.clone());
    let _ = app.rig.store.save(preferences);

    let _ = dialogs::asked();
    dialogs::answer_save(None);
    assert!(!app.save_as(), "the dialog was cancelled");
    assert!(dir.is_dir(), "Save As made the library folder");
    assert_eq!(dialogs::asked(), [Asked::Save(Some(dir.join("Untitled.ron")))]);
    std::fs::write(dir.join("Untitled.ron"), "Patch(version: 1)").unwrap();
    assert!(!app.save_as());
    assert_eq!(dialogs::asked(), [Asked::Save(Some(dir.join("Untitled 2.ron")))]);

    std::fs::remove_dir_all(&root).unwrap();
    let _ = app.update(AppMessage::File(FileAction::Browse));
    assert!(dir.is_dir(), "Browse made the library folder");
    assert!(app.patches.is_some());
    let _ = std::fs::remove_dir_all(root);
}

/// The reader stops within one file of the flag being set, wherever in the
/// folder that falls: it reads no file after the one it was on.
#[test]
fn the_reader_stops_within_one_file() {
    let dir = library("stop");
    let total = patch_files(&dir).len();
    assert!(total > 2, "the folder must have files left to skip");
    let stop = AtomicBool::new(false);
    let mut reads = 0;
    read_rows(
        &dir,
        &stop,
        |path| {
            reads += 1;
            if reads == 2 {
                stop.store(true, Ordering::Relaxed);
            }
            Row::read(path)
        },
        |_| true,
    );
    assert_eq!(reads, 2, "of {total} files");
    let _ = std::fs::remove_dir_all(dir);
}

/// With nobody listening, the reader stops too.
#[test]
fn the_reader_stops_when_nobody_listens() {
    let dir = library("listen");
    let mut reads = 0;
    read_rows(
        &dir,
        &AtomicBool::new(false),
        |path| {
            reads += 1;
            Row::read(path)
        },
        |_| false,
    );
    assert_eq!(reads, 1);
    let _ = std::fs::remove_dir_all(dir);
}

/// An app with its library at `dir` and the browser open, and the flag that
/// stops that browser's reader, not yet set.
fn app_browsing(dir: &Path) -> (App, Arc<AtomicBool>) {
    let mut app = App::new(crate::tests::app_args());
    let mut preferences = app.rig.store.saved().clone();
    preferences.library = Some(dir.to_path_buf());
    let _ = app.rig.store.save(preferences);
    let _ = app.update(AppMessage::File(FileAction::Browse));
    let stop = app.patches.as_ref().expect("the browser is open").stop.clone();
    assert!(!stop.load(Ordering::Relaxed));
    (app, stop)
}

/// Close stops the reader.
#[test]
fn closing_the_browser_stops_its_reader() {
    let dir = library("close");
    let (mut app, stop) = app_browsing(&dir);
    let _ = app.update(AppMessage::Patches(Message::Close));
    assert!(app.patches.is_none());
    assert!(stop.load(Ordering::Relaxed));
    let _ = std::fs::remove_dir_all(dir);
}

/// Opening a patch from the browser stops the reader, and so does replacing
/// the patch some other way while the browser is open.
#[test]
fn opening_a_patch_stops_the_reader() {
    let dir = library("opened");
    let (mut app, stop) = app_browsing(&dir);
    let _ = app.update(AppMessage::Patches(Message::Found {
        open: app.patches_open,
        row: Box::new(Row::read(&dir.join("pad.ron"))),
    }));
    let _ = app.update(AppMessage::Patches(Message::Submit));
    assert!(app.patches.is_none());
    assert!(stop.load(Ordering::Relaxed));

    let (mut app, stop) = app_browsing(&dir);
    assert!(app.replace(crate::Source::Empty));
    assert!(stop.load(Ordering::Relaxed));
    let _ = std::fs::remove_dir_all(dir);
}

/// Opening the browser again stops the earlier reader and starts a new one.
#[test]
fn opening_the_browser_again_stops_the_earlier_reader() {
    let dir = library("again");
    let (mut app, first) = app_browsing(&dir);
    let _ = app.update(AppMessage::File(FileAction::Browse));
    let second = app.patches.as_ref().expect("the browser is open").stop.clone();
    assert!(first.load(Ordering::Relaxed), "the earlier reader stops");
    assert!(!second.load(Ordering::Relaxed), "the new one keeps reading");
    let _ = std::fs::remove_dir_all(dir);
}

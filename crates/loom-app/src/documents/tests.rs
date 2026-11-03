use super::*;

const HOME: &str = "/home/sam";

/// The folder named by `text`, for a user whose home is [`HOME`].
fn named(text: &str) -> Option<PathBuf> {
    from_user_dirs(text, Path::new(HOME))
}

/// On a localised system the file names a translated folder under `$HOME`.
#[test]
fn a_translated_folder_under_home_is_found() {
    let text = "XDG_DESKTOP_DIR=\"$HOME/Schreibtisch\"\nXDG_DOCUMENTS_DIR=\"$HOME/Dokumente\"\n";
    assert_eq!(named(text), Some(PathBuf::from("/home/sam/Dokumente")));
}

/// The file may name a folder anywhere, by its full path.
#[test]
fn an_absolute_folder_is_used_as_written() {
    assert_eq!(
        named("XDG_DOCUMENTS_DIR=\"/mnt/data/docs\""),
        Some(PathBuf::from("/mnt/data/docs"))
    );
}

/// A comment is not a setting, even when it looks like one.
#[test]
fn comments_are_skipped() {
    let text = "# XDG_DOCUMENTS_DIR=\"$HOME/Old\"\n#XDG_DOCUMENTS_DIR=\"$HOME/Older\"\nXDG_DOCUMENTS_DIR=\"$HOME/New\"";
    assert_eq!(named(text), Some(PathBuf::from("/home/sam/New")));
}

/// No line for the folder gives no answer, so the caller uses the usual place.
#[test]
fn a_file_without_the_folder_gives_nothing() {
    assert_eq!(named(""), None);
    assert_eq!(named("XDG_MUSIC_DIR=\"$HOME/Music\""), None);
}

/// Setting a folder to home is how a user turns it off.
#[test]
fn a_folder_set_to_home_is_turned_off() {
    assert_eq!(named("XDG_DOCUMENTS_DIR=\"$HOME/\""), None);
    assert_eq!(named("XDG_DOCUMENTS_DIR=\"$HOME\""), None);
    assert_eq!(named("XDG_DOCUMENTS_DIR=\"/home/sam\""), None);
}

/// Text the format does not allow is not trusted: a relative path or a
/// missing quote would put the library somewhere nobody expects.
#[test]
fn a_malformed_line_gives_nothing() {
    assert_eq!(named("XDG_DOCUMENTS_DIR=Documents"), None);
    assert_eq!(named("XDG_DOCUMENTS_DIR=\"Documents\""), None);
    assert_eq!(named("XDG_DOCUMENTS_DIR=\"$HOME/Docs"), None);
}

/// `XDG_CONFIG_HOME` moves the file, and otherwise it is in `~/.config`.
#[test]
fn the_file_follows_the_config_home() {
    let home = Path::new(HOME);
    assert_eq!(
        user_dirs_file(Some(Path::new("/etc/sam")), home),
        PathBuf::from("/etc/sam/user-dirs.dirs")
    );
    assert_eq!(
        user_dirs_file(None, home),
        PathBuf::from("/home/sam/.config/user-dirs.dirs")
    );
    // The XDG rules say to ignore a relative value.
    assert_eq!(
        user_dirs_file(Some(Path::new("config")), home),
        PathBuf::from("/home/sam/.config/user-dirs.dirs")
    );
}

/// Tests never see the real folder.
#[test]
fn tests_never_see_the_real_folder() {
    assert_eq!(folder(), None);
}

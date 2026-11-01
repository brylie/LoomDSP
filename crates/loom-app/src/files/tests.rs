use super::*;
use crate::recovery::tests::TempDir;

/// A release bundle takes its own examples even while the checkout's
/// examples exist, and a checkout build still takes the source folder.
#[test]
fn examples_follow_the_executable() {
    let tmp = TempDir::new("example-candidates");
    let checkout = tmp.0.join("checkout/examples/patches");
    let bundle = tmp.0.join("bundle/examples/patches");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::write(bundle.join("example.ron"), "").unwrap();
    let release = tmp.0.join("bundle/loom-app");
    let development = tmp.0.join("checkout/target/debug/loom-app");
    let cargo_examples = tmp.0.join("checkout/target/debug/examples");
    std::fs::create_dir_all(&cargo_examples).unwrap();
    std::fs::write(cargo_examples.join("example.ron"), "").unwrap();
    std::fs::write(&release, "").unwrap();
    std::fs::write(&development, "").unwrap();
    assert_eq!(examples_for_executable(Some(&release), &checkout), Some(bundle.clone()));
    assert!(is_example(
        &tmp.0.join("bundle/examples/patches/example.ron"),
        Some(&bundle)
    ));
    let mac = tmp.0.join("bundle/LoomDSP.app/Contents/MacOS/loom-app");
    std::fs::create_dir_all(mac.parent().unwrap()).unwrap();
    std::fs::write(&mac, "").unwrap();
    assert_eq!(examples_for_executable(Some(&mac), &checkout), Some(bundle));

    // The release script stages archives in checkout/dist. The source
    // examples still exist, but the staged app owns its nearby copy.
    let staged = tmp.0.join("checkout/dist/LoomDSP/loom-app");
    let staged_examples = tmp.0.join("checkout/dist/LoomDSP/examples/patches");
    std::fs::create_dir_all(&staged_examples).unwrap();
    std::fs::write(staged_examples.join("example.ron"), "").unwrap();
    std::fs::write(&staged, "").unwrap();
    assert_eq!(examples_for_executable(Some(&staged), &checkout), Some(staged_examples));
    assert_eq!(
        examples_for_executable(Some(&development), &checkout),
        Some(checkout.clone())
    );
    std::fs::create_dir_all(tmp.0.join("checkout/crates/app")).unwrap();
    let from_manifest = tmp.0.join("checkout/crates/app/../../examples/patches");
    assert_eq!(
        examples_for_executable(Some(&development), &from_manifest),
        Some(from_manifest)
    );

    // An external Cargo target has an `examples` directory too, but it
    // holds binaries, not bundled patches.
    let external = tmp.0.join("external/target/debug/loom-app");
    let external_examples = tmp.0.join("external/target/debug/examples/patches");
    std::fs::create_dir_all(&external_examples).unwrap();
    std::fs::write(external_examples.join("some-binary"), "").unwrap();
    std::fs::write(&external, "").unwrap();
    assert_eq!(examples_for_executable(Some(&external), &checkout), Some(checkout));
}

/// A suggested file name with no folder starts the Save dialog where an
/// untitled patch starts it: in the Documents folder.
#[test]
fn a_bare_file_name_starts_in_the_documents_folder() {
    assert_eq!(start_dir(Some(Path::new("bass.ron"))), start_dir(None));
}

/// Run from the checkout, the bundled examples folder is its
/// `examples/patches`.
#[test]
fn the_checkout_examples_folder_is_found() {
    let found = bundled_examples().expect("tests run in a checkout");
    let expected = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches");
    assert_eq!(found.canonicalize().unwrap(), expected.canonicalize().unwrap());
}

/// A file counts as an example only inside the given folder, however
/// its path is spelled. A file that does not exist is not one.
#[test]
fn only_files_in_the_bundled_folder_are_examples() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches");
    let subtractive = examples.join("subtractive.ron");
    assert!(is_example(&subtractive, Some(&examples)));
    assert!(is_example(&examples.join("../patches/./sine.ron"), Some(&examples)));
    assert!(!is_example(&subtractive, None));
    assert!(!is_example(&examples.join("missing.ron"), Some(&examples)));
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/dev/test-widgets.ron");
    assert!(!is_example(&dev, Some(&examples)));
}

/// A release marker identifies examples even when the running app
/// cannot locate the download folder, as on a translocated Mac launch.
#[test]
fn a_marked_release_folder_is_an_example_without_a_known_folder() {
    let tmp = TempDir::new("marked-examples");
    let patches = tmp.0.join("examples/patches");
    std::fs::create_dir_all(&patches).unwrap();
    let patch = patches.join("example.ron");
    std::fs::write(&patch, "").unwrap();
    assert!(!is_example(&patch, None));
    std::fs::write(patches.join(BUNDLED_MARKER), "").unwrap();
    assert!(is_example(&patch, None));

    let own = tmp.0.join("music/examples/patches");
    std::fs::create_dir_all(&own).unwrap();
    let own_patch = own.join("own.ron");
    std::fs::write(&own_patch, "").unwrap();
    assert!(!is_example(&own_patch, None));
}

#[test]
fn title_shows_file_name_and_mark() {
    let file = Path::new("/music/patches/bass.ron");
    assert_eq!(titled(&display_name(Some(file)), false), "bass.ron - LoomDSP");
    assert_eq!(titled(&display_name(Some(file)), true), "* bass.ron - LoomDSP");
    assert_eq!(titled(&display_name(None), false), "Untitled - LoomDSP");
}

#[test]
fn save_adds_the_extension_only_when_missing() {
    assert_eq!(with_extension(PathBuf::from("a/bass")), PathBuf::from("a/bass.ron"));
    assert_eq!(with_extension(PathBuf::from("a/bass.ron")), PathBuf::from("a/bass.ron"));
    assert_eq!(with_extension(PathBuf::from("a/bass.txt")), PathBuf::from("a/bass.txt"));
}

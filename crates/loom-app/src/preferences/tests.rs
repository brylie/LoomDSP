use super::*;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("loom-prefs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_full_file_reads_back() {
    let preferences = Preferences {
        output_safety: OutputSafety::Clean,
        show_activity: false,
        audio: AudioPrefs {
            device: DeviceChoice::Named("MOTU M4".into()),
            input: InputChoice::Named("Microphone".into()),
            sample_rate: RateChoice::Hz(96_000),
            buffer: BufferChoice::Frames(128),
        },
        panic: Some(PanicPref::Cc(0, 123)),
        midi: MidiPrefs {
            off: vec!["Pad Controller MIDI 1".into()],
        },
    };
    let text = to_text(&preferences);
    assert_eq!(
        text,
        r#"(
    version: 1,
    output_safety: "clean",
    show_activity: false,
    audio: (
        device: Named("MOTU M4"),
        input: Named("Microphone"),
        sample_rate: Hz(96000),
        buffer: Frames(128),
    ),
    midi: (
        off: [
            "Pad Controller MIDI 1",
        ],
    ),
    panic: Some(Cc(0, 123)),
)
"#
    );
    assert_eq!(parse(&text).unwrap(), (preferences, Vec::new()));
}

#[test]
fn missing_fields_take_their_defaults() {
    let (preferences, notes) = parse("(output_safety: \"off\")").unwrap();
    assert_eq!(preferences.output_safety, OutputSafety::Off);
    assert!(preferences.show_activity);
    assert_eq!(preferences.audio, AudioPrefs::default());
    assert!(notes.is_empty());
    let (preferences, _) = parse("(audio: (device: Named(\"Fake B\")))").unwrap();
    assert_eq!(preferences.audio.device, DeviceChoice::Named("Fake B".into()));
    assert_eq!(preferences.audio.sample_rate, RateChoice::DeviceDefault);
}

#[test]
fn a_bad_value_falls_back_alone() {
    let text =
        "(output_safety: \"loud\", audio: (sample_rate: Hz(\"fast\"), buffer: Frames(256)), midi: (off: [\"Pad\"]))";
    let (preferences, notes) = parse(text).unwrap();
    assert_eq!(preferences.output_safety, OutputSafety::LowLatency);
    assert_eq!(preferences.audio.sample_rate, RateChoice::DeviceDefault);
    assert_eq!(preferences.audio.buffer, BufferChoice::Frames(256));
    assert_eq!(preferences.midi.off, ["Pad"]);
    assert_eq!(
        notes,
        [
            "Preferences: unknown output_safety \"loud\". Using \"low-latency\".",
            "Preferences: unknown audio.sample_rate Hz(\"fast\"). Using DeviceDefault.",
        ]
    );
}

#[test]
fn unknown_settings_are_ignored_and_named() {
    let (preferences, notes) = parse("(theme: \"dark\", audio: (channels: 4, device: SystemDefault))").unwrap();
    assert_eq!(preferences, Preferences::default());
    assert_eq!(
        notes,
        [
            "Preferences: ignored unknown setting \"theme\".",
            "Preferences: ignored unknown setting \"audio.channels\".",
        ]
    );
}

#[test]
fn no_file_gives_the_defaults_and_creates_nothing() {
    let dir = temp_dir("none");
    let path = dir.join(FILE_NAME);
    let (store, notes) = Store::open(Some(path.clone()));
    assert_eq!(store.saved(), &Preferences::default());
    assert!(notes.is_empty());
    assert!(!path.exists());
}

#[test]
fn a_broken_file_is_kept_aside_on_the_first_write() {
    let dir = temp_dir("broken");
    let path = dir.join(FILE_NAME);
    std::fs::write(&path, "(output_safety: \"clean\"").unwrap();
    let (mut store, notes) = Store::open(Some(path.clone()));
    assert_eq!(store.saved().output_safety, OutputSafety::LowLatency);
    assert_eq!(notes.len(), 1);
    assert!(notes[0].starts_with(&format!("Could not read preferences \"{}\": ", path.display())));
    assert!(notes[0].ends_with(". Using defaults."));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "(output_safety: \"clean\"",
        "unchanged"
    );

    let mut preferences = store.saved().clone();
    preferences.output_safety = OutputSafety::Clean;
    store.save(preferences).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join("preferences.broken.ron")).unwrap(),
        "(output_safety: \"clean\""
    );
    assert_eq!(load(&path).preferences.output_safety, OutputSafety::Clean);
}

#[test]
fn each_rig_keeps_its_own_broken_copy() {
    let dir = temp_dir("rigs");
    for rig in ["studio", "stage"] {
        let path = dir.join(format!("{rig}.ron"));
        std::fs::write(&path, format!("{rig} (")).unwrap();
        let (mut store, _) = Store::open(Some(path));
        let mut preferences = store.saved().clone();
        preferences.midi.set("Pad", false);
        store.save(preferences).unwrap();
    }
    for rig in ["studio", "stage"] {
        assert_eq!(
            std::fs::read_to_string(dir.join(format!("{rig}.broken.ron"))).unwrap(),
            format!("{rig} (")
        );
    }
}

#[test]
fn saving_what_is_saved_does_not_write() {
    let dir = temp_dir("same");
    let path = dir.join(FILE_NAME);
    let mut store = Store::at(Some(path.clone()));
    store.save(Preferences::default()).unwrap();
    assert!(!path.exists(), "nothing changed, nothing written");
    let mut preferences = Preferences::default();
    preferences.audio.buffer = BufferChoice::Frames(64);
    store.save(preferences.clone()).unwrap();
    let written = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    store.save(preferences).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), written);
}

#[test]
fn a_failed_write_keeps_the_value_for_this_run() {
    let dir = temp_dir("readonly");
    // A file where the folder should be: the folder cannot be made.
    let blocker = dir.join("blocked");
    std::fs::write(&blocker, "").unwrap();
    let mut store = Store::at(Some(blocker.join(FILE_NAME)));
    let preferences = Preferences {
        output_safety: OutputSafety::Clean,
        ..Preferences::default()
    };
    let error = store.save(preferences).unwrap_err();
    assert!(error.ends_with("The change applies until you quit."), "{error}");
    assert_eq!(store.saved().output_safety, OutputSafety::Clean);
}

#[test]
fn the_default_path_follows_each_os() {
    let env = |vars: &'static [(&'static str, &'static str)]| {
        move |name: &str| vars.iter().find(|(n, _)| *n == name).map(|(_, v)| v.to_string())
    };
    assert_eq!(
        path_for(Os::Mac, env(&[("HOME", "/Users/ada")])),
        Some(PathBuf::from(
            "/Users/ada/Library/Application Support/LoomDSP/preferences.ron"
        ))
    );
    assert_eq!(
        path_for(Os::Windows, env(&[("LOCALAPPDATA", "C:/Users/ada/AppData/Local")])),
        Some(PathBuf::from("C:/Users/ada/AppData/Local/LoomDSP/preferences.ron"))
    );
    assert_eq!(
        path_for(Os::Unix, env(&[("HOME", "/home/ada"), ("XDG_CONFIG_HOME", "/cfg")])),
        Some(PathBuf::from("/cfg/loomdsp/preferences.ron"))
    );
    for unset in [
        &[("HOME", "/home/ada")][..],
        &[("HOME", "/home/ada"), ("XDG_CONFIG_HOME", "")],
    ] {
        assert_eq!(
            path_for(Os::Unix, env(unset)),
            Some(PathBuf::from("/home/ada/.config/loomdsp/preferences.ron"))
        );
    }
    assert_eq!(path_for(Os::Windows, env(&[])), None);
}

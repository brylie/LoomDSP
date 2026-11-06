use super::*;

fn patch(text: &str) -> Patch {
    Patch::from_ron(text).unwrap()
}

fn resolve(text: &str, options: ResolveOptions) -> Result<Vec<ResolvedModule>, ResolveError> {
    patch(text)
        .resolve(&loom_modules::registry(), options)
        .map(|(modules, _)| modules)
}

/// Difference 1: a kind with no version. Clipboard text may leave it out,
/// and gets the registered module. A patch file may not.
#[test]
fn only_the_clipboard_accepts_a_kind_with_no_version() {
    let text = r#"Patch(version: 1, modules: [(id: 1, kind: "loom.osc.sine")])"#;
    let pasted = resolve(text, ResolveOptions::CLIPBOARD).unwrap();
    assert_eq!(pasted[0].descriptor.id, "loom.osc.sine");
    assert!(pasted[0].unknown.is_none());
    let loaded = resolve(text, ResolveOptions::FILE).unwrap_err();
    assert!(
        matches!(
            loaded,
            ResolveError::Patch(PatchError::Engine(LoomError::InvalidKind(_)))
        ),
        "{loaded:?}"
    );
}

/// Difference 2: an unknown parameter fails both, with the facts each
/// caller needs for its own message. Loading turns it into
/// `PatchError::UnknownParam`; the clipboard names the module type.
#[test]
fn an_unknown_parameter_fails_both_with_the_module_name() {
    let text = r#"Patch(version: 1, modules: [(id: 4, kind: "loom.osc.sine@1", params: {"nope": 1.0})])"#;
    for options in [ResolveOptions::FILE, ResolveOptions::CLIPBOARD] {
        let error = resolve(text, options).unwrap_err();
        let ResolveError::UnknownParam {
            module,
            module_name,
            ref param,
        } = error
        else {
            panic!("{error:?}");
        };
        assert_eq!((module, module_name, param.as_str()), (4, "Sine", "nope"));
        assert!(matches!(
            PatchError::from(error),
            PatchError::UnknownParam { module: 4, ref param } if param == "nope"
        ));
    }
}

/// Difference 3: a saved range on a parameter the module does not have.
/// A patch file is refused; the clipboard ignores the range.
#[test]
fn only_a_file_refuses_a_range_on_a_missing_parameter() {
    let text = r#"Patch(version: 1, modules: [(id: 2, kind: "loom.math.add@1", ranges: {"nope": (0.0, 1.0)})])"#;
    assert!(matches!(
        resolve(text, ResolveOptions::FILE),
        Err(ResolveError::UnknownParam { module: 2, .. })
    ));
    let pasted = resolve(text, ResolveOptions::CLIPBOARD).unwrap();
    let defaults: Vec<_> = pasted[0]
        .descriptor
        .params
        .iter()
        .map(ParamSpec::default_range)
        .collect();
    assert_eq!(pasted[0].ranges, defaults, "the range is ignored");
}

/// Difference 4 cannot happen today: the loader used to choose ranges
/// from raw file values and the clipboard from values with transient
/// parameters released. Both now use released values. That changes nothing
/// while no transient parameter offers ranges, which this test checks.
#[test]
fn no_transient_parameter_offers_ranges() {
    for descriptor in loom_modules::registry().iter() {
        for spec in descriptor.params.iter().filter(|spec| spec.is_transient()) {
            assert!(
                spec.options.ranges.is_empty(),
                "{} `{}` is transient and offers ranges: decide which value picks its range on load",
                descriptor.id,
                spec.name
            );
        }
    }
}

/// A saved transient value is released, and loading does not set it.
#[test]
fn a_transient_value_is_released_and_not_set() {
    let text = r#"Patch(version: 1, modules: [(id: 7, kind: "loom.ui.button@1", params: {"mode": 1.0, "hold": 1.0})])"#;
    let module = &resolve(text, ResolveOptions::FILE).unwrap()[0];
    let hold = module.descriptor.param_index("hold").unwrap();
    let mode = module.descriptor.param_index("mode").unwrap();
    assert_eq!(module.params[hold], module.descriptor.params[hold].default);
    assert_eq!(module.file_params, [mode]);
}

/// Every example patch resolves the same way for a file and for the
/// clipboard: the same descriptors, values, ranges, labels, and layout. So
/// copying a whole example gives what opening it gives.
#[test]
fn file_and_clipboard_agree_on_every_example() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches");
    let registry = loom_modules::registry();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "ron") {
            continue;
        }
        let patch = patch(&std::fs::read_to_string(&path).unwrap());
        let (file, file_notes) = patch.resolve(&registry, ResolveOptions::FILE).unwrap();
        let (clip, clip_notes) = patch.resolve(&registry, ResolveOptions::CLIPBOARD).unwrap();
        assert_eq!(file_notes, clip_notes, "{}", path.display());
        assert_eq!(file.len(), clip.len());
        for (a, b) in file.iter().zip(&clip) {
            let at = format!("{} module {}", path.display(), a.id);
            assert!(std::ptr::eq(a.descriptor, b.descriptor), "{at}");
            assert_eq!(a.params, b.params, "{at}");
            assert_eq!(a.ranges, b.ranges, "{at}");
            assert_eq!(a.label, b.label, "{at}");
            assert_eq!(a.layout, b.layout, "{at}");
        }
    }
}

/// A resolved module writes back the module it was read from, so a load
/// and a save change nothing.
#[test]
fn writing_a_resolved_module_gives_back_the_saved_module() {
    let text = r#"Patch(version: 1, modules: [
        (id: 3, kind: "loom.math.add@1", label: Some("Sum"), params: {"offset": 0.5}, position: Some((40.0, 80.0)), size: (6, 4)),
        (id: 9, kind: "future.effect@2", label: Some("Shimmer"), params: {"mix": 0.0}, ranges: {"mix": (0.0, 1.0)}),
    ])"#;
    let patch = patch(text);
    let (modules, _) = patch.resolve(&loom_modules::registry(), ResolveOptions::FILE).unwrap();
    let written: Vec<PatchModule> = modules.iter().map(ResolvedModule::write).collect();
    assert_eq!(written, patch.modules);
}

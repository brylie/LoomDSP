//! The crate has no dependencies at all, so it knows no UI toolkit and no
//! LoomDSP crate (docs/design/ui-layer.md, section 1, rule 1). Dev
//! dependencies are allowed, because they only build its tests.

/// True for a TOML key path, such as `target.'cfg(unix)'.dependencies.foo`,
/// that gives users of the crate a dependency.
fn is_user_dependency(path: &str) -> bool {
    path.split('.')
        .any(|part| part == "dependencies" || part == "build-dependencies")
}

/// The key paths in `manifest` that would give users a dependency.
fn user_dependencies(manifest: &str) -> Vec<String> {
    let mut section = String::new();
    let mut found = Vec::new();
    for line in manifest.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            // Up to the bracket, so a comment after a header is left out.
            section = rest.split(']').next().unwrap_or(rest).trim().to_string();
        } else {
            let key = line.split('=').next().unwrap_or(line).trim();
            let path = format!("{section}.{key}");
            if is_user_dependency(&path) {
                found.push(path);
            }
        }
    }
    found
}

/// The manifest lists no normal or build dependency, on any target.
#[test]
fn the_crate_has_no_dependencies() {
    assert_eq!(user_dependencies(include_str!("../Cargo.toml")), Vec::<String>::new());
}

/// The check sees a dependency in each place a manifest can declare one, and
/// leaves dev dependencies and other tables alone.
#[test]
fn the_check_finds_a_dependency_wherever_it_is_declared() {
    let manifest = "
        [package]
        name = \"x\"
        [dependencies]
        iced = \"0.14\"
        [dependencies.loom-core]
        path = \"../loom-core\"
        [target.'cfg(unix)'.dependencies]
        libc = \"1\"
        [build-dependencies]
        cc = \"1\"
        [dev-dependencies]
        naga = \"27\"
        [lints]
        workspace = true
    ";
    assert_eq!(
        user_dependencies(manifest),
        [
            "dependencies.iced",
            "dependencies.loom-core.path",
            "target.'cfg(unix)'.dependencies.libc",
            "build-dependencies.cc",
        ]
    );
}

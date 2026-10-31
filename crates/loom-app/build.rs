//! Bundles module notes for the offline help view.

use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let pages = root.join("../../docs/modules");
    println!("cargo:rerun-if-changed={}", pages.display());
    let mut files: Vec<_> = fs::read_dir(&pages)
        .expect("module pages")
        .map(|entry| entry.expect("module page").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    files.sort();
    let mut generated = String::from("fn bundled_page(id: &str) -> Option<&'static str> {\n    match id {\n");
    for path in files {
        let id = path.file_stem().expect("page name").to_string_lossy();
        if id == "README" {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        generated.push_str(&format!("        {id:?} => Some(include_str!({:?})),\n", path));
    }
    generated.push_str("        _ => None,\n    }\n}\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").expect("output directory")).join("help_pages.rs"),
        generated,
    )
    .expect("write bundled page map");
}

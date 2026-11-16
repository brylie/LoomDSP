/// Quiet text stays readable wherever it sits: secondary text on the
/// panels and on the browser's highlighted row, a closed tab's title,
/// and a quiet pick list's handle and placeholder, all at 4.5:1 or
/// more (WCAG 1.4.3).
#[test]
fn quiet_text_meets_contrast() {
    use loom_canvas::iced_convert::ToCore as _;
    use loom_canvas::{MIN_TEXT_CONTRAST, contrast_ratio};
    let app = crate::tests::app();
    let theme = app.theme();
    let palette = theme.extended_palette();
    let secondary = super::QUIET;
    let highlight = crate::browser::row_style(&theme, true)
        .background
        .map(|b| match b {
            iced::Background::Color(c) => c,
            _ => panic!("a flat highlight"),
        })
        .expect("a highlight fill");
    let closed_tab = super::tab(false)(&theme, iced::widget::button::Status::Active).text_color;
    let pick = super::quiet_pick_list(&theme, iced::widget::pick_list::Status::Active);
    for (name, text, fill) in [
        ("quiet text on the panel", secondary, palette.background.base.color),
        ("quiet text on a card", secondary, palette.background.weak.color),
        ("quiet text on the highlighted row", secondary, highlight),
        ("a closed tab's title", closed_tab, palette.background.base.color),
        (
            "a quiet pick list's handle on a card",
            pick.handle_color,
            palette.background.weak.color,
        ),
        (
            "a quiet pick list's placeholder on a card",
            pick.placeholder_color,
            palette.background.weak.color,
        ),
    ] {
        let ratio = contrast_ratio(text.to_core(), fill.to_core());
        assert!(ratio >= MIN_TEXT_CONTRAST, "{name}: {ratio:.2}:1");
    }
}

/// The app's Rust files under `dir`, in every folder, with `style.rs` and
/// test files left out: tests may write a value to check it.
fn app_sources(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("a source folder") {
        let path = entry.expect("a source entry").path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "tests" {
                app_sources(&path, files);
            }
        } else if path.extension().is_some_and(|e| e == "rs")
            && name != "style.rs"
            && name != "tests.rs"
            && !name.ends_with("_tests.rs")
        {
            files.push(path);
        }
    }
}

/// The calls whose first argument is a size, and so must be a token: a
/// literal number there is written inline.
const SIZE_CALLS: [&str; 4] = [".size(", "text_size(", ".radius(", ".border("];

/// The calls whose argument is a colour, and so must be a token: a colour
/// built there is written inline.
const COLOUR_CALLS: [&str; 2] = [".background(", ".colour("];

/// How a colour is built inline.
const COLOUR_BUILDERS: [&str; 5] = [
    "Color::rgb",
    "Color::from_rgb",
    "Color::rgba",
    "Color::from_rgba",
    "Color {",
];

/// No app source writes a size or a colour inline: every `.size(...)`,
/// `text_size(...)`, `.radius(...)`, and `.border(...)` takes a token from
/// this module, as does every `.background(...)` and `.colour(...)` of a
/// `loom-ui` element; and no view builds an Iced button, whose default role
/// is primary. Only this module may (visual-system.md, rule 1). Every file
/// is read, in every folder.
#[test]
fn sizes_and_buttons_come_only_from_this_module() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    app_sources(&dir, &mut files);
    assert!(files.len() > 50, "the scan found the app's files, not {}", files.len());
    let mut problems = Vec::new();
    for path in files {
        let source = std::fs::read_to_string(&path).expect("a readable source file");
        let name = path.strip_prefix(&dir).unwrap_or(&path).display().to_string();
        for (n, line) in source.lines().enumerate() {
            for call in SIZE_CALLS {
                for (at, _) in line.match_indices(call) {
                    let arg = line[at + call.len()..].trim_start();
                    if arg.starts_with(|c: char| c.is_ascii_digit()) {
                        problems.push(format!("{name}:{}: {}", n + 1, line.trim()));
                    }
                }
            }
            for call in COLOUR_CALLS {
                for (at, _) in line.match_indices(call) {
                    let arg = line[at + call.len()..].trim_start();
                    let arg = arg.strip_prefix("iced::").unwrap_or(arg);
                    let arg = arg.strip_prefix("loom_ui_core::").unwrap_or(arg);
                    if COLOUR_BUILDERS.iter().any(|built| arg.starts_with(built)) {
                        problems.push(format!("{name}:{}: {}", n + 1, line.trim()));
                    }
                }
            }
            let imports_button = line.contains("use iced::widget::")
                && (line.contains("button") || source_import_has_button(&source, n));
            if imports_button {
                problems.push(format!("{name}:{}: imports Iced's button", n + 1));
            }
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

/// True when the `use iced::widget::{...}` that starts on line `n`
/// lists `button` on one of its later lines.
fn source_import_has_button(source: &str, n: usize) -> bool {
    let rest: Vec<&str> = source.lines().skip(n).collect();
    let Some(end) = rest.iter().position(|l| l.contains("};")) else {
        return false;
    };
    rest[..=end].iter().any(|l| {
        l.split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|w| w == "button")
    })
}

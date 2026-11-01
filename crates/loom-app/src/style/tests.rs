/// Quiet text stays readable wherever it sits: secondary text on the
/// panels and on the browser's highlighted row, a closed tab's title,
/// and a quiet pick list's handle and placeholder, all at 4.5:1 or
/// more (WCAG 1.4.3).
#[test]
fn quiet_text_meets_contrast() {
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
        let ratio = contrast_ratio(text, fill);
        assert!(ratio >= MIN_TEXT_CONTRAST, "{name}: {ratio:.2}:1");
    }
}

/// No app source writes a size inline: every `.size(...)` and
/// `text_size(...)` takes a token from this module, and no view builds
/// an Iced button, whose default role is primary. Only this module may
/// (visual-system.md, rule 1).
#[test]
fn sizes_and_buttons_come_only_from_this_module() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut problems = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("the source folder") {
        let path = entry.expect("a source file").path();
        if path.extension().is_none_or(|e| e != "rs") || path.ends_with("style.rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("a readable source file");
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        for (n, line) in source.lines().enumerate() {
            for call in [".size(", "text_size("] {
                for (at, _) in line.match_indices(call) {
                    let arg = line[at + call.len()..].trim_start();
                    if arg.starts_with(|c: char| c.is_ascii_digit()) {
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

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
    let highlight = {
        use loom_canvas::iced_convert::ToIced as _;
        super::highlighted_row(&theme).fill.to_iced()
    };
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

/// Marks that identify a control are readable on both panels and cards.
#[test]
fn active_control_marks_and_family_tints_meet_contrast() {
    use iced::widget::checkbox::Status;
    use loom_canvas::iced_convert::ToCore as _;
    use loom_canvas::{Palette, contrast_ratio};
    let theme = crate::tests::app().theme();
    let app = theme.extended_palette();
    let canvas = Palette::default();
    for fill in [app.background.base.color, app.background.weak.color] {
        for family in loom_core::Family::ALL {
            assert!(
                contrast_ratio(crate::browser::family_swatch(family), fill.to_core()) >= 3.0,
                "{family:?}"
            );
        }
        for status in [
            Status::Active { is_checked: false },
            Status::Hovered { is_checked: false },
            Status::Active { is_checked: true },
            Status::Hovered { is_checked: true },
        ] {
            let style = super::check_box(&theme, status);
            assert!(
                contrast_ratio(style.border.color.to_core(), fill.to_core()) >= 3.0,
                "{status:?}"
            );
            if matches!(
                status,
                Status::Active { is_checked: true } | Status::Hovered { is_checked: true }
            ) {
                let iced::Background::Color(background) = style.background else {
                    panic!("solid check box");
                };
                assert!(contrast_ratio(style.icon_color.to_core(), background.to_core()) >= 3.0);
            }
        }
    }
    for (mark, fill) in [
        (canvas.tile_border, canvas.background),
        (canvas.control_edge, canvas.control_body),
        (canvas.control_edge, canvas.tile),
        (canvas.background, canvas.control_value),
        (canvas.light_off, canvas.tile),
        (canvas.focus, canvas.tile),
    ] {
        assert!(contrast_ratio(mark, fill) >= 3.0, "{mark:?} on {fill:?}");
    }
    assert!(contrast_ratio(canvas.text, canvas.control_track) >= loom_canvas::MIN_TEXT_CONTRAST);
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

/// How a colour is built inline. Matched by prefix, so `Color::rgb` also
/// finds `rgba`, `rgb8`, and `rgba8`, and `Color::from_rgb` their `from_`
/// forms.
const COLOUR_BUILDERS: [&str; 3] = ["Color::rgb", "Color::from_rgb", "Color {"];

/// The call whose second argument is a colour, after a size.
const BORDER: &str = ".border(";

/// True when `arg` starts by building a colour inline, with or without the
/// crate's path in front.
fn built_inline(arg: &str) -> bool {
    let arg = arg.strip_prefix("iced::").unwrap_or(arg);
    let arg = arg.strip_prefix("loom_ui_core::").unwrap_or(arg);
    COLOUR_BUILDERS.iter().any(|built| arg.starts_with(built))
}

/// The 1-based lines of `source` where a size or a colour is written
/// inline: a literal number as the first argument of a size call, or a
/// colour built inside a colour call or as a border's colour. When rustfmt
/// breaks a call after its `(`, the argument is read from the next line
/// that is not empty.
fn inline_values(source: &str) -> Vec<usize> {
    let lines: Vec<&str> = source.lines().collect();
    // The argument after `call` at byte `at` of line `n`.
    let argument = |n: usize, at: usize, call: &str| -> &str {
        let rest = lines[n][at + call.len()..].trim_start();
        if !rest.is_empty() {
            return rest;
        }
        lines[n + 1..]
            .iter()
            .map(|line| line.trim_start())
            .find(|line| !line.is_empty())
            .unwrap_or("")
    };
    let mut found = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        let size = SIZE_CALLS.iter().any(|call| {
            line.match_indices(call)
                .any(|(at, _)| argument(n, at, call).starts_with(|c: char| c.is_ascii_digit()))
        });
        let colour = COLOUR_CALLS.iter().any(|call| {
            line.match_indices(call)
                .any(|(at, _)| built_inline(argument(n, at, call)))
        });
        // `.border(width, colour)`: its colour is the argument after the
        // first comma, on the same line or on a later one.
        let border_colour = line.match_indices(BORDER).any(|(at, _)| {
            let rest = std::iter::once(&lines[n][at + BORDER.len()..])
                .chain(lines[n + 1..].iter().copied().take(3))
                .collect::<Vec<_>>()
                .join(" ");
            rest.split_once(',')
                .is_some_and(|(_, after)| built_inline(after.trim_start()))
        });
        if size || colour || border_colour {
            found.push(n + 1);
        }
    }
    found
}

/// The matcher finds a value on the call's own line and on the next line
/// when rustfmt split the call, and leaves tokens alone.
#[test]
fn the_matcher_finds_inline_values_on_split_lines_too() {
    let source = "\
e.radius(4.0)
e.background(
    Color::rgba(0.1, 0.2, 0.3, 1.0),
)
e.border(

    2.0,
    style::EDGE,
)
e.radius(style::RADIUS).background(style::PANEL)
e.colour(
    style::TEXT,
)
e.border(style::EDGE, Color::rgb(1.0, 0.0, 0.0))
e.border(
    style::EDGE,
    loom_ui_core::Color::rgb(1.0, 0.0, 0.0),
)
e.border(style::EDGE, style::LINE)
";
    assert_eq!(inline_values(source), [1, 2, 5, 14, 15]);
}

/// The focus ring stands out at 3:1 or more against every background a
/// panel part sits on: the panel, a card, and the highlighted row (WCAG
/// 2.2, 1.4.11 and 2.4.13), and is at least 2 logical pixels wide.
#[test]
fn the_focus_ring_stands_out() {
    use loom_canvas::contrast_ratio;
    use loom_canvas::iced_convert::ToCore as _;
    let ring = super::focus_ring();
    assert!(ring.width >= 2.0, "{} px", ring.width);
    let app = crate::tests::app();
    let theme = app.theme();
    let palette = theme.extended_palette();
    let highlight = {
        use loom_canvas::iced_convert::ToIced as _;
        super::highlighted_row(&theme).fill.to_iced()
    };
    for (name, fill) in [
        ("the panel", palette.background.base.color),
        ("a card", palette.background.weak.color),
        ("the highlighted row", highlight),
    ] {
        let ratio = contrast_ratio(ring.color, fill.to_core());
        assert!(ratio >= 3.0, "on {name}: {ratio:.2}:1");
    }
    // The ring is drawn `offset` outside the element, so a strip of the
    // panel's colour lies between it and a button's fill: its neighbours
    // on both sides are the panel's colour, checked above, whatever the
    // button's role (WCAG 2.2, SC 2.4.13 measures a focus indicator
    // against what is next to it). Against the fills themselves it is
    // 1.1:1 to 2.3:1 for the filled roles, which the gap makes harmless.
    assert!(
        ring.offset >= 1.0,
        "a gap between the ring and a fill: {} px",
        ring.offset
    );
}

/// A scroll container's thumb stands out at 3:1 or more against the panel
/// it runs over, since no track is drawn (WCAG 2.2, 1.4.11), and its
/// tokens are the spec's: 6 px wide, 2 px in, round ends (ui-layer.md,
/// section 5, rule 12).
#[test]
fn the_scrollbar_thumb_stands_out_against_the_panel() {
    use loom_canvas::contrast_ratio;
    use loom_canvas::iced_convert::ToCore as _;
    let scroll = super::scroll();
    assert_eq!((scroll.width, scroll.inset, scroll.radius), (6.0, 2.0, 3.0));
    let app = crate::tests::app();
    let theme = app.theme();
    let panel = theme.extended_palette().background.base.color;
    let ratio = contrast_ratio(scroll.thumb, panel.to_core());
    assert!(ratio >= 3.0, "{ratio:.2}:1");
}

/// The ring's reach is the room a scroll container keeps around its
/// content, so a focused part at its edge keeps its whole ring.
#[test]
fn a_scroll_container_keeps_room_for_the_ring() {
    let ring = super::focus_ring();
    assert_eq!(super::scroll().ring, ring.offset + ring.width);
}

/// A radio's note, in the panel style's quiet colour, reads at 4.5:1 or
/// more against the panel (WCAG 2.2, 1.4.3), and a radio's mark at 3:1 or
/// more (1.4.11; ui-layer.md, section 6, rule 9).
#[test]
fn a_radio_note_and_mark_read_against_the_panel() {
    use loom_canvas::contrast_ratio;
    use loom_canvas::iced_convert::ToCore as _;
    let app = crate::tests::app();
    let theme = app.theme();
    let panel = theme.extended_palette().background.base.color.to_core();
    let style = crate::iced_bridge::paint::panel_style(&theme);
    let note = contrast_ratio(style.quiet, panel);
    assert!(note >= 4.5, "the note: {note:.2}:1");
    let mark = contrast_ratio(style.mark, panel);
    assert!(mark >= 3.0, "the mark: {mark:.2}:1");
}

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
        let lines: Vec<&str> = source.lines().collect();
        for at in inline_values(&source) {
            problems.push(format!("{name}:{at}: {}", lines[at - 1].trim()));
        }
        for (n, line) in source.lines().enumerate() {
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

/// Every canvas menu state meets the text and edge thresholds on the
/// colours the tile and probe menu renderers actually use.
#[test]
fn canvas_menu_contrast() {
    use iced::widget::button::Status;
    use loom_canvas::iced_convert::ToCore as _;
    use loom_canvas::{MIN_TEXT_CONTRAST, Palette, contrast_ratio};
    let theme = crate::tests::app().theme();
    let menu = super::canvas_menu(&theme);
    let Some(iced::Background::Color(panel)) = menu.background else {
        panic!("opaque panel fill")
    };
    assert_eq!(panel.a, 1.0);
    for background in [Palette::default().background, Palette::default().tile, panel.to_core()] {
        let ratio = contrast_ratio(menu.border.color.to_core(), background);
        assert!(menu.border.width >= 1.0 && ratio >= 3.0, "menu edge {ratio:.2}:1");
    }
    for (status, focused) in [
        (Status::Active, false),
        (Status::Hovered, false),
        (Status::Pressed, false),
        (Status::Active, true),
        (Status::Disabled, false),
    ] {
        let row = super::menu_item(focused)(&theme, status);
        let Some(iced::Background::Color(fill)) = row.background else {
            panic!("opaque row fill")
        };
        assert_eq!((row.text_color.a, fill.a, row.border.color.a), (1.0, 1.0, 1.0));
        let text = contrast_ratio(row.text_color.to_core(), fill.to_core());
        let edge = contrast_ratio(row.border.color.to_core(), fill.to_core());
        let outside = contrast_ratio(row.border.color.to_core(), panel.to_core());
        assert!(text >= MIN_TEXT_CONTRAST, "{status:?} text {text:.2}:1");
        assert!(
            edge.min(outside) >= 3.0,
            "{status:?} edge {edge:.2}:1 inside, {outside:.2}:1 outside"
        );
    }
}

/// The panel background the role tokens are set for: the one dark theme's
/// (`boot.rs`). A light theme needs a token table of its own.
const ROLE_BACKGROUND: loom_ui_core::Color = loom_ui_core::Color::rgb8(0x22, 0x24, 0x2A);

/// One role's rows of the token table: its name and tokens, text on fill
/// for normal, hovered, pressed, and off, fill on the background for the
/// same, and hovered and pressed against normal.
type Row = (&'static str, super::RoleTokens, [f32; 4], [f32; 4], [f32; 2]);

/// Every role's tokens meet visual-system.md, Role tokens, and match its
/// table to two decimals: text at least 4.5:1 on its fill (or on the
/// background, for a state with no fill) in every state; a turned-on fill
/// of a filled role at least 3:1 against the background; hovered and
/// pressed at least 1.3:1 against normal for a filled role, and plain's
/// hover at least 1.5:1 on the background, with pressed a further step.
#[test]
fn the_role_tokens_meet_their_table() {
    use super::{DANGER, PLAIN, PRIMARY, RoleState, SECONDARY};
    use loom_canvas::contrast_ratio;
    let app = crate::tests::app();
    let background = app.theme().extended_palette().background.base.color;
    assert_eq!(
        loom_canvas::iced_convert::ToCore::to_core(background),
        ROLE_BACKGROUND,
        "the theme's background is the one the table is set for"
    );
    let round = |ratio: f32| (ratio * 100.0).round() / 100.0;
    let on = |state: RoleState| state.fill.unwrap_or(ROLE_BACKGROUND);
    let text = |state: RoleState| round(contrast_ratio(state.text, on(state)));
    let edge = |state: RoleState| round(contrast_ratio(on(state), ROLE_BACKGROUND));
    let step = |a: RoleState, b: RoleState| round(contrast_ratio(on(a), on(b)));
    // Each role's table rows: text on fill for normal, hovered, pressed,
    // off; fill on background for the same; and hovered and pressed
    // against normal.
    let table: [Row; 4] = [
        (
            "primary",
            PRIMARY,
            [8.31, 10.87, 6.31, 4.60],
            [7.65, 10.01, 5.80, 1.83],
            [1.31, 1.32],
        ),
        (
            "secondary",
            SECONDARY,
            [5.03, 6.61, 4.51, 4.75],
            [4.05, 5.31, 3.10, 1.52],
            [1.31, 1.31],
        ),
        (
            "danger",
            DANGER,
            [4.96, 6.56, 4.63, 5.01],
            [3.99, 5.28, 3.02, 1.46],
            [1.32, 1.32],
        ),
        (
            "plain",
            PLAIN,
            [12.66, 7.94, 6.02, 4.98],
            [1.00, 1.59, 2.10, 1.00],
            [1.59, 2.10],
        ),
    ];
    for (name, tokens, texts, edges, steps) in table {
        let states = [tokens.normal, tokens.hovered, tokens.pressed, tokens.off];
        for (state, (want_text, want_edge)) in states.iter().zip(texts.iter().zip(edges)) {
            assert_eq!(text(*state), *want_text, "{name}: text");
            assert_eq!(edge(*state), want_edge, "{name}: fill on the background");
            assert!(text(*state) >= 4.5, "{name}: text at 4.5:1 or more");
        }
        assert_eq!(
            [step(tokens.hovered, tokens.normal), step(tokens.pressed, tokens.normal)],
            steps,
            "{name}: states against normal"
        );
        if name == "plain" {
            assert!(edge(tokens.hovered) >= 1.5, "plain's hover shows on the bar");
            assert!(
                step(tokens.pressed, tokens.hovered) >= 1.3,
                "plain's press is a step past hover"
            );
        } else {
            for state in &states[..3] {
                assert!(edge(*state) >= 3.0, "{name}: a turned-on fill at 3:1 or more");
            }
            assert!(steps.iter().all(|step| *step >= 1.3), "{name}: no state is lost");
        }
    }
}

/// `loom-ui`'s buttons and Iced's role style functions take the same
/// tokens, state for state: one source for both toolkits.
#[test]
fn both_toolkits_read_the_same_role_tokens() {
    use super::Role;
    use iced::widget::button::Status;
    use loom_canvas::iced_convert::ToIced as _;
    let app = crate::tests::app();
    let theme = app.theme();
    let iced_style = |role: Role| match role {
        Role::Primary => super::primary,
        Role::Secondary => super::secondary,
        Role::Plain => super::plain,
        Role::Danger => |_: &iced::Theme, status| Role::Danger.iced_for_tests(status),
    };
    for role in [Role::Primary, Role::Secondary, Role::Danger, Role::Plain] {
        let looks = role.looks();
        let style = iced_style(role);
        for (status, state) in [
            (Status::Active, looks.normal),
            (Status::Hovered, looks.hovered),
            (Status::Pressed, looks.pressed),
            (Status::Disabled, looks.off),
        ] {
            let iced = style(&theme, status);
            let fill = match iced.background {
                Some(iced::Background::Color(colour)) => Some(colour),
                None => None,
                Some(other) => panic!("{role:?}: a plain fill, not {other:?}"),
            };
            assert_eq!(fill, state.background.map(|c| c.to_iced()), "{role:?} {status:?}: fill");
            assert_eq!(
                Some(iced.text_color),
                state.text.map(|c| c.to_iced()),
                "{role:?} {status:?}: text"
            );
        }
    }
}

/// Help's links read at 4.5:1 or more against the dialog they sit on
/// (WCAG 2.2, 1.4.3), always. Against the body text beside them, a link
/// that reaches less than 3:1 cannot be told apart by colour alone, so it
/// must be underlined (1.4.1, technique G183); at 3:1 or more the
/// underline is optional. Today's link colour is 1.65:1 against the body
/// text, which is why the link token underlines.
#[test]
fn help_links_read_and_stand_apart_from_the_text() {
    use loom_canvas::contrast_ratio;
    let theme = crate::tests::app().theme();
    let dialog = super::dialog(&theme);
    let link = super::link(&theme);
    let on_dialog = contrast_ratio(link.colour, dialog.fill);
    assert!(on_dialog >= 4.5, "on the dialog: {on_dialog:.2}:1");
    let beside_text = contrast_ratio(link.colour, dialog.text);
    assert!(
        beside_text >= 3.0 || link.underline,
        "{beside_text:.2}:1 against the body text, and no underline"
    );
    let quiet = contrast_ratio(loom_canvas::iced_convert::ToCore::to_core(super::QUIET), dialog.fill);
    assert!(quiet >= 4.5, "quiet text on the dialog: {quiet:.2}:1");
}

/// The module browser's highlighted row keeps its text readable on its
/// fill, and its outline stands out at 3:1 or more against the panel
/// (1.4.3, 1.4.11).
#[test]
fn the_highlighted_row_reads_and_stands_out() {
    use loom_canvas::contrast_ratio;
    let theme = crate::tests::app().theme();
    let panel = super::floating_panel(&theme);
    let row = super::highlighted_row(&theme);
    let text = contrast_ratio(panel.text, row.fill);
    assert!(text >= 4.5, "text on the row: {text:.2}:1");
    let quiet = contrast_ratio(loom_canvas::iced_convert::ToCore::to_core(super::QUIET), row.fill);
    assert!(quiet >= 4.5, "quiet text on the row: {quiet:.2}:1");
    let outline = contrast_ratio(row.border, panel.fill);
    assert!(outline >= 3.0, "the outline: {outline:.2}:1");
}

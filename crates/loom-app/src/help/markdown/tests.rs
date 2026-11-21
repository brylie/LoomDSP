use super::*;

/// The words of each block of `notes`, in order.
fn words(notes: &str) -> Vec<String> {
    parse(notes).blocks.iter().map(Block::words).collect()
}

/// Notes split into one block per paragraph, heading, list entry, and code
/// block, with a paragraph's lines joined by a space.
#[test]
fn notes_split_into_paragraphs_entries_headings_and_code() {
    let text = "## Use\n\nA first\nparagraph.\n\n- one\n- two\n\n```ron\nPatch()\n```\nLast.";
    assert_eq!(
        words(text),
        ["Use", "A first paragraph.", "one", "two", "Patch()", "Last."]
    );
    let kinds: Vec<_> = parse(text)
        .blocks
        .iter()
        .map(|block| match block {
            Block::Text { kind, .. } => Some(*kind),
            Block::Code(_) => None,
        })
        .collect();
    assert_eq!(
        kinds,
        [
            Some(Kind::Heading),
            Some(Kind::Paragraph),
            Some(Kind::Bullet),
            Some(Kind::Bullet),
            None,
            Some(Kind::Paragraph),
        ]
    );
}

/// A link reads as its label in its block's words, and is a link of the
/// block with its target. Bold and code marks are not read. A bracket that
/// is not a link stays as it is.
#[test]
fn a_link_reads_as_its_label_and_is_a_link_of_its_block() {
    let parsed = parse("See [the **clock**](loom.clock.clock.md) and [x] `here`.");
    let [block] = parsed.blocks.as_slice() else {
        panic!("one block: {:?}", parsed.blocks);
    };
    assert_eq!(block.words(), "See the clock and [x] here.");
    assert_eq!(
        block.links(),
        [Link {
            label: "the clock".to_string(),
            target: "loom.clock.clock.md".to_string(),
        }]
    );
    // A bracket before a link is not joined to it.
    let parsed = parse("See [x] then [there](a.md).");
    assert_eq!(parsed.blocks[0].words(), "See [x] then there.");
    assert_eq!(parsed.blocks[0].links()[0].label, "there");
}

/// Each piece says how it is set, and a link's pieces name their link, so
/// the view can draw them and the tree read them from the same blocks.
#[test]
fn pieces_carry_bold_code_and_their_link() {
    let parsed = parse("Use **two** `delay1`, see [Saw](loom.osc.saw.md).");
    let Block::Text { pieces, .. } = &parsed.blocks[0] else {
        panic!("a text block");
    };
    let shown: Vec<(&str, bool, bool, Option<usize>)> = pieces
        .iter()
        .map(|piece| (piece.text.as_str(), piece.bold, piece.code, piece.link))
        .collect();
    assert_eq!(
        shown,
        [
            ("Use ", false, false, None),
            ("two", true, false, None),
            (" ", false, false, None),
            ("delay1", false, true, None),
            (", see ", false, false, None),
            ("Saw", false, false, Some(0)),
            (".", false, false, None),
        ]
    );
}

/// A code block keeps its lines, without the last line break.
#[test]
fn a_code_block_is_whole() {
    let parsed = parse("```ron\nPatch(\n  modules: [],\n)\n```\n");
    assert_eq!(parsed.blocks, [Block::Code("Patch(\n  modules: [],\n)".to_string())]);
}

/// A list entry with two paragraphs reads them as one entry, a space
/// between, so no two words run together.
#[test]
fn an_entry_with_two_paragraphs_keeps_its_words_apart() {
    assert_eq!(words("- first\n\n  second\n"), ["first second"]);
    assert_eq!(words("- one\n- two\n"), ["one", "two"]);
}

/// A table is named, so a page with one fails the bundled-page test; a
/// line with a bar in it and no table is an ordinary paragraph.
#[test]
fn a_table_is_named_and_a_bar_is_not() {
    let table = parse("| A | B |\n| --- | --- |\n| x | y |\n");
    assert_eq!(table.unsupported, ["a table"]);
    let bar = parse("Use a | b here.\n");
    assert!(bar.unsupported.is_empty(), "{:?}", bar.unsupported);
    assert_eq!(bar.blocks[0].words(), "Use a | b here.");
}

/// What help does not show is still read as words, and is named once.
#[test]
fn markdown_beyond_helps_part_is_named() {
    let parsed = parse("# Title\n\n*soft* and *again*\n\n1. first\n\n> quoted\n");
    assert_eq!(words("*soft* and *again*"), ["soft and again"]);
    assert_eq!(
        parsed.unsupported,
        ["a heading other than ##", "emphasis", "a numbered list", "a quote"]
    );
}

/// Every bundled page, module and role alike, uses only the part of
/// Markdown help shows: a page that starts to use more fails here, before
/// a user meets it drawn as plain text.
#[test]
fn every_bundled_page_uses_only_helps_markdown() {
    let app = crate::tests::app();
    let registry = app.session.controller.registry();
    let mut pages: Vec<(String, String)> = app
        .modules
        .iter()
        .map(|descriptor| {
            (
                descriptor.id.to_string(),
                crate::help::Help::new(descriptor).model.notes,
            )
        })
        .collect();
    pages.extend(
        registry
            .roles()
            .map(|role| (role.id.to_string(), crate::help::Help::role(role, registry).model.notes)),
    );
    assert!(pages.len() > 50, "the pages are bundled: {}", pages.len());
    for (id, notes) in pages {
        let parsed = parse(&notes);
        assert!(parsed.unsupported.is_empty(), "{id} uses {:?}", parsed.unsupported);
        assert!(
            !notes.trim().is_empty() == !parsed.blocks.is_empty(),
            "{id} parses to its blocks"
        );
    }
}

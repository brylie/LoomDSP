//! Help's notes, parsed once into blocks that the view and the
//! accessibility tree both read (ui-layer.md, section 7, rule 1).
//!
//! The notes use a small part of Markdown: `##` headings, paragraphs, flat
//! bullet lists, fenced code blocks, inline code, bold, and links
//! (docs/design/help-pages.md). [`parse`] reads that part with
//! `pulldown-cmark`. Anything else still reads as its words, and is named in
//! [`Parsed::unsupported`], so a test over every bundled page fails when a
//! page starts to use something the view does not show.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// One run of words in a block, set one way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Piece {
    /// The words.
    pub(crate) text: String,
    /// Bold.
    pub(crate) bold: bool,
    /// Inline code.
    pub(crate) code: bool,
    /// The link it is part of, as an index into its block's links.
    pub(crate) link: Option<usize>,
}

/// A link in a block: the words it shows and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Link {
    /// The words, without their marks.
    pub(crate) label: String,
    /// The target as written: a page such as `loom.osc.saw.md`.
    pub(crate) target: String,
}

/// What kind of text block it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A heading.
    Heading,
    /// A paragraph.
    Paragraph,
    /// One entry of a bullet list.
    Bullet,
}

/// One block of the notes, in reading order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Block {
    /// A heading, a paragraph, or a list entry, made of pieces.
    Text {
        /// Which it is.
        kind: Kind,
        /// Its words, in order.
        pieces: Vec<Piece>,
        /// Its links, in order.
        links: Vec<Link>,
    },
    /// A fenced code block, whole, without its last line break.
    Code(String),
}

impl Block {
    /// Its words as plain text: what a screen reader reads, with a link
    /// read as its words and no marks.
    pub(crate) fn words(&self) -> String {
        match self {
            Self::Text { pieces, .. } => pieces.iter().map(|piece| piece.text.as_str()).collect(),
            Self::Code(code) => code.clone(),
        }
    }

    /// Its links, in order.
    pub(crate) fn links(&self) -> &[Link] {
        match self {
            Self::Text { links, .. } => links,
            Self::Code(_) => &[],
        }
    }
}

/// The blocks of some notes, and what in them the view does not show.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Parsed {
    /// The blocks, in reading order.
    pub(crate) blocks: Vec<Block>,
    /// Each kind of Markdown met that is not in help's part of it, once.
    pub(crate) unsupported: Vec<&'static str>,
}

/// Parses `notes` into blocks.
pub(crate) fn parse(notes: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let note = |name: &'static str, parsed: &mut Parsed| {
        if !parsed.unsupported.contains(&name) {
            parsed.unsupported.push(name);
        }
    };
    // The block being read: its kind, pieces, and links.
    let mut open: Option<(Kind, Vec<Piece>, Vec<Link>)> = None;
    let mut code: Option<String> = None;
    let (mut bold, mut lists) = (0_u32, 0_u32);
    let mut link: Option<usize> = None;
    let push = |open: &mut Option<(Kind, Vec<Piece>, Vec<Link>)>, text: &str, bold: bool, code: bool, link| {
        let Some((_, pieces, links)) = open.as_mut() else {
            return;
        };
        if let Some(at) = link {
            let link: &mut Link = &mut links[at];
            link.label.push_str(text);
        }
        match pieces.last_mut() {
            Some(last) if last.bold == bold && last.code == code && last.link == link => last.text.push_str(text),
            _ => pieces.push(Piece {
                text: text.to_string(),
                bold,
                code,
                link,
            }),
        }
    };
    let close = |open: &mut Option<(Kind, Vec<Piece>, Vec<Link>)>, parsed: &mut Parsed| {
        if let Some((kind, pieces, links)) = open.take()
            && !pieces.is_empty()
        {
            parsed.blocks.push(Block::Text { kind, pieces, links });
        }
    };
    // Tables are recognised only so that a page using one is named; help
    // shows none.
    for event in Parser::new_ext(notes, Options::ENABLE_TABLES) {
        if let Some(text) = code.as_mut() {
            match event {
                Event::Text(more) => text.push_str(&more),
                Event::End(TagEnd::CodeBlock) => {
                    let text = code.take().unwrap_or_default();
                    parsed.blocks.push(Block::Code(text.trim_end_matches('\n').to_string()));
                }
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                if level != pulldown_cmark::HeadingLevel::H2 {
                    note("a heading other than ##", &mut parsed);
                }
                close(&mut open, &mut parsed);
                open = Some((Kind::Heading, Vec::new(), Vec::new()));
            }
            // A paragraph inside a list entry is the entry's own text; a
            // second one is set apart from the first by a space.
            Event::Start(Tag::Paragraph) if open.as_ref().is_some_and(|(kind, ..)| *kind == Kind::Bullet) => {
                if open.as_ref().is_some_and(|(_, pieces, _)| !pieces.is_empty()) {
                    push(&mut open, " ", bold > 0, false, link);
                }
            }
            Event::Start(Tag::Paragraph) => {
                close(&mut open, &mut parsed);
                open = Some((Kind::Paragraph, Vec::new(), Vec::new()));
            }
            Event::Start(Tag::List(first)) => {
                lists += 1;
                if lists > 1 {
                    note("a nested list", &mut parsed);
                }
                if first.is_some() {
                    note("a numbered list", &mut parsed);
                }
            }
            Event::End(TagEnd::List(_)) => lists = lists.saturating_sub(1),
            Event::Start(Tag::Item) => {
                close(&mut open, &mut parsed);
                open = Some((Kind::Bullet, Vec::new(), Vec::new()));
            }
            Event::End(TagEnd::Heading(_) | TagEnd::Item) => close(&mut open, &mut parsed),
            Event::End(TagEnd::Paragraph) if open.as_ref().is_some_and(|(kind, ..)| *kind == Kind::Paragraph) => {
                close(&mut open, &mut parsed);
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                if !matches!(kind, CodeBlockKind::Fenced(_)) {
                    note("an indented code block", &mut parsed);
                }
                close(&mut open, &mut parsed);
                code = Some(String::new());
            }
            Event::Start(Tag::Strong) => bold += 1,
            Event::End(TagEnd::Strong) => bold = bold.saturating_sub(1),
            Event::Start(Tag::Emphasis) => note("emphasis", &mut parsed),
            Event::Start(Tag::Link { dest_url, .. }) => {
                if let Some((_, _, links)) = open.as_mut() {
                    links.push(Link {
                        label: String::new(),
                        target: dest_url.to_string(),
                    });
                    link = Some(links.len() - 1);
                }
            }
            Event::End(TagEnd::Link) => link = None,
            Event::Text(text) => push(&mut open, &text, bold > 0, false, link),
            Event::Code(text) => push(&mut open, &text, bold > 0, true, link),
            Event::SoftBreak | Event::HardBreak => push(&mut open, " ", bold > 0, false, link),
            Event::Start(Tag::BlockQuote(_)) => note("a quote", &mut parsed),
            Event::Start(Tag::Image { .. }) => note("an image", &mut parsed),
            Event::Start(Tag::Table(_)) => note("a table", &mut parsed),
            Event::Html(_) | Event::InlineHtml(_) => note("HTML", &mut parsed),
            Event::Rule => note("a rule", &mut parsed),
            _ => {}
        }
    }
    close(&mut open, &mut parsed);
    parsed
}

#[cfg(test)]
mod tests;

//! Patch metadata: authors, tags, and a licence, and the filters that find
//! patches by them. Spec: `docs/design/patch-metadata.md`.
//!
//! Everything here is plain data and text rules, so the app, the command
//! line, and a future library browser all tidy and match the same way
//! (rule 8). Nothing here refuses a patch: metadata never stops a patch
//! from opening (rule 1).

use serde::{Deserialize, Serialize};
use spdx::{Expression, LicenseReq, ParseMode};

/// A patch holds at most this many tags (rule 5).
pub const MAX_TAGS: usize = 20;

/// A tag is at most this many characters long (rule 5).
pub const MAX_TAG_CHARS: usize = 32;

/// The metadata of a patch: one field of [`crate::Patch`], and of a
/// sub-patch definition, so no struct repeats these fields (#652).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Metadata {
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// Free text for the patch author.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Who made it, in credit order (rule 2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    /// What it is, for finding it (rule 5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// An SPDX licence expression, kept as written (rule 1, rule 6).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_licence",
        deserialize_with = "deserialize_licence"
    )]
    pub license: Option<String>,
}

/// The head of a patch file: the one field the browser reads.
///
/// The patch browser reads only this, so a folder of large patches stays
/// quick to list. Unknown fields, such as `modules`, are skipped.
#[derive(Deserialize)]
#[serde(rename = "Patch")]
struct Head {
    #[serde(default)]
    metadata: Metadata,
}

impl Metadata {
    /// Reads the metadata of a patch file's text without its modules. A
    /// file that is not a patch, or whose metadata fields have the wrong
    /// shape, is an error.
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str::<Head>(text).map(|head| head.metadata)
    }
}

/// Writes a present licence as a plain string, as SPDX tools expect:
/// `license: "MIT"`, not `license: Some("MIT")`.
fn serialize_licence<S: serde::Serializer>(value: &Option<String>, serializer: S) -> Result<S::Ok, S::Error> {
    value.as_deref().unwrap_or_default().serialize(serializer)
}

/// Reads a plain licence string, leaving a missing field to its default.
fn deserialize_licence<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

/// Tidies typed text into a tag (rule 5): trimmed and lowercased, with
/// spaces and underscores as hyphens, other characters dropped, single
/// hyphens, and at most [`MAX_TAG_CHARS`] characters. `None` when nothing
/// is left.
pub fn tidy_tag(text: &str) -> Option<String> {
    let mut tag = String::new();
    let mut length = 0;
    for c in text.trim().chars().flat_map(char::to_lowercase) {
        let c = if c.is_whitespace() || c == '_' { '-' } else { c };
        if c == '-' {
            if !tag.is_empty() && !tag.ends_with('-') {
                tag.push('-');
                length += 1;
            }
        } else if c.is_alphanumeric() {
            tag.push(c);
            length += 1;
        }
        if length == MAX_TAG_CHARS {
            break;
        }
    }
    while tag.ends_with('-') {
        tag.pop();
    }
    (!tag.is_empty()).then_some(tag)
}

/// Why a tag was not added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagError {
    /// Nothing is left of the text once it is tidied.
    Empty,
    /// The patch has [`MAX_TAGS`] tags already.
    Full,
}

impl std::fmt::Display for TagError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("A tag needs at least one letter or digit."),
            Self::Full => write!(f, "A patch has at most {MAX_TAGS} tags. Remove one to add another."),
        }
    }
}

/// Adds typed text to `tags` as a tidy tag. Returns the tag, or `None`
/// when the patch has it already (a duplicate is not an error).
pub fn add_tag(tags: &mut Vec<String>, text: &str) -> Result<Option<String>, TagError> {
    let tag = tidy_tag(text).ok_or(TagError::Empty)?;
    if tags.iter().any(|t| tidy_tag(t).as_deref() == Some(tag.as_str())) {
        return Ok(None);
    }
    if tags.len() >= MAX_TAGS {
        return Err(TagError::Full);
    }
    tags.push(tag.clone());
    Ok(Some(tag))
}

/// Tidies a list of authors (rule 2): each one trimmed, empty ones
/// dropped, and later duplicates, ignoring case, dropped. The order stays.
pub fn tidy_authors<'a>(authors: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut tidy: Vec<String> = Vec::new();
    for author in authors {
        let author = author.trim();
        if !author.is_empty() && !has_author(&tidy, author) {
            tidy.push(author.to_string());
        }
    }
    tidy
}

/// True when `authors` holds `name`, ignoring case and surrounding spaces.
pub fn has_author(authors: &[String], name: &str) -> bool {
    let name = name.trim().to_lowercase();
    authors.iter().any(|a| a.trim().to_lowercase() == name)
}

/// What the licence chooser's first question allows (Licence chooser).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Changes {
    /// Others may change it and share the result.
    Yes,
    /// Yes, if they share it under the same licence.
    ShareAlike,
    /// No, only unchanged copies.
    No,
}

/// One answer of the licence chooser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Choice {
    /// One of the six Creative Commons 4.0 licences.
    Creative {
        /// The answer to "Allow others to change it and share the result?"
        changes: Changes,
        /// The answer to "Allow commercial use?"
        commercial: bool,
    },
    /// "I give up all rights: anyone may do anything with it."
    PublicDomain,
}

impl Choice {
    /// The SPDX identifier this answer stores (the chooser's table).
    pub fn licence(self) -> &'static str {
        match self {
            Self::PublicDomain => "CC0-1.0",
            Self::Creative { changes, commercial } => match (changes, commercial) {
                (Changes::Yes, true) => "CC-BY-4.0",
                (Changes::ShareAlike, true) => "CC-BY-SA-4.0",
                (Changes::No, true) => "CC-BY-ND-4.0",
                (Changes::Yes, false) => "CC-BY-NC-4.0",
                (Changes::ShareAlike, false) => "CC-BY-NC-SA-4.0",
                (Changes::No, false) => "CC-BY-NC-ND-4.0",
            },
        }
    }

    /// The answer that gives `licence`, if the chooser offers it. Opens
    /// the chooser on the patch's current choice.
    pub fn of(licence: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|choice| choice.licence() == licence.trim())
    }

    /// Every answer, in the order of the chooser's table.
    pub const ALL: [Self; 7] = [
        Self::Creative {
            changes: Changes::Yes,
            commercial: true,
        },
        Self::Creative {
            changes: Changes::ShareAlike,
            commercial: true,
        },
        Self::Creative {
            changes: Changes::No,
            commercial: true,
        },
        Self::Creative {
            changes: Changes::Yes,
            commercial: false,
        },
        Self::Creative {
            changes: Changes::ShareAlike,
            commercial: false,
        },
        Self::Creative {
            changes: Changes::No,
            commercial: false,
        },
        Self::PublicDomain,
    ];
}

/// How patch info shows a licence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Licence {
    /// The patch has no `license` field: "No licence stated".
    None,
    /// The text is not a valid SPDX expression. It is kept as written and
    /// shown as "Unrecognised licence" (rule 1).
    Unrecognised(String),
    /// A valid SPDX expression.
    Known(LicenceInfo),
}

/// A valid licence, as patch info shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicenceInfo {
    /// The expression as written, for example `CC-BY-SA-4.0`.
    pub short: String,
    /// One line in plain words.
    pub summary: String,
    /// The licence's own page, for a single licence.
    pub url: Option<String>,
}

impl Licence {
    /// Reads a patch's `license` field.
    pub fn of(license: Option<&str>) -> Self {
        let Some(text) = license else {
            return Self::None;
        };
        let Some(expression) = parse(text) else {
            return Self::Unrecognised(text.to_string());
        };
        let short = text.trim().to_string();
        let single: Vec<_> = expression.requirements().map(|r| &r.req).collect();
        let (summary, url) = match single[..] {
            [req] if req.addition.is_none() => match req.license.id() {
                Some(id) => match creative_commons(id.name) {
                    Some((summary, url)) => (summary.to_string(), Some(url.to_string())),
                    None => (
                        id.full_name.to_string(),
                        Some(format!("https://spdx.org/licenses/{}.html", id.name)),
                    ),
                },
                None => ("A licence that is not on the SPDX list.".to_string(), None),
            },
            _ => ("More than one licence applies. Read each one.".to_string(), None),
        };
        Self::Known(LicenceInfo { short, summary, url })
    }

    /// The short line patch info shows: the licence's short name,
    /// "Unrecognised licence", or "No licence stated".
    pub fn short(&self) -> String {
        match self {
            Self::None => "No licence stated".to_string(),
            Self::Unrecognised(_) => "Unrecognised licence".to_string(),
            Self::Known(info) => info.short.clone(),
        }
    }
}

/// True when `text` is a valid SPDX licence expression (rule 6).
pub fn is_valid(text: &str) -> bool {
    parse(text).is_some()
}

/// Parses an SPDX expression strictly: the identifiers on the SPDX list,
/// spelled as the list spells them.
fn parse(text: &str) -> Option<Expression> {
    Expression::parse_mode(text.trim(), ParseMode::STRICT).ok()
}

/// The summary and page of a Creative Commons licence the chooser offers.
fn creative_commons(id: &str) -> Option<(&'static str, &'static str)> {
    Some(match id {
        "CC0-1.0" => (
            "No rights kept: anyone may use, change, and share it for any purpose, without credit.",
            "https://creativecommons.org/publicdomain/zero/1.0/",
        ),
        "CC-BY-4.0" => (
            "Anyone may change and share it, also for money, if they credit the authors.",
            "https://creativecommons.org/licenses/by/4.0/",
        ),
        "CC-BY-SA-4.0" => (
            "Anyone may change and share it, also for money, if they credit the authors and share changes under the same licence.",
            "https://creativecommons.org/licenses/by-sa/4.0/",
        ),
        "CC-BY-ND-4.0" => (
            "Anyone may share unchanged copies, also for money, if they credit the authors.",
            "https://creativecommons.org/licenses/by-nd/4.0/",
        ),
        "CC-BY-NC-4.0" => (
            "Anyone may change and share it, but not for money, if they credit the authors.",
            "https://creativecommons.org/licenses/by-nc/4.0/",
        ),
        "CC-BY-NC-SA-4.0" => (
            "Anyone may change and share it, but not for money, if they credit the authors and share changes under the same licence.",
            "https://creativecommons.org/licenses/by-nc-sa/4.0/",
        ),
        "CC-BY-NC-ND-4.0" => (
            "Anyone may share unchanged copies, but not for money, if they credit the authors.",
            "https://creativecommons.org/licenses/by-nc-nd/4.0/",
        ),
        _ => return None,
    })
}

/// A licence filter of a patch browser (Patch browser filters).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LicenceFilter {
    /// The licence allows sharing a changed version.
    LetsMeChange,
    /// The licence allows commercial use.
    AllowsCommercialUse,
    /// The patch has no `license` field.
    NoLicenceStated,
}

impl LicenceFilter {
    /// Every filter, in the order the browser shows them.
    pub const ALL: [Self; 3] = [Self::LetsMeChange, Self::AllowsCommercialUse, Self::NoLicenceStated];

    /// The filter's label.
    pub fn label(self) -> &'static str {
        match self {
            Self::LetsMeChange => "Lets me change it",
            Self::AllowsCommercialUse => "Allows commercial use",
            Self::NoLicenceStated => "No licence stated",
        }
    }

    /// True when a patch with `license` passes this filter (rule 9). An
    /// unrecognised or missing licence allows nothing: showing too few
    /// patches is safe, showing too many is not.
    pub fn matches(self, license: Option<&str>) -> bool {
        match self {
            Self::NoLicenceStated => license.is_none(),
            Self::LetsMeChange | Self::AllowsCommercialUse => license
                .and_then(parse)
                // `OR` needs one side and `AND` both; `WITH` counts as its
                // licence, so the exception is not looked at.
                .is_some_and(|expression| expression.evaluate(|req| allows(req, self))),
        }
    }
}

/// What one licence allows (the table under Patch browser filters).
fn allows(req: &LicenseReq, filter: LicenceFilter) -> bool {
    let Some(id) = req.license.id() else {
        // A `LicenseRef-` is not on the SPDX list: unknown.
        return false;
    };
    let change = filter == LicenceFilter::LetsMeChange;
    match id.name {
        "CC0-1.0" | "CC-BY-4.0" | "CC-BY-SA-4.0" => true,
        "CC-BY-ND-4.0" => !change,
        "CC-BY-NC-4.0" | "CC-BY-NC-SA-4.0" => change,
        "CC-BY-NC-ND-4.0" => false,
        // Open source and free software licences allow both by their own
        // definitions. Any other licence is unknown, and unknown is no.
        _ => id.is_osi_approved() || id.is_fsf_free_libre(),
    }
}

/// What a patch browser shows: a search and the filters, all of which a
/// patch must match (Your patches browser).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Words to find in the name, description, or tags, ignoring case.
    pub search: String,
    /// Text to find in one of the authors, ignoring case.
    pub author: String,
    /// Tags a patch must all have, compared in their tidy form (rule 8).
    pub tags: Vec<String>,
    /// Licence filters a patch must all pass.
    pub licence: Vec<LicenceFilter>,
}

impl Filter {
    /// True when `metadata` passes the search and every filter.
    pub fn matches(&self, metadata: &Metadata) -> bool {
        self.matcher().matches(&Prepared::of(metadata))
    }

    /// This filter in the form that compares quickly with [`Prepared`] rows,
    /// for checking many patches at once.
    pub fn matcher(&self) -> Matcher<'_> {
        Matcher {
            words: self.search.split_whitespace().map(str::to_lowercase).collect(),
            author: self.author.trim().to_lowercase(),
            tags: self.tags.iter().filter_map(|t| tidy_tag(t)).collect(),
            licence: &self.licence,
        }
    }
}

/// What [`Filter`] compares in one patch, worked out once. A browser keeps
/// one per row, so that typing in the search does not lowercase every name
/// or parse every licence again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    /// The name, description, and tidy tags, in lowercase.
    text: String,
    /// The authors, in lowercase.
    authors: Vec<String>,
    /// The tags in their tidy form.
    tags: Vec<String>,
    /// What [`LicenceFilter::ALL`] says about this patch's licence, in the
    /// same order.
    licence: [bool; LicenceFilter::ALL.len()],
}

impl Prepared {
    /// Works out what the filters compare in `metadata`.
    pub fn of(metadata: &Metadata) -> Self {
        let tags: Vec<String> = metadata.tags.iter().filter_map(|t| tidy_tag(t)).collect();
        Self {
            text: format!("{} {} {}", metadata.name, metadata.description, tags.join(" ")).to_lowercase(),
            authors: metadata.authors.iter().map(|a| a.to_lowercase()).collect(),
            tags,
            licence: LicenceFilter::ALL.map(|filter| filter.matches(metadata.license.as_deref())),
        }
    }

    /// The patch's tags in their tidy form.
    pub fn tags(&self) -> &[String] {
        &self.tags
    }
}

/// A [`Filter`] ready to compare with [`Prepared`] rows.
#[derive(Debug, Clone)]
pub struct Matcher<'a> {
    /// The search's words, in lowercase.
    words: Vec<String>,
    /// The author text, trimmed and in lowercase.
    author: String,
    /// The wanted tags, in their tidy form.
    tags: Vec<String>,
    /// The licence filters a patch must all pass.
    licence: &'a [LicenceFilter],
}

impl Matcher<'_> {
    /// True when the patch passes the search and every filter.
    pub fn matches(&self, patch: &Prepared) -> bool {
        self.words.iter().all(|word| patch.text.contains(word))
            && (self.author.is_empty() || patch.authors.iter().any(|a| a.contains(&self.author)))
            && self.tags.iter().all(|wanted| patch.tags.contains(wanted))
            && self.licence.iter().all(|filter| {
                LicenceFilter::ALL
                    .iter()
                    .position(|known| known == filter)
                    .is_some_and(|at| patch.licence[at])
            })
    }
}

#[cfg(test)]
mod tests;

use super::*;
use crate::Patch;

/// A patch with authors, tags, and a licence, as the spec shows it.
const WITH_METADATA: &str = r#"Patch(
    version: 1,
    metadata: Metadata(
        name: "Low Drone",
        description: "Two detuned sines through a slow filter.",
        authors: ["Ada Lovelace", "Sam Rivera"],
        tags: ["drone", "ambient", "fm"],
        license: "CC-BY-SA-4.0",
    ),
    modules: [(id: 1, kind: "loom.osc.sine@1")],
)
"#;

/// Acceptance: a patch with no metadata saves with no new empty fields.
#[test]
fn a_patch_without_metadata_writes_no_new_fields() {
    let text = Patch::new("Plain").to_ron().unwrap();
    for field in ["authors", "tags", "license"] {
        assert!(!text.contains(field), "{field} in {text}");
    }
    assert_eq!(Patch::from_ron(&text).unwrap(), Patch::new("Plain"));
}

/// Acceptance: authors, tags, and a licence survive load and save
/// exactly, and the licence is written as a plain string.
#[test]
fn metadata_survives_load_and_save_exactly() {
    let patch = Patch::from_ron(WITH_METADATA).unwrap();
    assert_eq!(patch.metadata.authors, ["Ada Lovelace", "Sam Rivera"]);
    assert_eq!(patch.metadata.tags, ["drone", "ambient", "fm"]);
    assert_eq!(patch.metadata.license.as_deref(), Some("CC-BY-SA-4.0"));
    let text = patch.to_ron().unwrap();
    assert!(text.contains(r#"license: "CC-BY-SA-4.0""#), "{text}");
    assert_eq!(Patch::from_ron(&text).unwrap(), patch);
}

/// Acceptance: a licence that is not SPDX loads, reads as unrecognised,
/// and saves unchanged. Tags are kept as written, too (rule 1).
#[test]
fn an_unrecognised_licence_is_kept_as_written() {
    let text = r#"Patch(version: 1, metadata: Metadata(tags: ["Not Tidy"], license: "my own terms"))"#;
    let patch = Patch::from_ron(text).unwrap();
    let licence = Licence::of(patch.metadata.license.as_deref());
    assert_eq!(licence, Licence::Unrecognised("my own terms".to_string()));
    assert_eq!(licence.short(), "Unrecognised licence");
    let saved = patch.to_ron().unwrap();
    assert!(saved.contains(r#"license: "my own terms""#), "{saved}");
    assert!(saved.contains(r#""Not Tidy""#), "{saved}");
}

/// The browser reads only the metadata fields, skipping the modules.
#[test]
fn metadata_reads_without_the_modules() {
    let metadata = Metadata::from_ron(WITH_METADATA).unwrap();
    assert_eq!(metadata, Patch::from_ron(WITH_METADATA).unwrap().metadata);
    assert_eq!(metadata.name, "Low Drone");
}

/// Acceptance and rule 5: typed text is tidied, duplicates are stored
/// once, and a 21st tag is refused with a message.
#[test]
fn tags_are_tidied_and_limited() {
    assert_eq!(tidy_tag(" Bass Drone ").as_deref(), Some("bass-drone"));
    assert_eq!(tidy_tag("live_set").as_deref(), Some("live-set"));
    assert_eq!(tidy_tag("--a  --  b!--").as_deref(), Some("a-b"));
    assert_eq!(tidy_tag("Klänge").as_deref(), Some("klänge"));
    assert_eq!(tidy_tag("ドローン").as_deref(), Some("ドローン"));
    assert_eq!(tidy_tag("!!!"), None);
    assert_eq!(
        tidy_tag(&"a".repeat(40)).map(|t| t.chars().count()),
        Some(MAX_TAG_CHARS)
    );
    // A cut that ends on a hyphen loses the hyphen.
    let cut = format!("{} b", "a".repeat(MAX_TAG_CHARS - 1));
    assert_eq!(tidy_tag(&cut), Some("a".repeat(MAX_TAG_CHARS - 1)));

    let mut tags = Vec::new();
    assert_eq!(add_tag(&mut tags, "drone"), Ok(Some("drone".to_string())));
    assert_eq!(add_tag(&mut tags, " Drone "), Ok(None));
    assert_eq!(tags, ["drone"]);
    assert_eq!(add_tag(&mut tags, "?"), Err(TagError::Empty));
    for i in 1..MAX_TAGS {
        add_tag(&mut tags, &format!("tag{i}")).unwrap();
    }
    assert_eq!(tags.len(), MAX_TAGS);
    assert_eq!(add_tag(&mut tags, "one-more"), Err(TagError::Full));
    assert!(TagError::Full.to_string().contains("20"));
    // A duplicate is still not an error when the list is full.
    assert_eq!(add_tag(&mut tags, "drone"), Ok(None));
}

/// Rule 2: authors are trimmed, empty lines go, and duplicates ignoring
/// case go, in credit order.
#[test]
fn authors_are_tidied_in_credit_order() {
    let authors = tidy_authors("  Ada Lovelace \n\nSam Rivera\nada lovelace\n".lines());
    assert_eq!(authors, ["Ada Lovelace", "Sam Rivera"]);
    assert!(has_author(&authors, " SAM rivera"));
    assert!(!has_author(&authors, "Sam"));
}

/// Acceptance: each of the seven chooser answers stores its identifier.
#[test]
fn every_chooser_answer_stores_its_licence() {
    let licences: Vec<&str> = Choice::ALL.iter().map(|c| c.licence()).collect();
    assert_eq!(
        licences,
        [
            "CC-BY-4.0",
            "CC-BY-SA-4.0",
            "CC-BY-ND-4.0",
            "CC-BY-NC-4.0",
            "CC-BY-NC-SA-4.0",
            "CC-BY-NC-ND-4.0",
            "CC0-1.0"
        ]
    );
    for choice in Choice::ALL {
        assert_eq!(Choice::of(choice.licence()), Some(choice));
        assert!(is_valid(choice.licence()), "{}", choice.licence());
        let Licence::Known(info) = Licence::of(Some(choice.licence())) else {
            panic!("{} is known", choice.licence());
        };
        assert!(info.url.unwrap().starts_with("https://creativecommons.org/"));
    }
    assert_eq!(Choice::of("MIT"), None);
}

/// Patch info: other licences show their SPDX name and page; an
/// expression with several licences has no single page.
#[test]
fn licences_describe_themselves() {
    assert_eq!(Licence::of(None), Licence::None);
    assert_eq!(Licence::of(None).short(), "No licence stated");
    let Licence::Known(mit) = Licence::of(Some("MIT")) else {
        panic!("MIT is known");
    };
    assert_eq!(mit.short, "MIT");
    assert_eq!(mit.summary, "MIT License");
    assert_eq!(mit.url.as_deref(), Some("https://spdx.org/licenses/MIT.html"));
    let Licence::Known(both) = Licence::of(Some("CC-BY-4.0 OR MIT")) else {
        panic!("an OR expression is known");
    };
    assert_eq!(both.url, None);
    let Licence::Known(other) = Licence::of(Some("LicenseRef-mine")) else {
        panic!("a licence reference is valid SPDX");
    };
    assert_eq!(other.url, None);
    assert!(!is_valid("mit"));
    assert!(!is_valid(""));
}

/// Acceptance and rule 9: what each licence filter matches.
#[test]
fn licence_filters_match_only_what_is_known_to_be_allowed() {
    let commercial = LicenceFilter::AllowsCommercialUse;
    for yes in [
        "CC-BY-SA-4.0",
        "MIT",
        "MIT OR CC-BY-NC-4.0",
        "CC0-1.0",
        "GPL-3.0-or-later",
    ] {
        assert!(commercial.matches(Some(yes)), "{yes}");
    }
    for no in [
        "CC-BY-NC-4.0",
        "MIT AND CC-BY-NC-4.0",
        "my own terms",
        "LicenseRef-mine",
    ] {
        assert!(!commercial.matches(Some(no)), "{no}");
    }
    assert!(!commercial.matches(None));

    let change = LicenceFilter::LetsMeChange;
    assert!(change.matches(Some("CC-BY-NC-4.0")));
    assert!(change.matches(Some("CC-BY-ND-4.0 OR MIT")));
    assert!(change.matches(Some("Apache-2.0 WITH LLVM-exception")));
    assert!(!change.matches(Some("CC-BY-ND-4.0")));
    assert!(!change.matches(Some("CC-BY-NC-ND-4.0")));
    assert!(!change.matches(None));

    let none = LicenceFilter::NoLicenceStated;
    assert!(none.matches(None));
    assert!(!none.matches(Some("my own terms")));
    assert!(!none.matches(Some("MIT")));
    assert_eq!(LicenceFilter::ALL.map(LicenceFilter::label)[2], "No licence stated");
}

/// Acceptance and rule 8: two tags show only patches with both; the
/// author filter ignores case; search looks in name, description, tags.
#[test]
fn browser_filters_combine() {
    let drone = Metadata {
        name: "Low Drone".into(),
        description: "Slow filter".into(),
        authors: vec!["Ada Lovelace".into()],
        tags: vec!["Drone".into(), "fm".into()],
        license: Some("CC-BY-4.0".into()),
    };
    let bass = Metadata {
        name: "Bass".into(),
        tags: vec!["drone".into()],
        ..Metadata::default()
    };
    let two_tags = Filter {
        tags: vec!["drone".into(), "FM".into()],
        ..Filter::default()
    };
    assert!(two_tags.matches(&drone));
    assert!(!two_tags.matches(&bass));

    let author = Filter {
        author: "ADA".into(),
        ..Filter::default()
    };
    assert!(author.matches(&drone));
    assert!(!author.matches(&bass));

    for search in ["low", "SLOW filter", "fm", ""] {
        let filter = Filter {
            search: search.into(),
            ..Filter::default()
        };
        assert!(filter.matches(&drone), "{search}");
    }
    let licence = Filter {
        licence: vec![LicenceFilter::NoLicenceStated],
        ..Filter::default()
    };
    assert!(licence.matches(&bass));
    assert!(!licence.matches(&drone));
}

/// The prepared form gives the same answers as the filter itself, for every
/// mix of search, author, tag, and licence. Case and untidy text count.
#[test]
fn prepared_matching_agrees_with_the_filter() {
    let patches = [
        Metadata {
            name: "Low Drone".into(),
            description: "Slow FILTER".into(),
            authors: vec!["Ada Lovelace".into()],
            tags: vec!["Drone".into(), " FM ".into()],
            license: Some("CC-BY-NC-4.0".into()),
        },
        Metadata {
            name: "Bass".into(),
            authors: vec!["Sam Rivera".into()],
            license: Some("MIT".into()),
            ..Metadata::default()
        },
        Metadata::default(),
    ];
    let filters = [
        Filter::default(),
        Filter {
            search: " slow  low ".into(),
            ..Filter::default()
        },
        Filter {
            author: " ADA ".into(),
            ..Filter::default()
        },
        Filter {
            tags: vec!["DRONE".into(), "fm".into()],
            ..Filter::default()
        },
        Filter {
            licence: vec![LicenceFilter::AllowsCommercialUse],
            ..Filter::default()
        },
        Filter {
            licence: vec![LicenceFilter::LetsMeChange, LicenceFilter::NoLicenceStated],
            ..Filter::default()
        },
    ];
    let mut matched = 0;
    for filter in &filters {
        for patch in &patches {
            let direct = filter.matches(patch);
            assert_eq!(
                direct,
                filter.matcher().matches(&Prepared::of(patch)),
                "{filter:?} {patch:?}"
            );
            matched += usize::from(direct);
        }
    }
    assert!(matched > 3, "the cases must not all fail");
    // Spelled out, so this test cannot pass by both sides being wrong.
    assert!(filters[1].matches(&patches[0]));
    assert!(!filters[1].matches(&patches[1]));
    assert!(filters[3].matches(&patches[0]));
    assert!(filters[4].matches(&patches[1]));
    assert!(!filters[4].matches(&patches[0]));
    assert_eq!(Prepared::of(&patches[0]).tags(), ["drone", "fm"]);
}

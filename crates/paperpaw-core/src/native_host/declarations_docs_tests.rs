//! Format, reader, lookup and book declarations: syntax, limits, and refusal
//! until the host admits them.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use super::*;
use crate::native_host::registrations::Record;
use alloc::{format, string::String, vec, vec::Vec};
use paperpaw_native::host_glue::ERR_INVALID;

const PLUGIN: &str = "format-epub";

fn format<'a>(extensions: &'a str, flags: u32) -> FormatSpec<'a> {
    FormatSpec {
        name: "epub",
        title: "format.epub",
        extensions,
        flags,
        priority: -5,
        token: 3,
    }
}

#[test]
fn a_format_names_its_extensions_lowercase_without_the_dot() {
    assert_eq!(format_check(PLUGIN, &format("epub", 0)), Ok(()));
    assert_eq!(
        format_check(PLUGIN, &format("fb2.zip\nfb2", 0b11111)),
        Ok(())
    );
    for bad in [
        "",
        "EPUB",
        ".epub",
        "epub.",
        "a..b",
        "ep ub",
        "epub\n",
        "\nepub",
        "epub\nepub",
        "é",
    ] {
        assert_eq!(
            format_check(PLUGIN, &format(bad, 0)),
            Err(ERR_INVALID),
            "{bad:?}"
        );
    }
    // 16 extensions of 31 bytes fit; 17 or 32 bytes do not.
    let many = |n: usize| -> String {
        (0..n)
            .map(|i| format!("e{i:02}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(format_check(PLUGIN, &format(&many(16), 0)), Ok(()));
    assert_eq!(
        format_check(PLUGIN, &format(&many(17), 0)),
        Err(ERR_INVALID)
    );
    assert_eq!(format_check(PLUGIN, &format(&"a".repeat(31), 0)), Ok(()));
    assert_eq!(
        format_check(PLUGIN, &format(&"a".repeat(32), 0)),
        Err(ERR_INVALID)
    );
}

#[test]
fn a_format_has_a_name_a_title_and_only_the_five_flags() {
    assert_eq!(
        format_check(PLUGIN, &format("epub", 1 << 5)),
        Err(ERR_INVALID)
    );
    let mut spec = format("epub", 0);
    spec.title = "";
    assert_eq!(format_check(PLUGIN, &spec), Err(ERR_INVALID));
    spec = format("epub", 0);
    spec.name = "Bad Name";
    assert_eq!(format_check(PLUGIN, &spec), Err(ERR_INVALID));
    spec.name = "";
    assert_eq!(format_check(PLUGIN, &spec), Err(ERR_INVALID));
}

#[test]
fn a_reader_names_a_screen_and_at_most_eight_formats_or_all() {
    let reader = |screen, formats, flags| ReaderSpec {
        name: "main",
        screen,
        formats,
        flags,
    };
    assert_eq!(
        reader_check("reader", &reader("reader.main", "", 0)),
        Ok(())
    );
    let eight = "a.b\nc.d\ne.f\ng.h\ni.j\nk.l\nm.n\no.p";
    assert_eq!(reader_check("reader", &reader("main", eight, 0)), Ok(()));
    let nine = format!("{eight}\nq.r");
    assert_eq!(
        reader_check("reader", &reader("main", &nine, 0)),
        Err(ERR_INVALID)
    );
    for (screen, formats, flags) in [
        ("", "", 0),
        ("Main Screen", "", 0),
        ("main", "bad id", 0),
        ("main", "a.b\n", 0),
        ("main", "", 1),
    ] {
        assert_eq!(
            reader_check("reader", &reader(screen, formats, flags)),
            Err(ERR_INVALID),
            "{screen:?} {formats:?} {flags}"
        );
    }
}

fn lookup(languages: &str, flags: u32) -> LookupSpec<'_> {
    LookupSpec {
        name: "dict",
        title: "lookup.dict",
        languages,
        flags,
        token: 1,
    }
}

#[test]
fn a_lookup_provider_lists_up_to_32_language_tags_or_any() {
    assert_eq!(lookup_check("dictionary", &lookup("", 1)), Ok(()));
    assert_eq!(
        lookup_check("dictionary", &lookup("en\nde-CH\nzh-Hant-TW", 0)),
        Ok(())
    );
    let tags = |n: usize| -> String { vec!["en"; n].join("\n") };
    assert_eq!(lookup_check("dictionary", &lookup(&tags(32), 0)), Ok(()));
    assert_eq!(
        lookup_check("dictionary", &lookup(&tags(33), 0)),
        Err(ERR_INVALID)
    );
    let long = "a".repeat(35);
    assert_eq!(lookup_check("dictionary", &lookup(&long, 0)), Ok(()));
    assert_eq!(
        lookup_check("dictionary", &lookup(&"a".repeat(36), 0)),
        Err(ERR_INVALID)
    );
    for bad in ["en\n", "en--us", "-en", "en-", "e n", "en_US"] {
        assert_eq!(
            lookup_check("dictionary", &lookup(bad, 0)),
            Err(ERR_INVALID),
            "{bad:?}"
        );
    }
    assert_eq!(
        lookup_check("dictionary", &lookup("en", 2)),
        Err(ERR_INVALID)
    );
}

#[test]
fn book_fields_badges_and_data_follow_their_flags_and_kinds() {
    let field = |flags| BookFieldSpec {
        name: "rating",
        title: "field.rating",
        flags,
    };
    assert_eq!(book_field_check("stars", &field(0)), Ok(()));
    assert_eq!(book_field_check("stars", &field(1)), Ok(()));
    assert_eq!(book_field_check("stars", &field(2)), Err(ERR_INVALID));
    let badge = |flags| BookBadgeSpec {
        name: "new",
        token: 4,
        flags,
    };
    assert_eq!(book_badge_check("badges", &badge(0)), Ok(()));
    assert_eq!(book_badge_check("badges", &badge(1)), Err(ERR_INVALID));
    let data = |kind| BookDataSpec {
        name: "notes",
        kind,
    };
    assert_eq!(book_data_check("notes-vault", &data(0)), Ok(()));
    assert_eq!(book_data_check("notes-vault", &data(1)), Ok(()));
    assert_eq!(book_data_check("notes-vault", &data(2)), Err(ERR_INVALID));
}

#[test]
fn a_book_data_name_is_one_folder_name_of_lowercase_letters_digits_and_dashes() {
    let check = |name| book_data_check("notes-vault", &BookDataSpec { name, kind: 0 });
    let longest = "a".repeat(96);
    let too_long = "a".repeat(97);
    for name in ["notes", "a", "0", "notes-2", "-", "--", longest.as_str()] {
        assert_eq!(check(name), Ok(()), "{name}");
    }
    for name in [
        "",
        too_long.as_str(),
        "Notes",
        "nOtes",
        "a/b",
        "/",
        "a\\b",
        "..",
        ".",
        "a.b",
        "notes ",
        "no tes",
        "n\u{e9}",
        "a\0",
    ] {
        assert_eq!(check(name), Err(ERR_INVALID), "{name:?}");
    }
    // The longest name is 96 bytes however many characters it has.
    let multibyte = "\u{e4}".repeat(48);
    assert_eq!(check(&multibyte), Err(ERR_INVALID));
}

#[test]
fn a_format_is_admitted_and_a_bad_one_is_invalid() {
    use crate::native_host::declarations::validate_record;
    let spec = format("epub", 0);
    assert_eq!(validate_record(PLUGIN, &Record::FormatSpec(spec)), Ok(()));
    // The lookup and book declarations are checked and refused until the
    // host admits them.
    assert_eq!(
        validate_record(PLUGIN, &Record::LookupSpec(lookup("en", 0))),
        Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
    );
    let mut bad = format("EPUB", 0);
    bad.name = "epub";
    assert_eq!(
        validate_record(PLUGIN, &Record::FormatSpec(bad)),
        Err(ERR_INVALID)
    );
}

/// The SDK's `Declarations::format` refuses exactly what the host's
/// `format_check` refuses, for names, titles and extensions alike.
#[test]
fn the_sdk_builder_and_the_host_check_agree_on_formats() {
    use paperpaw_sdk::native::declare::{Declarations, FormatDecl};
    let long = "x".repeat(32);
    let many = (0..17)
        .map(|i| format!("e{i:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let fits = (0..16)
        .map(|i| format!("e{i:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let long_name = "a".repeat(96);
    let extensions = [
        "epub",
        "fb2.zip",
        "fb2\nfb2.zip",
        "a1",
        &fits,
        &"x".repeat(31),
        "",
        "EPUB",
        ".epub",
        "epub.",
        "a..b",
        "ep ub",
        "é",
        "a/b",
        "a-b",
        "a_b",
        "epub\n",
        "\nepub",
        "epub\nepub",
        "ep\tub",
        &long,
        &many,
    ];
    // A 97-byte name never reaches the host check: the codec caps it at 96.
    let names = [
        "epub", "a.b", &long_name, "", "Epub", "a b", ".a", "a..b", "a.",
    ];
    let titles = ["format.epub", "", "t"];
    let mut cases = Vec::new();
    for ext in extensions {
        cases.push(("epub", "format.epub", ext));
    }
    for name in names {
        cases.push((name, "format.epub", "epub"));
    }
    for title in titles {
        cases.push(("epub", title, "epub"));
    }
    for (name, title, extensions) in cases {
        let mut batches: Vec<Vec<u8>> = Vec::new();
        let mut declarations = Declarations::new(|batch: &[u8]| {
            batches.push(batch.to_vec());
            Ok(())
        });
        let sdk_result = declarations
            .format(FormatDecl::new(name, title, extensions, 1))
            .map(|_| ());
        let sdk_ok = sdk_result.is_ok();
        if sdk_ok {
            declarations.submit().unwrap();
        }
        drop(declarations);
        assert_eq!(batches.len(), usize::from(sdk_ok));
        let spec = FormatSpec {
            name,
            title,
            extensions,
            flags: 0,
            priority: 0,
            token: 1,
        };
        // A plugin id that keeps the joined id within 128 bytes for every case.
        let host_ok = format_check("p", &spec).is_ok();
        assert_eq!(
            sdk_ok, host_ok,
            "{name:?} {title:?} {extensions:?}: {sdk_result:?}"
        );
    }
}

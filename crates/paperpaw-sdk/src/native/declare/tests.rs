//! The builder's own checks: the bytes it writes, and each refusal once. That
//! what it writes is accepted by the host's validator is tested in
//! `paperpaw-core` (`tests/abi7_declare_roundtrip.rs`).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::records_wire::{Record, records_decode};
use super::*;

const GROUP: &str = "core.group.reading";

fn bool_spec(name: &str) -> SpecDecl<'_> {
    SpecDecl::new(name, Kind::Bool(true), GROUP, "x.setting.title")
}

fn collecting(
    batches: &mut Vec<Vec<u8>>,
) -> Declarations<impl FnMut(&[u8]) -> Result<(), i32> + '_> {
    Declarations::new(|batch: &[u8]| {
        batches.push(batch.to_vec());
        Ok(())
    })
}

fn choice<'a>(default: &'a str, choices: &'a [&'a str]) -> Kind<'a> {
    Kind::Choice { default, choices }
}

fn decode(batch: &[u8]) -> Vec<String> {
    records_decode(batch)
        .unwrap()
        .iter()
        .map(|record| match record {
            Record::SettingGroup(g) => alloc::format!("group {}", g.name),
            Record::SettingSpec(s) => alloc::format!("spec {} kind {}", s.name, s.kind),
            Record::SettingExtra(e) => alloc::format!("extra {}", e.name),
            Record::ResourceHyphenation(h) => {
                alloc::format!("hyphenation {} {}", h.language, h.path)
            }
            Record::ResourceFontFace(f) => alloc::format!("face {}", f.name),
            Record::ResourceFontExtension(e) => alloc::format!("extension {}", e.name),
            _ => "other".to_string(),
        })
        .collect()
}

#[test]
fn the_generated_copies_are_the_generated_files() {
    extern crate std;
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let generated = std::fs::read(here.join("../../abi/generated/records_wire.rs"))
        .expect("the generated records codec must be present");
    let copy = std::fs::read(here.join("src/native/declare/records_wire.rs")).unwrap();
    assert!(
        generated == copy,
        "src/native/declare/records_wire.rs is not abi/generated/records_wire.rs"
    );
    let generated = std::fs::read(here.join("../../abi/generated/settings_value.rs"))
        .expect("the generated settings codec must be present");
    let copy = std::fs::read(here.join("src/native/declare/settings_value.rs")).unwrap();
    assert!(
        generated == copy,
        "src/native/declare/settings_value.rs is not abi/generated/settings_value.rs"
    );
}

#[test]
fn every_record_kind_is_written_and_read_back_in_order() {
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    d.group(GroupDecl::new("page", "x.group.page")).unwrap();
    d.spec(bool_spec("flag")).unwrap();
    d.extra(ExtraDecl::new("flag")).unwrap();
    d.hyphenation("de-ch", "hyphenation/de-ch.pphp").unwrap();
    d.font_face(FontFaceDecl {
        name: "serif",
        path: "fonts/serif.ppfp",
        family: "",
        style: 0,
        weight: 0,
        role: "",
        priority: 0,
    })
    .unwrap();
    d.font_extension(FontExtensionDecl {
        name: "more",
        path: "fonts/more.ppfp",
        extends: "x.serif",
    })
    .unwrap();
    d.submit().unwrap();
    drop(d);
    assert_eq!(batches.len(), 1);
    assert_eq!(
        decode(&batches[0]),
        [
            "group page",
            "spec flag kind 0",
            "extra flag",
            "hyphenation de-ch hyphenation/de-ch.pphp",
            "face serif",
            "extension more"
        ]
    );
}

#[test]
fn the_same_items_give_the_same_bytes() {
    let build = || {
        let mut batches = Vec::new();
        let mut d = collecting(&mut batches);
        d.spec(bool_spec("a")).unwrap();
        d.spec(bool_spec("b")).unwrap();
        d.submit().unwrap();
        drop(d);
        batches
    };
    assert_eq!(build(), build());
}

#[test]
fn many_items_split_into_batches_of_eight_in_order() {
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    for n in 0..53 {
        d.spec(bool_spec(&alloc::format!("s{n}"))).unwrap();
    }
    d.submit().unwrap();
    drop(d);
    assert_eq!(batches.len(), 7);
    let names: Vec<String> = batches.iter().flat_map(|b| decode(b)).collect();
    assert_eq!(names.len(), 53);
    assert_eq!(names[0], "spec s0 kind 0");
    assert_eq!(names[52], "spec s52 kind 0");
    assert!(batches.iter().all(|b| b.len() <= BATCH_MAX_BYTES));
}

#[test]
fn big_records_split_by_bytes_before_by_count() {
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    let values: Vec<String> = (0..32).map(|n| alloc::format!("{n:0>30}")).collect();
    let choices: Vec<&str> = values.iter().map(String::as_str).collect();
    for n in 0..8 {
        let kind = Kind::Choice {
            default: choices[0],
            choices: &choices,
        };
        d.spec(SpecDecl::new(&alloc::format!("c{n}"), kind, GROUP, "x.t"))
            .unwrap();
    }
    d.submit().unwrap();
    drop(d);
    assert!(batches.len() > 1);
    assert!(batches.iter().all(|b| b.len() <= BATCH_MAX_BYTES));
    let total: usize = batches
        .iter()
        .map(|b| records_decode(b).unwrap().len())
        .sum();
    assert_eq!(total, 8);
}

#[test]
fn nothing_is_added_when_an_item_is_refused() {
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    assert!(d.group(GroupDecl::new("g", "")).is_err());
    assert!(d.is_empty());
    d.submit().unwrap();
    drop(d);
    assert!(batches.is_empty());
}

#[test]
fn invalid_setting_fields_are_refused() {
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    let long = |n| "a".repeat(n);
    let mut empty_group = bool_spec("n");
    empty_group.group = "";
    let mut empty_title = bool_spec("n");
    empty_title.title = "";
    let overlong = long(97);
    let mut long_name = bool_spec("n");
    long_name.name = &overlong;
    let mut long_help = bool_spec("n");
    long_help.help = &overlong;
    let mut secret_scoped = bool_spec("n");
    secret_scoped.secret = true;
    secret_scoped.scoped = true;
    let cases = [
        // (error, result)
        (
            DeclareError::Empty("setting group"),
            d.spec(empty_group).map(|_| ()),
        ),
        (
            DeclareError::Empty("setting title"),
            d.spec(empty_title).map(|_| ()),
        ),
        (
            DeclareError::TooLong("setting name"),
            d.spec(long_name).map(|_| ()),
        ),
        (
            DeclareError::TooLong("setting help"),
            d.spec(long_help).map(|_| ()),
        ),
        (
            DeclareError::SecretScoped,
            d.spec(secret_scoped).map(|_| ()),
        ),
    ];
    for (expected, result) in cases {
        assert_eq!(result, Err(expected));
    }
    assert!(d.is_empty());
}

#[test]
fn invalid_setting_kinds_are_refused() {
    let mut d = Declarations::new(|_| Ok(()));
    let spec = |kind| SpecDecl::new("n", kind, GROUP, "t");
    let long = |n| "a".repeat(n);
    let cases = [
        (
            DeclareError::Range,
            d.spec(spec(Kind::Int {
                default: 0,
                min: 5,
                max: 1,
            }))
            .map(|_| ()),
        ),
        (
            DeclareError::Default,
            d.spec(spec(Kind::Int {
                default: 9,
                min: 0,
                max: 5,
            }))
            .map(|_| ()),
        ),
        (
            DeclareError::Default,
            d.spec(spec(choice("c", &["a", "b"]))).map(|_| ()),
        ),
        (
            DeclareError::Choices,
            d.spec(spec(choice("a", &[]))).map(|_| ()),
        ),
        (
            DeclareError::Choices,
            d.spec(spec(choice("a", &["a", ""]))).map(|_| ()),
        ),
        (
            DeclareError::Choices,
            d.spec(spec(choice("a", &["a", "a"]))).map(|_| ()),
        ),
        (
            DeclareError::Choices,
            d.spec(spec(choice("a", &[&long(97)]))).map(|_| ()),
        ),
    ];
    for (expected, result) in cases {
        assert_eq!(result, Err(expected));
    }
    assert!(d.is_empty());
}

#[test]
fn choice_count_limit_is_inclusive() {
    let mut d = Declarations::new(|_| Ok(()));
    let values: Vec<String> = (0..33).map(|i| alloc::format!("v{i}")).collect();
    let choices: Vec<&str> = values.iter().map(String::as_str).collect();
    assert_eq!(
        d.spec(SpecDecl::new("n", choice(choices[0], &choices), GROUP, "t"))
            .map(|_| ()),
        Err(DeclareError::Choices)
    );
    assert!(d.is_empty());
    d.spec(SpecDecl::new(
        "n",
        choice(choices[0], &choices[..32]),
        GROUP,
        "t",
    ))
    .unwrap();
    assert_eq!(d.len(), 1);
}

#[test]
fn each_rule_of_the_other_records_is_violated_once() {
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    let long = |n| "a".repeat(n);
    let cases = [
        (
            DeclareError::Empty("group title"),
            d.group(GroupDecl::new("g", "")).map(|_| ()),
        ),
        (DeclareError::TooLong("group parent"), {
            let mut g = GroupDecl::new("g", "t");
            let parent = long(129);
            g.parent = &parent;
            d.group(g).map(|_| ())
        }),
        (DeclareError::Range, {
            let mut e = ExtraDecl::new("n");
            e.step = -1;
            d.extra(e).map(|_| ())
        }),
        (DeclareError::Empty("shown_if id"), {
            let mut e = ExtraDecl::new("n");
            e.shown_if = Some(("", "1"));
            d.extra(e).map(|_| ())
        }),
        (
            DeclareError::Empty("language"),
            d.hyphenation("", "a.pphp").map(|_| ()),
        ),
        (
            DeclareError::Empty("path"),
            d.hyphenation("de", "").map(|_| ()),
        ),
        (
            DeclareError::TooLong("language"),
            d.hyphenation(&long(36), "a.pphp").map(|_| ()),
        ),
        (
            DeclareError::TooLong("path"),
            d.hyphenation("de", &long(101)).map(|_| ()),
        ),
    ];
    for (expected, result) in cases {
        assert_eq!(result, Err(expected));
    }
    assert!(d.is_empty());
}

#[test]
fn a_default_over_the_wire_cap_is_refused() {
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    let overlong = "a".repeat(501);
    let text = SpecDecl::new("n", Kind::Text(&overlong), GROUP, "t");
    assert_eq!(
        d.spec(text).map(|_| ()),
        Err(DeclareError::TooLong("default"))
    );
    // 500 text bytes plus the 12-byte header is the most a default holds.
    let longest = "a".repeat(500);
    let text = SpecDecl::new("n", Kind::Text(&longest), GROUP, "t");
    assert!(d.spec(text).is_ok());
}

#[test]
fn flags_and_the_default_step_are_canonical() {
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    let mut spec = bool_spec("s");
    spec.scoped = true;
    spec.advanced = true;
    spec.kept_on_reset = true;
    d.spec(spec).unwrap();
    let mut extra = ExtraDecl::new("s");
    extra.step = 1;
    extra.slider = true;
    d.extra(extra).unwrap();
    d.submit().unwrap();
    drop(d);
    let batch = batches.remove(0);
    let batch = records_decode(&batch).unwrap();
    let records: Vec<_> = batch.iter().collect();
    match (&records[0], &records[1]) {
        (Record::SettingSpec(s), Record::SettingExtra(e)) => {
            assert_eq!(s.flags, 1 | 1 << 2 | 1 << 4);
            assert_eq!((e.step, e.flags, e.check), (0, 1, 0));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn integer_defaults_stay_inside_the_i32_wire_range() {
    let mut d = Declarations::new(|_| Ok(()));
    for default in [
        i64::MIN,
        i64::from(i32::MIN) - 1,
        i64::from(i32::MAX) + 1,
        i64::MAX,
    ] {
        assert_eq!(
            d.spec(SpecDecl::new(
                "n",
                Kind::Int {
                    default,
                    min: i32::MIN,
                    max: i32::MAX
                },
                GROUP,
                "t"
            ))
            .map(|_| ()),
            Err(DeclareError::Default)
        );
    }
    assert!(d.is_empty());
    for default in [i64::from(i32::MIN), i64::from(i32::MAX)] {
        d.spec(SpecDecl::new(
            "n",
            Kind::Int {
                default,
                min: i32::MIN,
                max: i32::MAX,
            },
            GROUP,
            "t",
        ))
        .unwrap();
    }
    assert_eq!(d.len(), 2);
    d.submit().unwrap();
}

#[test]
fn choice_text_limit_includes_separators() {
    let mut d = Declarations::new(|_| Ok(()));
    // Ten 96-byte choices, ten separators and a final 54-byte choice = 1024.
    let mut values: Vec<String> = (0..10).map(|n| alloc::format!("{n:0>96}")).collect();
    values.push("x".repeat(54));
    {
        let choices: Vec<&str> = values.iter().map(String::as_str).collect();
        d.spec(SpecDecl::new("n", choice(choices[0], &choices), GROUP, "t"))
            .unwrap();
    }
    values.last_mut().unwrap().push('x');
    let choices: Vec<&str> = values.iter().map(String::as_str).collect();
    assert_eq!(
        d.spec(SpecDecl::new("n", choice(choices[0], &choices), GROUP, "t"))
            .map(|_| ()),
        Err(DeclareError::TooLong("choices"))
    );
    assert_eq!(d.len(), 1);
    assert_eq!(
        d.spec(SpecDecl::new("n", choice("a", &["a", "b\nc"]), GROUP, "t"))
            .map(|_| ()),
        Err(DeclareError::Choices)
    );
}

#[test]
fn submit_flushes_only_pending_records_and_preserves_total() {
    use core::cell::Cell;
    let calls = Cell::new(0);
    let seen = Cell::new(0);
    let mut d = Declarations::new(|bytes: &[u8]| {
        let batch = records_decode(bytes).unwrap();
        assert!(!batch.is_empty());
        calls.set(calls.get() + 1);
        seen.set(seen.get() + batch.len());
        Ok(())
    });
    assert!(d.is_empty());
    d.submit().unwrap();
    assert_eq!(calls.get(), 0);
    for n in 0..BATCH_MAX_RECORDS + 1 {
        d.spec(bool_spec(&alloc::format!("s{n}"))).unwrap();
    }
    assert_eq!(calls.get(), 1);
    assert_eq!(seen.get(), BATCH_MAX_RECORDS);
    assert_eq!(d.len(), BATCH_MAX_RECORDS + 1);
    d.submit().unwrap();
    d.submit().unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(seen.get(), d.len());
    assert!(!d.is_empty());
    d.spec(bool_spec("after-submit")).unwrap();
    d.submit().unwrap();
    assert_eq!(calls.get(), 3);
    assert_eq!(d.len(), BATCH_MAX_RECORDS + 2);
    assert_eq!(seen.get(), d.len());
}

fn assert_poisoned<F: FnMut(&[u8]) -> Result<(), i32>>(d: &mut Declarations<F>, status: i32) {
    let count = d.len();
    let error = Err(DeclareError::Sink(status));
    for title in ["t", ""] {
        assert_eq!(d.group(GroupDecl::new("g", title)).map(|_| ()), error);
        assert_eq!(
            d.spec(SpecDecl::new("n", Kind::Bool(true), GROUP, title))
                .map(|_| ()),
            error
        );
    }
    for step in [1, -1] {
        let mut extra = ExtraDecl::new("n");
        extra.step = step;
        assert_eq!(d.extra(extra).map(|_| ()), error);
    }
    for path in ["a.pphp", ""] {
        assert_eq!(d.hyphenation("de", path).map(|_| ()), error);
    }
    for path in ["a.ppfp", ""] {
        assert_eq!(d.font_face(FontFaceDecl::new("f", path)).map(|_| ()), error);
        assert_eq!(
            d.font_extension(FontExtensionDecl::new("e", path, "x.f"))
                .map(|_| ()),
            error
        );
    }
    assert_eq!(d.submit(), error);
    assert_eq!(d.submit(), error);
    assert_eq!(d.len(), count);
}

#[test]
fn explicit_sink_refusal_is_permanent_for_every_add_even_invalid_ones() {
    use core::cell::Cell;
    for status in [-77, i32::MIN, i32::MAX] {
        let calls = Cell::new(0);
        let mut d = Declarations::new(|_: &[u8]| {
            calls.set(calls.get() + 1);
            Err(status)
        });
        d.spec(bool_spec("first")).unwrap();
        assert_eq!(d.submit(), Err(DeclareError::Sink(status)));
        assert_poisoned(&mut d, status);
        assert_eq!(calls.get(), 1);
        assert_eq!(d.len(), 1);
        drop(d);
        assert_eq!(calls.get(), 1);
    }
}

#[test]
fn count_limit_flush_refusal_does_not_accept_the_next_record() {
    use core::cell::Cell;
    let calls = Cell::new(0);
    let mut d = Declarations::new(|bytes: &[u8]| {
        calls.set(calls.get() + 1);
        assert_eq!(records_decode(bytes).unwrap().len(), BATCH_MAX_RECORDS);
        Err(-77)
    });
    for n in 0..BATCH_MAX_RECORDS {
        d.spec(bool_spec(&alloc::format!("s{n}"))).unwrap();
    }
    assert_eq!(
        d.spec(bool_spec("refused")).map(|_| ()),
        Err(DeclareError::Sink(-77))
    );
    assert_eq!(d.len(), BATCH_MAX_RECORDS);
    assert_poisoned(&mut d, -77);
    assert_eq!(calls.get(), 1);
}

#[test]
fn byte_limit_flush_refusal_stops_before_accepting_the_next_record() {
    use core::cell::Cell;
    let calls = Cell::new(0);
    let mut d = Declarations::new(|bytes: &[u8]| {
        calls.set(calls.get() + 1);
        assert!(bytes.len() <= BATCH_MAX_BYTES);
        assert!(records_decode(bytes).unwrap().len() < BATCH_MAX_RECORDS);
        Err(-77)
    });
    let values: Vec<String> = (0..32).map(|n| alloc::format!("{n:0>30}")).collect();
    let choices: Vec<&str> = values.iter().map(String::as_str).collect();
    let mut accepted = 0;
    for n in 0..BATCH_MAX_RECORDS {
        let result = d.spec(SpecDecl::new(
            &alloc::format!("c{n}"),
            choice(choices[0], &choices),
            GROUP,
            "t",
        ));
        if let Err(error) = result {
            assert_eq!(error, DeclareError::Sink(-77));
            break;
        }
        accepted += 1;
    }
    assert!(accepted > 0 && accepted < BATCH_MAX_RECORDS);
    assert_eq!(d.len(), accepted);
    assert_poisoned(&mut d, -77);
    assert_eq!(calls.get(), 1);
}

#[test]
fn dropping_a_pending_builder_does_not_submit() {
    use core::cell::Cell;
    let calls = Cell::new(0);
    {
        let mut d = Declarations::new(|_: &[u8]| {
            calls.set(calls.get() + 1);
            Ok(())
        });
        d.spec(bool_spec("pending")).unwrap();
    }
    assert_eq!(calls.get(), 0);
}

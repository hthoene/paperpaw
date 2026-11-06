//! The builder's own checks: the bytes it writes, and each refusal once. That
//! what it writes is accepted by the host's validator is tested in
//! `paperpaw-core` (`tests/abi7_declare_roundtrip.rs`).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::records_wire::{Record, records_decode};
use super::*;

const GROUP: &str = "core.group.reading";

fn bool_spec(name: &str) -> SpecDecl {
    SpecDecl::new(name, Kind::Bool(true), GROUP, "x.setting.title")
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
    // A vendored copy of the crate has no `abi/` beside it: nothing to compare.
    let Ok(generated) = std::fs::read(here.join("../../abi/generated/records_wire.rs")) else {
        return;
    };
    let copy = std::fs::read(here.join("src/native/declare/records_wire.rs")).unwrap();
    assert!(
        generated == copy,
        "src/native/declare/records_wire.rs is not abi/generated/records_wire.rs"
    );
    let generated = std::fs::read(here.join("../../abi/generated/settings_value.rs")).unwrap();
    let copy = std::fs::read(here.join("src/native/declare/settings_value.rs")).unwrap();
    assert!(
        generated == copy,
        "src/native/declare/settings_value.rs is not abi/generated/settings_value.rs"
    );
}

#[test]
fn every_record_kind_is_written_and_read_back_in_order() {
    let mut d = Declarations::new();
    d.group(GroupDecl::new("page", "x.group.page")).unwrap();
    d.spec(bool_spec("flag")).unwrap();
    d.extra(ExtraDecl::new("flag")).unwrap();
    d.hyphenation("de-ch", "hyphenation/de-ch.pphp").unwrap();
    d.font_face(FontFaceDecl {
        name: "serif".into(),
        path: "fonts/serif.ppfp".into(),
        family: String::new(),
        style: 0,
        weight: 0,
        role: String::new(),
        priority: 0,
    })
    .unwrap();
    d.font_extension(FontExtensionDecl {
        name: "more".into(),
        path: "fonts/more.ppfp".into(),
        extends: "x.serif".into(),
    })
    .unwrap();
    let batches = d.batches().unwrap();
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
        let mut d = Declarations::new();
        d.spec(bool_spec("a")).unwrap();
        d.spec(bool_spec("b")).unwrap();
        d.batches().unwrap()
    };
    assert_eq!(build(), build());
}

#[test]
fn many_items_split_into_batches_of_eight_in_order() {
    let mut d = Declarations::new();
    for n in 0..53 {
        d.spec(bool_spec(&alloc::format!("s{n}"))).unwrap();
    }
    let batches = d.batches().unwrap();
    assert_eq!(batches.len(), 7);
    let names: Vec<String> = batches.iter().flat_map(|b| decode(b)).collect();
    assert_eq!(names.len(), 53);
    assert_eq!(names[0], "spec s0 kind 0");
    assert_eq!(names[52], "spec s52 kind 0");
    assert!(batches.iter().all(|b| b.len() <= BATCH_MAX_BYTES));
}

#[test]
fn big_records_split_by_bytes_before_by_count() {
    let mut d = Declarations::new();
    let choices: Vec<String> = (0..32).map(|n| alloc::format!("{n:0>30}")).collect();
    for n in 0..8 {
        let kind = Kind::Choice {
            default: choices[0].clone(),
            choices: choices.clone(),
        };
        d.spec(SpecDecl::new(&alloc::format!("c{n}"), kind, GROUP, "x.t"))
            .unwrap();
    }
    let batches = d.batches().unwrap();
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
    let mut d = Declarations::new();
    assert!(d.group(GroupDecl::new("g", "")).is_err());
    assert!(d.is_empty());
    assert!(d.batches().unwrap().is_empty());
}

#[test]
fn each_rule_of_a_setting_is_violated_once() {
    let mut d = Declarations::new();
    let spec = |kind| SpecDecl::new("n", kind, GROUP, "t");
    let choice = |default: &str, values: &[&str]| Kind::Choice {
        default: default.to_string(),
        choices: values.iter().map(|v| v.to_string()).collect(),
    };
    let long = |n| "a".repeat(n);
    let mut empty_group = bool_spec("n");
    empty_group.group.clear();
    let mut empty_title = bool_spec("n");
    empty_title.title.clear();
    let mut long_name = bool_spec("n");
    long_name.name = long(97);
    let mut long_help = bool_spec("n");
    long_help.help = long(97);
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
    // 33 values are too many; 32 are not.
    let many = |n: usize| -> Kind {
        let values: Vec<String> = (0..n).map(|i| alloc::format!("v{i}")).collect();
        Kind::Choice {
            default: values[0].clone(),
            choices: values,
        }
    };
    assert_eq!(
        d.spec(spec(many(33))).map(|_| ()),
        Err(DeclareError::Choices)
    );
    assert!(d.spec(spec(many(32))).is_ok());
    assert_eq!(d.len(), 1);
}

#[test]
fn each_rule_of_the_other_records_is_violated_once() {
    let mut d = Declarations::new();
    let long = |n| "a".repeat(n);
    let cases = [
        (
            DeclareError::Empty("group title"),
            d.group(GroupDecl::new("g", "")).map(|_| ()),
        ),
        (DeclareError::TooLong("group parent"), {
            let mut g = GroupDecl::new("g", "t");
            g.parent = long(129);
            d.group(g).map(|_| ())
        }),
        (DeclareError::Range, {
            let mut e = ExtraDecl::new("n");
            e.step = -1;
            d.extra(e).map(|_| ())
        }),
        (DeclareError::Empty("shown_if id"), {
            let mut e = ExtraDecl::new("n");
            e.shown_if = Some((String::new(), "1".to_string()));
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
    let mut d = Declarations::new();
    let text = SpecDecl::new("n", Kind::Text("a".repeat(501)), GROUP, "t");
    assert_eq!(
        d.spec(text).map(|_| ()),
        Err(DeclareError::TooLong("default"))
    );
    // 500 text bytes plus the 12-byte header is the most a default holds.
    let text = SpecDecl::new("n", Kind::Text("a".repeat(500)), GROUP, "t");
    assert!(d.spec(text).is_ok());
}

#[test]
fn flags_and_the_default_step_are_canonical() {
    let mut d = Declarations::new();
    let mut spec = bool_spec("s");
    spec.scoped = true;
    spec.advanced = true;
    spec.kept_on_reset = true;
    d.spec(spec).unwrap();
    let mut extra = ExtraDecl::new("s");
    extra.step = 1;
    extra.slider = true;
    d.extra(extra).unwrap();
    let batch = d.batches().unwrap().remove(0);
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

//! What the SDK's declaration builder writes is accepted by the host's decoder
//! and validator, and nothing the validator refuses gets through the builder.
//! The builder cannot reach `validate_batch` (it lives in this crate), so the
//! two are held together here: every batch built is decoded and validated, and
//! each rule is violated once through the builder, which must refuse it itself
//! or leave it to the validator.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use paperpaw_core::native_host::{
    declarations::validate_batch,
    registrations::{Record, records_decode},
};
use paperpaw_native::host_glue::ERR_INVALID;
use paperpaw_sdk::native::declare::{
    Declarations, EditorDecl, ExtraDecl, FontExtensionDecl, FontFaceDecl, GroupDecl, Kind, SpecDecl,
};

const PLUGIN: &str = "weather";
const GROUP: &str = "weather.page";
/// 40 bytes: with a 90-byte name the full id is 131.
const LONG: &str = "a-plugin-with-a-long-id-for-this-test-xx";

fn spec(name: &str, kind: Kind) -> SpecDecl {
    SpecDecl::new(name, kind, GROUP, "weather.setting.title")
}

/// Every batch decodes, validates, and holds the records that were added.
fn assert_accepted(d: &Declarations, records: usize) {
    let mut seen = 0;
    for batch in d.batches().unwrap() {
        let decoded = records_decode(&batch).unwrap();
        validate_batch(PLUGIN, &decoded).unwrap();
        seen += decoded.len();
    }
    assert_eq!(seen, records);
}

#[test]
fn a_valid_declaration_of_every_record_and_kind_is_accepted() {
    let mut d = Declarations::new();
    let mut group = GroupDecl::new("page", "weather.group.page");
    group.order = 3;
    group.parent = "core.group.reading".into();
    d.group(group).unwrap();
    d.spec(spec("flag", Kind::Bool(false))).unwrap();
    d.spec(spec(
        "size",
        Kind::Int {
            default: 12,
            min: 8,
            max: 40,
        },
    ))
    .unwrap();
    d.spec(spec(
        "unit",
        Kind::Choice {
            default: "weather.unit.c".into(),
            choices: vec!["weather.unit.c".into(), "weather.unit.f".into()],
        },
    ))
    .unwrap();
    d.spec(spec("city", Kind::Text(String::new()))).unwrap();
    d.spec(spec("lang", Kind::Language("de".into()))).unwrap();
    d.spec(spec("items", Kind::List("a,b".into()))).unwrap();
    let mut extra = ExtraDecl::new("size");
    extra.unit = "core.unit.pt".into();
    extra.step = 2;
    extra.slider = true;
    extra.editor = Some(EditorDecl::Screen("picker".into()));
    extra.shown_if = Some(("weather.flag".into(), "true".into()));
    d.extra(extra).unwrap();
    d.hyphenation("de-ch", "hyphenation/de-ch.pphp").unwrap();
    d.font_face({
        let mut face = FontFaceDecl::new("serif", "fonts/serif.ppfp");
        face.family = "Serif".into();
        face.style = 1;
        face.role = "core.role.body".into();
        face.priority = 10;
        face
    })
    .unwrap();
    d.font_extension(FontExtensionDecl::new(
        "more",
        "fonts/more.ppfp",
        "weather.serif",
    ))
    .unwrap();
    assert_accepted(&d, 11);
}

#[test]
fn many_specs_in_many_batches_are_accepted() {
    let mut d = Declarations::new();
    for n in 0..53 {
        d.spec(spec(&format!("s{n}"), Kind::Bool(n % 2 == 0)))
            .unwrap();
    }
    assert_accepted(&d, 53);
}

/// What an author can still get wrong after the builder: the validator refuses
/// every one of these (`ERR_INVALID`), through a batch the builder wrote.
#[test]
fn rules_the_host_checks_are_refused_in_a_batch_the_builder_wrote() {
    let mut cases: Vec<(&str, Declarations)> = Vec::new();
    let mut with = |label, f: &dyn Fn(&mut Declarations)| {
        let mut d = Declarations::new();
        f(&mut d);
        cases.push((label, d));
    };
    with("name with a capital", &|d| {
        d.spec(spec("Flag", Kind::Bool(true))).unwrap();
    });
    with("name with an empty segment", &|d| {
        d.spec(spec("a..b", Kind::Bool(true))).unwrap();
    });
    with("group not an id", &|d| {
        d.spec(SpecDecl::new("flag", Kind::Bool(true), "Bad Group", "t"))
            .unwrap();
    });
    // A long plugin id (`LONG`) and a long name together pass 128 bytes.
    with("full id over 128 bytes", &|d| {
        d.spec(spec(&"a".repeat(90), Kind::Bool(true))).unwrap();
    });
    with("language tag with a space", &|d| {
        d.spec(spec("lang", Kind::Language("de ch".into())))
            .unwrap();
    });
    with("hyphenation path with ..", &|d| {
        d.hyphenation("de", "../x.pphp").unwrap();
    });
    with("hyphenation path with 4 segments", &|d| {
        d.hyphenation("de", "a/b/c/d.pphp").unwrap();
    });
    with("hyphenation language with an underscore", &|d| {
        d.hyphenation("de_ch", "a.pphp").unwrap();
    });
    with("font extension of a ttf", &|d| {
        d.font_extension(FontExtensionDecl::new(
            "more",
            "fonts/more.ttf",
            "weather.serif",
        ))
        .unwrap();
    });
    with("font face without a font file", &|d| {
        d.font_face(FontFaceDecl::new("serif", "fonts/serif.txt"))
            .unwrap();
    });
    with("priority without a role", &|d| {
        d.font_face({
            let mut face = FontFaceDecl::new("serif", "fonts/serif.ppfp");
            face.priority = 5;
            face
        })
        .unwrap();
    });
    with("shown_if value without a shown id", &|d| {
        let mut e = ExtraDecl::new("flag");
        e.shown_if = Some(("weather.flag".into(), String::new()));
        d.extra(e).unwrap();
    });
    for (label, d) in cases {
        let batch = d.batches().unwrap().remove(0);
        let decoded = records_decode(&batch).unwrap();
        let plugin = if label.starts_with("full id") {
            LONG
        } else {
            PLUGIN
        };
        assert_eq!(
            validate_batch(plugin, &decoded),
            Err(ERR_INVALID),
            "{label}"
        );
    }
}

/// What the builder refuses itself must be what the validator refuses too: the
/// builder is never stricter on a rule the host would accept.
#[test]
fn what_the_builder_refuses_the_host_would_refuse() {
    // An empty title: written by hand, the validator refuses it.
    use paperpaw_core::native_host::registrations::{SettingGroup, records_encode};
    let group = SettingGroup {
        name: "g",
        title: "",
        order: 0,
        page: "",
        parent: "",
    };
    let mut out = [0u8; 4096];
    let len = records_encode(&[Record::SettingGroup(group)], &mut out).unwrap();
    let decoded = records_decode(&out[..len]).unwrap();
    assert_eq!(validate_batch(PLUGIN, &decoded), Err(ERR_INVALID));
    assert!(Declarations::new().group(GroupDecl::new("g", "")).is_err());
}

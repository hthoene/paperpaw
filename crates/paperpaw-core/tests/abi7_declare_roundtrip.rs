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

fn spec<'a>(name: &'a str, kind: Kind<'a>) -> SpecDecl<'a> {
    SpecDecl::new(name, kind, GROUP, "weather.setting.title")
}

/// Every batch decodes, validates, and holds the records that were added.
fn assert_accepted(batches: &[Vec<u8>], records: usize) {
    let mut seen = 0;
    for batch in batches {
        let decoded = records_decode(batch).unwrap();
        validate_batch(PLUGIN, &decoded).unwrap();
        seen += decoded.len();
    }
    assert_eq!(seen, records);
}

#[test]
fn a_valid_declaration_of_every_record_and_kind_is_accepted() {
    let mut batches = Vec::new();
    let mut d = Declarations::new(|batch: &[u8]| {
        batches.push(batch.to_vec());
        Ok(())
    });
    let mut group = GroupDecl::new("page", "weather.group.page");
    group.order = 3;
    group.parent = "core.group.reading";
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
            default: "weather.unit.c",
            choices: &["weather.unit.c", "weather.unit.f"],
        },
    ))
    .unwrap();
    d.spec(spec("city", Kind::Text(""))).unwrap();
    d.spec(spec("lang", Kind::Language("de"))).unwrap();
    d.spec(spec("items", Kind::List("a,b"))).unwrap();
    let mut extra = ExtraDecl::new("size");
    extra.unit = "core.unit.pt";
    extra.step = 2;
    extra.slider = true;
    extra.editor = Some(EditorDecl::Screen("picker"));
    extra.shown_if = Some(("weather.flag", "true"));
    d.extra(extra).unwrap();
    d.hyphenation("de-ch", "hyphenation/de-ch.pphp").unwrap();
    d.font_face({
        let mut face = FontFaceDecl::new("serif", "fonts/serif.ppfp");
        face.family = "Serif";
        face.style = 1;
        face.role = "core.role.body";
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
    d.submit().unwrap();
    drop(d);
    assert_accepted(&batches, 11);
}

#[test]
fn many_specs_in_many_batches_are_accepted() {
    let mut batches = Vec::new();
    let mut d = Declarations::new(|batch: &[u8]| {
        batches.push(batch.to_vec());
        Ok(())
    });
    for n in 0..53 {
        d.spec(spec(&format!("s{n}"), Kind::Bool(n % 2 == 0)))
            .unwrap();
    }
    d.submit().unwrap();
    drop(d);
    assert_accepted(&batches, 53);
}

/// What an author can still get wrong after the builder: the validator refuses
/// every one of these (`ERR_INVALID`), through a batch the builder wrote.
#[test]
fn rules_the_host_checks_are_refused_in_a_batch_the_builder_wrote() {
    refused("name with a capital", |d| {
        d.spec(spec("Flag", Kind::Bool(true))).unwrap();
    });
    refused("name with an empty segment", |d| {
        d.spec(spec("a..b", Kind::Bool(true))).unwrap();
    });
    refused("group not an id", |d| {
        d.spec(SpecDecl::new("flag", Kind::Bool(true), "Bad Group", "t"))
            .unwrap();
    });
    // A long plugin id (`LONG`) and a long name together pass 128 bytes.
    refused("full id over 128 bytes", |d| {
        d.spec(spec(&"a".repeat(90), Kind::Bool(true))).unwrap();
    });
    refused("language tag with a space", |d| {
        d.spec(spec("lang", Kind::Language("de ch"))).unwrap();
    });
    refused("hyphenation path with ..", |d| {
        d.hyphenation("de", "../x.pphp").unwrap();
    });
    refused("hyphenation path with 4 segments", |d| {
        d.hyphenation("de", "a/b/c/d.pphp").unwrap();
    });
    refused("hyphenation language with an underscore", |d| {
        d.hyphenation("de_ch", "a.pphp").unwrap();
    });
    refused("font extension of a ttf", |d| {
        d.font_extension(FontExtensionDecl::new(
            "more",
            "fonts/more.ttf",
            "weather.serif",
        ))
        .unwrap();
    });
    refused("font face without a font file", |d| {
        d.font_face(FontFaceDecl::new("serif", "fonts/serif.txt"))
            .unwrap();
    });
    refused("priority without a role", |d| {
        d.font_face({
            let mut face = FontFaceDecl::new("serif", "fonts/serif.ppfp");
            face.priority = 5;
            face
        })
        .unwrap();
    });
    refused("shown_if value without a shown id", |d| {
        let mut e = ExtraDecl::new("flag");
        e.shown_if = Some(("weather.flag", ""));
        d.extra(e).unwrap();
    });
}

type Sink<'a> = &'a mut dyn FnMut(&[u8]) -> Result<(), i32>;

fn refused(label: &str, fill: impl FnOnce(&mut Declarations<Sink<'_>>)) {
    let mut batches = Vec::new();
    {
        let mut sink = |batch: &[u8]| {
            batches.push(batch.to_vec());
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as Sink<'_>);
        fill(&mut d);
        d.submit().unwrap();
    }
    assert_eq!(batches.len(), 1, "{label}");
    let decoded = records_decode(&batches[0]).unwrap();
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
    assert!(
        Declarations::new(|_| Ok(()))
            .group(GroupDecl::new("g", ""))
            .is_err()
    );
}

#[test]
fn ignored_builder_refusal_aborts_host_construction() {
    use paperpaw_core::native_host::{
        HostOwner,
        registration_scope::RegistrationScope,
        registrations::{self, Registration},
    };
    use paperpaw_sdk::native::declare::DeclareError;
    let owner = Box::pin(HostOwner::new(RegistrationScope::<Registration>::new(), ()));
    let slot = owner.as_ref().context();
    let mut construction = owner.as_ref().begin(PLUGIN).unwrap();
    let mut calls = 0;
    slot.scope(&mut construction, || {
        let mut builder = Declarations::new(|bytes: &[u8]| {
            calls += 1;
            assert!(bytes.is_empty(), "pending valid records must not escape");
            // SAFETY: bytes remains readable for the synchronous submission.
            let status = unsafe { registrations::submit(&slot, bytes.as_ptr(), bytes.len()) };
            assert_eq!(status, ERR_INVALID);
            Err(status)
        });
        builder
            .group(GroupDecl::new("page", "weather.page"))
            .unwrap();
        let _ = builder.group(GroupDecl::new("bad", ""));
        assert_eq!(builder.submit(), Err(DeclareError::Empty("group title")));
        assert_eq!(builder.submit(), Err(DeclareError::Empty("group title")));
        // Simulate make ignoring every returned error.
    })
    .unwrap();
    assert_eq!(calls, 1);
    assert!(construction.commit(|_| ()).is_err());
}

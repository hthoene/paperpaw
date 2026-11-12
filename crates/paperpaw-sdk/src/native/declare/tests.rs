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
    for (generated, copy) in [
        ("records_wire.rs", "src/native/declare/records_wire.rs"),
        ("settings_value.rs", "src/native/declare/settings_value.rs"),
        ("snapshot_wire.rs", "src/native/wire/snapshot_wire.rs"),
        ("color_wire.rs", "src/native/wire/color_wire.rs"),
        ("commands_wire.rs", "src/native/wire/commands_wire.rs"),
    ] {
        let source = std::fs::read(here.join("../../abi/generated").join(generated))
            .expect("the generated codec must be present");
        let kept = std::fs::read(here.join(copy)).unwrap();
        assert!(source == kept, "{copy} is not abi/generated/{generated}");
    }
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
    assert!(d.submit().is_err());
    drop(d);
    assert_eq!(batches, [Vec::<u8>::new()]);
}

#[test]
fn invalid_setting_fields_are_refused() {
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
            Declarations::new(|_| Ok(())).spec(empty_group).map(|_| ()),
        ),
        (
            DeclareError::Empty("setting title"),
            Declarations::new(|_| Ok(())).spec(empty_title).map(|_| ()),
        ),
        (
            DeclareError::TooLong("setting name"),
            Declarations::new(|_| Ok(())).spec(long_name).map(|_| ()),
        ),
        (
            DeclareError::TooLong("setting help"),
            Declarations::new(|_| Ok(())).spec(long_help).map(|_| ()),
        ),
        (
            DeclareError::SecretScoped,
            Declarations::new(|_| Ok(()))
                .spec(secret_scoped)
                .map(|_| ()),
        ),
    ];
    for (expected, result) in cases {
        assert_eq!(result, Err(expected));
    }
}

#[test]
fn invalid_setting_kinds_are_refused() {
    let spec = |kind| SpecDecl::new("n", kind, GROUP, "t");
    let long = |n| "a".repeat(n);
    let cases = [
        (
            DeclareError::Range,
            Declarations::new(|_| Ok(()))
                .spec(spec(Kind::Int {
                    default: 0,
                    min: 5,
                    max: 1,
                }))
                .map(|_| ()),
        ),
        (
            DeclareError::Default,
            Declarations::new(|_| Ok(()))
                .spec(spec(Kind::Int {
                    default: 9,
                    min: 0,
                    max: 5,
                }))
                .map(|_| ()),
        ),
        (
            DeclareError::Default,
            Declarations::new(|_| Ok(()))
                .spec(spec(choice("c", &["a", "b"])))
                .map(|_| ()),
        ),
        (
            DeclareError::Choices,
            Declarations::new(|_| Ok(()))
                .spec(spec(choice("a", &[])))
                .map(|_| ()),
        ),
        (
            DeclareError::Choices,
            Declarations::new(|_| Ok(()))
                .spec(spec(choice("a", &["a", ""])))
                .map(|_| ()),
        ),
        (
            DeclareError::Choices,
            Declarations::new(|_| Ok(()))
                .spec(spec(choice("a", &["a", "a"])))
                .map(|_| ()),
        ),
        (
            DeclareError::Choices,
            Declarations::new(|_| Ok(()))
                .spec(spec(choice("a", &[&long(97)])))
                .map(|_| ()),
        ),
    ];
    for (expected, result) in cases {
        assert_eq!(result, Err(expected));
    }
}

#[test]
fn choice_count_limit_is_inclusive() {
    let mut d = Declarations::new(|_| Ok(()));
    let values: Vec<String> = (0..65).map(|i| alloc::format!("v{i}")).collect();
    let choices: Vec<&str> = values.iter().map(String::as_str).collect();
    assert_eq!(
        d.spec(SpecDecl::new("n", choice(choices[0], &choices), GROUP, "t"))
            .map(|_| ()),
        Err(DeclareError::Choices)
    );
    assert!(d.is_empty());
    let mut d = Declarations::new(|_| Ok(()));
    d.spec(SpecDecl::new(
        "n",
        choice(choices[0], &choices[..64]),
        GROUP,
        "t",
    ))
    .unwrap();
    assert_eq!(d.len(), 1);
    d.submit().unwrap();
}

#[test]
fn each_rule_of_the_other_records_is_violated_once() {
    let long = |n| "a".repeat(n);
    let cases = [
        (
            DeclareError::Empty("group title"),
            Declarations::new(|_| Ok(()))
                .group(GroupDecl::new("g", ""))
                .map(|_| ()),
        ),
        (DeclareError::TooLong("group parent"), {
            let mut g = GroupDecl::new("g", "t");
            let parent = long(129);
            g.parent = &parent;
            Declarations::new(|_| Ok(())).group(g).map(|_| ())
        }),
        (DeclareError::Range, {
            let mut e = ExtraDecl::new("n");
            e.step = -1;
            Declarations::new(|_| Ok(())).extra(e).map(|_| ())
        }),
        (DeclareError::Empty("shown_if id"), {
            let mut e = ExtraDecl::new("n");
            e.shown_if = Some(("", "1"));
            Declarations::new(|_| Ok(())).extra(e).map(|_| ())
        }),
        (
            DeclareError::Empty("language"),
            Declarations::new(|_| Ok(()))
                .hyphenation("", "a.pphp")
                .map(|_| ()),
        ),
        (
            DeclareError::Empty("path"),
            Declarations::new(|_| Ok(()))
                .hyphenation("de", "")
                .map(|_| ()),
        ),
        (
            DeclareError::TooLong("language"),
            Declarations::new(|_| Ok(()))
                .hyphenation(&long(36), "a.pphp")
                .map(|_| ()),
        ),
        (
            DeclareError::TooLong("path"),
            Declarations::new(|_| Ok(()))
                .hyphenation("de", &long(101))
                .map(|_| ()),
        ),
    ];
    for (expected, result) in cases {
        assert_eq!(result, Err(expected));
    }
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
    drop(d);
    let mut d = collecting(&mut batches);
    // 500 text bytes plus the 12-byte header is the most a default holds.
    let longest = "a".repeat(500);
    let text = SpecDecl::new("n", Kind::Text(&longest), GROUP, "t");
    assert!(d.spec(text).is_ok());
    d.submit().unwrap();
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
    for default in [
        i64::MIN,
        i64::from(i32::MIN) - 1,
        i64::from(i32::MAX) + 1,
        i64::MAX,
    ] {
        // A fresh builder each time: a refusal latches, so a reused builder
        // would only repeat the first refusal.
        let mut d = Declarations::new(|_| Ok(()));
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
        assert!(d.is_empty());
    }
    let mut d = Declarations::new(|_| Ok(()));
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
    assert_eq!(d.submit(), Err(DeclareError::TooLong("choices")));
    let mut d = Declarations::new(|_| Ok(()));
    assert_eq!(
        d.spec(SpecDecl::new("n", choice("a", &["a", "b\nc"]), GROUP, "t"))
            .map(|_| ()),
        Err(DeclareError::Choices)
    );
    assert_eq!(d.submit(), Err(DeclareError::Choices));
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
    for title in ["screen.title", ""] {
        assert_eq!(d.screen(ScreenDecl::new("s", title, 1)).map(|_| ()), error);
        assert_eq!(d.action(ActionDecl::new("a", title, 2)).map(|_| ()), error);
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
    extern crate std;
    use core::cell::Cell;
    let calls = Cell::new(0);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut d = Declarations::new(|_: &[u8]| {
            calls.set(calls.get() + 1);
            Ok(())
        });
        d.spec(bool_spec("pending")).unwrap();
    }));
    assert_eq!(result.is_err(), cfg!(debug_assertions));
    assert_eq!(calls.get(), 0);
}

#[test]
fn local_refusals_abort_once_and_never_send_pending_records() {
    for error in [
        DeclareError::Empty("title"),
        DeclareError::TooLong("name"),
        DeclareError::Default,
        DeclareError::Range,
        DeclareError::Choices,
        DeclareError::SecretScoped,
        DeclareError::NoMemory,
        DeclareError::Encode(-1),
    ] {
        let mut calls = Vec::new();
        let mut d = Declarations::new(|batch: &[u8]| {
            calls.push(batch.to_vec());
            Err(-2)
        });
        d.group(GroupDecl::new("g", "g.title")).unwrap();
        // Exercise the common terminal state, including encoder failures which
        // are otherwise precluded by the public field bounds.
        d.failed = Some(super::Latched::Refused(error));
        assert_eq!(
            d.group(GroupDecl::new("h", "h.title")).map(|_| ()),
            Err(error)
        );
        assert_eq!(d.submit(), Err(error));
        assert_eq!(d.submit(), Err(error));
        drop(d);
        assert_eq!(calls, [Vec::<u8>::new()]);
    }
}

#[test]
fn screen_and_action_tokens_and_flags_round_trip() {
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    let mut screen = ScreenDecl::new("main", "demo.screen.title", 0x7654_3210);
    screen.full_refresh_on_open = true;
    let mut action = ActionDecl::new("refresh", "demo.action.title", 0xfedc_ba98);
    action.needs_screen = true;
    d.screen(screen).unwrap();
    d.action(action).unwrap();
    d.screen(ScreenDecl::new("other", "demo.other.title", 0))
        .unwrap();
    d.action(ActionDecl::new("global", "demo.global.title", u32::MAX))
        .unwrap();
    d.submit().unwrap();
    drop(d);
    let batch = records_decode(&batches[0]).unwrap();
    let records: Vec<_> = batch.iter().collect();
    match records.as_slice() {
        [
            Record::ScreenSpec(screen),
            Record::ActionSpec(action),
            Record::ScreenSpec(other),
            Record::ActionSpec(global),
        ] => {
            assert_eq!(
                (screen.name, screen.title, screen.token, screen.flags),
                ("main", "demo.screen.title", 0x7654_3210, 1)
            );
            assert_eq!(
                (action.name, action.title, action.token, action.flags),
                ("refresh", "demo.action.title", 0xfedc_ba98, 1)
            );
            assert_eq!((other.token, other.flags), (0, 0));
            assert_eq!((global.token, global.flags), (u32::MAX, 0));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_screen_declared_with_its_hints_hidden_has_flag_bit_8() {
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    d.screen(ScreenDecl::new("sleep", "demo.sleep.title", 1).hints_hidden())
        .unwrap();
    d.screen(
        ScreenDecl::new("lock", "demo.lock.title", 2)
            .guards()
            .hints_hidden(),
    )
    .unwrap();
    d.submit().unwrap();
    drop(d);
    let batch = records_decode(&batches[0]).unwrap();
    let flags: Vec<u32> = batch
        .iter()
        .filter_map(|record| match record {
            Record::ScreenSpec(screen) => Some(screen.flags),
            _ => None,
        })
        .collect();
    assert_eq!(flags, [0x100, 0x102]);
}

#[test]
fn a_screen_declared_best_in_grays_has_flag_bit_9() {
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    d.screen(ScreenDecl::new("page", "demo.page.title", 1).grays())
        .unwrap();
    d.screen(
        ScreenDecl::new("sleep", "demo.sleep.title", 2)
            .hints_hidden()
            .grays(),
    )
    .unwrap();
    d.submit().unwrap();
    drop(d);
    let batch = records_decode(&batches[0]).unwrap();
    let flags: Vec<u32> = batch
        .iter()
        .filter_map(|record| match record {
            Record::ScreenSpec(screen) => Some(screen.flags),
            _ => None,
        })
        .collect();
    assert_eq!(flags, [0x200, 0x300]);
}

#[test]
fn screen_and_action_generated_caps_are_inclusive() {
    let screen_name = "n".repeat(records_wire::SCREEN_SPEC_NAME_MAX_BYTES);
    let screen_title = "t".repeat(records_wire::SCREEN_SPEC_TITLE_MAX_BYTES);
    let action_name = "a".repeat(records_wire::ACTION_SPEC_NAME_MAX_BYTES);
    let action_title = "k".repeat(records_wire::ACTION_SPEC_TITLE_MAX_BYTES);
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    d.screen(ScreenDecl::new(&screen_name, &screen_title, 3))
        .unwrap();
    d.action(ActionDecl::new(&action_name, &action_title, 4))
        .unwrap();
    d.submit().unwrap();
    drop(d);
    let batch = records_decode(&batches[0]).unwrap();
    let records: Vec<_> = batch.iter().collect();
    match records.as_slice() {
        [Record::ScreenSpec(screen), Record::ActionSpec(action)] => {
            assert_eq!(
                (screen.name, screen.title),
                (screen_name.as_str(), screen_title.as_str())
            );
            assert_eq!(
                (action.name, action.title),
                (action_name.as_str(), action_title.as_str())
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn screen_and_action_field_refusals_abort_pending_construction() {
    for is_screen in [true, false] {
        let (name_cap, title_cap, name_field, title_field) = if is_screen {
            (
                records_wire::SCREEN_SPEC_NAME_MAX_BYTES,
                records_wire::SCREEN_SPEC_TITLE_MAX_BYTES,
                "screen name",
                "screen title",
            )
        } else {
            (
                records_wire::ACTION_SPEC_NAME_MAX_BYTES,
                records_wire::ACTION_SPEC_TITLE_MAX_BYTES,
                "action name",
                "action title",
            )
        };
        for (name, title, error) in [
            (
                String::new(),
                "title".to_string(),
                DeclareError::Empty(name_field),
            ),
            (
                "name".to_string(),
                String::new(),
                DeclareError::Empty(title_field),
            ),
            (
                "n".repeat(name_cap + 1),
                "title".to_string(),
                DeclareError::TooLong(name_field),
            ),
            (
                "name".to_string(),
                "t".repeat(title_cap + 1),
                DeclareError::TooLong(title_field),
            ),
        ] {
            let mut batches = Vec::new();
            let mut d = collecting(&mut batches);
            d.group(GroupDecl::new("pending", "group.title")).unwrap();
            let result = if is_screen {
                d.screen(ScreenDecl::new(&name, &title, 5))
            } else {
                d.action(ActionDecl::new(&name, &title, 6))
            };
            assert_eq!(result.map(|_| ()), Err(error));
            assert_eq!(d.len(), 1);
            assert_eq!(d.submit(), Err(error));
            assert_eq!(d.submit(), Err(error));
            drop(d);
            assert_eq!(batches, [Vec::<u8>::new()]);
        }
    }
}

#[test]
fn a_lookup_provider_declares_its_name_languages_network_and_token() {
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    d.lookup_provider(
        LookupProviderDecl::new("summary", "wikipedia.title", 7)
            .languages("en\nde\npt-BR")
            .online(),
    )
    .unwrap();
    d.lookup_provider(LookupProviderDecl::new("local", "dictionary.title", 8))
        .unwrap();
    d.submit().unwrap();
    drop(d);
    let rows: Vec<_> = records_decode(&batches[0]).unwrap().iter().collect();
    assert!(matches!(rows[0], Record::LookupSpec(s)
        if (s.name, s.title, s.languages, s.flags, s.token)
            == ("summary", "wikipedia.title", "en\nde\npt-BR", 1, 7)));
    assert!(matches!(rows[1], Record::LookupSpec(s)
        if (s.languages, s.flags, s.token) == ("", 0, 8)));
}

#[test]
fn a_lookup_provider_is_refused_for_what_the_host_would_refuse() {
    let refuse = |decl: LookupProviderDecl<'_>| {
        let mut batches = Vec::new();
        let mut d = collecting(&mut batches);
        let refused = d.lookup_provider(decl).is_err();
        if !refused {
            d.submit().unwrap();
        }
        refused
    };
    assert!(refuse(LookupProviderDecl::new("Summary", "t", 1)));
    assert!(refuse(LookupProviderDecl::new("a b", "t", 1)));
    assert!(refuse(LookupProviderDecl::new("a", "", 1)));
    assert!(refuse(LookupProviderDecl::new("a", "t", 1).languages("e")));
    assert!(refuse(
        LookupProviderDecl::new("a", "t", 1).languages("en\n\nde")
    ));
    assert!(refuse(
        LookupProviderDecl::new("a", "t", 1).languages("en_GB")
    ));
    let many = (0..33)
        .map(|i| alloc::format!("a{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(refuse(
        LookupProviderDecl::new("a", "t", 1).languages(&many)
    ));
    let fits = (0..32)
        .map(|i| alloc::format!("a{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!refuse(
        LookupProviderDecl::new("a", "t", 1).languages(&fits)
    ));
}
#[path = "tests/format.rs"]
mod format;
#[path = "tests/library.rs"]
mod library;
mod reader;

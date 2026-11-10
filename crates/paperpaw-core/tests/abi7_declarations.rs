//! Declaration records 5-10: independent golden bytes, malformed vectors,
//! caps, per-rule validation and a mutation sweep. The fixtures build wire
//! bytes by hand so they do not depend on the encoder under test.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use paperpaw_core::native_host::{
    HostOwner,
    declarations::{validate_batch, validate_record},
    registration_scope::RegistrationScope,
    registrations::{
        REGISTRATION_MAX_BYTES, REGISTRATION_MAX_RECORDS, Record, ResourceFontExtension,
        ResourceFontFace, ResourceHyphenation, SettingExtra, SettingGroup, SettingSpec,
        records_decode, records_encode,
    },
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_UNSUPPORTED, ERR_UTF8, OK};
use std::pin::Pin;

const PLUGIN: &str = "weather";

// ---- independent wire builder -------------------------------------------------

fn varint(mut value: usize) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

fn s(text: &str) -> Vec<u8> {
    b(text.as_bytes())
}

fn b(bytes: &[u8]) -> Vec<u8> {
    let mut out = varint(bytes.len());
    out.extend_from_slice(bytes);
    out
}

fn u(value: u32) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

fn i(value: i32) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

fn record(tag: usize, fields: &[Vec<u8>]) -> Vec<u8> {
    let mut body = vec![0];
    fields
        .iter()
        .for_each(|field| body.extend_from_slice(field));
    let mut out = varint(tag);
    out.extend(varint(body.len()));
    out.extend(body);
    out
}

fn batch(records: &[Vec<u8>]) -> Vec<u8> {
    let mut out = varint(records.len());
    records.iter().for_each(|r| out.extend_from_slice(r));
    out
}

fn default_bool(value: bool) -> Vec<u8> {
    [u(1), u(1), u(1), vec![u8::from(value)]].concat()
}

fn default_int(value: i64) -> Vec<u8> {
    [u(1), u(2), u(8), value.to_le_bytes().to_vec()].concat()
}

fn default_text(text: &str) -> Vec<u8> {
    [u(1), u(3), u(text.len() as u32), text.as_bytes().to_vec()].concat()
}

fn default_absent() -> Vec<u8> {
    [u(1), u(0), u(0)].concat()
}

// ---- valid records and their independent encodings ------------------------------

fn group() -> SettingGroup<'static> {
    SettingGroup {
        name: "page",
        title: "weather.group",
        order: -2,
        page: "main",
        parent: "core.display",
    }
}

fn group_fields() -> Vec<Vec<u8>> {
    vec![
        s("page"),
        s("weather.group"),
        i(-2),
        s("main"),
        s("core.display"),
    ]
}

fn spec() -> SettingSpec<'static> {
    SettingSpec {
        name: "units",
        kind: 2,
        flags: 0b10101,
        group: "weather.page",
        title: "weather.units",
        help: "weather.units.help",
        default: &[],
        min: 0,
        max: 0,
        choices: "metric\nimperial",
    }
}

fn spec_fields(default: &[u8]) -> Vec<Vec<u8>> {
    vec![
        s("units"),
        u(2),
        u(0b10101),
        s("weather.page"),
        s("weather.units"),
        s("weather.units.help"),
        b(default),
        i(0),
        i(0),
        s("metric\nimperial"),
    ]
}

fn extra() -> SettingExtra<'static> {
    SettingExtra {
        name: "units",
        unit: "weather.unit",
        section: "weather.section",
        follows: "core.other",
        shown_id: "weather.mode",
        shown_value: "full",
        editor_kind: 1,
        editor: "picker",
        step: 5,
        flags: 0b111,
        check: 0,
    }
}

fn extra_fields() -> Vec<Vec<u8>> {
    vec![
        s("units"),
        s("weather.unit"),
        s("weather.section"),
        s("core.other"),
        s("weather.mode"),
        s("full"),
        u(1),
        s("picker"),
        i(5),
        u(0b111),
        u(0),
    ]
}

fn hyphenation() -> ResourceHyphenation<'static> {
    ResourceHyphenation {
        language: "de-CH",
        path: "hyphenation/de.pphp",
    }
}

fn face() -> ResourceFontFace<'static> {
    ResourceFontFace {
        name: "serif-bold",
        path: "fonts/serif.ttf",
        family: "Serif",
        style: 3,
        weight: 700,
        role: "core.fonts.body",
        priority: -100,
    }
}

fn face_fields() -> Vec<Vec<u8>> {
    vec![
        s("serif-bold"),
        s("fonts/serif.ttf"),
        s("Serif"),
        u(3),
        u(700),
        s("core.fonts.body"),
        i(-100),
    ]
}

fn extension() -> ResourceFontExtension<'static> {
    ResourceFontExtension {
        name: "cjk",
        path: "fonts/cjk.ppfp",
        extends: "fonts-default.serif",
    }
}

fn encode(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = vec![0; REGISTRATION_MAX_BYTES];
    let length = records_encode(records, &mut out).unwrap();
    out.truncate(length);
    out
}

#[test]
fn every_new_record_matches_its_independent_golden_bytes() {
    let default = default_text("metric");
    let spec_record = SettingSpec {
        default: &default,
        ..spec()
    };
    let goldens: [(Record<'_>, Vec<u8>); 6] = [
        (Record::SettingGroup(group()), record(5, &group_fields())),
        (
            Record::SettingSpec(spec_record),
            record(6, &spec_fields(&default)),
        ),
        (Record::SettingExtra(extra()), record(7, &extra_fields())),
        (
            Record::ResourceHyphenation(hyphenation()),
            record(8, &[s("de-CH"), s("hyphenation/de.pphp")]),
        ),
        (Record::ResourceFontFace(face()), record(9, &face_fields())),
        (
            Record::ResourceFontExtension(extension()),
            record(
                10,
                &[s("cjk"), s("fonts/cjk.ppfp"), s("fonts-default.serif")],
            ),
        ),
    ];
    for (parsed, bytes) in goldens {
        let expected = batch(&[bytes]);
        assert_eq!(encode(&[parsed]), expected, "{parsed:?}");
        let decoded = records_decode(&expected).unwrap();
        assert_eq!(decoded.iter().collect::<Vec<_>>(), vec![parsed]);
    }
}

#[test]
fn a_mixed_batch_round_trips_in_order() {
    let default = default_text("metric");
    let spec_record = SettingSpec {
        default: &default,
        ..spec()
    };
    let records = [
        Record::SettingGroup(group()),
        Record::SettingSpec(spec_record),
        Record::SettingExtra(extra()),
        Record::ResourceHyphenation(hyphenation()),
        Record::ResourceFontFace(face()),
        Record::ResourceFontExtension(extension()),
    ];
    let bytes = encode(&records);
    let decoded = records_decode(&bytes).unwrap();
    assert_eq!(decoded.len(), 6);
    assert_eq!(decoded.iter().collect::<Vec<_>>(), records);
}

// ---- caps, framing, UTF-8 ----------------------------------------------------

/// (tag, valid fields, index of the capped field, cap, is_bytes)
fn capped_fields() -> Vec<(usize, Vec<Vec<u8>>, usize, usize)> {
    let default = default_text("metric");
    let mut out = Vec::new();
    for (tag, fields, caps) in [
        (5, group_fields(), vec![(0, 96), (1, 96), (3, 96), (4, 128)]),
        (
            6,
            spec_fields(&default),
            vec![(0, 96), (3, 128), (4, 96), (5, 96), (6, 512), (9, 1024)],
        ),
        (
            7,
            extra_fields(),
            vec![
                (0, 96),
                (1, 96),
                (2, 96),
                (3, 128),
                (4, 128),
                (5, 128),
                (7, 96),
            ],
        ),
        (8, vec![s("de"), s("a.pphp")], vec![(0, 35), (1, 100)]),
        (9, face_fields(), vec![(0, 96), (1, 100), (2, 64), (5, 96)]),
        (
            10,
            vec![s("cjk"), s("a.ppfp"), s("fonts-default.serif")],
            vec![(0, 96), (1, 100), (2, 128)],
        ),
    ] {
        for (index, cap) in caps {
            out.push((tag, fields.clone(), index, cap));
        }
    }
    out
}

#[test]
fn every_field_cap_accepts_n_and_refuses_n_plus_one() {
    for (tag, fields, index, cap) in capped_fields() {
        for (len, expected) in [(cap, Ok(())), (cap + 1, Err(ERR_NO_SPACE))] {
            let mut fields = fields.clone();
            fields[index] = b(&vec![b'a'; len]);
            let bytes = batch(&[record(tag, &fields)]);
            let got = records_decode(&bytes).map(|_| ());
            assert_eq!(got, expected, "tag {tag} field {index} len {len}");
        }
    }
}

#[test]
fn invalid_utf8_in_any_text_field_is_err_utf8_but_bytes_default_is_opaque() {
    for (tag, fields, index, _) in capped_fields() {
        let mut fields = fields;
        fields[index] = b(&[0xff, 0xfe]);
        let got = records_decode(&batch(&[record(tag, &fields)])).map(|_| ());
        let expected = if (tag, index) == (6, 6) {
            Ok(())
        } else {
            Err(ERR_UTF8)
        };
        assert_eq!(got, expected, "tag {tag} field {index}");
    }
}

#[test]
fn framing_count_tag_and_byte_limits_are_enforced() {
    let draw = record(2, &[u(1)]);
    let nine = batch(&vec![draw.clone(); REGISTRATION_MAX_RECORDS + 1]);
    assert_eq!(records_decode(&nine).map(|_| ()), Err(ERR_NO_SPACE));
    assert!(records_decode(&batch(&vec![draw.clone(); REGISTRATION_MAX_RECORDS])).is_ok());
    assert_eq!(
        records_decode(&batch(&[record(191, &[])])).map(|_| ()),
        Err(ERR_UNSUPPORTED)
    );
    assert_eq!(
        records_decode(&batch(&[record(0, &[])])).map(|_| ()),
        Err(ERR_UNSUPPORTED)
    );
    // A trailing field (accepted extension) pads a record to the exact limit.
    let pad = |total: usize| {
        let mut bytes = batch(&[record(2, &[u(1), vec![0; total]])]);
        // Fix the length: recompute with the real padding that reaches `total`.
        let overhead = bytes.len() - total;
        bytes = batch(&[record(2, &[u(1), vec![0; total - overhead]])]);
        bytes
    };
    let exact = pad(REGISTRATION_MAX_BYTES);
    assert_eq!(exact.len(), REGISTRATION_MAX_BYTES);
    assert!(records_decode(&exact).is_ok());
    let mut over = exact;
    over.push(0);
    assert_eq!(over.len(), REGISTRATION_MAX_BYTES + 1);
    assert_eq!(records_decode(&over).map(|_| ()), Err(ERR_NO_SPACE));
}

#[test]
fn every_truncation_and_trailing_byte_of_a_valid_batch_is_refused() {
    let default = default_text("metric");
    let bytes = batch(&[
        record(5, &group_fields()),
        record(6, &spec_fields(&default)),
        record(7, &extra_fields()),
        record(8, &[s("de"), s("a.pphp")]),
        record(9, &face_fields()),
    ]);
    assert!(records_decode(&bytes).is_ok());
    for cut in 0..bytes.len() {
        assert!(records_decode(&bytes[..cut]).is_err(), "cut at {cut}");
    }
    let mut longer = bytes;
    longer.push(0);
    assert!(records_decode(&longer).is_err());
}

#[test]
fn flipping_and_truncating_bytes_never_panics_and_validation_follows_decode() {
    let default = default_text("metric");
    let bytes = batch(&[
        record(5, &group_fields()),
        record(6, &spec_fields(&default)),
        record(7, &extra_fields()),
        record(8, &[s("de"), s("a.pphp")]),
        record(9, &face_fields()),
        record(10, &[s("cjk"), s("a.ppfp"), s("fonts-default.serif")]),
    ]);
    for at in 0..bytes.len() {
        for mask in [0x01u8, 0x80, 0xff] {
            let mut mutated = bytes.clone();
            mutated[at] ^= mask;
            if let Ok(decoded) = records_decode(&mutated) {
                let _ = validate_batch(PLUGIN, &decoded);
            }
        }
        let mut cut = bytes.clone();
        cut.truncate(at);
        if let Ok(decoded) = records_decode(&cut) {
            let _ = validate_batch(PLUGIN, &decoded);
        }
    }
}

// ---- validation rules ------------------------------------------------------------

fn check(record: Record<'_>) -> Result<(), i32> {
    validate_record(PLUGIN, &record)
}

/// A valid spec of each kind with a matching default.
fn valid_spec(kind: u32, default: &'static [u8]) -> SettingSpec<'static> {
    let base = SettingSpec {
        kind,
        default,
        flags: 0,
        choices: "",
        ..spec()
    };
    match kind {
        1 => SettingSpec {
            min: -5,
            max: 5,
            ..base
        },
        2 => SettingSpec {
            choices: "metric\nimperial",
            ..base
        },
        _ => base,
    }
}

fn leaked(bytes: Vec<u8>) -> &'static [u8] {
    Box::leak(bytes.into_boxed_slice())
}

#[test]
fn valid_records_of_every_kind_pass() {
    let bool_default = leaked(default_bool(true));
    let int_default = leaked(default_int(-5));
    let text = leaked(default_text("metric"));
    let language = leaked(default_text("de-CH"));
    let empty_language = leaked(default_text(""));
    for record in [
        Record::SettingGroup(group()),
        Record::SettingGroup(SettingGroup {
            page: "",
            parent: "",
            ..group()
        }),
        Record::SettingSpec(valid_spec(0, bool_default)),
        Record::SettingSpec(valid_spec(1, int_default)),
        Record::SettingSpec(valid_spec(2, text)),
        Record::SettingSpec(valid_spec(3, empty_language)),
        Record::SettingSpec(valid_spec(4, language)),
        Record::SettingSpec(valid_spec(5, text)),
        Record::SettingSpec(SettingSpec {
            flags: 0b11010,
            help: "",
            ..valid_spec(0, bool_default)
        }),
        Record::SettingExtra(extra()),
        Record::SettingExtra(SettingExtra {
            follows: "",
            shown_id: "",
            shown_value: "",
            editor_kind: 0,
            editor: "",
            step: 0,
            flags: 0,
            ..extra()
        }),
        Record::ResourceHyphenation(hyphenation()),
        Record::ResourceFontFace(face()),
        Record::ResourceFontFace(ResourceFontFace {
            path: "fonts/Pack.PPFP",
            weight: 0,
            style: 0,
            family: "",
            role: "",
            priority: 0,
            ..face()
        }),
        Record::ResourceFontExtension(extension()),
    ] {
        assert_eq!(check(record), Ok(()), "{record:?}");
    }
}

fn expect(cases: Vec<(&str, Record<'_>, i32)>) {
    for (what, record, status) in cases {
        assert_eq!(check(record), Err(status), "{what}");
    }
}

#[test]
fn name_and_full_id_syntax_is_enforced() {
    let long_plugin = "p".repeat(100);
    let g = |name| Record::SettingGroup(SettingGroup { name, ..group() });
    let mut cases = vec![
        ("empty", g(""), ERR_INVALID),
        ("empty segment", g("a..b"), ERR_INVALID),
        ("leading dot", g(".a"), ERR_INVALID),
        ("trailing dot", g("a."), ERR_INVALID),
        ("uppercase", g("Page"), ERR_INVALID),
        ("underscore", g("a_b"), ERR_INVALID),
        ("slash", g("a/b"), ERR_INVALID),
        ("space", g("a b"), ERR_INVALID),
    ];
    cases.push((
        "parent bad",
        Record::SettingGroup(SettingGroup {
            parent: "Core",
            ..group()
        }),
        ERR_INVALID,
    ));
    cases.push((
        "page bad",
        Record::SettingGroup(SettingGroup {
            page: "a..b",
            ..group()
        }),
        ERR_INVALID,
    ));
    expect(cases);
    assert_eq!(check(g("dotted.name-1")), Ok(()));
    // Full id at 128 bytes passes, 129 fails.
    let fits = "n".repeat(128 - long_plugin.len() - 1);
    let over = "n".repeat(128 - long_plugin.len());
    fn group_named(name: &str) -> Record<'_> {
        Record::SettingGroup(SettingGroup { name, ..group() })
    }
    assert_eq!(validate_record(&long_plugin, &group_named(&fits)), Ok(()));
    assert_eq!(
        validate_record(&long_plugin, &group_named(&over)),
        Err(ERR_INVALID)
    );
}

#[test]
fn group_rules() {
    expect(vec![(
        "empty title",
        Record::SettingGroup(SettingGroup {
            title: "",
            ..group()
        }),
        ERR_INVALID,
    )]);
}

#[test]
fn spec_kind_flags_and_title_group_rules() {
    let d = leaked(default_text("metric"));
    let t = |f: fn(SettingSpec<'static>) -> SettingSpec<'static>| {
        Record::SettingSpec(f(valid_spec(2, d)))
    };
    expect(vec![
        (
            "kind 6",
            t(|s| SettingSpec { kind: 6, ..s }),
            ERR_UNSUPPORTED,
        ),
        (
            "kind max",
            t(|s| SettingSpec {
                kind: u32::MAX,
                ..s
            }),
            ERR_UNSUPPORTED,
        ),
        (
            "flag bit 5",
            t(|s| SettingSpec {
                flags: 0b10_0000,
                ..s
            }),
            ERR_UNSUPPORTED,
        ),
        (
            "flag bit 31",
            t(|s| SettingSpec {
                flags: 1 << 31,
                ..s
            }),
            ERR_UNSUPPORTED,
        ),
        (
            "secret and scoped",
            t(|s| SettingSpec { flags: 0b1001, ..s }),
            ERR_INVALID,
        ),
        (
            "empty group",
            t(|s| SettingSpec { group: "", ..s }),
            ERR_INVALID,
        ),
        (
            "bad group",
            t(|s| SettingSpec { group: "A.b", ..s }),
            ERR_INVALID,
        ),
        (
            "empty title",
            t(|s| SettingSpec { title: "", ..s }),
            ERR_INVALID,
        ),
        (
            "bad name",
            t(|s| SettingSpec { name: "a b", ..s }),
            ERR_INVALID,
        ),
    ]);
    for flags in [0b01000, 0b00001, 0b10110, 0b11110] {
        assert_eq!(
            check(Record::SettingSpec(SettingSpec {
                flags,
                ..valid_spec(2, d)
            })),
            Ok(())
        );
    }
}

#[test]
fn spec_default_rules() {
    let absent = leaked(default_absent());
    let bool_d = leaked(default_bool(false));
    let int_d = leaked(default_int(0));
    let text_d = leaked(default_text("metric"));
    let other_text = leaked(default_text("kelvin"));
    let two_values = leaked([default_bool(true), default_bool(true)].concat());
    let trailing = leaked([default_bool(true), vec![0]].concat());
    let bad_kind = leaked([u(1), u(9), u(0)].concat());
    let bad_layout = leaked([u(2), u(1), u(1), vec![1]].concat());
    let bad_bool = leaked([u(1), u(1), u(1), vec![2]].concat());
    let bad_text = leaked([u(1), u(3), u(1), vec![0xff]].concat());
    let spec_of = |kind, default| Record::SettingSpec(valid_spec(kind, default));
    expect(vec![
        ("empty default", spec_of(0, &[]), ERR_INVALID),
        ("absent", spec_of(0, absent), ERR_INVALID),
        ("bool vs int", spec_of(1, bool_d), ERR_INVALID),
        ("int vs bool", spec_of(0, int_d), ERR_INVALID),
        ("text vs bool", spec_of(0, text_d), ERR_INVALID),
        ("int vs text", spec_of(2, int_d), ERR_INVALID),
        ("bool vs text", spec_of(3, bool_d), ERR_INVALID),
        ("two values", spec_of(0, two_values), ERR_INVALID),
        ("trailing byte", spec_of(0, trailing), ERR_INVALID),
        ("unknown value kind", spec_of(0, bad_kind), ERR_UNSUPPORTED),
        ("unknown layout", spec_of(0, bad_layout), ERR_UNSUPPORTED),
        ("bad bool payload", spec_of(0, bad_bool), ERR_INVALID),
        ("bad utf8 default", spec_of(3, bad_text), ERR_UTF8),
        (
            "choice default not listed",
            spec_of(2, other_text),
            ERR_INVALID,
        ),
        (
            "language charset",
            spec_of(4, leaked(default_text("de_CH"))),
            ERR_INVALID,
        ),
        (
            "language too long",
            spec_of(4, leaked(default_text(&"a".repeat(36)))),
            ERR_INVALID,
        ),
    ]);
    assert_eq!(
        check(spec_of(4, leaked(default_text(&"a".repeat(35))))),
        Ok(())
    );
}

#[test]
fn spec_int_range_rules() {
    let int = |n, min, max| {
        Record::SettingSpec(SettingSpec {
            min,
            max,
            ..valid_spec(1, leaked(default_int(n)))
        })
    };
    assert_eq!(check(int(-5, -5, 5)), Ok(()));
    assert_eq!(check(int(5, -5, 5)), Ok(()));
    assert_eq!(check(int(0, 3, 3)), Err(ERR_INVALID));
    assert_eq!(check(int(3, 3, 3)), Ok(()));
    expect(vec![
        ("below min", int(-6, -5, 5), ERR_INVALID),
        ("above max", int(6, -5, 5), ERR_INVALID),
        ("min over max", int(3, 5, 4), ERR_INVALID),
        (
            "beyond i32",
            int(i64::from(i32::MAX) + 1, 0, i32::MAX),
            ERR_INVALID,
        ),
    ]);
}

#[test]
fn spec_inapplicable_fields_must_be_zero_or_empty() {
    let bool_d = leaked(default_bool(true));
    let text_d = leaked(default_text("a"));
    let mk = |kind, default, f: fn(SettingSpec<'static>) -> SettingSpec<'static>| {
        Record::SettingSpec(f(valid_spec(kind, default)))
    };
    expect(vec![
        (
            "bool min",
            mk(0, bool_d, |s| SettingSpec { min: 1, ..s }),
            ERR_INVALID,
        ),
        (
            "bool max",
            mk(0, bool_d, |s| SettingSpec { max: 1, ..s }),
            ERR_INVALID,
        ),
        (
            "text min",
            mk(3, text_d, |s| SettingSpec { min: -1, ..s }),
            ERR_INVALID,
        ),
        (
            "choice max",
            mk(2, text_d, |s| SettingSpec {
                max: 1,
                choices: "a",
                ..s
            }),
            ERR_INVALID,
        ),
        (
            "bool choices",
            mk(0, bool_d, |s| SettingSpec { choices: "a", ..s }),
            ERR_INVALID,
        ),
        (
            "text choices",
            mk(3, text_d, |s| SettingSpec { choices: "a", ..s }),
            ERR_INVALID,
        ),
        (
            "int choices",
            Record::SettingSpec(SettingSpec {
                choices: "a",
                ..valid_spec(1, leaked(default_int(0)))
            }),
            ERR_INVALID,
        ),
    ]);
}

#[test]
fn spec_choice_list_rules() {
    let default = leaked(default_text("a"));
    let with = |choices: &'static str| {
        Record::SettingSpec(SettingSpec {
            choices,
            ..valid_spec(2, default)
        })
    };
    let many = |n: usize| -> &'static str {
        Box::leak(
            (0..n)
                .map(|k| {
                    if k == 0 {
                        "a".to_owned()
                    } else {
                        format!("v{k}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
                .into_boxed_str(),
        )
    };
    let wide =
        |n: usize| -> &'static str { Box::leak(format!("a\n{}", "x".repeat(n)).into_boxed_str()) };
    assert_eq!(check(with("a")), Ok(()));
    assert_eq!(check(with(many(64))), Ok(()));
    assert_eq!(check(with(wide(96))), Ok(()));
    expect(vec![
        ("empty list", with(""), ERR_INVALID),
        ("65 values", with(many(65)), ERR_INVALID),
        ("97 byte value", with(wide(97)), ERR_INVALID),
        ("empty value", with("a\n\nb"), ERR_INVALID),
        ("trailing newline", with("a\n"), ERR_INVALID),
        ("leading newline", with("\na"), ERR_INVALID),
        ("duplicate", with("a\nb\na"), ERR_INVALID),
        // 512 one-byte values in 1023 bytes: refused at the 65th, not compared all.
        (
            "512 tiny values",
            with(Box::leak(
                core::iter::repeat_n("a", 512)
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into_boxed_str(),
            )),
            ERR_INVALID,
        ),
    ]);
}

#[test]
fn extra_rules() {
    let e =
        |f: fn(SettingExtra<'static>) -> SettingExtra<'static>| Record::SettingExtra(f(extra()));
    expect(vec![
        (
            "check set",
            e(|x| SettingExtra { check: 1, ..x }),
            ERR_UNSUPPORTED,
        ),
        (
            "check max",
            e(|x| SettingExtra {
                check: u32::MAX,
                ..x
            }),
            ERR_UNSUPPORTED,
        ),
        (
            "flag bit 3",
            e(|x| SettingExtra { flags: 0b1000, ..x }),
            ERR_UNSUPPORTED,
        ),
        (
            "editor kind 3",
            e(|x| SettingExtra {
                editor_kind: 3,
                ..x
            }),
            ERR_UNSUPPORTED,
        ),
        (
            "negative step",
            e(|x| SettingExtra { step: -1, ..x }),
            ERR_INVALID,
        ),
        (
            "shown id without value",
            e(|x| SettingExtra {
                shown_value: "",
                ..x
            }),
            ERR_INVALID,
        ),
        (
            "shown value without id",
            e(|x| SettingExtra { shown_id: "", ..x }),
            ERR_INVALID,
        ),
        (
            "bad follows",
            e(|x| SettingExtra { follows: "A", ..x }),
            ERR_INVALID,
        ),
        (
            "bad shown id",
            e(|x| SettingExtra {
                shown_id: "a..b",
                ..x
            }),
            ERR_INVALID,
        ),
        (
            "bad name",
            e(|x| SettingExtra { name: "", ..x }),
            ERR_INVALID,
        ),
    ]);
    assert_eq!(
        check(e(|x| SettingExtra {
            step: i32::MAX,
            ..x
        })),
        Ok(())
    );
}

#[test]
fn extra_editor_rules() {
    let e =
        |f: fn(SettingExtra<'static>) -> SettingExtra<'static>| Record::SettingExtra(f(extra()));
    expect(vec![
        (
            "editor kind 0 with editor",
            e(|x| SettingExtra {
                editor_kind: 0,
                ..x
            }),
            ERR_INVALID,
        ),
        (
            "editor kind 2 without editor",
            e(|x| SettingExtra { editor: "", ..x }),
            ERR_INVALID,
        ),
        (
            "screen editor not a relative name",
            e(|x| SettingExtra {
                editor_kind: 1,
                editor: "Picker",
                ..x
            }),
            ERR_INVALID,
        ),
        (
            "slot editor not a full id",
            e(|x| SettingExtra {
                editor_kind: 2,
                editor: "core..slot",
                ..x
            }),
            ERR_INVALID,
        ),
    ]);
    for kind in [0, 1, 2] {
        let editor = if kind == 0 { "" } else { "x" };
        assert_eq!(
            check(Record::SettingExtra(SettingExtra {
                editor_kind: kind,
                editor,
                ..extra()
            })),
            Ok(())
        );
    }
}

#[test]
fn path_reference_rule() {
    let p = |path: &'static str| {
        Record::ResourceHyphenation(ResourceHyphenation {
            path,
            ..hyphenation()
        })
    };
    let long = Box::leak(format!("{}.pphp", "a".repeat(95)).into_boxed_str());
    let too_long = Box::leak(format!("{}.pphp", "a".repeat(96)).into_boxed_str());
    for good in ["a", "a/b/c.pphp", "A_b-1.x", "a.b.c", long] {
        assert_eq!(check(p(good)), Ok(()), "{good}");
    }
    assert_eq!(long.len(), 100);
    for bad in [
        "",
        "/a",
        "a/",
        "a//b",
        ".",
        "..",
        "a/./b",
        "a/../b",
        ".hidden",
        "a/.hidden",
        "a/b/c/d",
        "a b",
        "a\\b",
        "ä",
        "a:b",
        too_long,
    ] {
        assert_eq!(check(p(bad)), Err(ERR_INVALID), "{bad:?}");
    }
    let f = |path: &'static str| {
        Record::ResourceFontExtension(ResourceFontExtension {
            path,
            ..extension()
        })
    };
    assert_eq!(check(f("..")), Err(ERR_INVALID));
    assert_eq!(check(f("a/b.ppfp")), Ok(()));
    let g = |path: &'static str| Record::ResourceFontFace(ResourceFontFace { path, ..face() });
    assert_eq!(check(g("/etc")), Err(ERR_INVALID));
}

#[test]
fn hyphenation_language_rule() {
    let l = |language: &'static str| {
        Record::ResourceHyphenation(ResourceHyphenation {
            language,
            ..hyphenation()
        })
    };
    let thirty_five = "a".repeat(35).leak();
    let thirty_six = "a".repeat(36).leak();
    for good in ["de", "de-CH", "zh-Hant-TW", "X", thirty_five] {
        assert_eq!(check(l(good)), Ok(()), "{good}");
    }
    for bad in ["", "de_CH", "de ch", "dé", "de.CH", thirty_six] {
        assert_eq!(check(l(bad)), Err(ERR_INVALID), "{bad:?}");
    }
}

#[test]
fn font_face_rules() {
    let f = |f: fn(ResourceFontFace<'static>) -> ResourceFontFace<'static>| {
        Record::ResourceFontFace(f(face()))
    };
    expect(vec![
        (
            "style bit 2",
            f(|x| ResourceFontFace { style: 4, ..x }),
            ERR_UNSUPPORTED,
        ),
        (
            "style max",
            f(|x| ResourceFontFace {
                style: u32::MAX,
                ..x
            }),
            ERR_UNSUPPORTED,
        ),
        (
            "weight 1001",
            f(|x| ResourceFontFace { weight: 1001, ..x }),
            ERR_INVALID,
        ),
        (
            "priority 101",
            f(|x| ResourceFontFace { priority: 101, ..x }),
            ERR_INVALID,
        ),
        (
            "priority -101",
            f(|x| ResourceFontFace {
                priority: -101,
                ..x
            }),
            ERR_INVALID,
        ),
        (
            "bad role",
            f(|x| ResourceFontFace { role: "Core", ..x }),
            ERR_INVALID,
        ),
        (
            "priority without role",
            f(|x| ResourceFontFace {
                role: "",
                priority: 5,
                ..x
            }),
            ERR_INVALID,
        ),
        (
            "bad name",
            f(|x| ResourceFontFace { name: "A", ..x }),
            ERR_INVALID,
        ),
    ]);
    for weight in [0, 1, 400, 1000] {
        assert_eq!(check(f(|x| x)), Ok(()));
        assert_eq!(
            check(Record::ResourceFontFace(ResourceFontFace {
                weight,
                ..face()
            })),
            Ok(())
        );
    }
    for style in 0..4 {
        assert_eq!(
            check(Record::ResourceFontFace(ResourceFontFace {
                style,
                ..face()
            })),
            Ok(())
        );
    }
}

#[test]
fn font_face_path_rules() {
    let f = |f: fn(ResourceFontFace<'static>) -> ResourceFontFace<'static>| {
        Record::ResourceFontFace(f(face()))
    };
    expect(vec![
        (
            "weight on ppfp",
            f(|x| ResourceFontFace {
                path: "f/a.ppfp",
                weight: 400,
                ..x
            }),
            ERR_INVALID,
        ),
        (
            "weight on upper-case ppfp",
            f(|x| ResourceFontFace {
                path: "f/a.PpFp",
                weight: 400,
                ..x
            }),
            ERR_INVALID,
        ),
        (
            "unknown font extension",
            f(|x| ResourceFontFace {
                path: "fonts/serif.woff",
                ..x
            }),
            ERR_INVALID,
        ),
        (
            "font extension only",
            f(|x| ResourceFontFace { path: ".ttf", ..x }),
            ERR_INVALID,
        ),
    ]);
}

#[test]
fn font_extension_rules() {
    let e = |f: fn(ResourceFontExtension<'static>) -> ResourceFontExtension<'static>| {
        Record::ResourceFontExtension(f(extension()))
    };
    expect(vec![
        (
            "empty extends",
            e(|x| ResourceFontExtension { extends: "", ..x }),
            ERR_INVALID,
        ),
        (
            "bad extends",
            e(|x| ResourceFontExtension {
                extends: "Fonts",
                ..x
            }),
            ERR_INVALID,
        ),
        (
            "bad name",
            e(|x| ResourceFontExtension { name: "x y", ..x }),
            ERR_INVALID,
        ),
        (
            "not a ppfp",
            e(|x| ResourceFontExtension {
                path: "fonts/cjk.ttf",
                ..x
            }),
            ERR_INVALID,
        ),
        (
            "ppfp extension only",
            e(|x| ResourceFontExtension { path: ".ppfp", ..x }),
            ERR_INVALID,
        ),
    ]);
    let upper = e(|x| ResourceFontExtension {
        path: "fonts/CJK.PPFP",
        ..x
    });
    assert_eq!(check(upper), Ok(()));
}

// ---- the hook: whole-batch validation and poisoning ----------------------------------

type Context = RegistrationScope<paperpaw_core::native_host::registrations::Registration>;

fn run(
    bytes: &[u8],
) -> (
    Result<(), i32>,
    Result<(), paperpaw_core::native_host::Error>,
) {
    let owner: Pin<Box<HostOwner<Context, ()>>> =
        Box::pin(HostOwner::new(RegistrationScope::new(), ()));
    // SAFETY: projecting the pinned context without moving it.
    let slot: Pin<&Context> = owner.as_ref().context();
    let mut construction = owner.as_ref().begin(PLUGIN).unwrap();
    let status = slot
        .scope(&mut construction, || {
            // SAFETY: the slice stays readable and unmodified for the call.
            unsafe {
                paperpaw_core::native_host::registrations::submit(
                    &slot,
                    bytes.as_ptr(),
                    bytes.len(),
                )
            }
        })
        .unwrap();
    let committed = construction
        .commit(paperpaw_core::native_host::Teardown {
            report: |_, _, _| {},
            cancel: |_| Ok(()),
            release: |_| Ok(()),
            destroy: |_| (),
        })
        .map(|_| ());
    (if status == OK { Ok(()) } else { Err(status) }, committed)
}

#[test]
fn a_valid_wire_declaration_passes_validation_but_keeps_the_activation_gate() {
    let default = default_text("metric");
    let bytes = batch(&[
        record(5, &group_fields()),
        record(6, &spec_fields(&default)),
        record(2, &[u(7)]),
    ]);
    assert_eq!(
        run(&bytes),
        (
            Ok(()),
            Err(paperpaw_core::native_host::Error::DeclarationViewsUnavailable)
        )
    );
}

#[test]
fn one_bad_record_refuses_the_whole_batch_and_poisons_construction() {
    let default = default_text("metric");
    let bad_group = [s("Bad"), s("t"), i(0), s(""), s("")];
    let cases: Vec<(Vec<u8>, i32)> = vec![
        (
            batch(&[record(5, &group_fields()), record(5, &bad_group)]),
            ERR_INVALID,
        ),
        (
            batch(&[
                record(6, &spec_fields(&default)),
                record(8, &[s(""), s("a")]),
            ]),
            ERR_INVALID,
        ),
        (
            batch(&[record(
                9,
                &[s("n"), s("a.ttf"), s(""), u(4), u(0), s(""), i(0)],
            )]),
            ERR_UNSUPPORTED,
        ),
        (batch(&[record(191, &[])]), ERR_UNSUPPORTED),
        (vec![], ERR_INVALID),
        (vec![1], ERR_INVALID),
        (vec![0; REGISTRATION_MAX_BYTES + 1], ERR_NO_SPACE),
    ];
    for (bytes, status) in cases {
        let (result, committed) = run(&bytes);
        assert_eq!(result, Err(status));
        assert!(committed.is_err(), "refusal must poison construction");
    }
}

#[test]
fn validate_batch_returns_the_first_failure() {
    let default = default_text("metric");
    let bytes = batch(&[
        record(5, &[s("Bad"), s("t"), i(0), s(""), s("")]),
        record(8, &[s(""), s("a")]),
        record(6, &spec_fields(&default)),
    ]);
    let decoded = records_decode(&bytes).unwrap();
    assert_eq!(validate_batch(PLUGIN, &decoded), Err(ERR_INVALID));
}

#[test]
fn event_only_wire_batch_remains_committable() {
    assert_eq!(run(&batch(&[record(2, &[u(7)])])), (Ok(()), Ok(())));
}

/// Input/help declarations are admitted; an empty one (no name, no screen) is
/// invalid and poisons construction, as the widget and status ones.
#[test]
fn empty_input_and_help_declarations_are_invalid() {
    for tag in [29, 30] {
        // Sixty-four zero bytes are a valid body of every generated record.
        let bytes = batch(&[record(tag, &[vec![0; 64]])]);
        let decoded = records_decode(&bytes).unwrap();
        let first = decoded.iter().next().unwrap();
        assert_eq!(
            validate_record(PLUGIN, &first),
            Err(ERR_INVALID),
            "tag {tag}"
        );
        let (result, committed) = run(&bytes);
        assert_eq!(result, Err(ERR_INVALID), "tag {tag}");
        assert!(
            committed.is_err(),
            "tag {tag}: refusal must poison construction"
        );
    }
}

#[test]
fn malformed_widget_and_status_declarations_poison_construction() {
    for tag in [224, 225, 226, 227, 228] {
        let bytes = batch(&[record(tag, &[vec![0; 64]])]);
        let decoded = records_decode(&bytes).unwrap();
        assert_eq!(
            validate_record(PLUGIN, &decoded.iter().next().unwrap()),
            Err(ERR_INVALID)
        );
        let (result, committed) = run(&bytes);
        assert_eq!(result, Err(ERR_INVALID), "tag {tag}");
        assert!(
            committed.is_err(),
            "invalid declarations are never dropped silently"
        );
    }
}

/// DOC and LIB declarations (`docs/abi/document.md`, `library.md`: lookup
/// providers, book badges and library sources) decode but stay refused until
/// the host admits them, as the UIX ones. Formats and readers are admitted
/// (`format_and_reader_declarations_are_admitted`).
#[test]
fn doc_and_lib_declarations_stay_refused_until_admitted() {
    use paperpaw_core::native_host::registrations::{
        BookBadgeSpec, BookDataSpec, BookFieldSpec, LookupSpec,
    };
    // Well-formed ones: checked, then refused, never accepted and dropped.
    let valid = [
        Record::LookupSpec(LookupSpec {
            name: "dict",
            title: "lookup.dict",
            languages: "en",
            flags: 0,
            token: 1,
        }),
        Record::BookFieldSpec(BookFieldSpec {
            name: "rating",
            title: "field.rating",
            flags: 0,
        }),
        Record::BookBadgeSpec(BookBadgeSpec {
            name: "new",
            token: 1,
            flags: 0,
        }),
        Record::BookDataSpec(BookDataSpec {
            name: "notes",
            kind: 0,
        }),
    ];
    for record in valid {
        let bytes = encode(&[record]);
        let decoded = records_decode(&bytes).unwrap();
        let first = decoded.iter().next().unwrap();
        assert_eq!(
            validate_record(PLUGIN, &first),
            Err(ERR_UNSUPPORTED),
            "{first:?}"
        );
        let (result, committed) = run(&bytes);
        assert_eq!(result, Err(ERR_UNSUPPORTED), "{first:?}");
        assert!(
            committed.is_err(),
            "{first:?}: refusal must poison construction"
        );
    }
    // Zero bodies decode but name nothing: malformed, not merely unsupported.
    for tag in [31, 229, 230, 231, 232, 233] {
        let bytes = batch(&[record(tag, &[vec![0; 128]])]);
        let decoded = records_decode(&bytes).unwrap();
        let first = decoded.iter().next().unwrap();
        assert_eq!(
            validate_record(PLUGIN, &first),
            Err(ERR_INVALID),
            "tag {tag}"
        );
        assert_eq!(run(&bytes).0, Err(ERR_INVALID), "tag {tag}");
    }
}

/// A format and a reader are checked and admitted (`document.md` section 1);
/// the cross-record rules (extensions, screens, limits) are the commit's.
#[test]
fn format_and_reader_declarations_are_admitted() {
    use paperpaw_core::native_host::registrations::{FormatSpec, ReaderSpec};
    let admitted = [
        Record::FormatSpec(FormatSpec {
            name: "epub",
            title: "format.epub",
            extensions: "epub",
            flags: 0,
            priority: 0,
            token: 1,
        }),
        Record::ReaderSpec(ReaderSpec {
            name: "main",
            screen: "weather.main",
            formats: "",
            flags: 0,
        }),
    ];
    for record in admitted {
        let bytes = encode(&[record]);
        let decoded = records_decode(&bytes).unwrap();
        let first = decoded.iter().next().unwrap();
        assert_eq!(validate_record(PLUGIN, &first), Ok(()), "{first:?}");
        assert_eq!(run(&bytes).0, Ok(()), "{first:?}");
    }
    // A bad extension and an unknown flag bit are invalid, not unsupported.
    let bad = Record::FormatSpec(FormatSpec {
        name: "epub",
        title: "format.epub",
        extensions: "EPUB",
        flags: 0,
        priority: 0,
        token: 1,
    });
    let bytes = encode(&[bad]);
    assert_eq!(run(&bytes).0, Err(ERR_INVALID));
}

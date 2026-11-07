#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]

use super::*;
use alloc::{format, string::String, vec, vec::Vec};
use core::{pin::pin, ptr};
use paperpaw_i18n::Catalog;
use paperpaw_native::host_glue::{ERR_STATE, ERR_UTF8};
use paperpaw_platform::Instant;

const EN: &str = "[meta]\nlanguage = en\nplural = one-other\n[messages]\nhost.message = {{Hi}} {name}: {n}; {missing} } {\nhost.books.one = {count} book for {name}\nhost.books.other = {count} books for {name}\nhost.fallback.one = English one\nhost.fallback.other = English other\nhost.empty = \n";
const FA: &str = "[meta]\nlanguage = fa\nplural = zero-one-other\ndigits = arabext\n[messages]\nhost.books.one = {count} کتاب {name}\nhost.books.other = {count} کتاب‌ها {name}\n";

fn bytes(value: &[u8]) -> PpBytes {
    PpBytes {
        ptr: value.as_ptr(),
        len: value.len(),
    }
}

fn output(value: &mut [u8]) -> PpBuffer {
    PpBuffer {
        ptr: value.as_mut_ptr(),
        cap: value.len(),
    }
}

fn encode(records: &[wire::Record<'_>]) -> Vec<u8> {
    let mut buffer = vec![0; buffers::TEXT_MAX];
    let len = wire::records_encode(records, &mut buffer).unwrap();
    buffer.truncate(len);
    buffer
}

fn named<'a>(name: &'a str, value: &'a str) -> wire::Record<'a> {
    wire::Record::I18nArgStr(wire::I18nArgStr { name, value })
}

fn number(name: &str, value: i64) -> wire::Record<'_> {
    wire::Record::I18nArgInt(wire::I18nArgInt {
        name,
        value: value as u64,
    })
}

fn count(value: i64) -> wire::Record<'static> {
    wire::Record::I18nCount(wire::I18nCount {
        value: value as u64,
    })
}

fn with_catalog(language: &str, extra: Option<String>, run: impl FnOnce(&EnvironmentScope)) {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    kernel.shared.translator.add(Catalog::parse(EN).unwrap());
    kernel.shared.translator.add(Catalog::parse(FA).unwrap());
    if let Some(extra) = extra {
        kernel.shared.translator.add(Catalog::parse(extra).unwrap());
    }
    kernel.shared.translator.set_language(language);
    let env = kernel.shared.env(Instant(0));
    let scope = pin!(EnvironmentScope::new());
    scope.as_ref().scope(&env, || run(&scope)).unwrap();
}

// SAFETY: these wrappers lend stable, initialized inputs and disjoint,
// exclusively borrowed outputs for the duration of each synchronous call.
fn formatted(scope: &EnvironmentScope, key: &str, args: &[u8], out: &mut [u8]) -> i32 {
    unsafe { format(scope, bytes(key.as_bytes()), bytes(args), output(out)) }
}

fn copied(scope: &EnvironmentScope, key: &str, out: &mut [u8]) -> i32 {
    // SAFETY: the borrowed input and exclusive output are valid and disjoint.
    unsafe { copy(scope, bytes(key.as_bytes()), output(out)) }
}

fn localized(scope: &EnvironmentScope, text: &str, out: &mut [u8]) -> i32 {
    // SAFETY: the borrowed input and exclusive output are valid and disjoint.
    unsafe { digits(scope, bytes(text.as_bytes()), output(out)) }
}

#[test]
fn substitutions_match_translator_and_return_full_length_for_every_output_prefix() {
    with_catalog("fa", None, |scope| {
        let args = encode(&[named("name", "Ada12"), number("n", i64::MIN)]);
        let expected = "{Hi} Ada12: -۹۲۲۳۳۷۲۰۳۶۸۵۴۷۷۵۸۰۸; {missing} } {";
        for capacity in 0..=expected.len() + 1 {
            let mut out = vec![0xa5; capacity + 1];
            let result = formatted(scope, "host.message", &args, &mut out[..capacity]);
            assert_eq!(result, expected.len() as i32);
            let prefix = capacity.min(expected.len());
            assert_eq!(&out[..prefix], &expected.as_bytes()[..prefix]);
            assert!(out[prefix..].iter().all(|&byte| byte == 0xa5));
        }
        for value in [i64::MIN, -1, 0, 1, i64::MAX] {
            let args = encode(&[named("name", "Ada12"), number("n", value)]);
            let mut out = [0; 256];
            let len = formatted(scope, "host.message", &args, &mut out) as usize;
            scope
                .with_frame(|frame| {
                    let expected = frame.translator().format(
                        "host.message",
                        &[("name", Arg::Str("Ada12")), ("n", Arg::Int(value))],
                    );
                    assert_eq!(&out[..len], expected.as_bytes());
                })
                .unwrap();
        }
    });
}

#[test]
fn plurals_use_the_current_catalog_rule_and_the_fallback_catalog_rule() {
    with_catalog("fa", None, |scope| {
        for key in ["host.books", "host.fallback"] {
            for value in [i64::MIN, -2, -1, 0, 1, 2, 12, i64::MAX] {
                let args = encode(&[count(value), named("name", "Ada12")]);
                let mut out = [0; 256];
                let len = formatted(scope, key, &args, &mut out) as usize;
                scope
                    .with_frame(|frame| {
                        let expected =
                            frame
                                .translator()
                                .plural(key, value, &[("name", Arg::Str("Ada12"))]);
                        assert_eq!(&out[..len], expected.as_bytes());
                    })
                    .unwrap();
            }
        }
        let args = encode(&[count(0), named("name", "Ada")]);
        let mut out = [0; 128];
        let len = formatted(scope, "host.books", &args, &mut out) as usize;
        assert_eq!(&out[..len], "۰ کتاب Ada".as_bytes());
    });
}

#[test]
fn missing_keys_preserve_output_and_empty_translations_succeed() {
    with_catalog("en", None, |scope| {
        let mut out = [0xa5; 16];
        assert_eq!(copied(scope, "host.missing", &mut out), ERR_NOT_FOUND);
        assert_eq!(
            formatted(scope, "host.missing", &[0], &mut out),
            ERR_NOT_FOUND
        );
        assert_eq!(
            formatted(scope, "host.message", &encode(&[count(1)]), &mut out),
            ERR_NOT_FOUND
        );
        assert_eq!(out, [0xa5; 16]);
        assert_eq!(copied(scope, "host.empty", &mut out), 0);
        assert_eq!(formatted(scope, "host.empty", &[0], &mut out), 0);
        assert_eq!(out, [0xa5; 16]);
        assert_eq!(
            copied(scope, "host.message", &mut out[..3]),
            EN_MESSAGE.len() as i32
        );
        assert_eq!(&out[..3], b"{{H");
        assert_eq!(out[3..], [0xa5; 13]);
    });
}

const EN_MESSAGE: &str = "{{Hi}} {name}: {n}; {missing} } {";

#[test]
fn digit_conversion_preserves_non_ascii_digits_and_can_split_utf8_prefixes() {
    with_catalog("fa", None, |scope| {
        let expected = "p. ۱۲ / ۳۰۰ ٣";
        for capacity in 0..=expected.len() + 1 {
            let mut out = vec![0xa5; capacity + 1];
            let len = localized(scope, "p. 12 / 300 ٣", &mut out[..capacity]);
            assert_eq!(len, expected.len() as i32);
            let prefix = capacity.min(expected.len());
            assert_eq!(&out[..prefix], &expected.as_bytes()[..prefix]);
            assert!(out[prefix..].iter().all(|&byte| byte == 0xa5));
        }
        assert_eq!(localized(scope, "", &mut []), 0);
    });
    with_catalog("en", None, |scope| {
        let mut out = [0; 16];
        assert_eq!(localized(scope, "p. 12 ٣", &mut out), 8);
        assert_eq!(&out[..8], "p. 12 ٣".as_bytes());
    });
}

#[test]
fn duplicate_names_counts_and_count_name_collisions_are_invalid_in_both_orders() {
    with_catalog("en", None, |scope| {
        for records in [
            vec![named("n", "x"), number("n", 1)],
            vec![number("n", 1), named("n", "x")],
            vec![count(1), count(2)],
            vec![count(1), named("count", "x")],
            vec![number("count", 1), count(2)],
            vec![named("", "x"), named("", "y")],
        ] {
            let mut out = [0xa5; 16];
            assert_eq!(
                formatted(scope, "missing", &encode(&records), &mut out),
                ERR_INVALID
            );
            assert_eq!(out, [0xa5; 16]);
        }
    });
}

#[test]
fn framing_utf8_and_foreign_records_are_validated_before_lookup_or_output() {
    with_catalog("en", None, |scope| {
        let valid = encode(&[named("name", "Ada"), number("n", 5)]);
        let mut malformed = Vec::new();
        for end in 0..valid.len() {
            malformed.push((valid[..end].to_vec(), ERR_INVALID));
        }
        let mut trailing = valid.clone();
        trailing.push(0);
        malformed.push((trailing, ERR_INVALID));
        malformed.push((vec![0x80, 0], ERR_INVALID));
        malformed.push((vec![1, 127, 0], ERR_UNSUPPORTED));
        malformed.push((
            vec![1, wire::I18N_ARG_STR_TAG as u8, 4, 0, 1, 0xff, 0],
            ERR_UTF8,
        ));
        for (args, status) in malformed {
            let mut out = [0xa5; 16];
            assert_eq!(
                formatted(scope, "missing", &args, &mut out),
                status,
                "{args:?}"
            );
            assert_eq!(out, [0xa5; 16]);
        }
        let foreign = encode(&[wire::Record::EventStarted(wire::EventStarted { now_ms: 0 })]);
        assert_eq!(
            formatted(scope, "missing", &foreign, &mut []),
            ERR_UNSUPPORTED
        );
        // Any record that is not an argument is refused before its body is
        // read (#985): a declaration, or one with bad UTF-8 in its text.
        let group = encode(&[wire::Record::SettingGroup(wire::SettingGroup {
            name: "g",
            title: "t",
            order: 0,
            page: "",
            parent: "",
        })]);
        assert_eq!(
            formatted(scope, "missing", &group, &mut []),
            ERR_UNSUPPORTED
        );
        let mut bad = group.clone();
        let at = bad.iter().position(|&byte| byte == b'g').unwrap();
        bad[at] = 0xff;
        assert_eq!(formatted(scope, "missing", &bad, &mut []), ERR_UNSUPPORTED);
        assert_eq!(formatted(scope, "missing", &[9], &mut []), ERR_NO_SPACE);
    });
}

#[test]
fn two_pass_output_initializes_only_the_fitting_prefix() {
    with_catalog("fa", None, |scope| {
        let mut out = [MaybeUninit::<u8>::uninit(); 3];
        // SAFETY: output is writable and disjoint from the static input.
        let length = unsafe {
            digits(
                scope,
                bytes(b"12"),
                PpBuffer {
                    ptr: out.as_mut_ptr().cast(),
                    cap: out.len(),
                },
            )
        };
        let expected = "۱۲".as_bytes();
        assert_eq!(length, expected.len() as i32);
        for (actual, expected) in out.iter().zip(expected) {
            // SAFETY: success writes the complete fitting prefix, all 3 bytes.
            assert_eq!(unsafe { actual.assume_init() }, *expected);
        }
    });
    with_catalog("en", None, |scope| {
        let mut out = [MaybeUninit::<u8>::uninit(); 3];
        // SAFETY: disjoint valid key, empty argument batch and writable output.
        let length = unsafe {
            format(
                scope,
                bytes(b"host.message"),
                bytes(&[0]),
                PpBuffer {
                    ptr: out.as_mut_ptr().cast(),
                    cap: out.len(),
                },
            )
        };
        assert!(length > out.len() as i32);
        for (actual, expected) in out.iter().zip(b"{Hi") {
            // SAFETY: success wrote the entire fitting prefix.
            assert_eq!(unsafe { actual.assume_init() }, *expected);
        }
    });
}

#[test]
fn complete_output_is_bounded_before_any_prefix_is_written() {
    let exact = "x".repeat(buffers::TEXT_MAX);
    let too_long = "x".repeat(buffers::TEXT_MAX + 1);
    let repeated = "{name}".repeat(17);
    let catalog = format!(
        "[meta]\nlanguage = en\n[messages]\nhost.exact = {exact}\nhost.long = {too_long}\nhost.repeat = {repeated}\n"
    );
    with_catalog("en", Some(catalog), |scope| {
        let mut out = [0xa5; 2];
        assert_eq!(
            formatted(scope, "host.exact", &[0], &mut out),
            buffers::TEXT_MAX as i32
        );
        assert_eq!(out, *b"xx");
        out.fill(0xa5);
        assert_eq!(formatted(scope, "host.long", &[0], &mut out), ERR_NO_SPACE);
        assert_eq!(copied(scope, "host.long", &mut out), ERR_NO_SPACE);
        let argument = "a".repeat(256);
        assert_eq!(
            formatted(
                scope,
                "host.repeat",
                &encode(&[named("name", &argument)]),
                &mut out
            ),
            ERR_NO_SPACE
        );
        assert_eq!(out, [0xa5; 2]);
    });
    with_catalog("fa", None, |scope| {
        let mut out = [0xa5; 2];
        assert_eq!(
            localized(scope, &"1".repeat(buffers::TEXT_MAX), &mut out),
            ERR_NO_SPACE
        );
        assert_eq!(out, [0xa5; 2]);
        assert_eq!(
            localized(scope, &"1".repeat(buffers::TEXT_MAX / 2), &mut out),
            buffers::TEXT_MAX as i32
        );
        assert_eq!(out, *"۱".as_bytes());
    });
}

#[test]
fn all_raw_pairs_are_checked_even_without_a_dispatch_frame() {
    let scope = EnvironmentScope::new();
    let mut out = [0xa5; 8];
    assert_eq!(copied(&scope, "host.message", &mut out), ERR_STATE);
    assert_eq!(formatted(&scope, "host.message", &[0], &mut out), ERR_STATE);
    assert_eq!(localized(&scope, "123", &mut out), ERR_STATE);
    assert_eq!(
        formatted(&scope, "host.message", &[], &mut out),
        ERR_INVALID
    );
    // SAFETY: null and oversize pairs are refused before dereference. Other
    // inputs and the exclusive output point to valid, disjoint local storage.
    unsafe {
        let null = PpBytes {
            ptr: ptr::null(),
            len: 1,
        };
        assert_eq!(copy(&scope, null, output(&mut out)), ERR_INVALID);
        assert_eq!(digits(&scope, null, output(&mut out)), ERR_INVALID);
        assert_eq!(
            format(&scope, bytes(b"key"), null, output(&mut out)),
            ERR_INVALID
        );
        assert_eq!(copy(&scope, bytes(&[0xff]), output(&mut out)), ERR_UTF8);
        assert_eq!(digits(&scope, bytes(&[0xff]), output(&mut out)), ERR_UTF8);
        assert_eq!(
            format(&scope, bytes(&[0xff]), bytes(&[0]), output(&mut out)),
            ERR_UTF8
        );
        let bad_out = PpBuffer {
            ptr: ptr::null_mut(),
            cap: 1,
        };
        assert_eq!(copy(&scope, bytes(b"key"), bad_out), ERR_INVALID);
        assert_eq!(
            format(&scope, bytes(b"key"), bytes(&[0]), bad_out),
            ERR_INVALID
        );
        assert_eq!(digits(&scope, bytes(b"1"), bad_out), ERR_INVALID);
        let empty = PpBytes {
            ptr: ptr::null(),
            len: 0,
        };
        let empty_out = PpBuffer {
            ptr: ptr::null_mut(),
            cap: 0,
        };
        assert_eq!(copy(&scope, empty, empty_out), ERR_STATE);
        assert_eq!(digits(&scope, empty, empty_out), ERR_STATE);
    }
    assert_eq!(out, [0xa5; 8]);
}

#[test]
fn generated_input_limits_are_independent_and_enforced() {
    let scope = EnvironmentScope::new();
    assert_eq!(
        formatted(
            &scope,
            &"k".repeat(wire::I18N_FORMAT_MAX_KEY_BYTES),
            &[0],
            &mut []
        ),
        ERR_STATE
    );
    assert_eq!(
        formatted(
            &scope,
            &"k".repeat(wire::I18N_FORMAT_MAX_KEY_BYTES + 1),
            &[0],
            &mut []
        ),
        ERR_INVALID
    );
    assert_eq!(
        formatted(
            &scope,
            "k",
            &vec![0; wire::I18N_FORMAT_MAX_ARGUMENT_BYTES + 1],
            &mut []
        ),
        ERR_INVALID
    );
    assert_eq!(
        localized(&scope, &"1".repeat(wire::I18N_DIGITS_MAX_BYTES), &mut []),
        ERR_STATE
    );
    assert_eq!(
        localized(
            &scope,
            &"1".repeat(wire::I18N_DIGITS_MAX_BYTES + 1),
            &mut []
        ),
        ERR_INVALID
    );
    assert_eq!(
        copied(&scope, &"k".repeat(buffers::TEXT_MAX + 1), &mut []),
        ERR_INVALID
    );
    let records = [
        number("a", 1),
        number("b", 2),
        number("c", 3),
        number("d", 4),
        number("e", 5),
        number("f", 6),
        number("g", 7),
        number("h", 8),
    ];
    assert_eq!(records.len(), wire::I18N_FORMAT_MAX_RECORDS);
    assert_eq!(
        formatted(&scope, "k", &encode(&records), &mut []),
        ERR_STATE
    );
    let mut oversized_out = vec![0xa5; buffers::TEXT_MAX + 1];
    assert_eq!(localized(&scope, "", &mut oversized_out), ERR_INVALID);
    assert!(oversized_out.iter().all(|&byte| byte == 0xa5));
}

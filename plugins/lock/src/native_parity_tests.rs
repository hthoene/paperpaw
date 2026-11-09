//! The native screen-lock rules give what the interface-6 code did: the same
//! hashes, the same tries read back; and keep the rules the lock screen had
//! (the wait after five wrong tries, the keypad, the pattern's dots).
#![allow(clippy::unwrap_used, reason = "fixtures")]
extern crate std;

use crate::native_model as native;
use alloc::{format, string::String, vec::Vec};
use proptest::prelude::*;

/// Hashes as the interface-6 code took them (a salted SHA-1 taken 2,000 times, then the SHA-1 of the last digest in hex),
/// made once with an independent implementation (Python's `hashlib`), so a code
/// kept before the port still opens the lock.
const GOLDEN: [(&str, &str, &str); 6] = [
    ("salt", "1234", "2ed87ba1bbb0f9f6bda5f41655454c004f3b057d"),
    (
        "v2.0123456789abcdef",
        "correct horse battery staple",
        "b39239c23323523e128224b290ea738860c88e6a",
    ),
    ("", "", "ad462891d88102e54d7cba5e6a8715a29471323c"),
    ("s", "pässwörd", "0f718820059f0ef521851f4069614fb9df386874"),
    (
        "v2.abcdefghijklmnop",
        "0124",
        "81a2e52d8379b2bcdf05d5da4a909cf832c13951",
    ),
    ("legacy", "1234", "a7032eaf78baa1df4241bb0cccfb88640faa4b2e"),
];

#[test]
fn hashes_are_taken_as_before() {
    for (salt, code, hash) in GOLDEN {
        assert_eq!(native::hash(salt, code).unwrap(), hash);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]

    /// A code kept is opened by itself and by nothing else.
    #[test]
    fn a_kept_code_opens_with_itself_only(
        code in "[0-8]{4,16}",
        other in "[0-8]{4,16}",
        kind in prop_oneof![Just("pin"), Just("pattern"), Just("password")],
    ) {
        let salt = native::salt(Some("abcdefghijklmnop"), "", 0).unwrap();
        let kept = native::kept_value(kind, &salt, &code).unwrap();
        prop_assert!(native::check(Some(&kept), &code).unwrap());
        prop_assert_eq!(native::check(Some(&kept), &other).unwrap(), other == code);
        prop_assert_eq!(native::kept_kind(Some(&kept)), Some(kind));
        // A random salt needs no renewal.
        prop_assert_eq!(native::renewal(Some(&kept), &code).unwrap(), None);
    }
}

#[test]
fn a_code_kept_with_a_salt_of_the_code_is_to_be_kept_anew() {
    // Kept before salts were random: its salt is not `v2.` and random letters.
    let old = native::kept_value("pin", "legacy-salt", "1234").unwrap();
    assert_eq!(native::renewal(Some(&old), "1234").unwrap(), Some("pin"));
    // Only when the code opened it.
    assert_eq!(native::renewal(Some(&old), "9999").unwrap(), None);
    assert_eq!(native::renewal(None, "1234").unwrap(), None);
    assert_eq!(native::renewal(Some("pin"), "1234").unwrap(), None);
}

#[test]
fn a_damaged_code_locks_nothing() {
    for stored in [
        None,
        Some(""),
        Some("pin"),
        Some("pin:salt"),
        Some("pin::hash"),
        Some("pin:salt:"),
        Some("off:salt:hash"),
        Some("other:salt:hash"),
    ] {
        assert_eq!(native::kept_kind(stored), None, "{stored:?}");
        assert_eq!(native::active_kind("lock.kind.pin", stored), None);
        assert!(!native::check(stored, "1234").unwrap());
    }
    assert_eq!(native::kept_kind(Some("pattern:s:h")), Some("pattern"));
    // The setting must name the kind kept.
    let kept = Some("pin:s:h");
    assert_eq!(native::active_kind("lock.kind.pin", kept), Some("pin"));
    assert_eq!(native::active_kind("pin", kept), Some("pin"));
    assert_eq!(native::active_kind("lock.kind.off", kept), None);
    assert_eq!(native::active_kind("lock.kind.pattern", kept), None);
}

#[test]
fn a_salt_is_random_or_of_the_device_never_of_the_code() {
    assert_eq!(native::salt(Some("abcd"), "x", 1).unwrap(), "v2.abcd");
    let a = native::salt(None, "device", 7).unwrap();
    assert!(a.starts_with(native::SALT_MARK));
    assert_eq!(a.len(), native::SALT_MARK.len() + native::SALT_LETTERS);
    assert_eq!(a, native::salt(None, "device", 7).unwrap());
    assert_ne!(a, native::salt(None, "device", 8).unwrap());
    assert_ne!(a, native::salt(None, "other", 7).unwrap());
}

proptest! {
    /// The tries are read from any bytes the card holds, never more than a
    /// byte's worth, and what is kept reads back.
    #[test]
    fn tries_are_read_from_any_bytes_and_kept_whole(
        bytes in proptest::collection::vec(any::<u8>(), 0..40),
        tries in 0u8..=255,
    ) {
        let _ = native::parse_tries(&bytes);
        let text = native::format_tries(tries).unwrap();
        prop_assert_eq!(native::parse_tries(text.as_bytes()), tries);
    }
}

#[test]
fn the_tries_of_version_1_are_read() {
    assert_eq!(
        native::parse_tries(include_bytes!("../testdata/tries.txt.v1")),
        3
    );
}

#[test]
fn five_wrong_tries_make_the_next_one_wait() {
    let mut attempts = native::Attempts::default();
    for n in 1..native::TRIES {
        let failed = attempts.failed(1_000);
        assert_eq!(failed.tries, n);
        assert_eq!(failed.wake_at, None);
        assert!(!attempts.waiting(1_000));
    }
    let failed = attempts.failed(1_000);
    assert_eq!(failed.tries, native::TRIES);
    assert_eq!(failed.wake_at, Some(1_000 + native::WAIT_MS));
    assert!(attempts.waiting(1_000));
    assert!(attempts.waiting(30_999));
    assert!(!attempts.waiting(31_000));
    assert_eq!(attempts.seconds_left(1_000), 30);
    assert_eq!(attempts.seconds_left(30_500), 1);
    assert_eq!(attempts.seconds_left(31_000), 0);
    // More wrong tries keep the count at five.
    assert_eq!(attempts.failed(40_000).tries, native::TRIES);
    // The right one forgets them, and says there was something to forget.
    assert!(attempts.succeeded());
    assert!(!attempts.waiting(40_000));
    assert!(!attempts.succeeded());
}

#[test]
fn a_wait_due_when_paperpaw_stopped_is_waited_anew() {
    let mut attempts = native::Attempts::default();
    attempts.restore(2, 500);
    assert_eq!(attempts.failures(), 2);
    assert!(!attempts.waiting(500));
    attempts.restore(native::TRIES, 500);
    assert!(attempts.waiting(500));
    assert!(!attempts.waiting(500 + native::WAIT_MS));
}

#[test]
fn a_code_is_too_short_below_four_characters() {
    assert!(native::too_short(""));
    assert!(native::too_short("123"));
    assert!(!native::too_short("1234"));
    // Characters, not bytes.
    assert!(native::too_short("äöü"));
    assert!(!native::too_short("äöüß"));
}

#[test]
fn the_keypad_enters_deletes_and_submits() {
    let key = |name: &str| native::KEYS.iter().position(|k| *k == name).unwrap();
    let mut pin = native::Pin::default();
    for digit in ["1", "2", "3", "9"] {
        assert_eq!(pin.press(key(digit)).unwrap(), native::Pressed::Changed);
    }
    assert_eq!(pin.digits(), "1239");
    pin.press(key("delete")).unwrap();
    assert_eq!(pin.digits(), "123");
    // A key past the end changes nothing.
    pin.press(99).unwrap();
    assert_eq!(pin.digits(), "123");
    assert_eq!(
        pin.press(key("OK")).unwrap(),
        native::Pressed::Submit(native::Code::new(String::from("123")))
    );
    assert!(pin.is_empty());
    // Deleting nothing is harmless.
    pin.press(key("delete")).unwrap();
}

proptest! {
    /// Any presses keep at most sixteen digits, and only digits.
    #[test]
    fn any_presses_keep_a_pin_short_and_made_of_digits(keys in proptest::collection::vec(0usize..14, 0..60)) {
        let mut pin = native::Pin::default();
        for key in keys {
            if let native::Pressed::Submit(code) = pin.press(key).unwrap() {
                prop_assert!(code.len() <= native::MAX_PIN);
                prop_assert!(code.bytes().all(|b| b.is_ascii_digit()));
            }
            prop_assert!(pin.len() <= native::MAX_PIN);
            prop_assert!(pin.digits().bytes().all(|b| b.is_ascii_digit()));
        }
    }

    /// A pattern takes each dot once, and only the nine.
    #[test]
    fn any_dots_make_a_pattern_of_distinct_dots(dots in proptest::collection::vec(0usize..14, 0..40)) {
        let mut entered = String::new();
        for dot in dots {
            let present = dot < native::DOTS && entered.contains(char::from(b'0' + dot as u8));
            let added = native::add_dot(&mut entered, dot).unwrap();
            prop_assert_eq!(added, dot < native::DOTS && !present);
        }
        let mut seen: Vec<char> = entered.chars().collect();
        seen.sort_unstable();
        seen.dedup();
        prop_assert_eq!(seen.len(), entered.chars().count());
        prop_assert!(entered.bytes().all(|b| (b'0'..=b'8').contains(&b)));
    }
}

#[test]
fn what_is_secret_is_zeroed_where_it_is_kept() {
    let mut pin = native::Pin::default();
    for key in [0, 1, 2, 3] {
        pin.press(key).unwrap();
    }
    assert_eq!(pin.len(), 4);
    pin.clear();
    assert!(pin.is_empty());
    let mut text = String::from("1234");
    native::wipe_string(&mut text);
    assert_eq!(text, "\0\0\0\0");
    // A pattern too is cleared the same way.
    let mut dots = format!("{}", 5);
    native::add_dot(&mut dots, 7).unwrap();
    native::wipe_string(&mut dots);
    assert!(dots.bytes().all(|b| b == 0));
}

#[test]
fn the_wait_ends_after_thirty_seconds_and_forgets_the_tries() {
    let mut attempts = native::Attempts::default();
    for _ in 0..native::TRIES {
        attempts.failed(10_000);
    }
    assert_eq!(attempts.failures(), native::TRIES);
    // Not yet: the wait runs.
    assert!(!attempts.expire(10_000));
    assert!(!attempts.expire(10_000 + native::WAIT_MS - 1));
    assert_eq!(attempts.failures(), native::TRIES);
    // Over: the tries are forgotten (the caller clears the file), once.
    assert!(attempts.expire(10_000 + native::WAIT_MS));
    assert_eq!(attempts.failures(), 0);
    assert!(!attempts.waiting(10_000 + native::WAIT_MS));
    assert!(!attempts.expire(10_000 + native::WAIT_MS));
    // With no wait due, a few wrong tries are kept as they are.
    attempts.failed(50_000);
    assert!(!attempts.expire(500_000));
    assert_eq!(attempts.failures(), 1);
}

/// Codes as the interface-6 code stored them (made once with its `code::hash`,
/// as literals): one with a salt of the code's day (legacy), one with a random
/// `v2.` salt. The native check opens them with the code `1234` only.
const LEGACY: &str = "pin:legacy-salt:630cc1d253113f3f561cf9bda0d685037d0380ac";
const RANDOM: &str = "pin:v2.0123456789abcdef:da5f4294fb875aefe74eb8f07ec8c95a1a012aea";

#[test]
fn codes_stored_by_interface_6_still_open() {
    for stored in [LEGACY, RANDOM] {
        assert!(native::check(Some(stored), "1234").unwrap(), "{stored}");
        assert!(!native::check(Some(stored), "1235").unwrap(), "{stored}");
        assert!(!native::check(Some(stored), "").unwrap(), "{stored}");
        assert_eq!(native::kept_kind(Some(stored)), Some("pin"));
        assert_eq!(
            native::active_kind("lock.kind.pin", Some(stored)),
            Some("pin")
        );
    }
    // The old salt is to be kept anew (at random) once the code opened it; the
    // random one is not.
    assert_eq!(native::renewal(Some(LEGACY), "1234").unwrap(), Some("pin"));
    assert_eq!(native::renewal(Some(LEGACY), "1235").unwrap(), None);
    assert_eq!(native::renewal(Some(RANDOM), "1234").unwrap(), None);
    // What the native code keeps opens the same way, with the old parts.
    let kept = native::kept_value("pin", "v2.0123456789abcdef", "1234").unwrap();
    assert_eq!(kept, RANDOM);
}

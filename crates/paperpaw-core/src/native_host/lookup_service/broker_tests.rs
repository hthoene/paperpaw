#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The broker page: a provider's "not found" is an empty entry; the asker's
//! loop (cursor 0, 1, 2 ... until `ERR_NOT_FOUND`) still reaches every provider.
extern crate std;
use super::*;
use crate::native_host::lookup_service::check;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_IO, ERR_NO_SPACE, ERR_UNSUPPORTED};
use std::string::{String, ToString};
use std::vec::Vec;

/// The bytes of a provider's own answer.
fn entry_bytes(provider: &str, text: &str) -> Vec<u8> {
    let mut out = std::vec![0; 4096];
    let used = wire::record_encode_one(
        &Record::ServiceLookupEntry(wire::ServiceLookupEntry {
            provider,
            title: "Word",
            text,
            flags: 0,
            opens: "",
        }),
        &mut out,
    )
    .unwrap();
    out.truncate(used);
    out
}

/// What an entry holds: provider, title, text, flags.
fn read(reply: &[u8]) -> (String, String, String, u32) {
    let Record::ServiceLookupEntry(e) = wire::record_decode_one(reply).unwrap() else {
        panic!("an entry")
    };
    (
        e.provider.to_string(),
        e.title.to_string(),
        e.text.to_string(),
        e.flags,
    )
}

/// The asker's loop over `providers`, each answering as `answers` says.
fn walk(
    providers: &[&str],
    answers: &[Result<&str, i32>],
) -> (Vec<(String, String, String, u32)>, i32) {
    let mut seen = Vec::new();
    for cursor in 0..=providers.len() as u32 {
        let mut out = std::vec![0; 4096];
        let mut call = |name: &str, out: &mut [u8]| {
            let at = providers.iter().position(|p| *p == name).unwrap();
            match answers[at] {
                Ok(text) => {
                    let bytes = entry_bytes(name, text);
                    out[..bytes.len()].copy_from_slice(&bytes);
                    Ok(bytes.len())
                }
                Err(status) => Err(status),
            }
        };
        match page(providers, cursor, &mut call, &mut out) {
            Ok(length) => {
                check(&out[..length]).unwrap();
                seen.push(read(&out[..length]));
            }
            Err(status) => return (seen, status),
        }
    }
    panic!("the loop must end with a status");
}

#[test]
fn a_provider_without_an_entry_does_not_hide_the_next() {
    // A has no entry, B has one: B's entry still arrives, then the loop ends.
    // A's empty entry says more providers follow, as the host's own do.
    let (seen, end) = walk(&["a", "b"], &[Err(ERR_NOT_FOUND), Ok("definition")]);
    assert_eq!(
        seen,
        [
            ("a".to_string(), String::new(), String::new(), 1),
            (
                "b".to_string(),
                "Word".to_string(),
                "definition".to_string(),
                0
            ),
        ]
    );
    assert_eq!(end, ERR_NOT_FOUND, "past the last provider");
}

#[test]
fn the_last_provider_without_an_entry_is_an_empty_entry_too() {
    let (seen, end) = walk(&["a", "b"], &[Ok("x"), Err(ERR_NOT_FOUND)]);
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1], ("b".to_string(), String::new(), String::new(), 0));
    assert_eq!(
        seen[0].3, 1,
        "the provider's own answer gets the broker's more bit"
    );
    assert_eq!(end, ERR_NOT_FOUND);
    // Nothing but providers without an entry: one empty entry each.
    let (seen, _) = walk(&["a", "b", "c"], &[Err(ERR_NOT_FOUND); 3]);
    assert_eq!(seen.len(), 3);
    assert_eq!(seen.iter().map(|e| e.3).collect::<Vec<_>>(), [1, 1, 0]);
}

#[test]
fn a_cursor_past_the_providers_and_no_providers_end_the_loop() {
    let mut out = std::vec![0; 4096];
    let mut never = |_: &str, _: &mut [u8]| -> Result<usize, i32> { panic!("no provider") };
    assert_eq!(page(&[], 0, &mut never, &mut out), Err(ERR_NOT_FOUND));
    assert_eq!(page(&["a"], 1, &mut never, &mut out), Err(ERR_NOT_FOUND));
    assert_eq!(
        page(&["a"], u32::MAX, &mut never, &mut out),
        Err(ERR_NOT_FOUND)
    );
}

#[test]
fn a_providers_other_statuses_pass_as_they_are() {
    for status in [ERR_UNSUPPORTED, ERR_IO, ERR_INVALID] {
        let mut out = std::vec![0; 4096];
        let mut call = |_: &str, _: &mut [u8]| Err(status);
        assert_eq!(page(&["a"], 0, &mut call, &mut out), Err(status));
    }
}

#[test]
fn the_empty_entry_needs_a_name_it_can_carry_and_room() {
    let mut call = |_: &str, _: &mut [u8]| Err(ERR_NOT_FOUND);
    // A name over 64 bytes cannot be carried: refused, never cut.
    let long = "n".repeat(65);
    let mut out = std::vec![0; 4096];
    assert_eq!(
        page(&[long.as_str()], 0, &mut call, &mut out),
        Err(ERR_INVALID)
    );
    assert_eq!(page(&[""], 0, &mut call, &mut out), Err(ERR_INVALID));
    let longest = "n".repeat(64);
    assert!(page(&[longest.as_str()], 0, &mut call, &mut out).is_ok());
    // A scratch too small for it.
    let mut small = [0_u8; 4];
    assert_eq!(page(&["a"], 0, &mut call, &mut small), Err(ERR_NO_SPACE));
}

#[test]
fn a_providers_own_entry_gets_the_more_bit_of_its_place() {
    // The broker's rule: `more` where providers follow, clear on the last;
    // the provider's own `pending` bit stays; the texts are untouched.
    let pending = |flags: u32| {
        move |name: &str, out: &mut [u8]| {
            let mut bytes = std::vec![0; 4096];
            let used = wire::record_encode_one(
                &Record::ServiceLookupEntry(wire::ServiceLookupEntry {
                    provider: name,
                    title: "Word",
                    text: "definition",
                    flags,
                    opens: "",
                }),
                &mut bytes,
            )?;
            out[..used].copy_from_slice(&bytes[..used]);
            Ok(used)
        }
    };
    let run = |flags: u32, cursor: u32| {
        let mut out = std::vec![0; 4096];
        let mut call = pending(flags);
        let length = page(&["a", "b", "c"], cursor, &mut call, &mut out).unwrap();
        check(&out[..length]).unwrap();
        read(&out[..length])
    };
    // A provider that set `more` itself on the last page: cleared.
    assert_eq!(run(0, 0).3, 1);
    assert_eq!(run(1, 1).3, 1);
    assert_eq!(run(1, 2).3, 0);
    assert_eq!(run(0, 2).3, 0);
    // `pending` (bit 1) is the provider's: kept either way.
    assert_eq!(run(2, 0).3, 3);
    assert_eq!(run(3, 2).3, 2);
    let (provider, title, text, _) = run(2, 1);
    assert_eq!(
        (provider.as_str(), title.as_str(), text.as_str()),
        ("b", "Word", "definition")
    );
    // An answer that is not one entry is the provider's malformed answer.
    let mut out = std::vec![0; 4096];
    let mut short = |_: &str, _: &mut [u8]| Ok(2);
    assert_eq!(page(&["a"], 0, &mut short, &mut out), Err(ERR_INVALID));
}

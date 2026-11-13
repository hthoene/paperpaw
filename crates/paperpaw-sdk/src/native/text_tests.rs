#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
//! The wrapper against a slot 38 that answers as the shell's does
//! (`hyphen_service`): the shipped English and German tables, the tag
//! fallback, the statuses of `document.md` 6.1.
use super::*;
use alloc::{string::String, vec, vec::Vec};
use core::{cell::RefCell, ffi::c_void, ptr};
use paperpaw_hyphen::{Patterns, compile};
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes, PpHost};

/// One question to the slot: language, word, left, right, `out`'s room.
type Asked = (String, String, u32, u32, usize);

/// The tables the host has: (tag, compiled table).
#[derive(Default)]
struct Context {
    tables: Vec<(&'static str, Vec<u8>)>,
    /// What was asked: (language, word, left, right, cap).
    asked: RefCell<Vec<Asked>>,
    /// An answer to give instead: the status and the bytes to write.
    forced: Option<(i32, Vec<u8>)>,
}

fn english() -> (&'static str, Vec<u8>) {
    (
        "en",
        compile(
            include_str!("../../../../plugins/hyphenation/patterns/hyph-en-us.pat.txt"),
            include_str!("../../../../plugins/hyphenation/patterns/hyph-en-us.hyp.txt"),
            2,
            3,
        ),
    )
}

fn german() -> (&'static str, Vec<u8>) {
    (
        "de",
        compile(
            include_str!("../../../../plugins/hyphenation-de/patterns/hyph-de-1996.pat.txt"),
            "",
            2,
            2,
        ),
    )
}

unsafe fn text<'a>(input: PpBytes) -> &'a str {
    // SAFETY: the wrapper passes the caller's live UTF-8 text.
    unsafe { core::str::from_utf8(core::slice::from_raw_parts(input.ptr, input.len)).unwrap() }
}

/// The shell's answer (`hyphen_service::hyphenate`) over the tables of `ctx`.
fn answer(ctx: &Context, tag: &str, word: &str, left: u32, right: u32, out: &mut [u8]) -> i32 {
    if left > 16 || right > 16 || word.len() > 192 {
        return abi::ERR_INVALID;
    }
    let mut tried = tag.to_ascii_lowercase();
    let table = loop {
        if let Some((_, table)) = ctx.tables.iter().find(|(t, _)| *t == tried) {
            break table;
        }
        match tried.rfind('-') {
            Some(cut) => tried.truncate(cut),
            None => return abi::ERR_NOT_FOUND,
        }
    };
    let chars = word.chars().count();
    let (l, r) = (left as usize, right as usize);
    if word.is_empty() || chars > 48 || (l != 0 && r != 0 && chars < l + r) {
        return 0;
    }
    let mut places = Vec::new();
    Patterns::new(table)
        .unwrap()
        .with_minimums(l, r)
        .hyphenate(word, &mut |at| places.push(u8::try_from(at).unwrap()));
    if places.len() > out.len() {
        return abi::ERR_NO_SPACE;
    }
    out[..places.len()].copy_from_slice(&places);
    i32::try_from(places.len()).unwrap()
}

unsafe extern "C" fn hyphenate_slot(
    ctx: *mut c_void,
    language: PpBytes,
    word: PpBytes,
    left: u32,
    right: u32,
    out: PpBuffer,
) -> i32 {
    // SAFETY: the wrapper supplies a live context, readable texts and a
    // writable buffer of `cap` bytes.
    unsafe {
        let ctx = &*ctx.cast::<Context>();
        let (tag, word) = (text(language), text(word));
        ctx.asked
            .borrow_mut()
            .push((tag.into(), word.into(), left, right, out.cap));
        let out = core::slice::from_raw_parts_mut(out.ptr, out.cap);
        if let Some((status, bytes)) = &ctx.forced {
            out[..bytes.len()].copy_from_slice(bytes);
            return *status;
        }
        answer(ctx, tag, word, left, right, out)
    }
}

fn ask(
    ctx: &mut Context,
    language: &str,
    word: &str,
    minimums: (u32, u32),
    out: &mut [u8],
) -> Result<usize, Error> {
    let mut table: PpHost = abi::host_table();
    table.text_hyphenate = hyphenate_slot;
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(ctx).cast(),
    };
    hyphenate(&host, language, word, minimums.0, minimums.1, out)
}

/// `word` with a hyphen at each place the host answered.
fn hyphenated(ctx: &mut Context, language: &str, word: &str, minimums: (u32, u32)) -> String {
    let mut out = [0; BREAKS_MAX];
    let count = ask(ctx, language, word, minimums, &mut out).unwrap();
    let mut shown = String::from(word);
    for &at in out[..count].iter().rev() {
        shown.insert(usize::from(at), '-');
    }
    shown
}

fn both() -> Context {
    Context {
        tables: vec![english(), german()],
        ..Context::default()
    }
}

#[test]
fn a_german_and_an_english_word_break_where_their_tables_say() {
    let mut ctx = both();
    assert_eq!(
        hyphenated(&mut ctx, "de", "Silbentrennung", (0, 0)),
        "Sil-ben-tren-nung"
    );
    assert_eq!(hyphenated(&mut ctx, "de", "Bücher", (0, 0)), "Bü-cher");
    assert_eq!(
        hyphenated(&mut ctx, "en", "hyphenation", (0, 0)),
        "hy-phen-ation"
    );
    // The host's tag fallback: `de-CH-1996` → `de-CH` → `de`.
    assert_eq!(
        hyphenated(&mut ctx, "de-CH-1996", "Silbentrennung", (0, 0)),
        "Sil-ben-tren-nung"
    );
    // The reader's minimums pass as they are.
    assert_eq!(
        hyphenated(&mut ctx, "de", "Silbentrennung", (4, 5)),
        "Silben-trennung"
    );
    let asked = ctx.asked.borrow();
    assert_eq!(asked.len(), 5);
    assert_eq!(
        asked[4],
        ("de".into(), "Silbentrennung".into(), 4, 5, BREAKS_MAX)
    );
}

#[test]
fn an_empty_word_asks_whether_the_language_has_a_table() {
    let mut ctx = both();
    let mut out = [0; BREAKS_MAX];
    assert_eq!(ask(&mut ctx, "de", "", (0, 0), &mut out), Ok(0));
    assert_eq!(
        ask(&mut ctx, "fr", "", (0, 0), &mut out),
        Err(Error::NotFound)
    );
}

#[test]
fn without_a_table_for_the_language_there_is_none() {
    let mut ctx = Context::default();
    let mut out = [0; BREAKS_MAX];
    assert_eq!(
        ask(&mut ctx, "de", "Silbentrennung", (0, 0), &mut out),
        Err(Error::NotFound)
    );
}

#[test]
fn a_word_too_short_or_too_long_in_characters_is_left_whole() {
    let mut ctx = both();
    assert_eq!(hyphenated(&mut ctx, "de", "Hut", (2, 2)), "Hut");
    let long = "Donaudampfschifffahrtsgesellschaftskapitänsmützenband";
    assert!(long.chars().count() > 48 && long.len() <= WORD_MAX);
    assert_eq!(hyphenated(&mut ctx, "de", long, (0, 0)), long);
}

#[test]
fn a_bad_argument_is_refused_without_asking_the_host() {
    let mut ctx = both();
    let mut out = [0; BREAKS_MAX];
    let word = "a".repeat(WORD_MAX + 1);
    let tag = "a".repeat(LANGUAGE_MAX + 1);
    for (language, word, minimums) in [
        ("de", word.as_str(), (0, 0)),
        ("", "Wort", (0, 0)),
        (tag.as_str(), "Wort", (0, 0)),
        ("de_DE", "Wort", (0, 0)),
        ("dé", "Wort", (0, 0)),
        ("de", "Wort", (MINIMUM_MAX + 1, 0)),
        ("de", "Wort", (0, MINIMUM_MAX + 1)),
    ] {
        assert_eq!(
            ask(&mut ctx, language, word, minimums, &mut out),
            Err(Error::Invalid),
            "{language} {minimums:?}"
        );
    }
    assert!(ctx.asked.borrow().is_empty());
    // At the limits it is asked.
    let word = "a".repeat(WORD_MAX);
    let tag = "a".repeat(LANGUAGE_MAX);
    assert_eq!(
        ask(&mut ctx, "de", &word, (MINIMUM_MAX, MINIMUM_MAX), &mut out),
        Ok(0)
    );
    assert_eq!(
        ask(&mut ctx, &tag, "Wort", (0, 0), &mut out),
        Err(Error::NotFound)
    );
}

#[test]
fn an_out_too_small_is_no_space() {
    let mut ctx = both();
    let mut out = [0; 2];
    assert_eq!(
        ask(&mut ctx, "de", "Silbentrennung", (0, 0), &mut out),
        Err(Error::NoSpace)
    );
    assert_eq!(ctx.asked.borrow()[0].4, 2, "the caller's room is passed");
}

#[test]
fn an_answer_that_breaks_the_contract_is_invalid() {
    let word = "Bücher";
    for (status, bytes) in [
        // More than `out` holds.
        (5, vec![]),
        // Not ascending.
        (2, vec![3, 1]),
        // Twice the same place.
        (2, vec![3, 3]),
        // At the start, at the end, past it.
        (1, vec![0]),
        (1, vec![7]),
        (1, vec![9]),
        // Inside `ü` (bytes 1 and 2).
        (1, vec![2]),
    ] {
        let mut ctx = Context {
            forced: Some((status, bytes.clone())),
            ..Context::default()
        };
        let mut out = [0; 4];
        assert_eq!(
            ask(&mut ctx, "de", word, (0, 0), &mut out),
            Err(Error::Invalid),
            "{status} {bytes:?}"
        );
    }
}

#[test]
fn the_hosts_refusals_pass_through() {
    for (status, error) in [
        (abi::ERR_UNSUPPORTED, Error::Unsupported),
        (abi::ERR_BUSY, Error::Busy),
        (abi::ERR_UTF8, Error::Utf8),
    ] {
        let mut ctx = Context {
            forced: Some((status, vec![])),
            ..Context::default()
        };
        let mut out = [0; BREAKS_MAX];
        assert_eq!(ask(&mut ctx, "de", "Wort", (0, 0), &mut out), Err(error));
    }
}

#[test]
fn a_host_that_has_not_wired_the_slot_is_unsupported() {
    let table: PpHost = abi::host_table();
    let host = Host {
        table: &table,
        ctx: ptr::null_mut(),
    };
    let mut out = [0; BREAKS_MAX];
    assert_eq!(
        hyphenate(&host, "de", "Wort", 0, 0, &mut out),
        Err(Error::Unsupported)
    );
}

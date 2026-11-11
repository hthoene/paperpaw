//! The whole-plugin rules of formats and reading apps (`document.md` section
//! 1): admitted at all, eight formats per plugin, one claim per extension
//! within a plugin, a reader's screen is the plugin's own.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use super::*;
use crate::native_host::{
    raw_declarations::RawDeclarations,
    registrations::{
        REGISTRATION_MAX_BYTES, ReaderSpec, ScreenSpec, records_decode, records_encode,
    },
    staged::Declarations,
};
use alloc::{format, string::String, vec, vec::Vec};

fn fmt<'a>(name: &'a str, extensions: &'a str) -> Record<'a> {
    Record::FormatSpec(FormatSpec {
        name,
        title: "Title",
        extensions,
        flags: 0,
        priority: 0,
        token: 1,
    })
}

fn screen(name: &str) -> Record<'_> {
    Record::ScreenSpec(ScreenSpec {
        name,
        title: "Screen",
        token: 2,
        flags: 0,
    })
}

fn reader<'a>(name: &'a str, screen: &'a str, formats: &'a str) -> Record<'a> {
    Record::ReaderSpec(ReaderSpec {
        name,
        screen,
        formats,
        flags: 0,
    })
}

/// The plugin `epub`'s batches, committed and checked at the end.
fn finish(batches: &[&[Record<'_>]]) -> Result<(), i32> {
    refused(batches).0
}

/// As `finish`, with the cap the check said it refused the batch at.
fn refused(batches: &[&[Record<'_>]]) -> (Result<(), i32>, Option<Cap>) {
    let mut raw = RawDeclarations::default();
    for records in batches {
        let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
        let len = records_encode(records, &mut bytes).unwrap();
        bytes.truncate(len);
        let pending = raw
            .prepare(None, Some(&bytes), &records_decode(&bytes).unwrap())
            .unwrap();
        raw.commit(pending);
    }
    raw.sort_index();
    let owned = Declarations::default();
    let view = View {
        raw: &raw,
        owned: &owned,
        image: None,
        plugin: "epub",
        refused: Default::default(),
    };
    let result = view.finish();
    (result, view.refused.get())
}

#[test]
fn formats_and_readers_are_admitted_by_the_syntax_check() {
    assert_eq!(
        crate::native_host::declarations::validate_record("epub", &fmt("epub", "epub\nepub3")),
        Ok(())
    );
    assert_eq!(
        crate::native_host::declarations::validate_record(
            "epub",
            &reader("app", "epub.main", "epub.epub")
        ),
        Ok(())
    );
    // A bad extension is still the syntax check's `ERR_INVALID`.
    assert_eq!(
        crate::native_host::declarations::validate_record("epub", &fmt("epub", "EPUB")),
        Err(ERR_INVALID)
    );
}

#[test]
fn a_format_name_is_declared_once_and_an_extension_is_claimed_once_by_a_plugin() {
    assert_eq!(finish(&[&[fmt("a", "epub"), fmt("b", "fb2")]]), Ok(()));
    assert_eq!(
        finish(&[&[fmt("a", "epub")], &[fmt("a", "fb2")]]),
        Err(ERR_INVALID)
    );
    // The same extension in two formats of one plugin is refused, in one
    // batch or across batches, whatever its place in the list.
    assert_eq!(
        finish(&[&[fmt("a", "epub\nfb2"), fmt("b", "txt\nfb2")]]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        finish(&[&[fmt("a", "epub")], &[fmt("b", "epub")]]),
        Err(ERR_INVALID)
    );
    // A longer extension is another claim (`fb2.zip` and `zip`).
    assert_eq!(finish(&[&[fmt("a", "fb2.zip"), fmt("b", "zip")]]), Ok(()));
}

#[test]
fn a_plugin_declares_eight_formats_and_the_ninth_is_full() {
    let names: Vec<String> = (0..9).map(|n| format!("f{n}")).collect();
    let exts: Vec<String> = (0..9).map(|n| format!("x{n}")).collect();
    let rows: Vec<Record<'_>> = names
        .iter()
        .zip(&exts)
        .map(|(name, ext)| fmt(name, ext))
        .collect();
    assert_eq!(finish(&[&rows[..4], &rows[4..8]]), Ok(()));
    // Refused at the plugin's cap: problem 14, arg 8.
    assert_eq!(
        refused(&[&rows[..4], &rows[4..8], &rows[8..]]),
        (Err(ERR_FULL), Some(Cap::of(Cap::FORMATS, 8)))
    );
    assert_eq!(refused(&[&rows[..4]]).1, None);
}

#[test]
fn a_reader_shows_one_screen_of_its_own_plugin() {
    assert_eq!(
        finish(&[&[screen("main"), reader("app", "epub.main", "")]]),
        Ok(())
    );
    assert_eq!(
        finish(&[&[reader("app", "epub.nowhere", "")]]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        finish(&[&[screen("main"), reader("app", "other.main", "")]]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        finish(&[&[
            screen("main"),
            reader("app", "epub.main", ""),
            reader("app", "epub.main", "")
        ]]),
        Err(ERR_INVALID)
    );
}

#[test]
fn a_plugin_declares_four_reading_apps_and_the_fifth_is_invalid() {
    let names = ["a", "b", "c", "d", "e"];
    let rows: Vec<Record<'_>> = names
        .iter()
        .map(|name| reader(name, "epub.main", ""))
        .collect();
    let mut all = vec![screen("main")];
    all.extend(rows[..4].iter().copied());
    assert_eq!(finish(&[&all[..3], &all[3..]]), Ok(()));
    all.push(rows[4]);
    // Refused at the plugin's cap: problem 15, arg 4 (the status stays
    // `ERR_INVALID`: the cap decides the value, not the status).
    assert_eq!(
        refused(&[&all[..3], &all[3..]]),
        (Err(ERR_INVALID), Some(Cap::of(Cap::READERS, 4)))
    );
}

fn provider(name: &str) -> Record<'_> {
    Record::LookupSpec(crate::native_host::registrations::LookupSpec {
        name,
        title: "Title",
        languages: "en",
        flags: 0,
        token: 3,
    })
}

#[test]
fn a_plugin_declares_eight_lookup_providers_and_the_ninth_is_invalid() {
    let names = ["a", "b", "c", "d", "e", "f", "g", "h", "i"];
    let rows: Vec<Record<'_>> = names.iter().map(|n| provider(n)).collect();
    assert_eq!(finish(&[&rows[..4], &rows[4..8]]), Ok(()));
    // The ninth refuses the whole set, over any number of batches: problem
    // 18, arg 8.
    assert_eq!(
        refused(&[&rows[..4], &rows[4..8], &rows[8..]]),
        (Err(ERR_INVALID), Some(Cap::of(Cap::LOOKUPS, 8)))
    );
    assert_eq!(refused(&[&rows[..4]]).1, None);
}

#[test]
fn a_provider_name_is_declared_once_per_plugin() {
    assert_eq!(finish(&[&[provider("a"), provider("a")]]), Err(ERR_INVALID));
    assert_eq!(
        finish(&[&[provider("a")], &[provider("a")]]),
        Err(ERR_INVALID)
    );
    assert_eq!(finish(&[&[provider("a"), provider("b")]]), Ok(()));
}

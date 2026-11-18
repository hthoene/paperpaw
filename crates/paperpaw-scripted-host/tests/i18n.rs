//! The scripted `i18n.copy`, `i18n.format` and `i18n.digits`: the plugin's
//! own catalogs, the shell's lookup, fallback, plural and placeholder rules.
#![allow(clippy::unwrap_used, reason = "fixture failures fail the test")]
use core::ffi::c_void;
use paperpaw_core::native_host::registrations::{
    I18nArgInt, I18nArgStr, I18nCount, Record, records_encode,
};
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_UNSUPPORTED, OK, PpBuffer, PpBytes, PpEntry,
    PpHost, PpPlugin,
};
use paperpaw_scripted_host::{ScriptedHost, Slot};
use std::cell::RefCell;

const EN: &str = "[meta]\nlanguage = en\nplural = one-other\n[messages]\nt.hello = Hello\nt.only-en = Only English\nt.greet = Hi {name}, {n} new; {{literal}} {missing}\nt.books.one = {count} book\nt.books.other = {count} books\n";
const DE: &str = "[meta]\nlanguage = de\nplural = one-other\n[messages]\nt.hello = Hallo\nt.greet = Hallo {name}, {n} neu\nt.books.one = {count} Buch\nt.books.other = {count} Bücher\n";
const FA: &str =
    "[meta]\nlanguage = fa\nplural = zero-one-other\ndigits = arabext\n[messages]\nt.hello = سلام\n";

thread_local! {
    static SEEN: RefCell<Vec<(i32, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
}

struct Probe {
    host: *const PpHost,
    ctx: *mut c_void,
}

/// Input: [slot u8][cap u16 LE][key len u16 LE][key][rest = args or nothing].
unsafe extern "C" fn event(state: *mut c_void, _t: u32, input: PpBytes, _o: PpBuffer) -> i32 {
    // SAFETY: the leaked `Probe` and the readable input.
    let (probe, input) = unsafe {
        (
            &*state.cast::<Probe>(),
            core::slice::from_raw_parts(input.ptr, input.len),
        )
    };
    // SAFETY: the scripted table outlives the plugin.
    let host = unsafe { &*probe.host };
    let cap = usize::from(u16::from_le_bytes([input[1], input[2]]));
    let key_len = usize::from(u16::from_le_bytes([input[3], input[4]]));
    let key = &input[5..5 + key_len];
    let args = &input[5 + key_len..];
    let bytes = |data: &[u8]| PpBytes {
        ptr: if data.is_empty() {
            core::ptr::null()
        } else {
            data.as_ptr()
        },
        len: data.len(),
    };
    let mut out = vec![0_u8; cap];
    let buffer = PpBuffer {
        ptr: out.as_mut_ptr(),
        cap,
    };
    // SAFETY: scripted slots; the pointers live for the call.
    let status = unsafe {
        match input[0] {
            0 => (host.i18n_copy)(probe.ctx, bytes(key), buffer),
            1 => (host.i18n_format)(probe.ctx, bytes(key), bytes(args), buffer),
            _ => (host.i18n_digits)(probe.ctx, bytes(key), buffer),
        }
    };
    out.truncate(usize::try_from(status).unwrap_or(0).min(cap));
    SEEN.with(|seen| seen.borrow_mut().push((status, out)));
    OK
}

unsafe extern "C" fn make(host: *const PpHost, ctx: *mut c_void, out: *mut *const PpPlugin) -> i32 {
    let state = Box::into_raw(Box::new(Probe { host, ctx }));
    let table = Box::new(PpPlugin {
        size: core::mem::size_of::<PpPlugin>() as u32,
        state: state.cast(),
        drop: None,
        dispatch_event: Some(event),
        draw: None,
        input: None,
    });
    // SAFETY: the host's writable slot; the table is leaked for the test.
    unsafe { *out = Box::into_raw(table) };
    OK
}

fn entry() -> PpEntry {
    PpEntry {
        size: core::mem::size_of::<PpEntry>() as u32,
        abi_major: 7,
        needs_minor: 1,
        metadata: PpBytes {
            ptr: core::ptr::null(),
            len: 0,
        },
        make: Some(make),
    }
}

fn host() -> ScriptedHost {
    let host = ScriptedHost::for_plugin("t");
    for source in [EN, DE, FA] {
        host.add_catalog(source).unwrap();
    }
    host
}

/// The status and text of slot `which` (0 copy, 1 format, 2 digits).
fn ask(
    host: &ScriptedHost,
    which: u8,
    key: &str,
    args: &[Record<'_>],
    cap: usize,
) -> (i32, String) {
    let mut batch = vec![0_u8; 1024];
    let len = if args.is_empty() {
        0
    } else {
        records_encode(args, &mut batch).unwrap()
    };
    let mut input = vec![which];
    input.extend_from_slice(&u16::try_from(cap).unwrap().to_le_bytes());
    input.extend_from_slice(&u16::try_from(key.len()).unwrap().to_le_bytes());
    input.extend_from_slice(key.as_bytes());
    input.extend_from_slice(&batch[..len]);
    let entry = entry();
    let plugin = host.load(&entry).unwrap();
    SEEN.with(|seen| seen.borrow_mut().clear());
    assert_eq!(plugin.event(0, &input).unwrap().status, OK);
    let (status, out) = SEEN.with(|seen| seen.borrow_mut().pop().unwrap());
    (status, String::from_utf8(out).unwrap())
}

fn named<'a>(name: &'a str, value: &'a str) -> Record<'a> {
    Record::I18nArgStr(I18nArgStr { name, value })
}

fn number(name: &str, value: i64) -> Record<'_> {
    Record::I18nArgInt(I18nArgInt {
        name,
        value: value as u64,
    })
}

#[test]
fn copy_answers_the_language_then_english_and_not_found_for_no_key() {
    let host = host();
    assert_eq!(ask(&host, 0, "t.hello", &[], 64), (5, "Hello".into()));
    host.set_language("de");
    assert_eq!(ask(&host, 0, "t.hello", &[], 64), (5, "Hallo".into()));
    // A key the language lacks falls back to English.
    assert_eq!(ask(&host, 0, "t.only-en", &[], 64).1, "Only English");
    assert_eq!(
        ask(&host, 0, "t.nothing", &[], 64),
        (ERR_NOT_FOUND, String::new())
    );
    // The full length is answered; the fitting prefix is written.
    assert_eq!(ask(&host, 0, "t.hello", &[], 2), (5, "Ha".into()));
}

#[test]
fn format_binds_named_arguments_and_leaves_what_it_does_not_know() {
    let host = host();
    let args = [named("name", "Ada"), number("n", 3)];
    assert_eq!(
        ask(&host, 1, "t.greet", &args, 128).1,
        "Hi Ada, 3 new; {literal} {missing}"
    );
    host.set_language("de");
    assert_eq!(ask(&host, 1, "t.greet", &args, 128).1, "Hallo Ada, 3 neu");
    // A negative number, and a name given twice.
    assert_eq!(
        ask(
            &host,
            1,
            "t.greet",
            &[named("name", "Z"), number("n", -4)],
            128
        )
        .1,
        "Hallo Z, -4 neu"
    );
    assert_eq!(
        ask(
            &host,
            1,
            "t.greet",
            &[named("name", "A"), named("name", "B")],
            128
        )
        .0,
        ERR_INVALID
    );
    assert_eq!(ask(&host, 1, "t.nothing", &args, 128).0, ERR_NOT_FOUND);
}

#[test]
fn format_picks_the_plural_of_the_catalog_and_binds_count() {
    let host = host();
    let count = |value: i64| {
        [Record::I18nCount(I18nCount {
            value: value as u64,
        })]
    };
    assert_eq!(ask(&host, 1, "t.books", &count(1), 64).1, "1 book");
    assert_eq!(ask(&host, 1, "t.books", &count(2), 64).1, "2 books");
    host.set_language("de");
    assert_eq!(ask(&host, 1, "t.books", &count(1), 64).1, "1 Buch");
    assert_eq!(ask(&host, 1, "t.books", &count(5), 64).1, "5 Bücher");
    // Two counts are refused.
    let two = [count(1)[0], count(2)[0]];
    assert_eq!(ask(&host, 1, "t.books", &two, 64).0, ERR_INVALID);
}

#[test]
fn digits_follow_the_language_and_the_result_may_grow() {
    let host = host();
    assert_eq!(ask(&host, 2, "a1b2", &[], 64), (4, "a1b2".into()));
    host.set_language("fa");
    let (status, text) = ask(&host, 2, "12", &[], 64);
    assert_eq!(text, "۱۲");
    assert_eq!(status, 4);
}

#[test]
fn the_plugins_own_catalog_files_are_read_from_its_folder() {
    let host = ScriptedHost::for_plugin("about");
    let folder = concat!(env!("CARGO_MANIFEST_DIR"), "/../../plugins/about/i18n");
    assert_eq!(host.add_catalogs_in(folder), Ok(2));
    let (status, title) = ask(&host, 0, "about.title", &[], 128);
    assert_eq!((status > 0, title.as_str()), (true, "About PaperPaw"));
    host.set_language("de");
    let german = ask(&host, 0, "about.title", &[], 128).1;
    assert_ne!(german, "About PaperPaw");
    assert!(host.add_catalogs_in("/nonexistent/i18n").is_err());
}

#[test]
fn a_refusal_and_the_slot_counts_apply_to_the_i18n_slots() {
    let host = host();
    host.refuse_nth(Slot::I18nFormat, 1, ERR_NO_SPACE);
    let any = [named("x", "y")];
    assert_eq!(ask(&host, 1, "t.hello", &any, 64).0, ERR_NO_SPACE);
    assert_eq!(ask(&host, 1, "t.hello", &any, 64).0, 5);
    assert_eq!(host.calls(Slot::I18nFormat), 2);
    // A catalog that does not parse is refused, not half-added.
    assert!(host.add_catalog("[messages]\nx = 1\n").is_err());
    // Without catalogs nothing is found (not `ERR_UNSUPPORTED` as before).
    let empty = ScriptedHost::for_plugin("t");
    assert_ne!(ask(&empty, 0, "t.hello", &[], 64).0, ERR_UNSUPPORTED);
}

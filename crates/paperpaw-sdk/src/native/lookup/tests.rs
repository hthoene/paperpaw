#![allow(clippy::unwrap_used, reason = "fixture refusals must fail tests")]
use super::*;
use crate::native::declare::records_wire::Record;
use alloc::{string::String, vec, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
};
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes, PpHost};
#[derive(Default)]
struct State {
    op: Cell<u32>,
    calls: Cell<u32>,
    input: RefCell<Vec<u8>>,
    reply: RefCell<Vec<u8>>,
    error: Cell<i32>,
}
unsafe extern "C" fn open(
    _: *mut c_void,
    key: PpBytes,
    major: u16,
    minor: u16,
    out: *mut u32,
) -> i32 {
    // SAFETY: SDK provides a live key and aligned output.
    unsafe {
        assert_eq!(
            core::slice::from_raw_parts(key.ptr, key.len),
            b"core.lookup"
        );
        out.write(7);
    }
    assert_eq!((major, minor), (1, 0));
    abi::OK
}
unsafe extern "C" fn call(ctx: *mut c_void, _: u32, op: u32, input: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: fixture state and SDK buffers remain live through this call.
    let state = unsafe { &*ctx.cast::<State>() };
    state.op.set(op);
    state.calls.set(state.calls.get() + 1);
    // SAFETY: SDK encoded a live input slice.
    state
        .input
        .replace(unsafe { core::slice::from_raw_parts(input.ptr, input.len) }.to_vec());
    if state.error.get() < 0 {
        return state.error.get();
    }
    let reply = state.reply.borrow();
    assert!(reply.len() <= out.cap);
    // SAFETY: the fixture reply and exclusive SDK output are disjoint.
    unsafe {
        core::ptr::copy_nonoverlapping(reply.as_ptr(), out.ptr, reply.len());
    }
    i32::try_from(reply.len()).unwrap()
}
fn table() -> PpHost {
    let mut t = abi::host_table();
    t.service_open = open;
    t.service_call = call;
    t
}
fn reply(state: &State, rows: &[Record<'_>]) {
    let mut bytes = vec![0; 4096];
    let mut n = 0;
    for row in rows {
        n += wire::record_encode_one(row, &mut bytes[n..]).unwrap();
    }
    bytes.truncate(n);
    state.reply.replace(bytes);
}
fn entry(flags: u32) -> Record<'static> {
    Record::ServiceLookupEntry(wire::ServiceLookupEntry {
        provider: "wikipedia.summary",
        title: "Rabbit",
        text: "A small mammal.",
        flags,
        opens: "",
    })
}
fn with_host<T>(run: impl FnOnce(&Host<'_>, &State) -> T) -> T {
    let table = table();
    let mut state = State::default();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut state).cast(),
    };
    let state = unsafe { &*core::ptr::from_ref(&state) };
    run(&host, state)
}

#[test]
fn an_ask_goes_out_with_its_filters_and_the_entry_is_borrowed() {
    with_host(|host, state| {
        let lookup = Lookup::open(host).unwrap();
        let question = Question::new("rabbit")
            .unwrap()
            .language("en-GB")
            .unwrap()
            .context("the white rabbit")
            .unwrap()
            .cursor(2);
        reply(state, &[entry(0b11)]);
        let (mut input, mut out) = ([0; BUFFER], [0; BUFFER]);
        let got = lookup
            .ask(host, &question, &mut input, &mut out)
            .unwrap()
            .unwrap();
        assert_eq!(
            (
                got.provider(),
                got.title(),
                got.text(),
                got.more(),
                got.pending()
            ),
            ("wikipedia.summary", "Rabbit", "A small mammal.", true, true)
        );
        assert_eq!(state.op.get(), schema::OP_LOOKUP);
        assert!(matches!(
            wire::records_decode(&state.input.borrow()).unwrap().iter().next(),
            Some(Record::ServiceLookupAsk(a))
                if (a.text, a.language, a.context, a.cursor) == ("rabbit", "en-GB", "the white rabbit", 2)
        ));
        reply(state, &[entry(0)]);
        let got = lookup
            .ask(host, &question, &mut input, &mut out)
            .unwrap()
            .unwrap();
        assert!(!got.more() && !got.pending());
    });
}

#[test]
fn past_the_last_provider_is_none_and_other_refusals_are_errors() {
    with_host(|host, state| {
        let lookup = Lookup::open(host).unwrap();
        let question = Question::new("rabbit").unwrap();
        let (mut input, mut out) = ([0; BUFFER], [0; BUFFER]);
        state.error.set(abi::ERR_NOT_FOUND);
        assert_eq!(lookup.ask(host, &question, &mut input, &mut out), Ok(None));
        state.error.set(abi::ERR_INVALID);
        assert_eq!(
            lookup.ask(host, &question, &mut input, &mut out),
            Err(Error::Invalid)
        );
        state.error.set(0);
        reply(state, &[]);
        assert_eq!(
            lookup.ask(host, &question, &mut input, &mut out),
            Err(Error::Invalid)
        );
    });
}

#[test]
fn questions_are_checked_before_the_host_sees_them() {
    assert_eq!(Question::new("").unwrap_err(), Error::Invalid);
    assert_eq!(
        Question::new(&"x".repeat(MAX_TEXT + 1)).unwrap_err(),
        Error::NoSpace
    );
    assert!(Question::new(&"x".repeat(MAX_TEXT)).is_ok());
    let q = Question::new("x").unwrap();
    assert!(q.language("").is_ok() && q.language("zh-Hant-TW").is_ok());
    assert_eq!(q.language("e").unwrap_err(), Error::Invalid);
    assert_eq!(q.language("en_GB").unwrap_err(), Error::Invalid);
    assert_eq!(q.language("abcdefghi").unwrap_err(), Error::Invalid);
    assert_eq!(q.language(&"a".repeat(36)).unwrap_err(), Error::NoSpace);
    assert_eq!(
        q.context(&"x".repeat(MAX_CONTEXT + 1)).unwrap_err(),
        Error::NoSpace
    );
}

#[test]
fn a_provider_reads_the_ask_and_answers_one_entry_the_host_accepts() {
    let ask = stage::LookupAsk::new("Hund", "de", "der Hund bellt", 1);
    let mut payload = [0; 3840];
    let n = stage::input_encode(&[stage::Record::LookupAsk(ask)], &mut payload).unwrap();
    let read = Ask::decode(&payload[..n]).unwrap();
    assert_eq!(
        (read.text(), read.language(), read.context(), read.cursor()),
        ("Hund", Some("de"), "der Hund bellt", 1)
    );
    let any = stage::LookupAsk::new("dog", "", "", 0);
    let n = stage::input_encode(&[stage::Record::LookupAsk(any)], &mut payload).unwrap();
    assert_eq!(Ask::decode(&payload[..n]).unwrap().language(), None);
    assert_eq!(Ask::decode(&[]).unwrap_err(), Error::Invalid);

    let mut buffer = [core::mem::MaybeUninit::uninit(); 3840];
    let mut reply = Reply::new(&mut buffer);
    answer(
        &mut reply,
        &Entry::new("Dog", "A domestic animal.").unwrap().more(),
    )
    .unwrap();
    assert!(!reply.is_empty());
    let len = reply.len();
    // The bytes are what the stage's reply codec reads.
    // SAFETY: `Reply::write` initialised exactly the first `len` slots.
    let written: Vec<u8> = buffer[..len]
        .iter()
        .map(|b| unsafe { b.assume_init() })
        .collect();
    let mut rows = stage::reply_decode(&written).unwrap();
    assert!(matches!(
        rows.next(),
        Some(Ok(stage::Record::LookupEntry(e))) if (e.title, e.text, e.flags) == ("Dog", "A domestic animal.", 1)
    ));
}

/// What `answer` wrote for `entry`: title, text, flags, opens.
fn answered(entry: &Entry<'_>) -> (String, String, u32, String) {
    let mut buffer = [core::mem::MaybeUninit::uninit(); 3840];
    let mut reply = Reply::new(&mut buffer);
    answer(&mut reply, entry).unwrap();
    let len = reply.len();
    // SAFETY: `Reply::write` initialised exactly the first `len` slots.
    let written: Vec<u8> = buffer[..len]
        .iter()
        .map(|b| unsafe { b.assume_init() })
        .collect();
    let mut rows = stage::reply_decode(&written).unwrap();
    let Some(Ok(stage::Record::LookupEntry(e))) = rows.next() else {
        panic!("not one lookup.entry")
    };
    (e.title.into(), e.text.into(), e.flags, e.opens.into())
}

#[test]
fn a_provider_that_fetches_answers_pending_and_an_entry_may_open_more() {
    // Pending: an empty text with bit 1, the source's name as the title.
    assert_eq!(
        answered(&Entry::pending("Wikipedia").unwrap()),
        ("Wikipedia".into(), String::new(), PENDING, String::new())
    );
    // The kept answer names the screen that shows the whole article.
    let entry = Entry::new("Rabbit", "A small mammal.")
        .unwrap()
        .opens("wikipedia.article")
        .unwrap();
    assert_eq!(
        answered(&entry),
        (
            "Rabbit".into(),
            "A small mammal.".into(),
            0,
            "wikipedia.article".into()
        )
    );
    assert_eq!(
        Entry::new("t", "x").unwrap().opens("").unwrap_err(),
        Error::Invalid
    );
    let long = "s".repeat(MAX_OPENS + 1);
    assert_eq!(
        Entry::new("t", "x").unwrap().opens(&long).unwrap_err(),
        Error::Invalid
    );
    assert_eq!(
        Entry::pending(&"t".repeat(MAX_TITLE + 1)).unwrap_err(),
        Error::NoSpace
    );
}

#[test]
fn the_asker_reads_the_screen_an_entry_opens() {
    with_host(|host, state| {
        let lookup = Lookup::open(host).unwrap();
        let question = Question::new("rabbit").unwrap();
        let (mut input, mut out) = ([0; BUFFER], [0; BUFFER]);
        reply(
            state,
            &[Record::ServiceLookupEntry(wire::ServiceLookupEntry {
                provider: "wikipedia.summary",
                title: "Rabbit",
                text: "A small mammal.",
                flags: 0,
                opens: "wikipedia.article",
            })],
        );
        let got = lookup
            .ask(host, &question, &mut input, &mut out)
            .unwrap()
            .unwrap();
        assert_eq!(got.opens(), Some("wikipedia.article"));
        reply(state, &[entry(0)]);
        let got = lookup
            .ask(host, &question, &mut input, &mut out)
            .unwrap()
            .unwrap();
        assert_eq!(got.opens(), None);
    });
}

#[test]
fn an_entry_that_would_not_fit_is_refused_not_cut() {
    assert_eq!(
        Entry::new(&"t".repeat(MAX_TITLE + 1), "x").unwrap_err(),
        Error::NoSpace
    );
    assert_eq!(
        Entry::new("t", &"x".repeat(MAX_ENTRY + 1)).unwrap_err(),
        Error::NoSpace
    );
    assert!(Entry::new(&"t".repeat(MAX_TITLE), &"x".repeat(MAX_ENTRY)).is_ok());
    // A full entry fits the host's reply buffer.
    let mut buffer = [core::mem::MaybeUninit::uninit(); 3840];
    let mut reply = Reply::new(&mut buffer);
    answer(
        &mut reply,
        &Entry::new(&"t".repeat(MAX_TITLE), &"x".repeat(MAX_ENTRY)).unwrap(),
    )
    .unwrap();
    // And a reply buffer that is too small says so.
    let mut small = [core::mem::MaybeUninit::uninit(); 8];
    let mut reply = Reply::new(&mut small);
    assert_eq!(
        answer(&mut reply, &Entry::new("t", "text").unwrap()),
        Err(Error::NoSpace)
    );
}

#[test]
fn a_reply_the_host_would_refuse_is_invalid_here_too() {
    with_host(|host, state| {
        let lookup = Lookup::open(host).unwrap();
        let question = Question::new("rabbit").unwrap();
        let (mut input, mut out) = ([0; BUFFER], [0; BUFFER]);
        let mut ask = |rows: &[Record<'_>]| {
            reply(state, rows);
            lookup
                .ask(host, &question, &mut input, &mut out)
                .map(|e| e.is_some())
        };
        assert_eq!(ask(&[entry(0)]), Ok(true));
        // An empty provider id, an unknown flag bit.
        let no_provider = Record::ServiceLookupEntry(wire::ServiceLookupEntry {
            provider: "",
            title: "t",
            text: "x",
            flags: 0,
            opens: "",
        });
        assert_eq!(ask(&[no_provider]), Err(Error::Invalid));
        assert_eq!(ask(&[entry(0b100)]), Err(Error::Invalid));
        // Two rows (a trailing one), or another record.
        assert_eq!(ask(&[entry(0), entry(0)]), Err(Error::Invalid));
        let other = Record::ServiceLookupAsk(wire::ServiceLookupAsk {
            text: "x",
            language: "",
            context: "",
            cursor: 0,
        });
        assert_eq!(ask(&[other]), Err(Error::Invalid));
        // Bytes cut short, or trailing after the one row.
        let mut bytes = vec![0; 4096];
        let n = wire::record_encode_one(&entry(0), &mut bytes).unwrap();
        for bad in [bytes[..n - 1].to_vec(), {
            let mut more = bytes[..n].to_vec();
            more.push(0);
            more
        }] {
            state.reply.replace(bad);
            assert_eq!(
                lookup
                    .ask(host, &question, &mut input, &mut out)
                    .map(|e| e.is_some()),
                Err(Error::Invalid)
            );
        }
    });
}

#[test]
fn a_short_input_buffer_is_refused_before_the_host_is_called() {
    with_host(|host, state| {
        let lookup = Lookup::open(host).unwrap();
        let question = Question::new("rabbit").unwrap();
        reply(state, &[entry(0)]);
        let (mut input, mut out) = ([0; 4], [0; BUFFER]);
        state.op.set(0);
        assert_eq!(
            lookup.ask(host, &question, &mut input, &mut out),
            Err(Error::NoSpace)
        );
        assert_eq!(state.op.get(), 0);
    });
}

#[test]
fn an_answer_that_does_not_fit_writes_nothing_and_keeps_the_prefix() {
    let entry = Entry::new("Dog", "A domestic animal.").unwrap();
    let mut buffer = [core::mem::MaybeUninit::new(7); 16];
    let mut reply = Reply::new(&mut buffer);
    reply.write(b"ab").unwrap();
    assert_eq!(answer(&mut reply, &entry), Err(Error::NoSpace));
    assert_eq!((reply.len(), reply.remaining()), (2, 14));
    // SAFETY: the buffer began initialized.
    let prefix = [0, 1].map(|i| unsafe { buffer[i].assume_init() });
    assert_eq!(&prefix, b"ab");
}

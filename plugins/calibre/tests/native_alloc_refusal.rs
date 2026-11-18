//! Every allocation of the wire format the native plugin writes and reads, and
//! of the session that answers Calibre, may be refused: each ends in `NoMemory`
//! or succeeds, never aborts. (The glue of `native_plugin.rs` needs a host and
//! is not swept: `PORTING-7.md`.)
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
mod fake;
use paperpaw_plugin_calibre::native_model::{self as model, Device, Framing, frame, op};
use paperpaw_plugin_calibre::native_session::{Placed, Session};

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

const WIRE: &str = "29[8, {\"lpath\": \"a/b.epub\", \"x\": 1}]";
const EXTENSIONS: [&str; 2] = ["epub", "txt"];

#[test]
fn frames_and_strings_survive_refused_allocations() {
    fail_alloc::sweep(|| {
        model::frame(8, "{\"ok\": true}")?;
        model::json_string("a \"quoted\" \n line")?;
        Ok(())
    });
}

#[test]
fn the_framing_survives_refused_allocations() {
    fail_alloc::sweep(|| {
        let mut framing = Framing::default();
        match framing.feed(WIRE) {
            Ok(Some(_)) => Ok(()),
            Ok(None) | Err(_) => Err(paperpaw_sdk::native::Error::NoMemory),
        }
    });
}

#[test]
fn the_answers_survive_refused_allocations() {
    fail_alloc::sweep(|| {
        model::initialization(&Device {
            name: "Reader",
            extensions: &EXTENSIONS,
            challenge: "abc",
            password: Some("pw"),
            version: "1.0.9",
        })?;
        model::device_information(Some("{\"k\": 1}"), "Reader", "1.0.9")?;
        model::device_information(None, "Reader", "1.0.9")?;
        Ok(())
    });
}

#[test]
fn paths_and_messages_survive_refused_allocations() {
    fail_alloc::sweep(|| {
        model::safe_path("dir/sub/book.epub")?;
        model::book_path("dir\\sub\\book.EPUB", &EXTENSIONS)?;
        model::parse("[3, {\"a\": 1}]")?;
        Ok(())
    });
}

/// Calibre says `op` with `args` to `session`, in the pieces the session asks for.
fn says(
    session: &mut Session,
    device: &mut fake::Quiet,
    op: u8,
    args: &str,
) -> Result<Vec<paperpaw_plugin_calibre::native_session::Step>, paperpaw_sdk::native::Error> {
    let wire = frame(op, args)?;
    let text = core::str::from_utf8(&wire).map_err(|_| paperpaw_sdk::native::Error::Utf8)?;
    let mut rest = text;
    let mut last = Vec::new();
    while !rest.is_empty() {
        let n = (session.wanted() as usize).min(rest.len());
        let (part, tail) = rest.split_at(n);
        rest = tail;
        last = session.received(device, part)?;
    }
    Ok(last)
}

#[test]
fn a_session_survives_refused_allocations() {
    // The frames are made outside: only the session's allocations are refused
    // (the test's own `String`s would abort).
    fail_alloc::sweep(|| {
        let mut device = fake::Quiet;
        let mut session = Session::new(64 * 1024, true);
        session.start(64 * 1024, "")?;
        session.answer(
            &device,
            "calibre wireless device client (on pc);9090",
            "10.0.0.2",
        )?;
        session.connected()?;
        says(
            &mut session,
            &mut device,
            op::GET_INITIALIZATION_INFO,
            "{\"passwordChallenge\": \"abc\"}",
        )?;
        says(
            &mut session,
            &mut device,
            op::SET_CALIBRE_DEVICE_INFO,
            "{\"k\": 1}",
        )?;
        says(&mut session, &mut device, op::GET_DEVICE_INFORMATION, "{}")?;
        says(&mut session, &mut device, op::FREE_SPACE, "{}")?;
        says(
            &mut session,
            &mut device,
            op::SET_LIBRARY_INFO,
            "{\"libraryName\": \"L\"}",
        )?;
        says(&mut session, &mut device, op::GET_BOOK_COUNT, "{}")?;
        while session.listing() {
            session.list_part(&device)?;
        }
        let steps = says(
            &mut session,
            &mut device,
            op::SEND_BOOK,
            "{\"lpath\": \"Ann/A.epub\", \"length\": 10, \"metadata\": {\"title\": \"A\"}}",
        )?;
        let _ = steps;
        for step in session.saved()? {
            if let paperpaw_plugin_calibre::native_session::Step::Import(coming) = step {
                session.imported(&mut device, &coming, Ok(()))?;
                session.imported(&mut device, &coming, Err(Placed::NotReplaced))?;
            }
        }
        says(
            &mut session,
            &mut device,
            op::DELETE_BOOK,
            "{\"lpaths\": [\"Ann/A.epub\", \"B/b.epub\"]}",
        )?;
        says(&mut session, &mut device, op::ERROR, "{\"message\": \"x\"}")?;
        session.closed(Some("lost"))?;
        Ok(())
    });
}

#[test]
fn strings_of_an_array_survive_refused_allocations() {
    fail_alloc::sweep(|| {
        model::strings("{\"lpaths\": [\"a\", 1, \"b\\\"c\"]}", "lpaths")?;
        model::text("{\"title\": \"T\\u00e9\"}", "title")?;
        model::without_member("{\"x\": 1, \"thumbnail\": [1,2], \"y\": 2}", "thumbnail")?;
        Ok(())
    });
}

//! Every allocation of the wire format the native plugin writes and reads may
//! be refused: each ends in `NoMemory` or succeeds, never aborts. (Members of
//! the JSON are read by the interim reader of `paperpaw-json`, which has no
//! refusal path: it is not swept here.)
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_calibre::native_model::{self as model, Device, Framing};

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

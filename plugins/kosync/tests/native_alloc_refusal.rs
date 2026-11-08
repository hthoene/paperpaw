//! Every allocation of the address, key and body the sync writes, and of the
//! answers it reads, may be refused: each ends in `NoMemory` or succeeds,
//! never aborts. (The answer's JSON is read by the interim reader of
//! `paperpaw-json`, which has no refusal path: it is not swept here.)
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_kosync::native_model as model;

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[test]
fn the_writers_survive_refused_allocations() {
    fail_alloc::sweep(|| {
        for text in ["", "sync.example.org/", "http://x:8080", "https://a.b//"] {
            model::normalize_server(text)?;
        }
        model::password_key("secret")?;
        model::json_string("a \"quoted\" \n line \u{1}")?;
        model::progress_body("d41d8cd9", "/body/DocFragment[3]", 0.3333, "Reader", "ID")?;
        Ok(())
    });
}

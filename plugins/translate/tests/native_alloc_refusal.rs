//! Every allocation of the language, the request and the answer read may be
//! refused: each ends in `NoMemory` or succeeds, never aborts.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_translate::native_model as model;

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[test]
fn the_request_and_the_answer_survive_refused_allocations() {
    fail_alloc::sweep(|| {
        for (to, ui) in [
            ("translate.to.pt-br", "de"),
            ("translate.to.auto", "de-AT"),
            ("translate.to.auto", "en"),
            ("translate.to.auto", "xx"),
        ] {
            model::target(to, ui)?;
        }
        for key in ["abc:fx", "abc"] {
            model::url(key)?;
            let secret = model::authorization(key)?;
            assert!(secret.as_str().ends_with(key));
        }
        model::body("Hello world, \u{e4}\u{4e16} & more", "DE")?;
        let answer =
            r#"{"translations":[{"detected_source_language":"EN","text":"Hallo \"Welt\" ä"}]}"#;
        for (status, body) in [
            (200, answer),
            (200, "{}"),
            (401, ""),
            (456, ""),
            (0, ""),
            (500, ""),
        ] {
            model::outcome(status, body)?;
        }
        Ok(())
    });
}

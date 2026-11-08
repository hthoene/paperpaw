//! Every allocation of the addresses and of the summary read may be refused:
//! each ends in `NoMemory` or succeeds, never aborts.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_wikipedia::native_model as model;

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[test]
fn the_addresses_and_the_summary_survive_refused_allocations() {
    fail_alloc::sweep(|| {
        for language in ["", "de-DE", "pt_BR"] {
            model::wiki(language)?;
            for word in ["Hund", "white rabbit", "Ärger & Öl"] {
                model::summary_url(language, word)?;
                model::article_url(language, word)?;
            }
        }
        for (status, body) in [
            (
                200,
                r#"{"title":"Hund","extract":"Der Hund ist ein Haustier."}"#,
            ),
            (200, r#"{"title":"A ä name","extract":"Line\nbreak"}"#),
            (200, r#"{"extract":"   "}"#),
            (404, ""),
            (500, ""),
        ] {
            model::outcome(status, body)?;
        }
        Ok(())
    });
}

//! Every allocation the sync engine makes may be refused: each run ends in
//! `NoMemory` or succeeds, never aborts, and the engine is usable afterwards.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_kosync::native_model::Remote;
use paperpaw_plugin_kosync::native_sync::{Answer, Answered, Ctx, Engine, Position};

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[test]
fn the_engine_survives_refused_allocations() {
    let further = Remote {
        progress: 0.7,
        device_id: Some("other".into()),
        device: Some("Reader".into()),
        xpointer: Some("/body/DocFragment[2]".into()),
    };
    let same = Remote {
        progress: 0.1,
        ..Remote::clone(&further)
    };
    fail_alloc::sweep(|| {
        let mut e = Engine::new("me")?;
        let mut out = Vec::new();
        let c = Ctx::new(0);
        let mut off = Ctx::new(0);
        off.online = false;
        e.book_opened(off, Position::new("a.epub", (1, 0), 0.4)?, &mut out)?;
        e.book_opened(off, Position::new("b.epub", (1, 0), 0.1)?, &mut out)?;
        e.allowed(c, &mut out)?;
        e.pulled(
            c,
            &Answer::new("a.epub", false, 0.4, 200, Some(&same)),
            &mut out,
        )?;
        e.page_turned(c, Position::new("b.epub", (1, 5), 0.2)?, &mut out)?;
        e.pulled(
            c,
            &Answer::new("b.epub", false, 0.1, 200, Some(&further)),
            &mut out,
        )?;
        e.answered(
            c,
            &Answered {
                path: "b.epub",
                percentage: "0.7",
                went: false,
                stay: false,
            },
            &mut out,
        )?;
        e.answered(
            c,
            &Answered {
                path: "b.epub",
                percentage: "0.7",
                went: false,
                stay: true,
            },
            &mut out,
        )?;
        e.pulled(c, &Answer::new("b.epub", true, 0.2, 500, None), &mut out)?;
        e.sync_now(c, &mut out)?;
        e.resumed(c, &mut out)?;
        e.book_closed(c, Position::new("b.epub", (2, 0), 0.3)?, &mut out)?;
        Ok(())
    });
}

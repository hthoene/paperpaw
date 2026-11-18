//! Every allocation the sync engine makes may be refused: each run ends in
//! `NoMemory` or succeeds, never aborts, and the engine is usable afterwards.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_kosync::native_model::Remote;
use paperpaw_plugin_kosync::native_sync::{Answer, Answered, Ctx, Engine, Position};

use std::{
    alloc::{GlobalAlloc, Layout},
    cell::Cell,
};

std::thread_local! {
    static LIMIT: Cell<usize> = const { Cell::new(usize::MAX) };
}

struct Limited;

// SAFETY: delegates to the existing allocator or refuses without changing the allocation.
unsafe impl GlobalAlloc for Limited {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() > LIMIT.with(Cell::get) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller's layout is forwarded unchanged.
        unsafe { fail_alloc::Failing.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if layout.size() > LIMIT.with(Cell::get) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller's layout is forwarded unchanged.
        unsafe { fail_alloc::Failing.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the pointer and layout came from the delegated allocator.
        unsafe { fail_alloc::Failing.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if size > LIMIT.with(Cell::get) {
            return std::ptr::null_mut();
        }
        // SAFETY: the pointer, layout and new size are forwarded unchanged.
        unsafe { fail_alloc::Failing.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOC: Limited = Limited;

#[test]
fn a_many_member_reply_uses_only_small_fallible_allocations() {
    use paperpaw_plugin_kosync::native_model::{MAX_ANSWER, read_answer, remote_progress};
    let mut text = String::from(r#"{"percentage":0.8"#);
    while text.len() + 6 <= MAX_ANSWER {
        text.push_str(r#","":0"#);
    }
    text.push('}');
    while text.len() < MAX_ANSWER {
        text.push(' ');
    }
    assert_eq!(text.len(), MAX_ANSWER);
    LIMIT.with(|l| l.set(4096));
    let answer = read_answer(&text);
    let progress = remote_progress(&text);
    LIMIT.with(|l| l.set(usize::MAX));
    assert_eq!(answer.unwrap().unwrap().progress, 0.8);
    assert_eq!(progress.unwrap(), Some(0.8));
    fail_alloc::sweep(|| {
        read_answer(r#"{"percentage":0.8,"device":"Other","device_id":"id","progress":"/body"}"#)?;
        Ok(())
    });
}

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

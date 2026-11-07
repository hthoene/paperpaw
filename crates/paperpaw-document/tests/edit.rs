//! Fallible edits of a block (`Block::try_transform`, `TextEdit`):
//! refusals, a limit reached and a filter's own error leave the block
//! unchanged, and every allocation an edit makes fails cleanly where the
//! heap refuses it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_document::{Align, Block, BlockKind, EditError, ImageRef, OffsetMap, Run, TextStyle};

/// Fails the n-th allocation of this thread from now (0: none), so each
/// allocation of an edit can be refused in turn.
struct Refusing;

thread_local! {
    static FAIL_IN: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: forwards to the system allocator, or returns null (a refusal),
// which callers of `GlobalAlloc` must handle.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let refuse = FAIL_IN
            .try_with(|n| match n.get() {
                0 => false,
                1 => {
                    n.set(0);
                    true
                }
                k => {
                    n.set(k - 1);
                    false
                }
            })
            .unwrap_or(false);
        if refuse {
            return core::ptr::null_mut();
        }
        // SAFETY: as the caller's contract for `alloc`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `System.alloc` with `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A growth counts as an allocation.
        let refuse = FAIL_IN
            .try_with(|n| match n.get() {
                0 => false,
                1 => {
                    n.set(0);
                    true
                }
                k => {
                    n.set(k - 1);
                    false
                }
            })
            .unwrap_or(false);
        if refuse {
            return core::ptr::null_mut();
        }
        // SAFETY: as the caller's contract for `realloc`.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

fn block() -> Block {
    let mut map = OffsetMap::default();
    map.checkpoint(3, 20);
    Block::new(BlockKind::Paragraph, 10, 40, String::from("ab cdé fgh"))
        .map(map)
        .runs(vec![
            Run::new(0, 2, TextStyle::BOLD),
            Run::new(3, 7, TextStyle::ITALIC),
        ])
}

/// The steps of a caption filter: text replaced, grown, runs set anew,
/// the picture made a centred paragraph.
fn caption(edit: &mut paperpaw_document::TextEdit) -> Result<(), EditError> {
    edit.try_replace(3..7, "a much longer text than before")?;
    edit.try_replace(0..2, "")?;
    edit.try_set_runs(&[Run::new(0, 4, TextStyle::BOLD)])?;
    edit.try_push_run(Run::new(5, 6, TextStyle::ITALIC))?;
    edit.set_kind(BlockKind::Paragraph);
    edit.set_align(Align::Center);
    edit.drop_image();
    Ok(())
}

fn picture() -> Block {
    let mut block = block();
    block.kind = BlockKind::Image;
    block.image = Some(ImageRef {
        path: String::from("cache/img-0.gray"),
        width: 4,
        height: 3,
    });
    block
}

#[test]
fn every_allocation_of_an_edit_can_fail_cleanly() {
    let original = picture();
    for n in 1..32 {
        let mut target = original.clone();
        FAIL_IN.with(|f| f.set(n));
        let result = target.try_transform(4096, caption);
        let unused = FAIL_IN.with(|f| f.replace(0));
        match result {
            Err(error) => {
                assert_eq!(error, EditError::NoMemory, "allocation {n}");
                assert_eq!(target, original, "allocation {n}");
            }
            Ok(()) => {
                // Fewer allocations than n: none was refused.
                assert_ne!(unused, 0, "allocation {n}");
                assert_eq!(target.text, " a much longer text than before fgh");
                assert_eq!(
                    (target.kind, target.align),
                    (BlockKind::Paragraph, Align::Center)
                );
                assert!(target.image.is_none());
                return;
            }
        }
    }
    panic!("the edit makes more allocations than the test refuses");
}

#[test]
fn a_limit_refuses_before_anything_grows() {
    let original = picture();
    // Too small for the copy itself.
    let mut target = original.clone();
    assert_eq!(target.try_transform(4, caption), Err(EditError::Limit));
    assert_eq!(target, original);
    // Room for the copy, not for the longer text: refused at that step,
    // before it reserves anything.
    let mut target = original.clone();
    let result = target.try_transform(64, |edit| {
        let before = edit.heap_bytes();
        let refused = edit.try_replace(3..7, "a much longer text than before, and longer still");
        assert_eq!(edit.heap_bytes(), before);
        refused
    });
    assert_eq!(result, Err(EditError::Limit));
    assert_eq!(target, original);
    // Repeated doubling stops at the limit, never past it.
    let mut target = Block::new(BlockKind::Paragraph, 0, 2, String::from("aa"));
    let limit = 1024;
    let result = target.try_transform(limit, |edit| {
        loop {
            let len = edit.text().len();
            let doubled = edit.text().to_string();
            edit.try_replace(len..len, &doubled)?;
            assert!(edit.heap_bytes() <= limit);
        }
    });
    assert_eq!(result, Err::<(), _>(EditError::Limit));
    assert_eq!(target.text, "aa");
}

/// A filter's own error, after steps that succeeded, leaves the block as
/// it was.
#[test]
fn a_filter_error_keeps_the_block() {
    #[derive(Debug, PartialEq)]
    enum Mine {
        Edit(EditError),
        Rule,
    }
    impl From<EditError> for Mine {
        fn from(error: EditError) -> Self {
            Mine::Edit(error)
        }
    }
    let original = picture();
    let mut target = original.clone();
    let result = target.try_transform(4096, |edit| {
        caption(edit)?;
        Err::<(), _>(Mine::Rule)
    });
    assert_eq!(result, Err(Mine::Rule));
    assert_eq!(target, original);
}

#[test]
fn refusals_leave_the_copy_unchanged() {
    let mut block = block();
    let original = block.clone();
    let result = block.try_transform(4096, |edit| {
        let text = edit.text().to_string();
        let runs = edit.runs().to_vec();
        // Off a character boundary (inside "é"), past the end, reversed.
        assert_eq!(edit.try_replace(5..6, "x"), Err(EditError::Range));
        assert_eq!(edit.try_replace(0..99, "x"), Err(EditError::Range));
        assert_eq!(
            edit.try_replace(core::ops::Range { start: 4, end: 2 }, "x"),
            Err(EditError::Range)
        );
        // Runs out of order, overlapping, empty, past the text.
        assert_eq!(
            edit.try_set_runs(&[
                Run::new(3, 4, TextStyle::BOLD),
                Run::new(0, 1, TextStyle::BOLD)
            ]),
            Err(EditError::Range)
        );
        assert_eq!(
            edit.try_set_runs(&[
                Run::new(0, 4, TextStyle::BOLD),
                Run::new(3, 5, TextStyle::BOLD)
            ]),
            Err(EditError::Range)
        );
        assert_eq!(
            edit.try_push_run(Run::new(8, 8, TextStyle::BOLD)),
            Err(EditError::Range)
        );
        assert_eq!(
            edit.try_push_run(Run::new(8, 99, TextStyle::BOLD)),
            Err(EditError::Range)
        );
        assert_eq!(
            edit.try_push_run(Run::new(1, 2, TextStyle::BOLD)),
            Err(EditError::Range)
        );
        assert_eq!((edit.text(), edit.runs()), (text.as_str(), runs.as_slice()));
        assert!(edit.heap_bytes() >= text.len() && edit.limit() == 4096);
        Ok::<(), EditError>(())
    });
    assert_eq!(result, Ok(()));
    assert_eq!(block, original);
}

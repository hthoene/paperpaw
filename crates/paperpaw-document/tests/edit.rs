//! Fallible edits of a block (`Block::try_transform`, `TextEdit`):
//! refusals, a limit reached and a filter's own error leave the block
//! unchanged, every allocation an edit makes fails cleanly where the
//! heap refuses it, and the heap an edit takes stays under its limit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use paperpaw_document::{Align, Block, BlockKind, EditError, ImageRef, OffsetMap, Run, TextStyle};

/// Fails the n-th allocation of this thread from now (0: none), so each
/// allocation of an edit can be refused in turn, and counts this thread's
/// allocation attempts and the bytes it holds.
struct Refusing;

thread_local! {
    static FAIL_IN: Cell<usize> = const { Cell::new(0) };
    static ATTEMPTS: Cell<usize> = const { Cell::new(0) };
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

/// Counts an attempt to allocate `bytes` more.
fn attempt() {
    let _ = ATTEMPTS.try_with(|n| n.set(n.get() + 1));
}

/// Adds `delta` to the bytes this thread holds.
fn hold(delta: isize) {
    let _ = LIVE.try_with(|live| {
        live.set(live.get() + delta);
        let _ = PEAK.try_with(|peak| peak.set(peak.get().max(live.get())));
    });
}

/// The allocation attempts and the most bytes held above the start while
/// `f` runs.
fn observe<T>(f: impl FnOnce() -> T) -> (T, usize, usize) {
    let attempts = ATTEMPTS.with(Cell::get);
    let base = LIVE.with(Cell::get);
    PEAK.with(|peak| peak.set(base));
    let value = f();
    let peak = PEAK.with(Cell::get) - base;
    (
        value,
        ATTEMPTS.with(Cell::get) - attempts,
        usize::try_from(peak).unwrap(),
    )
}

// SAFETY: forwards to the system allocator, or returns null (a refusal),
// which callers of `GlobalAlloc` must handle.
unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        attempt();
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
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            hold(layout.size() as isize);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        hold(-(layout.size() as isize));
        // SAFETY: `ptr` came from `System.alloc` with `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A growth counts as an allocation.
        attempt();
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
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            // Counted as a move: the old room is held until the new one
            // is there.
            hold(new_size as isize);
            hold(-(layout.size() as isize));
        }
        new
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

/// A refusal at the limit attempts no allocation at all: the copy, a
/// longer text, new runs and the runs' growth are checked first.
#[test]
fn a_limit_refusal_attempts_no_allocation() {
    let mut target = picture();
    let (result, attempts, _) = observe(|| target.try_transform(4, caption));
    assert_eq!((result, attempts), (Err(EditError::Limit), 0));
    let long = "a much longer text than before, and longer still";
    // Sorted runs on the character boundaries of "ab cdé fgh".
    let runs = [0, 1, 2, 3, 4, 7, 8, 9].map(|at| Run::new(at, at + 1, TextStyle::BOLD));
    let mut target = picture();
    let result = target.try_transform(64, |edit| {
        let (refused, attempts, _) = observe(|| edit.try_replace(3..7, long));
        assert_eq!((refused, attempts), (Err(EditError::Limit), 0));
        let (refused, attempts, _) = observe(|| edit.try_set_runs(&runs));
        assert_eq!((refused, attempts), (Err(EditError::Limit), 0));
        // Fill the runs' room, then the growth is refused unattempted.
        edit.try_set_runs(&[Run::new(0, 1, TextStyle::BOLD)])?;
        let mut at = 1;
        let refused = loop {
            let run = Run::new(at, at + 1, TextStyle::ITALIC);
            let (pushed, attempts, _) = observe(|| edit.try_push_run(run));
            match pushed {
                Ok(()) => at += 1,
                Err(error) => {
                    assert_eq!(attempts, 0);
                    break error;
                }
            }
        };
        assert_eq!(refused, EditError::Limit);
        Ok::<(), EditError>(())
    });
    assert_eq!(result, Ok(()));
}

/// The heap an edit takes at its peak, as the allocator sees it, stays
/// under the smallest limit the edit succeeds with.
#[test]
fn the_heap_an_edit_takes_stays_under_its_limit() {
    let original = picture();
    let limit = (0..4096)
        .find(|&limit| original.clone().try_transform(limit, caption).is_ok())
        .unwrap();
    let mut target = original.clone();
    let (result, _, peak) = observe(|| target.try_transform(limit, caption));
    assert_eq!(result, Ok(()));
    assert!(peak <= limit, "peak {peak} over the limit {limit}");
}

/// Text that doubles and runs that grow until the limit refuses, without
/// the test's own allocations: the allocator's peak never passes it.
#[test]
fn growth_until_the_limit_stays_under_it() {
    static LONG: &str = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklm\
        nopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz";
    for limit in [64, 100, 256, 333, 1024] {
        let mut target = Block::new(BlockKind::Paragraph, 0, 2, String::from("ab"));
        let (result, _, peak) = observe(|| {
            target.try_transform(limit, |edit| {
                loop {
                    let len = edit.text().len();
                    edit.try_replace(len..len, &LONG[..len.min(LONG.len())])?;
                }
            })
        });
        assert_eq!(result, Err::<(), _>(EditError::Limit));
        assert!(peak <= limit, "text: peak {peak} over the limit {limit}");
        let mut target = Block::new(BlockKind::Paragraph, 0, 999, LONG.to_string());
        let (result, _, peak) = observe(|| {
            target.try_transform(limit, |edit| {
                for at in 0.. {
                    edit.try_push_run(Run::new(at, at + 1, TextStyle::BOLD))?;
                }
                Ok(())
            })
        });
        assert_eq!(result, Err::<(), _>(EditError::Limit));
        assert!(peak <= limit, "runs: peak {peak} over the limit {limit}");
    }
}

/// Source offsets past `u32::MAX`, from a hostile block or from text that
/// grows near the top, are refused as `Range` before anything changes;
/// the copy and the block stay as they were.
#[test]
fn source_offsets_past_u32_are_refused() {
    let at_top = Block::new(BlockKind::Paragraph, u32::MAX, u32::MAX, String::from("ab"));
    let mut map = OffsetMap::default();
    map.checkpoint(0, u32::MAX);
    let mapped_top = Block::new(BlockKind::Paragraph, 0, u32::MAX, String::from("ab")).map(map);
    // Indices 0 and 1 map to MAX - 2 and MAX - 1: valid until text grows.
    let near_top = Block::new(
        BlockKind::Paragraph,
        u32::MAX - 2,
        u32::MAX,
        String::from("ab"),
    );
    for (original, with) in [(&at_top, "x"), (&mapped_top, "x"), (&near_top, "xyz")] {
        let mut target = original.clone();
        let result = target.try_transform(4096, |edit| {
            let heap = edit.heap_bytes();
            assert_eq!(edit.try_replace(1..1, with), Err(EditError::Range));
            assert_eq!((edit.text(), edit.heap_bytes()), ("ab", heap));
            Ok::<(), EditError>(())
        });
        assert_eq!(result, Ok(()));
        assert_eq!(&target, original);
        assert_eq!(
            target.try_transform(4096, |edit| edit.try_replace(1..1, with)),
            Err(EditError::Range)
        );
        assert_eq!(&target, original);
        // The infallible path refuses the same way, and reading the
        // hostile map saturates.
        target.replace(1..1, with);
        assert_eq!(&target, original);
        let _ = target.source_offset(1);
    }
    // Valid offsets that a first step would push past the top: the step
    // itself is refused, so no later step meets an overflowing map.
    let mut target = near_top.clone();
    let result = target.try_transform(4096, |edit| {
        assert_eq!(edit.try_replace(0..1, "xxxxx"), Err(EditError::Range));
        edit.try_replace(0..1, "xx")?;
        assert_eq!(edit.source_offset(1), u32::MAX - 1);
        // At the end the new text starts at `end` (MAX): one byte fits.
        assert_eq!(edit.try_replace(3..3, "zz"), Err(EditError::Range));
        edit.try_replace(3..3, "z")?;
        assert_eq!(edit.source_offset(3), u32::MAX);
        Ok::<(), EditError>(())
    });
    assert_eq!(result, Ok(()));
    assert_eq!(target.text, "xxbz");
    // Up to the top itself is fine.
    let mut target = near_top.clone();
    assert_eq!(
        target.try_transform(4096, |edit| edit.try_replace(1..1, "xy")),
        Ok(())
    );
    assert_eq!(target.text, "axyb");
    assert_eq!(target.source_offset(2), u32::MAX);
    assert_eq!(mapped_top.source_offset(1), u32::MAX);
}

/// A whole text set anew maps from the block's own start, the end to the
/// block's end, whatever the old map said; a replacement of the whole
/// text keeps the old map's first offset instead.
#[test]
fn a_text_set_anew_maps_from_the_block_start() {
    let mut map = OffsetMap::default();
    map.checkpoint(0, 40);
    let mut alt = Block::new(BlockKind::Image, 4, 90, String::from("an alt text"))
        .map(map)
        .runs(vec![Run::new(0, 2, TextStyle::BOLD)]);
    let empty = Block::new(BlockKind::Image, 4, 90, String::new());
    for mut block in [alt.clone(), empty] {
        block
            .try_transform(4096, |edit| {
                edit.try_set_text("Caption")?;
                assert!(edit.runs().is_empty());
                assert_eq!(edit.source_offset(0), 4);
                assert_eq!(edit.source_offset(3), 7);
                assert_eq!(edit.source_offset(7), 90);
                edit.try_set_runs(&[Run::new(0, 7, TextStyle::ITALIC)])
            })
            .unwrap();
        assert_eq!(block.text, "Caption");
        assert_eq!(block.map, OffsetMap::IDENTITY);
        assert_eq!((block.source_offset(0), block.source_offset(6)), (4, 10));
        assert_eq!(block.source_offset(7), 90);
    }
    // The replacement of the whole text keeps the old map's start.
    alt.try_transform(4096, |edit| {
        let len = edit.text().len();
        edit.try_replace(0..len, "Caption")?;
        assert_eq!(edit.source_offset(0), 40);
        Ok::<(), EditError>(())
    })
    .unwrap();
}

#[test]
fn a_text_set_anew_is_all_or_nothing() {
    let original = picture();
    // Every allocation it makes can fail, the block unchanged.
    for n in 1..16 {
        let mut target = original.clone();
        FAIL_IN.with(|f| f.set(n));
        let result = target.try_transform(4096, |edit| {
            edit.try_set_text("a caption longer than the text before")
        });
        let unused = FAIL_IN.with(|f| f.replace(0));
        match result {
            Err(error) => {
                assert_eq!(error, EditError::NoMemory, "allocation {n}");
                assert_eq!(target, original, "allocation {n}");
            }
            Ok(()) => {
                assert_ne!(unused, 0, "allocation {n}");
                assert_eq!(target.text, "a caption longer than the text before");
                break;
            }
        }
    }
    // A limit refuses before anything is reserved, the copy unchanged.
    let mut target = original.clone();
    let result = target.try_transform(64, |edit| {
        let (before, text) = (edit.heap_bytes(), edit.text().to_string());
        let refused = edit.try_set_text(&"x".repeat(200));
        assert_eq!((edit.heap_bytes(), edit.text()), (before, text.as_str()));
        refused
    });
    assert_eq!(result, Err(EditError::Limit));
    assert_eq!(target, original);
    // Source offsets past `u32::MAX` are refused.
    let mut near = Block::new(
        BlockKind::Paragraph,
        u32::MAX - 2,
        u32::MAX,
        String::from("ab"),
    );
    let kept = near.clone();
    let result = near.try_transform(4096, |edit| edit.try_set_text("abcd"));
    assert_eq!(result, Err(EditError::Range));
    assert_eq!(near, kept);
}

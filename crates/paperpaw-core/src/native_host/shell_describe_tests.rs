//! Slot 42's shell wrapper checks the label before anything else
//! (`screens-input.md` 6.1, Code Reviewer on #1166): longer than 256 bytes is
//! `ERR_INVALID`, not UTF-8 is `ERR_UTF8`; a fitting label reaches the canvas,
//! which refuses outside a draw (`ERR_STATE`).
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use paperpaw_native::host_glue::{ERR_STATE, ERR_UTF8};

#[test]
fn a_long_or_broken_label_is_refused_before_the_canvas_is_asked() {
    reset(NORMAL);
    let hosted = fake();
    let batch = hosted
        .construct_registered(hosted.begin().unwrap())
        .unwrap();
    let context = hosted.owner().context();
    let ctx = ptr::from_ref(context.get_ref()).cast_mut().cast::<c_void>();
    let describe = |label: &[u8]| {
        let pair = PpBytes {
            ptr: label.as_ptr(),
            len: label.len(),
        };
        // SAFETY: the pinned context of a live owner, a valid byte pair.
        unsafe { (context.table.canvas_describe)(ctx, 2, 0, pair, 0, 0, 8, 8) }
    };
    assert_eq!(describe(&[b'a'; 257]), ERR_INVALID);
    assert_eq!(describe(&[b'a', 0xff, b'b']), ERR_UTF8);
    assert_eq!(
        describe(&[b'a'; 256]),
        ERR_STATE,
        "fits; no draw is running"
    );
    drop(batch);
}

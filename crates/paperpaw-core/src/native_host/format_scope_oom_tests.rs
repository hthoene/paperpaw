//! Each allocation of entering a format call refuses: no entry is left and
//! the call is not made.
#![allow(clippy::unwrap_used, reason = "fixture refusal must fail the test")]
use super::{NORMAL, arm_failure, reset};
use crate::native_host::format_scope::{BookKind, FormatBook, FormatScope};
use paperpaw_native::host_glue::ERR_NO_MEMORY;

#[test]
fn entering_a_format_call_refuses_cleanly_for_every_allocation() {
    reset(NORMAL);
    let scope = FormatScope::new();
    let book = FormatBook {
        path: "books/a/",
        kind: BookKind::Siblings,
        opened: "p1.png",
        document: 3,
        cache: "",
        css: "",
        cache_cap: 0,
        quota: None,
    };
    // The path and the opened name.
    for nth in 1..=2 {
        arm_failure(nth);
        let result = scope.enter(5, &book);
        arm_failure(0);
        assert_eq!(result.err(), Some(ERR_NO_MEMORY), "allocation {nth}");
        assert!(
            !scope.current(5, &mut |_| ()),
            "allocation {nth} left an entry"
        );
    }
    let guard = scope.enter(5, &book).unwrap();
    assert!(scope.current(5, &mut |_| ()));
    drop(guard);
    assert!(!scope.current(5, &mut |_| ()));
}

//! Real allocator refusals for each allocation site in the declaration stream.
#![allow(clippy::unwrap_used, reason = "test setup and checks fail loudly")]

use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, ExtraDecl, FontExtensionDecl, FontFaceDecl, GroupDecl, Kind,
    SpecDecl,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Refusing;

thread_local! {
    static UNTIL_FAILURE: Cell<Option<usize>> = const { Cell::new(None) };
    static FAILED: Cell<bool> = const { Cell::new(false) };
}

fn refuse() -> bool {
    UNTIL_FAILURE
        .try_with(|remaining| match remaining.get() {
            Some(1) => {
                remaining.set(None);
                FAILED.with(|failed| failed.set(true));
                true
            }
            Some(count) => {
                remaining.set(Some(count - 1));
                false
            }
            None => false,
        })
        .unwrap_or(false)
}

unsafe impl GlobalAlloc for Refusing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refuse() {
            core::ptr::null_mut()
        } else {
            unsafe { System.alloc(layout) }
        }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if refuse() {
            core::ptr::null_mut()
        } else {
            unsafe { System.realloc(pointer, layout, size) }
        }
    }
}

#[global_allocator]
static ALLOCATOR: Refusing = Refusing;

fn fail_allocation<T>(number: usize, operation: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            UNTIL_FAILURE.with(|remaining| remaining.set(None));
        }
    }
    assert!(number > 0);
    FAILED.with(|failed| failed.set(false));
    UNTIL_FAILURE.with(|remaining| remaining.set(Some(number)));
    let reset = Reset;
    let result = operation();
    drop(reset);
    assert!(
        FAILED.with(Cell::get),
        "allocation failure was not exercised"
    );
    result
}

fn assert_no_memory<F: FnMut(&[u8]) -> Result<(), i32>>(builder: &mut Declarations<F>) {
    let expected = Err(DeclareError::NoMemory);
    assert_eq!(builder.submit(), expected);
    assert_eq!(builder.group(GroupDecl::new("", "")).map(|_| ()), expected);
    assert_eq!(
        builder
            .spec(SpecDecl::new("", Kind::Bool(false), "", ""))
            .map(|_| ()),
        expected
    );
    assert_eq!(builder.extra(ExtraDecl::new("")).map(|_| ()), expected);
    assert_eq!(builder.hyphenation("", "").map(|_| ()), expected);
    assert_eq!(
        builder.font_face(FontFaceDecl::new("", "")).map(|_| ()),
        expected
    );
    assert_eq!(
        builder
            .font_extension(FontExtensionDecl::new("", "", ""))
            .map(|_| ()),
        expected
    );
    assert_eq!(builder.submit(), expected);
}

#[test]
fn batch_allocation_failure_is_latched_without_calling_the_sink() {
    let calls = Cell::new(0);
    let mut builder = fail_allocation(1, || {
        Declarations::new(|batch| {
            assert!(batch.is_empty());
            calls.set(calls.get() + 1);
            Ok(())
        })
    });
    assert_no_memory(&mut builder);
    assert!(builder.is_empty());
    drop(builder);
    assert_eq!(calls.get(), 1);
}

#[test]
fn default_and_choices_allocation_failures_abandon_pending_batches() {
    for allocation in [1, 2] {
        let calls = Cell::new(0);
        let mut builder = Declarations::new(|batch| {
            assert!(batch.is_empty());
            calls.set(calls.get() + 1);
            Ok(())
        });
        builder.group(GroupDecl::new("page", "x.page")).unwrap();
        let error = fail_allocation(allocation, || {
            builder
                .spec(SpecDecl::new(
                    "choice",
                    Kind::Choice {
                        default: "a",
                        choices: &["a", "b"],
                    },
                    "x.page",
                    "x.choice",
                ))
                .map(|_| ())
        });
        assert_eq!(error, Err(DeclareError::NoMemory));
        assert_no_memory(&mut builder);
        assert_eq!(builder.len(), 1);
        drop(builder);
        assert_eq!(calls.get(), 1);
    }
}

#[test]
fn sink_no_memory_status_remains_a_sink_error() {
    let calls = Cell::new(0);
    let mut builder = Declarations::new(|_| {
        calls.set(calls.get() + 1);
        Err(-4)
    });
    builder.group(GroupDecl::new("page", "x.page")).unwrap();
    assert_eq!(builder.submit(), Err(DeclareError::Sink(-4)));
    assert_eq!(
        builder.group(GroupDecl::new("", "")).map(|_| ()),
        Err(DeclareError::Sink(-4))
    );
    assert_eq!(builder.submit(), Err(DeclareError::Sink(-4)));
    drop(builder);
    assert_eq!(calls.get(), 1);
}

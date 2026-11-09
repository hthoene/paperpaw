#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use alloc::{string::String, vec, vec::Vec};
use core::{cell::RefCell, ffi::c_void, ptr};
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes, PpHost};

#[derive(Default)]
struct Context {
    names: RefCell<Vec<String>>,
    caps: RefCell<Vec<usize>>,
    /// What the slot writes and the length it returns (or a status).
    write: RefCell<Vec<u8>>,
    result: core::cell::Cell<Option<i32>>,
}
unsafe extern "C" fn icon_slot(ctx: *mut c_void, name: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: the wrapper supplies a live context, readable name and writable buffer.
    unsafe {
        let ctx = &*ctx.cast::<Context>();
        let name = core::str::from_utf8(core::slice::from_raw_parts(name.ptr, name.len)).unwrap();
        ctx.names.borrow_mut().push(name.into());
        ctx.caps.borrow_mut().push(out.cap);
        let write = ctx.write.borrow();
        if let Some(status) = ctx.result.get() {
            return status;
        }
        let n = write.len().min(out.cap);
        ptr::copy_nonoverlapping(write.as_ptr(), out.ptr, n);
        i32::try_from(write.len()).unwrap()
    }
}
fn table() -> PpHost {
    let mut table = abi::host_table();
    table.icon_glyph = icon_slot;
    table
}
fn ask(ctx: &mut Context, name: &str) -> Result<IconGlyph, Error> {
    let table = table();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(ctx).cast(),
    };
    IconGlyph::named(&host, name)
}

#[test]
fn the_character_of_a_name_is_returned_for_one_to_four_bytes() {
    for glyph in ["a", "é", "☰", "\u{e000}", "\u{1f4d6}"] {
        let mut ctx = Context::default();
        *ctx.write.borrow_mut() = glyph.as_bytes().to_vec();
        let icon = ask(&mut ctx, "book-2").unwrap();
        assert_eq!(icon.as_str(), glyph);
        assert_eq!(*ctx.names.borrow(), vec![String::from("book-2")]);
        assert_eq!(*ctx.caps.borrow(), vec![4]);
    }
}

#[test]
fn a_malformed_name_is_refused_without_asking_the_host() {
    let long = "a".repeat(65);
    for name in ["", "Book", "a b", "a_b", "é", "a.b", "a/b", long.as_str()] {
        let mut ctx = Context::default();
        *ctx.write.borrow_mut() = b"x".to_vec();
        assert_eq!(ask(&mut ctx, name), Err(Error::Invalid), "{name:?}");
        assert!(ctx.names.borrow().is_empty());
    }
    let mut ctx = Context::default();
    *ctx.write.borrow_mut() = b"x".to_vec();
    assert!(ask(&mut ctx, &"a".repeat(64)).is_ok());
}

#[test]
fn the_hosts_refusals_pass_through() {
    for (status, error) in [
        (abi::ERR_NOT_FOUND, Error::NotFound),
        (abi::ERR_UNSUPPORTED, Error::Unsupported),
        (abi::ERR_NO_SPACE, Error::NoSpace),
        (abi::ERR_INVALID, Error::Invalid),
    ] {
        let mut ctx = Context::default();
        ctx.result.set(Some(status));
        assert_eq!(ask(&mut ctx, "home"), Err(error), "{status}");
    }
}

#[test]
fn an_answer_that_is_not_one_character_is_invalid() {
    let bad: [&[u8]; 4] = [
        b"",           // empty
        b"ab",         // two characters
        &[0xff],       // not UTF-8
        &[0xe2, 0x98], // truncated character
    ];
    for write in bad {
        let mut ctx = Context::default();
        *ctx.write.borrow_mut() = write.to_vec();
        assert_eq!(ask(&mut ctx, "home"), Err(Error::Invalid), "{write:?}");
    }
    // A length beyond the buffer (a host that ignores its capacity).
    let mut ctx = Context::default();
    *ctx.write.borrow_mut() = b"abcde".to_vec();
    assert_eq!(ask(&mut ctx, "home"), Err(Error::Invalid));
}

#[test]
fn debug_shows_the_character_only() {
    let mut ctx = Context::default();
    *ctx.write.borrow_mut() = "☰".as_bytes().to_vec();
    assert_eq!(
        alloc::format!("{:?}", ask(&mut ctx, "menu").unwrap()),
        "IconGlyph(\"☰\")"
    );
}

#[test]
fn a_refusal_is_not_remembered_and_the_next_ask_reaches_the_host() {
    // No icon font bound, then one bound: the host's answer is read each time.
    let mut ctx = Context::default();
    ctx.result.set(Some(abi::ERR_NOT_FOUND));
    assert_eq!(ask(&mut ctx, "home"), Err(Error::NotFound));
    ctx.result.set(None);
    *ctx.write.borrow_mut() = "☰".as_bytes().to_vec();
    assert_eq!(ask(&mut ctx, "home").unwrap().as_str(), "☰");
    assert_eq!(ctx.names.borrow().len(), 2);
}

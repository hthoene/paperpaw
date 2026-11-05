//! The bounded text and buffer helpers of the host table: null and zero
//! handling, the operation limit, UTF-8 before effects, the snprintf rule of a
//! query and the copied count of a blob read.

use std::mem::MaybeUninit;

use paperpaw_native::host_glue::buffers::{
    TEXT_MAX, buffer_out, bytes_in, copy_count, copy_required, query_text, text_in,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_UTF8, PpBuffer, PpBytes};

fn bytes(data: &[u8]) -> PpBytes {
    PpBytes {
        ptr: data.as_ptr(),
        len: data.len(),
    }
}

fn buffer(data: &mut [u8]) -> PpBuffer {
    PpBuffer {
        ptr: data.as_mut_ptr(),
        cap: data.len(),
    }
}

/// Initialised bytes as the helpers' output type (to look at them afterwards).
fn uninit(data: &mut [u8]) -> &mut [MaybeUninit<u8>] {
    // SAFETY: `MaybeUninit<u8>` has the layout of `u8`; initialised bytes are valid.
    unsafe { &mut *(std::ptr::from_mut(data) as *mut [MaybeUninit<u8>]) }
}

const NULL_IN: PpBytes = PpBytes {
    ptr: core::ptr::null(),
    len: 0,
};
const NULL_OUT: PpBuffer = PpBuffer {
    ptr: core::ptr::null_mut(),
    cap: 0,
};

#[test]
fn null_with_zero_length_is_empty_and_null_with_a_length_is_refused() {
    // SAFETY: null with zero length reads nothing.
    unsafe {
        assert_eq!(bytes_in(NULL_IN, 8), Ok(&[][..]));
        assert_eq!(text_in(NULL_IN, 8), Ok(""));
        assert_eq!(buffer_out(NULL_OUT, 8).map(|b| b.len()), Ok(0));
        let null_with_len = PpBytes {
            ptr: core::ptr::null(),
            len: 3,
        };
        assert_eq!(bytes_in(null_with_len, 8), Err(ERR_INVALID));
        let null_with_cap = PpBuffer {
            ptr: core::ptr::null_mut(),
            cap: 3,
        };
        assert_eq!(
            buffer_out(null_with_cap, 8).map(|b| b.len()),
            Err(ERR_INVALID)
        );
    }
}

#[test]
fn the_limit_is_inclusive_and_one_above_is_refused() {
    let data = vec![b'a'; TEXT_MAX + 1];
    let mut out = vec![0u8; TEXT_MAX + 1];
    // SAFETY: the pairs describe live vectors.
    unsafe {
        let at = bytes_in(bytes(&data[..TEXT_MAX]), TEXT_MAX).unwrap();
        assert_eq!(at.len(), TEXT_MAX);
        assert_eq!(bytes_in(bytes(&data), TEXT_MAX), Err(ERR_INVALID));
        let at = buffer_out(buffer(&mut out[..TEXT_MAX]), TEXT_MAX).unwrap();
        assert_eq!(at.len(), TEXT_MAX);
        assert_eq!(
            buffer_out(buffer(&mut out), TEXT_MAX).map(|b| b.len()),
            Err(ERR_INVALID)
        );
        // A smaller operation limit than the table's.
        assert_eq!(bytes_in(bytes(&data[..129]), 128), Err(ERR_INVALID));
        assert!(bytes_in(bytes(&data[..128]), 128).is_ok());
    }
}

#[test]
fn text_that_is_not_utf8_is_refused_with_its_own_status() {
    // SAFETY: the pair describes a live array.
    unsafe {
        assert_eq!(text_in(bytes(&[0xff, 0xfe]), 8), Err(ERR_UTF8));
        assert_eq!(text_in(bytes("grüße".as_bytes()), 16), Ok("grüße"));
        // A cut in the middle of a character is not UTF-8 either.
        assert_eq!(text_in(bytes(&"ü".as_bytes()[..1]), 8), Err(ERR_UTF8));
    }
}

#[test]
fn a_query_returns_the_needed_length_and_writes_at_most_the_capacity() {
    let src = b"hello world";
    // No room at all: nothing written, the length is the need.
    let mut none = [];
    assert_eq!(copy_required(src, uninit(&mut none), TEXT_MAX), 11);
    // Too small: filled, the need still reported so the caller can repeat.
    let mut small = [0u8; 5];
    assert_eq!(copy_required(src, uninit(&mut small), TEXT_MAX), 11);
    assert_eq!(&small, b"hello");
    // Exact fit and larger: whole value, the rest untouched.
    let mut exact = [0u8; 11];
    assert_eq!(copy_required(src, uninit(&mut exact), TEXT_MAX), 11);
    assert_eq!(&exact, src);
    let mut big = [0xaau8; 16];
    assert_eq!(copy_required(src, uninit(&mut big), TEXT_MAX), 11);
    assert_eq!(&big[..11], src);
    assert_eq!(&big[11..], &[0xaa; 5]);
    // An empty value needs zero.
    assert_eq!(copy_required(b"", uninit(&mut big), TEXT_MAX), 0);
    // A value over the operation's limit cannot be delivered: nothing written.
    let long = vec![b'x'; TEXT_MAX + 1];
    let mut sink = [0xaau8; 4];
    assert_eq!(
        copy_required(&long, uninit(&mut sink), TEXT_MAX),
        ERR_NO_SPACE
    );
    assert_eq!(sink, [0xaa; 4]);
}

#[test]
fn a_blob_read_returns_the_count_copied() {
    let src = b"0123456789";
    let mut out = [0u8; 4];
    assert_eq!(copy_count(src, uninit(&mut out)), 4);
    assert_eq!(&out, b"0123");
    let mut wide = [0xaau8; 16];
    assert_eq!(copy_count(src, uninit(&mut wide)), 10);
    assert_eq!(&wide[10..], &[0xaa; 6]);
    assert_eq!(copy_count(b"", uninit(&mut wide)), 0);
    assert_eq!(copy_count(src, &mut []), 0);
}

#[test]
fn a_refused_query_never_reaches_the_backend() {
    let mut out = [0xaau8; 8];
    let calls = std::cell::Cell::new(0u32);
    let call = |key: PpBytes, out: PpBuffer, max: usize| {
        // SAFETY: the pairs describe live arrays (or are null with length zero).
        unsafe {
            query_text(key, out, max, |k| {
                calls.set(calls.get() + 1);
                Ok::<_, i32>(if k == "name" { "Paw" } else { "" })
            })
        }
    };
    assert_eq!(call(bytes(&[0xff]), buffer(&mut out), 128), ERR_UTF8);
    assert_eq!(
        call(
            bytes(b"name"),
            PpBuffer {
                ptr: core::ptr::null_mut(),
                cap: 4
            },
            128
        ),
        ERR_INVALID
    );
    assert_eq!(
        call(bytes(&[b'k'; 129]), buffer(&mut out), 128),
        ERR_INVALID
    );
    assert_eq!(out, [0xaa; 8], "a refused call wrote nothing");
    // The key is checked before the buffer: both bad, the key's status.
    assert_eq!(
        call(
            bytes(&[0xff]),
            PpBuffer {
                ptr: core::ptr::null_mut(),
                cap: 4
            },
            128
        ),
        ERR_UTF8
    );
    assert_eq!(calls.get(), 0, "the backend ran for a refused call");
    // A valid call reaches it once and gets the value.
    assert_eq!(call(bytes(b"name"), buffer(&mut out), 128), 3);
    assert_eq!(&out[..3], b"Paw");
    assert_eq!(calls.get(), 1);
    // Null key with length zero is the empty key.
    assert_eq!(call(NULL_IN, NULL_OUT, 128), 0);
    assert_eq!(calls.get(), 2);
}

#[test]
fn a_backend_status_passes_through_and_a_positive_one_is_a_bug() {
    let mut out = [0u8; 4];
    // SAFETY: the pairs describe live arrays.
    unsafe {
        let missing = query_text(bytes(b"k"), buffer(&mut out), 128, |_| Err::<&str, i32>(-6));
        assert_eq!(missing, -6);
        let bug = query_text(bytes(b"k"), buffer(&mut out), 128, |_| Err::<&str, i32>(7));
        assert_eq!(bug, ERR_INVALID);
    }
}

#[test]
fn output_storage_need_not_be_initialised() {
    // Storage that was never written (as `malloc` or a `MaybeUninit` buffer
    // gives it): the helpers write the copied bytes and read none.
    let mut raw = [MaybeUninit::<u8>::uninit(); 16];
    let pair = PpBuffer {
        ptr: raw.as_mut_ptr().cast(),
        cap: raw.len(),
    };
    // SAFETY: `pair` describes the live array, which nothing else touches.
    let out = unsafe { buffer_out(pair, TEXT_MAX) }.unwrap();
    assert_eq!(out.len(), 16);
    assert_eq!(copy_required(b"uninit", out, TEXT_MAX), 6);
    // SAFETY: the first six bytes were just written.
    let written: Vec<u8> = raw[..6]
        .iter()
        .map(|b| unsafe { b.assume_init() })
        .collect();
    assert_eq!(written, b"uninit");
    // Through a whole query, with a key and a backend.
    let mut raw = [MaybeUninit::<u8>::uninit(); 8];
    let pair = PpBuffer {
        ptr: raw.as_mut_ptr().cast(),
        cap: raw.len(),
    };
    // SAFETY: as above.
    let need = unsafe { query_text(bytes(b"k"), pair, 128, |_| Ok::<_, i32>("value")) };
    assert_eq!(need, 5);
    // SAFETY: the first five bytes were written.
    let written: Vec<u8> = raw[..5]
        .iter()
        .map(|b| unsafe { b.assume_init() })
        .collect();
    assert_eq!(written, b"value");
    // A refused call writes nothing (here: the key is not UTF-8).
    let mut raw = [MaybeUninit::<u8>::new(0x5a); 4];
    let pair = PpBuffer {
        ptr: raw.as_mut_ptr().cast(),
        cap: raw.len(),
    };
    // SAFETY: as above.
    let status = unsafe { query_text(bytes(&[0xff]), pair, 128, |_| Ok::<_, i32>("x")) };
    assert_eq!(status, ERR_UTF8);
    // SAFETY: the bytes were initialised to 0x5a above.
    assert!(raw.iter().all(|b| unsafe { b.assume_init() } == 0x5a));
}

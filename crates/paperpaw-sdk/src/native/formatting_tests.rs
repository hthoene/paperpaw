#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use alloc::{string::String, vec, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
};
use paperpaw_native::host_glue::{self as abi, PpHost};

#[derive(Default)]
struct Context {
    inputs: RefCell<Vec<(String, Vec<u8>)>>,
    result: Cell<i32>,
    copy_reply: RefCell<Option<Vec<u8>>>,
}
unsafe fn call(ctx: *mut c_void, key: PpBytes, args: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: tests provide a live context, readable inputs and writable output.
    unsafe {
        let ctx = &*ctx.cast::<Context>();
        let key = core::str::from_utf8(core::slice::from_raw_parts(key.ptr, key.len)).unwrap();
        let args = core::slice::from_raw_parts(args.ptr, args.len);
        ctx.inputs.borrow_mut().push((key.into(), args.into()));
        let result = ctx.result.get();
        if result >= 0 {
            ptr::write_bytes(out.ptr, b'x', out.cap.min(result as usize));
        }
        result
    }
}
unsafe extern "C" fn format_slot(
    ctx: *mut c_void,
    key: PpBytes,
    args: PpBytes,
    out: PpBuffer,
) -> i32 {
    // SAFETY: forwarded unchanged from the safe wrapper.
    unsafe { call(ctx, key, args, out) }
}
unsafe extern "C" fn digits_slot(ctx: *mut c_void, text: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: forwarded unchanged; empty arguments are readable at zero length.
    unsafe { call(ctx, text, bytes(b""), out) }
}
unsafe extern "C" fn copy_slot(ctx: *mut c_void, key: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: the wrapper supplies a live context and exclusively borrowed output.
    unsafe {
        let context = &*ctx.cast::<Context>();
        if let Some(reply) = context.copy_reply.borrow().as_ref() {
            ptr::copy_nonoverlapping(reply.as_ptr(), out.ptr, reply.len().min(out.cap));
            return reply.len() as i32;
        }
        digits_slot(ctx, key, out)
    }
}
fn table() -> PpHost {
    let mut table = abi::host_table();
    table.i18n_copy = copy_slot;
    table.i18n_format = format_slot;
    table.i18n_digits = digits_slot;
    table
}

#[test]
fn format_encodes_signed_arguments_and_plural_count_with_generated_codec() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let args = [
        Arg::Text {
            name: "title",
            value: "Grüße",
        },
        Arg::Int {
            name: "offset",
            value: i64::MIN,
        },
        Arg::Count(-2),
    ];
    ctx.result.set(12);
    let mut out = [0; 3];
    assert_eq!(
        format_message(
            &host,
            "books",
            &args,
            &mut FormatBuffers::new(&mut [0; 128], &mut out)
        ),
        Ok(12)
    );
    assert_eq!(out, *b"xxx");
    let inputs = ctx.inputs.borrow();
    assert_eq!(inputs[0].0, "books");
    let batch = wire::records_decode(&inputs[0].1).unwrap();
    let expected = [
        Record::I18nArgStr(wire::I18nArgStr {
            name: "title",
            value: "Grüße",
        }),
        Record::I18nArgInt(wire::I18nArgInt {
            name: "offset",
            value: i64::MIN as u64,
        }),
        Record::I18nCount(wire::I18nCount {
            value: (-2_i64) as u64,
        }),
    ];
    assert_eq!(batch.iter().collect::<Vec<_>>(), expected);
}

#[test]
fn duplicate_arguments_never_reach_host_or_modify_output() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut out = [77; 4];
    let mut scratch = [0; MAX_ARGUMENT_BYTES];
    for args in [
        vec![Arg::Count(1), Arg::Count(2)],
        vec![
            Arg::Text {
                name: "n",
                value: "one",
            },
            Arg::Int {
                name: "n",
                value: 2,
            },
        ],
        vec![
            Arg::Count(1),
            Arg::Int {
                name: "count",
                value: 2,
            },
        ],
    ] {
        assert_eq!(
            format_message(
                &host,
                "books",
                &args,
                &mut FormatBuffers::new(&mut scratch, &mut out)
            ),
            Err(Error::Invalid)
        );
    }
    assert!(ctx.inputs.borrow().is_empty());
    assert_eq!(out, [77; 4]);
}

#[test]
fn argument_limits_never_reach_host_or_modify_output() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut out = [77; 4];
    let mut scratch = [0; MAX_ARGUMENT_BYTES];
    assert_eq!(
        format_message(
            &host,
            "x".repeat(MAX_KEY_BYTES + 1),
            &[],
            &mut FormatBuffers::new(&mut scratch, &mut out)
        ),
        Err(Error::NoSpace)
    );
    assert_eq!(
        format_message(
            &host,
            "books",
            &[],
            &mut FormatBuffers::new(&mut [], &mut out)
        ),
        Err(Error::NoSpace)
    );
    assert_eq!(
        format_message(
            &host,
            "books",
            &[Arg::Text {
                name: "n",
                value: &"x".repeat(wire::I18N_ARG_STR_VALUE_MAX_BYTES + 1)
            }],
            &mut FormatBuffers::new(&mut scratch, &mut out)
        ),
        Err(Error::NoSpace)
    );
    assert_eq!(
        digits(&host, "x".repeat(MAX_DIGIT_BYTES + 1), &mut out),
        Err(Error::NoSpace)
    );
    let value = "x".repeat(wire::I18N_ARG_STR_VALUE_MAX_BYTES);
    let names = ["a", "b", "c", "d", "e", "f", "g", "h"];
    let oversized: Vec<_> = names
        .iter()
        .map(|name| Arg::Text {
            name,
            value: &value,
        })
        .collect();
    assert_eq!(
        format_message(
            &host,
            "books",
            &oversized,
            &mut FormatBuffers::new(&mut scratch, &mut out)
        ),
        Err(Error::NoSpace)
    );
    assert_eq!(
        format_message(
            &host,
            "books",
            &[Arg::Count(1); wire::I18N_FORMAT_MAX_RECORDS + 1],
            &mut FormatBuffers::new(&mut scratch, &mut out)
        ),
        Err(Error::NoSpace)
    );
    assert!(ctx.inputs.borrow().is_empty());
    assert_eq!(out, [77; 4]);
}

#[test]
fn missing_keys_preserve_output_and_digits_return_required_length() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut out = [77; 4];
    ctx.result.set(abi::ERR_NOT_FOUND);
    assert_eq!(
        format_message(
            &host,
            "missing",
            &[],
            &mut FormatBuffers::new(&mut [0; 1], &mut out)
        ),
        Err(Error::NotFound)
    );
    assert_eq!(out, [77; 4]);
    ctx.result.set(7);
    assert_eq!(digits(&host, "123", &mut out), Ok(7));
    assert_eq!(out, *b"xxxx");
    assert_eq!(digits(&host, "", &mut []), Ok(7));
    ctx.result.set(-124);
    assert_eq!(digits(&host, "1", &mut out), Err(Error::Other(-124)));
}

#[test]
fn copy_message_forwards_keys_and_borrows_complete_output_without_a_local_key_cap() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    ctx.result.set(9);
    assert_eq!(
        copy_message(&host, "battery.title", &mut []),
        Err(Error::NoSpace)
    );
    let mut out = [0; 4];
    assert_eq!(
        copy_message(&host, "battery.title", &mut out),
        Err(Error::NoSpace)
    );
    assert_eq!(out, *b"xxxx");
    let mut complete = [0; 9];
    let start = complete.as_ptr();
    let message = copy_message(&host, "battery.title", &mut complete).unwrap();
    assert_eq!(message, "xxxxxxxxx");
    assert_eq!(message.as_ptr(), start);
    let key = "x".repeat(MAX_KEY_BYTES + 1);
    assert_eq!(copy_message(&host, &key, &mut complete), Ok("xxxxxxxxx"));
    assert_eq!(ctx.inputs.borrow()[3].0, key);
    ctx.result.set(0);
    assert_eq!(copy_message(&host, "empty", &mut []), Ok(""));
    ctx.result.set(abi::ERR_NOT_FOUND);
    out = [77; 4];
    assert_eq!(
        copy_message(&host, "missing", &mut out),
        Err(Error::NotFound)
    );
    assert_eq!(out, [77; 4]);
    ctx.result.set(-125);
    assert_eq!(
        copy_message(&host, "battery.title", &mut out),
        Err(Error::Other(-125))
    );
}

#[test]
fn copy_message_refuses_multibyte_truncation_before_decoding_and_invalid_utf8() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    *ctx.copy_reply.borrow_mut() = Some("é".as_bytes().to_vec());
    let mut short = [0; 1];
    assert_eq!(
        copy_message(&host, "accent", &mut short),
        Err(Error::NoSpace)
    );
    assert_eq!(short, [0xc3]);
    assert_eq!(copy_message(&host, "accent", &mut [0; 2]), Ok("é"));
    *ctx.copy_reply.borrow_mut() = Some(vec![0xff]);
    assert_eq!(copy_message(&host, "broken", &mut [0; 1]), Err(Error::Utf8));
}

#[test]
fn copy_message_default_table_is_unsupported() {
    let table = abi::host_table();
    let host = Host {
        table: &table,
        ctx: ptr::null_mut(),
    };
    assert_eq!(
        copy_message(&host, "battery.title", &mut [0; 4]),
        Err(Error::Unsupported)
    );
}

#[test]
fn message_capacity_is_bounded_even_with_larger_caller_storage() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut out = vec![77; MAX_MESSAGE_BYTES + 2];
    ctx.result.set(MAX_MESSAGE_BYTES as i32);
    assert_eq!(
        copy_message(&host, "full", &mut out).unwrap().len(),
        MAX_MESSAGE_BYTES
    );
    assert_eq!(&out[MAX_MESSAGE_BYTES..], &[77, 77]);
    ctx.result.set((MAX_MESSAGE_BYTES + 1) as i32);
    assert_eq!(
        copy_message(&host, "oversized", &mut out),
        Err(Error::NoSpace)
    );
    assert_eq!(&out[MAX_MESSAGE_BYTES..], &[77, 77]);
}

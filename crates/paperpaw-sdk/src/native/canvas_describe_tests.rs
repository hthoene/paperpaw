#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use crate::native::Host;
use alloc::{string::String, vec::Vec};
use core::{cell::RefCell, ffi::c_void, ptr};
use paperpaw_native::host_glue::{self as abi, PpHost};

/// One told part: role, flags, label, x, y, w, h.
type Part = (u32, u32, String, i32, i32, u32, u32);

/// What the host was told, and what it answers.
#[derive(Default)]
struct Told {
    parts: RefCell<Vec<Part>>,
    answer: i32,
}

#[allow(clippy::too_many_arguments, reason = "one argument per slot field")]
unsafe extern "C" fn describe_slot(
    ctx: *mut c_void,
    role: u32,
    flags: u32,
    label: PpBytes,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> i32 {
    // SAFETY: the test's host keeps this context; the label is borrowed text.
    let (told, label) = unsafe {
        (
            &*ctx.cast::<Told>(),
            core::slice::from_raw_parts(label.ptr, label.len),
        )
    };
    let label = String::from(core::str::from_utf8(label).unwrap());
    told.parts
        .borrow_mut()
        .push((role, flags, label, x, y, width, height));
    told.answer
}

fn told(answer: i32) -> (PpHost, Told) {
    let mut table = abi::host_table();
    table.canvas_describe = describe_slot;
    (
        table,
        Told {
            answer,
            ..Told::default()
        },
    )
}

#[test]
fn a_part_goes_to_the_host_as_its_role_number_flags_label_and_area() {
    let (table, mut ctx) = told(1);
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let frame = FrameRect::new(0, 0, 480, 800);
    let canvas = Canvas::from(&host, frame, frame);
    let area = FrameRect::new(-4, 20, 480, 40);
    let flags = PartFlags::FOCUSED | PartFlags::ON;
    assert_eq!(
        canvas.describe_part(PartRole::Switch, "Wi-Fi", area, flags),
        Ok(true)
    );
    assert_eq!(
        canvas.describe_part(PartRole::Progress, "", area, PartFlags::NONE),
        Ok(true)
    );
    assert_eq!(
        *ctx.parts.borrow(),
        [
            (4, 0b111, String::from("Wi-Fi"), -4, 20, 480, 40),
            (13, 0, String::new(), -4, 20, 480, 40),
        ]
    );
    assert_eq!(PartFlags::OFF.bits(), 0b10);
    assert_eq!(PartFlags::DISABLED.bits(), 0b1000);
}

#[test]
fn nobody_asking_and_refusals_come_back_as_they_are() {
    let frame = FrameRect::new(0, 0, 10, 10);
    for (answer, expected) in [
        (0, Ok(false)),
        (abi::ERR_STATE, Err(Error::State)),
        (abi::ERR_INVALID, Err(Error::Invalid)),
    ] {
        let (table, mut ctx) = told(answer);
        let host = Host {
            table: &table,
            ctx: ptr::from_mut(&mut ctx).cast(),
        };
        let canvas = Canvas::from(&host, frame, frame);
        assert_eq!(
            canvas.describe_part(PartRole::Row, "A", frame, PartFlags::NONE),
            expected
        );
    }
    // A host without the slot (an older one) says it does not describe.
    let table = abi::host_table();
    let host = Host {
        table: &table,
        ctx: ptr::null_mut(),
    };
    let canvas = Canvas::from(&host, frame, frame);
    assert_eq!(
        canvas.describe_part(PartRole::Row, "A", frame, PartFlags::NONE),
        Err(Error::Unsupported)
    );
}

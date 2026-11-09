#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use crate::native::{
    commands::CommandSender,
    declare::records_wire::{Record, ServiceDocImage, record_encode_one, records_encode},
    events::Event,
};
use alloc::vec::Vec;
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
};
use paperpaw_native::host_glue::{self as abi, PpBytes, PpHost};

#[derive(Default)]
struct Context {
    info_reply: RefCell<Vec<u8>>,
    info_status: Cell<i32>,
    info_asked: Cell<u32>,
    draws: RefCell<Vec<[i64; 6]>>,
    draw_status: Cell<i32>,
    batches: RefCell<Vec<Vec<u8>>>,
}

unsafe extern "C" fn info_slot(ctx: *mut c_void, image: u32, out: PpBuffer) -> i32 {
    // SAFETY: tests retain the context for the host's life.
    let ctx = unsafe { &*ctx.cast::<Context>() };
    ctx.info_asked.set(image);
    if ctx.info_status.get() < 0 {
        return ctx.info_status.get();
    }
    let reply = ctx.info_reply.borrow();
    assert!(reply.len() <= out.cap);
    // SAFETY: the SDK lends `cap` writable bytes; the fixture reply is disjoint.
    unsafe { ptr::copy_nonoverlapping(reply.as_ptr(), out.ptr, reply.len()) };
    i32::try_from(reply.len()).unwrap()
}
unsafe extern "C" fn draw_slot(
    ctx: *mut c_void,
    image: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    flags: u32,
) -> i32 {
    // SAFETY: tests retain the context for the host's life.
    let ctx = unsafe { &*ctx.cast::<Context>() };
    ctx.draws.borrow_mut().push([
        i64::from(image),
        i64::from(x),
        i64::from(y),
        i64::from(w),
        i64::from(h),
        i64::from(flags),
    ]);
    ctx.draw_status.get()
}
unsafe extern "C" fn submit(ctx: *mut c_void, batch: PpBytes) -> i32 {
    // SAFETY: tests retain the context; the sender lends initialized bytes.
    let (ctx, bytes) = unsafe {
        (
            &*ctx.cast::<Context>(),
            core::slice::from_raw_parts(batch.ptr, batch.len),
        )
    };
    ctx.batches.borrow_mut().push(bytes.to_vec());
    abi::OK
}
fn table() -> PpHost {
    let mut table = abi::host_table();
    table.image_info = info_slot;
    table.canvas_draw_image = draw_slot;
    table.commands_submit = submit;
    table
}
fn doc_image(width: u32, height: u32, flags: u32) -> Vec<u8> {
    let mut bytes = [0; 64];
    let n = record_encode_one(
        &Record::ServiceDocImage(ServiceDocImage {
            image: 9,
            index: 2,
            width,
            height,
            flags,
        }),
        &mut bytes,
    )
    .unwrap();
    bytes[..n].to_vec()
}

#[test]
fn image_ids_are_never_zero() {
    assert_eq!(ImageId::new(0), Err(Error::Invalid));
    assert_eq!(ImageId::new(7).unwrap().get(), 7);
}

#[test]
fn info_reads_the_size_and_the_colour_and_not_decoded_bits_only() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let id = ImageId::new(9).unwrap();
    ctx.info_reply.replace(doc_image(480, 800, 1));
    let info = ImageInfo::read(&host, id).unwrap();
    assert_eq!(ctx.info_asked.get(), 9);
    assert_eq!(
        (info.width(), info.height(), info.colour()),
        (480, 800, true)
    );
    // Other bits are dropped.
    ctx.info_reply.replace(doc_image(10, 20, 0b110));
    let info = ImageInfo::read(&host, id).unwrap();
    assert_eq!(
        (info.width(), info.height(), info.colour()),
        (10, 20, false)
    );
}

#[test]
fn info_refusals_and_malformed_answers() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let id = ImageId::new(9).unwrap();
    ctx.info_status.set(abi::ERR_NOT_FOUND);
    assert_eq!(ImageInfo::read(&host, id), Err(Error::NotFound));
    ctx.info_status.set(abi::ERR_UNSUPPORTED);
    assert_eq!(ImageInfo::read(&host, id), Err(Error::Unsupported));
    ctx.info_status.set(0);
    // Empty, torn and two-record answers are Invalid.
    let good = doc_image(1, 1, 0);
    for bad in [
        Vec::new(),
        good[..good.len() - 1].to_vec(),
        [good.clone(), good].concat(),
    ] {
        ctx.info_reply.replace(bad);
        assert_eq!(ImageInfo::read(&host, id), Err(Error::Invalid));
    }
}

#[test]
fn draw_passes_the_rectangle_and_only_defined_flags() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let frame = FrameRect::new(0, 0, 480, 800);
    let canvas = Canvas::from(&host, frame, frame);
    let id = ImageId::new(9).unwrap();
    let rect = FrameRect::new(-5, 6, 70_000, 90_000);
    canvas.draw_image(id, rect, ImageStyle::new()).unwrap();
    canvas
        .draw_image(
            id,
            rect,
            ImageStyle::new().fill().small().transparency(0x80),
        )
        .unwrap();
    // Setting the transparency twice keeps only the last.
    canvas
        .draw_image(
            id,
            rect,
            ImageStyle::new().transparency(255).transparency(1),
        )
        .unwrap();
    assert_eq!(
        ctx.draws.borrow().as_slice(),
        &[
            [9, -5, 6, 70_000, 90_000, 0],
            [9, -5, 6, 70_000, 90_000, 0b11 | (0x80 << 8)],
            [9, -5, 6, 70_000, 90_000, 1 << 8],
        ]
    );
    ctx.draw_status.set(abi::ERR_STATE);
    assert_eq!(
        canvas.draw_image(id, rect, ImageStyle::new()),
        Err(Error::State)
    );
    ctx.draw_status.set(abi::ERR_NOT_FOUND);
    assert_eq!(
        canvas.draw_image(id, rect, ImageStyle::new()),
        Err(Error::NotFound)
    );
}

#[test]
fn prepare_and_release_stage_their_commands_and_refuse_a_long_path_first() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 1024];
    let mut sender = CommandSender::new(&host, &mut scratch);
    sender
        .image_prepare(&PrepareImage::new(PrepareRoot::Data, "cats/1.jpg", 77))
        .unwrap();
    sender
        .image_prepare(
            &PrepareImage::new(PrepareRoot::Library, "a.png", 78)
                .side(9_999)
                .colour(),
        )
        .unwrap();
    sender
        .image_prepare(&PrepareImage::new(PrepareRoot::Data, &"p".repeat(512), 2))
        .unwrap();
    let long = "p".repeat(513);
    assert_eq!(
        sender.image_prepare(&PrepareImage::new(PrepareRoot::Data, &long, 1)),
        Err(Error::Invalid)
    );
    sender.image_release(ImageId::new(3).unwrap()).unwrap();
    let batches = ctx.batches.borrow();
    assert_eq!(batches.len(), 4, "the refused request stages nothing");
    let sent = |i: usize| {
        let commands = crate::native::wire::commands_wire::commands_decode(&batches[i]).unwrap();
        commands.iter().next().unwrap()
    };
    assert_eq!(
        sent(0),
        Command::ImagePrepare {
            root: 0,
            path: "cats/1.jpg",
            side: 0,
            cookie: 77,
            flags: 0
        }
    );
    assert_eq!(
        sent(1),
        Command::ImagePrepare {
            root: 1,
            path: "a.png",
            side: 9_999,
            cookie: 78,
            flags: 1
        }
    );
    assert_eq!(sent(3), Command::ImageRelease { image: 3 });
}

fn ready(status: u32, image: u32) -> Result<ImageReady, Error> {
    ImageReady::decode(wire::EventImageReady {
        now_ms: 5,
        cookie: 77,
        status,
        image,
        width: 320,
        height: 200,
    })
}

#[test]
fn image_ready_reads_every_status_and_an_unknown_one_as_damaged() {
    let got = ready(0, 4).unwrap();
    assert_eq!(
        (
            got.cookie(),
            got.status(),
            got.image(),
            got.width(),
            got.height()
        ),
        (
            77,
            ImageStatus::Ready,
            Some(ImageId::new(4).unwrap()),
            320,
            200
        )
    );
    for (code, status) in [
        (1, ImageStatus::Unsupported),
        (2, ImageStatus::Damaged),
        (3, ImageStatus::TooLarge),
        (4, ImageStatus::CardFailure),
        (5, ImageStatus::NoMemory),
        (6, ImageStatus::Damaged),
        (u32::MAX, ImageStatus::Damaged),
    ] {
        let got = ready(code, 0).unwrap();
        assert_eq!((got.status(), got.image()), (status, None), "{code}");
        assert_eq!((got.width(), got.height()), (0, 0), "{code}");
    }
    // A ready picture without an id breaks the contract.
    assert_eq!(ready(0, 0), Err(Error::Invalid));
}

#[test]
fn image_ready_arrives_as_an_event() {
    let mut bytes = [0; 128];
    let record = Record::EventImageReady(wire::EventImageReady {
        now_ms: 5,
        cookie: 77,
        status: 0,
        image: 4,
        width: 320,
        height: 200,
    });
    let n = records_encode(&[record], &mut bytes).unwrap();
    let Event::ImageReady(got) = Event::decode(&bytes[..n]).unwrap() else {
        panic!("not an image event")
    };
    assert_eq!((got.now_ms(), got.cookie()), (5, 77));
}

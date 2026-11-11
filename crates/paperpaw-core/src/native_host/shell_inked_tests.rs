//! A native plugin's draw callback reaches slot 41 `text.draw_inked` through the
//! table the shell gives it (the stub would answer `ERR_UNSUPPORTED`).
use super::*;
use paperpaw_native::host_glue::OK;

std::thread_local! {
    /// The status and the advance of the call the draw callback made.
    static INKED: Cell<Option<(i32, f32)>> = const { Cell::new(None) };
}

/// The draw callback's part: draws "Hello" with the table's `text.draw_inked`
/// and leaves what came back in [`INKED`].
pub(super) fn draw(state: *mut c_void) -> i32 {
    // SAFETY: as for `event`.
    let (host, ctx) = unsafe { ((*state.cast::<Fake>()).host, (*state.cast::<Fake>()).ctx) };
    // SAFETY: the host table is valid for this call.
    let table = unsafe { &*host };
    let bytes = |text: &'static [u8]| PpBytes {
        ptr: text.as_ptr(),
        len: text.len(),
    };
    let mut font = 0;
    // SAFETY: readable key, writable output, the call's own context.
    let status = unsafe { (table.font_acquire)(ctx, bytes(b"role:ui"), &mut font) };
    if status != OK {
        INKED.with(|r| r.set(Some((status, 0.0))));
        return OK;
    }
    let mut advance = 0.0_f32;
    // SAFETY: readable text and tone (empty), writable advance, the call's own context.
    let status = unsafe {
        (table.text_draw_inked)(
            ctx,
            font,
            16.0,
            4.0,
            24,
            0,
            bytes(b"Hello"),
            0,
            bytes(b""),
            0,
            &mut advance,
        )
    };
    INKED.with(|r| r.set(Some((status, advance))));
    OK
}

/// A native plugin's draw callback reaches slot 41 through the table the shell
/// gives it (the stub would answer `ERR_UNSUPPORTED`): the text is drawn on the
/// canvas of the draw and the advance comes back.
#[test]
fn a_native_draw_callback_draws_inked_text_through_the_host_table() {
    reset(NORMAL);
    DRAW_INKED.with(|d| d.set(true));
    INKED.with(|r| r.set(None));
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .build()
        .unwrap();
    kernel.start(paperpaw_platform::Instant(0));
    kernel.run_commands();
    let hosted = fake();
    let batch = hosted
        .make_registered(hosted.begin().unwrap())
        .unwrap()
        .commit()
        .unwrap();
    let depth = DispatchDepth::new();
    let now = paperpaw_platform::Instant(12);
    let mut framebuffer =
        paperpaw_graphics::Framebuffer::new(160, 40, paperpaw_platform::PixelFormat::Gray8)
            .unwrap();
    framebuffer
        .canvas()
        .fill_rect(0, 0, 160, 40, paperpaw_sdk::Color::WHITE);
    assert_eq!(
        kernel.draw_native_for_test(&batch, 1, &depth, now, &mut framebuffer.canvas()),
        Ok(OK)
    );
    let (status, advance) = INKED.with(Cell::get).expect("the callback drew");
    assert_eq!(status, OK, "text.draw_inked reached the host");
    assert!(advance > 0.0, "advance {advance}");
    let view = framebuffer.view();
    let inked = (0..40)
        .flat_map(|y| (0..160).map(move |x| (x, y)))
        .filter(|&(x, y)| view.pixel(x, y).luma() < 200)
        .count();
    assert!(inked > 20, "{inked} dark pixels");
    DRAW_INKED.with(|d| d.set(false));
    drop(batch);
    finish(&hosted);
}

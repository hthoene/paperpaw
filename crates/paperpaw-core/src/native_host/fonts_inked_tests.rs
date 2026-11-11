//! Slot 41 `text.draw_inked`: the widening is clamped, the curve is the call's
//! own, `sharp` follows the canvas, and a refused call draws nothing, in the
//! order canvas, handle, arguments.
use super::*;
use crate::native_host::HostResources;
use core::pin::pin;
use paperpaw_graphics::Framebuffer;
use paperpaw_native::host_glue::ERR_UTF8;
use paperpaw_platform::{Instant, PixelFormat};
use std::collections::BTreeSet;

extern crate std;

fn bytes(text: &[u8]) -> PpBytes {
    PpBytes {
        ptr: text.as_ptr(),
        len: text.len(),
    }
}

/// A kernel with the default fonts, started.
fn kernel() -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.run_commands();
    kernel
}

/// What one call did: its status and the pixels afterwards.
struct Run {
    status: i32,
    advance: f32,
    pixels: std::vec::Vec<u8>,
}

/// Calls slot 41 on a fresh white canvas (or none) with a handle of `ui`.
#[allow(clippy::too_many_arguments, reason = "the slot's own arguments")]
fn call(
    handle: Option<u32>,
    with_canvas: bool,
    extra: u32,
    tone: &[u8],
    flags: u32,
    text: &[u8],
) -> Run {
    let kernel = kernel();
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let env = kernel.shared.env(Instant(0));
    let frame = crate::abi7::core_frame::CoreFrame {
        callback: None,
        instance: None,
        env: &env,
        meter: &kernel.shared.meter,
        depth: None,
        directory: None,
        stack_generation: kernel.shared.stack_rows.generation(),
        stack_rows: &kernel.shared.stack_rows,
        font_removals: &kernel.shared.font_removals,
        registry_generation: &kernel.shared.registry_generation,
        usage: &kernel.shared.usage,
        plugins: &kernel.shared.plugins,
        work: &kernel.shared.work_states,
        log: Default::default(),
        ui: None,
    };
    let scope = pin!(EnvironmentScope::new());
    let canvas_scope = pin!(CanvasScope::new());
    let mut framebuffer = Framebuffer::new(160, 40, PixelFormat::Gray8).unwrap();
    framebuffer.canvas().fill_rect(0, 0, 160, 40, Color::WHITE);
    let mut result = (0, 0.0);
    let mut work = |canvas_scope: &CanvasScope| {
        scope
            .as_ref()
            .scope_core(&frame, || {
                let mut key = 0;
                // SAFETY: live readable input and an exclusive scalar output.
                let acquired = unsafe { acquire(&scope, &owner, bytes(b"role:ui"), &mut key) };
                assert_eq!(acquired, OK);
                let mut advance = 0.0_f32;
                // SAFETY: live readable inputs and an exclusive output.
                let status = unsafe {
                    draw_inked(
                        &scope,
                        &owner,
                        canvas_scope,
                        handle.unwrap_or(key),
                        14.0,
                        4.0,
                        24,
                        0x00_00_00_00,
                        bytes(text),
                        extra,
                        bytes(tone),
                        flags,
                        &mut advance,
                    )
                };
                result = (status, advance);
            })
            .unwrap();
    };
    if with_canvas {
        let mut canvas = framebuffer.canvas();
        canvas_scope
            .as_ref()
            .scope(&mut canvas, || work(&canvas_scope))
            .unwrap();
    } else {
        work(&canvas_scope);
    }
    let view = framebuffer.view();
    let mut pixels = std::vec::Vec::new();
    for y in 0..40 {
        for x in 0..160 {
            pixels.push(view.pixel(x, y).luma());
        }
    }
    Run {
        status: result.0,
        advance: result.1,
        pixels,
    }
}

const WHITE: u8 = 255;

fn drew(run: &Run) -> bool {
    run.pixels.iter().any(|&p| p != WHITE)
}

fn levels(run: &Run) -> BTreeSet<u8> {
    run.pixels.iter().copied().collect()
}

/// A tone that is every level full: any covered pixel is drawn solid.
const FULL: [u8; 16] = [255; 16];

#[test]
fn extra_16_and_17_draw_the_same_and_the_widening_shows() {
    let at_16 = call(None, true, 16, &[], 0, b"Hamburg");
    let at_17 = call(None, true, 17, &[], 0, b"Hamburg");
    let huge = call(None, true, u32::MAX, &[], 0, b"Hamburg");
    let none = call(None, true, 0, &[], 0, b"Hamburg");
    assert_eq!((at_16.status, at_17.status, huge.status), (OK, OK, OK));
    assert_eq!(at_16.pixels, at_17.pixels, "17 is clamped to 16");
    assert_eq!(at_16.pixels, huge.pixels);
    assert_ne!(at_16.pixels, none.pixels, "16 widens");
    assert!(drew(&none));
    // The advance is the measure's: widening does not change it.
    assert_eq!(at_16.advance, none.advance);
    assert!(none.advance > 0.0);
}

#[test]
fn a_tone_is_the_calls_own_and_an_empty_one_is_the_handles() {
    let own = call(None, true, 0, &[], 0, b"Hamburg");
    let toned = call(None, true, 0, &FULL, 0, b"Hamburg");
    assert_eq!((own.status, toned.status), (OK, OK));
    assert!(drew(&toned));
    assert_ne!(own.pixels, toned.pixels, "the curve is the call's");
    // Level 0 is drawn as 0 whatever it says.
    let mut nothing = [0_u8; 16];
    nothing[1] = 0;
    let blank = call(None, true, 0, &nothing, 0, b"Hamburg");
    assert_eq!(blank.status, OK);
    assert!(!drew(&blank));
    // Any length but 0 and 16 is refused, and nothing is drawn.
    for bad in [1_usize, 15, 17, 32] {
        let run = call(None, true, 0, &std::vec![9_u8; bad], 0, b"Hamburg");
        assert_eq!(run.status, ERR_INVALID, "tone of {bad} bytes");
        assert!(!drew(&run));
    }
}

#[test]
fn unknown_flag_bits_are_refused_and_nothing_is_drawn() {
    for flags in [2, 4, 0x8000_0000, 3] {
        let run = call(None, true, 4, &[], flags, b"Hamburg");
        assert_eq!(run.status, ERR_INVALID, "flags {flags:#x}");
        assert!(!drew(&run));
    }
}

#[test]
fn sharp_draws_without_grays_with_the_given_tone_or_the_handles_own() {
    let smooth = call(None, true, 0, &[], 0, b"Hamburg");
    let sharp = call(None, true, 0, &[], 1, b"Hamburg");
    let sharp_toned = call(None, true, 0, &FULL, 1, b"Hamburg");
    assert_eq!(
        (smooth.status, sharp.status, sharp_toned.status),
        (OK, OK, OK)
    );
    assert!(levels(&smooth).len() > 2, "the handle's own tone has grays");
    assert!(
        levels(&sharp).len() <= 2,
        "sharp: only paper and ink, got {:?}",
        levels(&sharp)
    );
    assert!(levels(&sharp_toned).len() <= 2);
    assert!(drew(&sharp) && drew(&sharp_toned));
    // The chosen curve is the one made sharp: a tone that is all full at
    // coverage 1 and up draws at least as much as the handle's own sharp one.
    let count = |run: &Run| run.pixels.iter().filter(|&&p| p != WHITE).count();
    assert!(count(&sharp_toned) >= count(&sharp));
}

#[test]
fn the_order_is_no_canvas_then_a_stale_handle_then_the_arguments() {
    // No canvas: ERR_STATE whatever else is wrong (a bad flag, a bad handle).
    let no_canvas = call(None, false, 4, &[], 2, b"Hamburg");
    assert_eq!(no_canvas.status, ERR_STATE);
    let no_canvas_stale = call(Some(9999), false, 4, &[], 2, b"Hamburg");
    assert_eq!(no_canvas_stale.status, ERR_STATE);
    assert!(!drew(&no_canvas));
    // A canvas and a stale handle: ERR_STATE before the bad argument.
    let stale = call(Some(9999), true, 4, &[], 2, b"Hamburg");
    assert_eq!(stale.status, ERR_STATE);
    assert!(!drew(&stale));
    // A canvas and a good handle: the arguments are looked at.
    let bad = call(None, true, 4, &[], 2, b"Hamburg");
    assert_eq!(bad.status, ERR_INVALID);
    // Text that is no UTF-8 is ERR_UTF8, and nothing is drawn.
    let utf8 = call(None, true, 4, &[], 0, &[0xff, 0xfe]);
    assert_eq!(utf8.status, ERR_UTF8);
    assert!(!drew(&utf8));
}

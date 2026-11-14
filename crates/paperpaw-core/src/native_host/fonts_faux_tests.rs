//! Faux bold (`screens-input.md` 4): a bold reading role of a family without
//! a bold face is its regular face drawn wider, one pixel for every 24 px,
//! with the advances of the regular face; a family with its bold face is
//! drawn in it, unchanged.
use super::tests::{bytes, get, scoped};
use super::*;
use crate::native_host::HostResources;
use core::pin::pin;
use paperpaw_graphics::Framebuffer;
use paperpaw_platform::{Instant, PixelFormat};

extern crate std;

const WIDTH: u16 = 320;
const HEIGHT: u16 = 80;
const TEXT: &[u8] = b"Bold milk";

/// A started kernel with the lean fonts (book text in the UI font, no bold
/// face) or the default ones (Literata with its bold face).
fn kernel(lean: bool) -> crate::Kernel {
    let builder = crate::Kernel::builder(crate::tests::DEVICE);
    let builder = if lean {
        builder.plugin(paperpaw_plugin_fonts_default::LeanFontPacks)
    } else {
        builder.plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
    };
    let mut kernel = builder.build().unwrap();
    kernel.start(Instant(0));
    kernel.run_commands();
    kernel
}

/// What a draw of [`TEXT`] gave.
#[derive(Debug, PartialEq)]
struct Drawn {
    advance: f32,
    measured: f32,
    /// Top, bottom, left, right of slot 30.
    ink: [i32; 4],
    pixels: std::vec::Vec<u8>,
}

impl Drawn {
    /// The rightmost column with ink.
    fn right(&self) -> usize {
        (0..usize::from(WIDTH))
            .rev()
            .find(|x| {
                (0..usize::from(HEIGHT)).any(|y| self.pixels[y * usize::from(WIDTH) + x] != 255)
            })
            .unwrap()
    }
}

/// Draws [`TEXT`] through `key` at `size` on a white canvas: with slot 14, or
/// with slot 41 and that `extra`.
fn drawn(kernel: &crate::Kernel, key: &str, size: f32, extra: Option<u32>) -> Drawn {
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let canvas_scope = pin!(CanvasScope::new());
    let mut framebuffer = Framebuffer::new(WIDTH, HEIGHT, PixelFormat::Gray8).unwrap();
    framebuffer
        .canvas()
        .fill_rect(0, 0, WIDTH.into(), HEIGHT.into(), Color::WHITE);
    let (mut advance, mut measured, mut ink) = (0.0_f32, 0.0_f32, [0_i32; 4]);
    {
        let mut canvas = framebuffer.canvas();
        canvas_scope
            .as_ref()
            .scope(&mut canvas, || {
                scoped(kernel, |scope| {
                    let font = get(scope, &owner, key);
                    let text = bytes(TEXT);
                    let [top, bottom, left, right] = &mut ink;
                    // SAFETY: live readable inputs and exclusive outputs.
                    let status = unsafe {
                        assert_eq!(measure(scope, &owner, font, size, text, &mut measured), OK);
                        assert_eq!(
                            super::ink(scope, &owner, font, size, text, top, bottom, left, right),
                            OK
                        );
                        match extra {
                            None => draw(
                                scope,
                                &owner,
                                &canvas_scope,
                                font,
                                size,
                                8.0,
                                56,
                                0,
                                text,
                                &mut advance,
                            ),
                            Some(extra) => draw_inked(
                                scope,
                                &owner,
                                &canvas_scope,
                                font,
                                size,
                                8.0,
                                56,
                                0,
                                text,
                                extra,
                                bytes(b""),
                                0,
                                &mut advance,
                            ),
                        }
                    };
                    assert_eq!(status, OK);
                });
            })
            .unwrap();
    }
    let view = framebuffer.view();
    let mut pixels = std::vec::Vec::new();
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            pixels.push(view.pixel(x, y).luma());
        }
    }
    Drawn {
        advance,
        measured,
        ink,
        pixels,
    }
}

#[test]
fn a_bold_role_of_a_family_without_a_bold_face_is_the_regular_face_drawn_wider() {
    let kernel = kernel(true);
    for (size, wider) in [(20.0, 1), (24.0, 1), (40.0, 2)] {
        let regular = drawn(&kernel, "role:reading", size, None);
        let bold = drawn(&kernel, "role:reading-bold", size, None);
        // Exactly the regular face with slot 41's `extra`: every glyph widened
        // to the right, in the face's own tone.
        let widened = drawn(&kernel, "role:reading", size, Some(wider));
        assert_eq!(bold.pixels, widened.pixels, "{size} px");
        assert_ne!(bold.pixels, regular.pixels, "{size} px");
        assert_eq!(bold.right(), regular.right() + wider as usize, "{size} px");
        // The widths are the regular face's: a layout need not know.
        assert_eq!(bold.advance, regular.advance, "{size} px");
        assert_eq!(bold.measured, regular.measured, "{size} px");
        // The ink a plugin asks for is the ink drawn.
        let [top, bottom, left, right] = regular.ink;
        assert_eq!(bold.ink, [top, bottom, left, right + wider as i32]);
        // A reader's own weight comes on top of it.
        let heavier = drawn(&kernel, "role:reading-bold", size, Some(2));
        let both = drawn(&kernel, "role:reading", size, Some(2 + wider));
        assert_eq!(heavier.pixels, both.pixels, "{size} px");
        // The bold italic falls back the same way (no italic face either).
        let bold_italic = drawn(&kernel, "role:reading-bold-italic", size, None);
        assert_eq!(bold_italic.pixels, bold.pixels, "{size} px");
        // An italic run is not bold.
        let italic = drawn(&kernel, "role:reading-italic", size, None);
        assert_eq!(italic, regular, "{size} px");
    }
}

#[test]
fn a_family_with_its_bold_face_is_drawn_in_it_unchanged() {
    let kernel = kernel(false);
    let bold = drawn(&kernel, "role:reading-bold", 24.0, None);
    let face = drawn(&kernel, "fonts-default.literata-bold", 24.0, None);
    assert_eq!(bold, face);
    let bold_italic = drawn(&kernel, "role:reading-bold-italic", 24.0, None);
    let face = drawn(&kernel, "fonts-default.literata-bold-italic", 24.0, None);
    assert_eq!(bold_italic, face);
    // Nor is any other role or a font by its id made bolder.
    let lean = self::kernel(true);
    for key in ["role:ui", "role:status", "fonts-default.source-sans"] {
        let plain = drawn(&lean, key, 24.0, None);
        let none = drawn(&lean, key, 24.0, Some(0));
        assert_eq!(plain.pixels, none.pixels, "{key}");
    }
}

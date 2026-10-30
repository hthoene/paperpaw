//! Screens with pictures shown in grays on panels of black and white:
//! when, and when not (a screen that says no, a refresh set exactly).

use super::*;

/// A home screen with a picture over most of it, which may be shown in
/// grays or says no.
struct CoverPlugin {
    in_grays: bool,
}

struct CoverScreen {
    in_grays: bool,
}

impl Screen for CoverScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let image = paperpaw_sdk::document::ImageRef {
            path: "p.gray".into(),
            width: 8,
            height: 8,
        };
        paperpaw_sdk::ui::draw_image(canvas, env.storage(), &image, Rect::new(10, 10, 60, 60));
    }

    fn pictures_in_grays(&self) -> bool {
        self.in_grays
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for CoverPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("cover"), "Cover", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let in_grays = self.in_grays;
        r.add_screen(ScreenSpec::new(
            Id::new_static("cover.page"),
            "page",
            Box::new(move |_, _| Box::new(CoverScreen { in_grays })),
        ))?;
        r.bind_slot(slots::HOME, Id::new_static("cover.page"), Priority::DEFAULT)
    }
}

/// A kernel showing [`CoverScreen`] on `format`, with `config`; whether
/// it came up in grays.
fn comes_up_in_grays(format: PixelFormat, in_grays: bool, config: &str) -> (Kernel, bool) {
    use paperpaw_sdk::image::{GrayWriter, ImageSink};
    let storage = paperpaw_sdk::MemoryStorage::new().with("paperpaw.ini", config);
    let mut writer = GrayWriter::new(&storage, "p.gray");
    writer.start(8, 8).unwrap();
    for _ in 0..8 {
        writer.row(&[90; 8]).unwrap();
    }
    writer.finish().unwrap();
    let device = DEVICE.with_display(DEVICE.display.with_format(format));
    let mut kernel = Kernel::builder(device)
        .storage(storage)
        .plugin(CoverPlugin { in_grays })
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    assert!(effects.refresh().is_some());
    (kernel, effects.grays())
}

#[test]
fn screens_with_pictures_come_up_in_grays_on_panels_of_black_and_white() {
    let (mut kernel, grays) = comes_up_in_grays(PixelFormat::Mono1, true, "");
    assert!(grays);
    render(&mut kernel);
    // Drawn again after grays: in black and white.
    let effects = kernel.draw_again(Instant(100));
    assert!(effects.refresh().is_some() && !effects.grays());
    // Not where the setting is off, the screen says no, or the panel has
    // grays of its own.
    let off = "[core]\npictures-in-grays = false\n";
    assert!(!comes_up_in_grays(PixelFormat::Mono1, true, off).1);
    assert!(!comes_up_in_grays(PixelFormat::Mono1, false, "").1);
    assert!(!comes_up_in_grays(PixelFormat::Gray4, true, "").1);
}

#[test]
fn a_refresh_set_exactly_is_not_turned_into_grays() {
    let (mut kernel, _) = comes_up_in_grays(PixelFormat::Mono1, true, "");
    render(&mut kernel);
    let whole = kernel.framebuffer.bounds();
    // Most of the screen with its picture changed: in grays...
    kernel.invalidate(whole);
    assert!(kernel.finish().grays());
    render(&mut kernel);
    // ...but not where the screen set its refresh exactly.
    kernel.invalidate(whole);
    kernel.output.refresh_set = Some(RefreshMode::Fast);
    let effects = kernel.finish();
    assert!(!effects.grays());
    assert_eq!(
        effects.refresh().map(|(_, mode)| mode),
        Some(RefreshMode::Fast)
    );
}

#[test]
fn the_initial_picture_frame_keeps_automatic_grays_and_covers_every_row() {
    let (mut kernel, grays) = comes_up_in_grays(PixelFormat::Mono1, true, "");
    assert!(grays);
    let mut next_row = 0;
    assert!(kernel.render_gray(&mut |top, rows| {
        assert_eq!(top, next_row);
        assert_eq!(rows.format, PixelFormat::Gray2);
        next_row += rows.height;
    }));
    assert_eq!(next_row, DEVICE.display.height);
}

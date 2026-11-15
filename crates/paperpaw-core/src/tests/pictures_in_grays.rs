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

/// The screen's frame is drawn and shown, then the calm passes: the effects
/// of the call after the calm.
fn after_the_calm(kernel: &mut Kernel) -> crate::Effects {
    render(kernel);
    // The call after the frame is shown begins the calm.
    let effects = kernel.tick(Instant(100));
    assert!(!effects.grays() && effects.refresh().is_none());
    assert_eq!(effects.wake_at(), Some(Instant(3_100)));
    kernel.tick(Instant(3_100))
}

#[test]
fn screens_with_pictures_come_up_in_black_and_white_and_get_grays_when_calm() {
    // Shown at once in black and white, the covers dithered.
    let (mut kernel, grays) = comes_up_in_grays(PixelFormat::Mono1, true, "");
    assert!(!grays);
    // The grays follow by themselves after a calm.
    let effects = after_the_calm(&mut kernel);
    assert!(effects.refresh().is_some() && effects.grays());
    render(&mut kernel);
    // And once only: nothing is due after the pass.
    let effects = kernel.tick(Instant(9_000));
    assert!(effects.refresh().is_none() && effects.wake_at().is_none());
    // Drawn again after grays: in black and white.
    let effects = kernel.draw_again(Instant(10_000));
    assert!(effects.refresh().is_some() && !effects.grays());
}

#[test]
fn the_grays_wait_for_calm_and_a_key_begins_the_wait_again() {
    let (mut kernel, _) = comes_up_in_grays(PixelFormat::Mono1, true, "");
    render(&mut kernel);
    assert_eq!(kernel.tick(Instant(100)).wake_at(), Some(Instant(3_100)));
    // Not yet.
    let effects = kernel.tick(Instant(2_000));
    assert!(effects.refresh().is_none());
    // A key: the calm is counted from it, the pass is not lost.
    press(&mut kernel, 2_500, "down");
    let effects = kernel.tick(Instant(4_000));
    assert!(!effects.grays() && effects.refresh().is_none());
    let effects = kernel.tick(Instant(5_600));
    assert!(effects.refresh().is_some() && effects.grays());
}

#[test]
fn no_pass_where_the_setting_is_off_the_screen_says_no_or_the_panel_has_grays() {
    for (format, in_grays, config) in [
        (
            PixelFormat::Mono1,
            true,
            "[core]\npictures-in-grays = false\n",
        ),
        (PixelFormat::Mono1, false, ""),
        (PixelFormat::Gray4, true, ""),
    ] {
        let (mut kernel, grays) = comes_up_in_grays(format, in_grays, config);
        assert!(!grays);
        render(&mut kernel);
        let effects = kernel.tick(Instant(100));
        assert!(effects.wake_at().is_none());
        let effects = kernel.tick(Instant(9_000));
        assert!(effects.refresh().is_none());
    }
}

#[test]
fn a_refresh_set_exactly_is_not_turned_into_grays() {
    let (mut kernel, _) = comes_up_in_grays(PixelFormat::Mono1, true, "");
    render(&mut kernel);
    let whole = kernel.framebuffer.bounds();
    // Most of the screen with its picture changed: grays are to follow...
    kernel.invalidate(whole);
    let effects = kernel.finish();
    assert!(effects.refresh().is_some() && !effects.grays());
    assert_eq!(kernel.frame.gray_later, crate::gray_later::GrayLater::Asked);
    render(&mut kernel);
    // ...but not where the screen set its refresh exactly.
    kernel.invalidate(whole);
    kernel.output.refresh_set = Some(RefreshMode::Fast);
    let effects = kernel.finish();
    assert!(!effects.grays());
    assert_eq!(
        kernel.frame.gray_later,
        crate::gray_later::GrayLater::Nothing
    );
    assert_eq!(
        effects.refresh().map(|(_, mode)| mode),
        Some(RefreshMode::Fast)
    );
}

#[test]
fn the_picture_pass_in_grays_covers_every_row() {
    let (mut kernel, grays) = comes_up_in_grays(PixelFormat::Mono1, true, "");
    assert!(!grays);
    assert!(after_the_calm(&mut kernel).grays());
    let mut next_row = 0;
    assert!(kernel.render_gray(&mut |top, rows| {
        assert_eq!(top, next_row);
        assert_eq!(rows.format, PixelFormat::Gray2);
        next_row += rows.height;
    }));
    assert_eq!(next_row, DEVICE.display.height);
}

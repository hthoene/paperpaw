//! Screens with pictures shown in grays on panels of black and white:
//! when, and when not (a screen that says no, a refresh set exactly).

use super::*;
use core::cell::Cell;

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

/// A half screen over the page, as a dialog is.
struct OverScreen;

impl Screen for OverScreen {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(Rect::new(0, 45, 90, 45))
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        canvas.fill_rect(0, 45, 90, 45, Color::BLACK);
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
        // Another full screen with no pictures to show in grays, and a half
        // screen over it (a dialog).
        r.add_screen(ScreenSpec::new(
            Id::new_static("cover.plain"),
            "plain",
            Box::new(|_, _| Box::new(CoverScreen { in_grays: false })),
        ))?;
        r.add_screen(ScreenSpec::new(
            Id::new_static("cover.over"),
            "over",
            Box::new(|_, _| Box::new(OverScreen)),
        ))?;
        r.add_screen(ScreenSpec::new(
            Id::new_static("cover.book"),
            "book",
            Box::new(move |_, _| Box::new(CoverScreen { in_grays })),
        ))?;
        r.bind_slot(
            slots::READER,
            Id::new_static("cover.book"),
            Priority::DEFAULT,
        )?;
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
fn a_refresh_set_exactly_is_not_turned_into_grays_and_keeps_the_pass() {
    use crate::gray_later::Phase;
    let (mut kernel, _) = comes_up_in_grays(PixelFormat::Mono1, true, "");
    render(&mut kernel);
    assert_eq!(kernel.tick(Instant(100)).wake_at(), Some(Instant(3_100)));
    let whole = kernel.framebuffer.bounds();
    // A focus step: the screen set its refresh exactly. Not grays, and the
    // pass is not lost.
    kernel.invalidate(whole);
    kernel.output.refresh_set = Some(RefreshMode::Fast);
    let effects = kernel.finish();
    assert!(!effects.grays());
    assert_eq!(
        effects.refresh().map(|(_, mode)| mode),
        Some(RefreshMode::Fast)
    );
    assert_eq!(kernel.frame.gray_later.phase, Phase::At(Instant(3_100)));
    render(&mut kernel);
    let effects = kernel.tick(Instant(3_100));
    assert!(effects.refresh().is_some() && effects.grays());
}

#[test]
fn a_dialog_over_the_screen_holds_the_pass_and_another_screen_drops_it() {
    // A dialog: the pass waits, and is counted from when it is gone.
    let (mut kernel, _) = comes_up_in_grays(PixelFormat::Mono1, true, "");
    render(&mut kernel);
    kernel.tick(Instant(100));
    kernel.open_screen(
        Instant(500),
        Id::new_static("cover.over"),
        paperpaw_sdk::Params::new(),
    );
    render(&mut kernel);
    let effects = kernel.tick(Instant(3_500));
    assert!(effects.refresh().is_none() && effects.wake_at().is_none());
    press(&mut kernel, 4_000, "back");
    // Not while it waits to be counted from its closing: the pass comes 3 s
    // after the last thing that happened.
    kernel.tick(Instant(5_000));
    let effects = kernel.tick(Instant(8_100));
    assert!(effects.refresh().is_some() && effects.grays());
    // Another full screen without pictures for grays: no pass for the first.
    let (mut kernel, _) = comes_up_in_grays(PixelFormat::Mono1, true, "");
    render(&mut kernel);
    kernel.tick(Instant(100));
    kernel.open_screen(
        Instant(500),
        Id::new_static("cover.plain"),
        paperpaw_sdk::Params::new(),
    );
    render(&mut kernel);
    let effects = kernel.tick(Instant(3_500));
    assert!(effects.refresh().is_none() && !effects.grays());
    assert!(effects.wake_at().is_none());
}

#[test]
fn a_screen_that_had_its_grays_keeps_them_until_a_key_and_a_frame_after_grays_loses_nothing() {
    use crate::gray_later::Phase;
    let (mut kernel, _) = comes_up_in_grays(PixelFormat::Mono1, true, "");
    assert!(after_the_calm(&mut kernel).grays());
    render(&mut kernel);
    assert_eq!(kernel.frame.gray_later.phase, Phase::Done);
    let whole = kernel.framebuffer.bounds();
    // It draws itself again (Home each minute): in grays at once.
    kernel.invalidate(whole);
    let effects = kernel.finish();
    assert!(effects.refresh().is_some() && effects.grays());
    render(&mut kernel);
    // A key, then it draws itself: black and white first, grays after.
    press(&mut kernel, 9_000, "down");
    kernel.invalidate(whole);
    let effects = kernel.finish();
    assert!(effects.refresh().is_some() && !effects.grays());
    assert_eq!(kernel.frame.gray_later.phase, Phase::Asked);
    // The X4's shell shows the next frame after grays drawn again: that
    // frame does not end the wait.
    let effects = kernel.draw_again(Instant(9_200));
    assert!(effects.refresh().is_some() && !effects.grays());
    assert_eq!(kernel.frame.gray_later.phase, Phase::Asked);
    render(&mut kernel);
    assert_eq!(kernel.frame.gray_later.phase, Phase::Pictured);
}

#[test]
fn the_screen_a_device_sleeps_on_is_in_grays_at_once_and_a_page_with_a_follow_up_is_counted_in_it()
{
    use crate::gray_later::Phase;
    let (mut kernel, _) = comes_up_in_grays(PixelFormat::Mono1, true, "");
    render(&mut kernel);
    kernel.sleep = true;
    let whole = kernel.framebuffer.bounds();
    kernel.invalidate(whole);
    let effects = kernel.finish();
    assert!(effects.sleep() && effects.grays());
    assert_eq!(kernel.frame.gray_later.phase, Phase::Nothing);
    // The pictures of a page may come in the frame after it: the wait stays
    // while that follow-up waits, and ends when it has none to show.
    kernel.frame.gray_later.phase = Phase::Asked;
    kernel.frame.followup = Some(crate::native_host::commands::Followup {
        screen: paperpaw_sdk::ScreenKey::new(1),
        mode: None,
        areas: Vec::new(),
    });
    kernel.gray_seen(0, 8_100);
    assert_eq!(kernel.frame.gray_later.phase, Phase::Asked);
    kernel.frame.followup = None;
    kernel.gray_seen(0, 8_100);
    assert_eq!(kernel.frame.gray_later.phase, Phase::Nothing);
    kernel.frame.gray_later.phase = Phase::Asked;
    kernel.gray_seen(3_600, 8_100);
    assert_eq!(kernel.frame.gray_later.phase, Phase::Pictured);
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

#[test]
fn a_book_page_keeps_its_grays_at_once() {
    // Home is in black and white first; a page of a book (the reader's slot)
    // as before: in grays where it has pictures, at once.
    let (mut kernel, grays) = comes_up_in_grays(PixelFormat::Mono1, true, "");
    assert!(!grays);
    render(&mut kernel);
    let effects = kernel.open_slot(Instant(500), slots::READER, paperpaw_sdk::Params::new());
    assert!(effects.refresh().is_some() && effects.grays());
    render(&mut kernel);
    // And no pass after it.
    let effects = kernel.tick(Instant(9_000));
    assert!(effects.refresh().is_none() && effects.wake_at().is_none());
}

#[test]
fn a_pass_due_in_a_call_that_makes_a_frame_comes_within_the_retry_with_no_event() {
    let (mut kernel, _) = comes_up_in_grays(PixelFormat::Mono1, true, "");
    render(&mut kernel);
    assert_eq!(kernel.tick(Instant(100)).wake_at(), Some(Instant(3_100)));
    // The calm is over in a call that itself draws a part of the screen.
    kernel.invalidate(Rect::new(0, 0, 8, 8));
    let effects = kernel.tick(Instant(3_100));
    assert!(!effects.grays() && effects.refresh().is_some());
    render(&mut kernel);
    let wake = effects.wake_at().expect("a call is asked for");
    assert!(wake.0 > 3_100 && wake.0 <= 3_100 + 250, "{wake:?}");
    let effects = kernel.tick(wake);
    assert!(effects.refresh().is_some() && effects.grays());
}

/// A home screen with no picture in its first frame; one cell comes later (a cover).
struct LateScreen(Rc<Cell<bool>>);

impl Screen for LateScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        if self.0.get() {
            let image = paperpaw_sdk::document::ImageRef {
                path: "p.gray".into(),
                width: 8,
                height: 8,
            };
            for x in [10, 40] {
                paperpaw_sdk::ui::draw_image(
                    canvas,
                    env.storage(),
                    &image,
                    Rect::new(x, 10, 20, 20),
                );
            }
        }
    }

    fn pictures_in_grays(&self) -> bool {
        true
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

struct LatePlugin(Rc<Cell<bool>>);

impl Plugin for LatePlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("late"), "Late", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let late = self.0.clone();
        r.add_screen(ScreenSpec::new(
            Id::new_static("late.home"),
            "home",
            Box::new(move |_, _| Box::new(LateScreen(late.clone()))),
        ))?;
        r.bind_slot(slots::HOME, Id::new_static("late.home"), Priority::DEFAULT)
    }
}

/// [`LateScreen`] on a panel of black and white, its first frame shown.
fn late_kernel() -> (Kernel, Rc<Cell<bool>>) {
    use paperpaw_sdk::image::{GrayWriter, ImageSink};
    let late = Rc::new(Cell::new(false));
    let storage = paperpaw_sdk::MemoryStorage::new();
    let mut writer = GrayWriter::new(&storage, "p.gray");
    writer.start(8, 8).unwrap();
    for _ in 0..8 {
        writer.row(&[90; 8]).unwrap();
    }
    writer.finish().unwrap();
    let device = DEVICE.with_display(DEVICE.display.with_format(PixelFormat::Mono1));
    let mut kernel = Kernel::builder(device)
        .storage(storage)
        .plugin(LatePlugin(late.clone()))
        .build()
        .unwrap();
    assert!(kernel.start(Instant(0)).refresh().is_some());
    render(&mut kernel);
    kernel.tick(Instant(100));
    (kernel, late)
}

/// A cover comes: its cell is drawn in a call of its own (no key).
fn a_cover_comes(kernel: &mut Kernel, late: &Cell<bool>) -> crate::Effects {
    late.set(true);
    kernel.invalidate(Rect::new(10, 10, 20, 20));
    kernel.finish()
}

#[test]
fn a_cover_that_comes_after_the_first_frame_gets_one_pass_after_the_calm() {
    use crate::gray_later::Phase;
    let (mut kernel, late) = late_kernel();
    assert_eq!(kernel.frame.gray_later.phase, Phase::Nothing);
    // The cell is shown dithered, and grays are due once the calm is over.
    let effects = a_cover_comes(&mut kernel, &late);
    assert!(effects.refresh().is_some() && !effects.grays());
    render(&mut kernel);
    let calm = kernel.tick(Instant(200)).wake_at();
    assert_eq!(calm, Some(Instant(3_200)));
    let effects = kernel.tick(Instant(3_200));
    assert!(effects.refresh().is_some() && effects.grays());
    render(&mut kernel);
    // A part frame is never marked in grays (the shell skips a small frame
    // over grays). A second cover after the pass has pictures: grays are due
    // again, once.
    late.set(true);
    kernel.invalidate(Rect::new(40, 10, 20, 20));
    let effects = kernel.finish();
    assert!(effects.refresh().is_some() && !effects.grays());
    render(&mut kernel);
    assert_eq!(kernel.frame.gray_later.phase, Phase::Pictured);
    kernel.tick(Instant(4_000));
    let effects = kernel.tick(Instant(7_000));
    assert!(effects.refresh().is_some() && effects.grays());
    render(&mut kernel);
    let effects = kernel.tick(Instant(20_000));
    assert!(effects.refresh().is_none() && effects.wake_at().is_none());
}

#[test]
fn a_part_without_pictures_leaves_the_grays_of_the_screen_as_they_are() {
    use crate::gray_later::Phase;
    let (mut kernel, late) = late_kernel();
    a_cover_comes(&mut kernel, &late);
    render(&mut kernel);
    kernel.tick(Instant(200));
    assert!(kernel.tick(Instant(3_200)).grays());
    render(&mut kernel);
    assert_eq!(kernel.frame.gray_later.phase, Phase::Done);
    // The clock: a small part with no picture, not in grays, no pass after.
    late.set(false);
    kernel.invalidate(Rect::new(40, 40, 10, 10));
    let effects = kernel.finish();
    assert!(effects.refresh().is_some() && !effects.grays());
    render(&mut kernel);
    assert_eq!(kernel.frame.gray_later.phase, Phase::Done);
    let effects = kernel.tick(Instant(20_000));
    assert!(effects.refresh().is_none() && effects.wake_at().is_none());
}

#[test]
fn a_focus_moved_over_a_cover_costs_no_pass() {
    use crate::gray_later::Phase;
    let (mut kernel, late) = late_kernel();
    late.set(true);
    // A key, and its frame is a part of the screen: no wait begins.
    press(&mut kernel, 500, "down");
    kernel.invalidate(Rect::new(10, 10, 20, 20));
    let effects = kernel.finish();
    assert!(effects.refresh().is_some() && !effects.grays());
    assert_eq!(kernel.frame.gray_later.phase, Phase::Nothing);
    render(&mut kernel);
    let effects = kernel.tick(Instant(9_000));
    assert!(effects.refresh().is_none() && effects.wake_at().is_none());
}

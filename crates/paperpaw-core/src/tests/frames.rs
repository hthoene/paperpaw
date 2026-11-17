//! What is drawn again and when: screens turned, slots opened anew,
//! overlays at the edge of the screen.

use super::*;
use crate::Items;
use paperpaw_sdk::Params;

const BASE: Id = Id::new_static("frames.base");
const PANEL: Id = Id::new_static("frames.panel");

struct Panel {
    region: Rect,
}

impl crate::Layer for Panel {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        let r = self.region;
        canvas.fill_rect(
            i32::from(r.x),
            i32::from(r.y),
            u32::from(r.width),
            u32::from(r.height),
            Color::BLACK,
        );
    }

    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

struct Base;

impl crate::Layer for Base {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

/// A full screen on the home slot, and an overlay on the menu slot from
/// row `top` (its parameter) down, reaching past the screen with `past`.
fn frames_plugin(kept: Log) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| {
        let log = kept.clone();
        r.add_layer(crate::LayerSpec::new(
            BASE,
            "base",
            Box::new(move |_, _| {
                log.borrow_mut().push(String::from("base made"));
                Box::new(Base)
            }),
        ))?;
        r.bind_slot(slots::HOME, BASE, Priority::DEFAULT)?;
        r.add_layer(crate::LayerSpec::new(
            PANEL,
            "panel",
            Box::new(|_, params| {
                let top = params.get("top").and_then(|t| t.parse().ok()).unwrap_or(45);
                let past = params.get("past").is_some();
                let height = if past { 90 } else { 90 - top };
                Box::new(Panel {
                    region: Rect::new(0, top, 90, height),
                })
            }),
        ))?;
        r.bind_slot(slots::MENU, PANEL, Priority::DEFAULT)?;
        Ok(())
    }
}

/// What [`frames_plugin`] registers under.
fn frames_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("frames"), "Frames", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

fn frames_kernel(builder: crate::KernelBuilder, log: &Log) -> Kernel {
    let mut kernel = builder
        .items(frames_plugin_manifest(), frames_plugin(log.clone()))
        .build()
        .unwrap();
    kernel.start(Instant(0));
    render(&mut kernel);
    kernel
}

fn panel(top: u16) -> Params {
    Params::new().with("top", alloc::format!("{top}"))
}

/// A slot opened again for something else shows only the new screen:
/// what the old one covered beyond it is drawn again.
#[test]
fn a_slot_opened_for_something_else_leaves_nothing_of_the_old_screen() {
    let log = log();
    let mut kernel = frames_kernel(Kernel::builder(DEVICE), &log);
    kernel.open_slot(Instant(10), slots::MENU, panel(30));
    render(&mut kernel);
    assert_eq!(kernel.frame().pixel(5, 40), Color::BLACK);
    kernel.open_slot(Instant(20), slots::MENU, panel(60));
    render(&mut kernel);
    assert_eq!(kernel.frame().pixel(5, 70), Color::BLACK);
    assert_eq!(kernel.frame().pixel(5, 40), Color::WHITE, "the old panel");
}

/// An overlay that says it reaches past the screen is refreshed only on
/// the screen.
#[test]
fn an_overlay_reaching_past_the_screen_refreshes_only_the_screen() {
    let log = log();
    let mut kernel = frames_kernel(Kernel::builder(DEVICE), &log);
    let params = panel(60).with("past", "1");
    let effects = kernel.open_slot(Instant(10), slots::MENU, params);
    let (region, _) = effects.refresh().unwrap();
    assert!(region.bottom() <= 90, "{region:?}");
    let effects = kernel.run_action(Instant(20), actions::BACK);
    let (region, _) = effects.refresh().unwrap();
    assert!(region.bottom() <= 90, "{region:?}");
}

/// Where the shell turns the screen, choosing another rotation makes the
/// screens anew once, when the shell resizes the kernel for it, not also
/// before; turned half way round (the same size) the row of hints goes
/// and comes back.
#[test]
fn turning_the_screen_makes_the_screens_once() {
    let log = log();
    let builder = Kernel::builder(DEVICE)
        .button_row(&["back", "confirm", "left", "right"])
        .rotation(true);
    let mut kernel = frames_kernel(builder, &log);
    let row = kernel.hint.height;
    assert!(row > 0);
    log.borrow_mut().clear();
    kernel
        .set_setting(Instant(10), "core.rotation", settings::ROTATIONS[2])
        .unwrap();
    assert!(log.borrow().is_empty(), "made before the shell turned it");
    kernel.resize(Instant(20), 90, 90);
    assert_eq!(*log.borrow(), ["base made"]);
    assert_eq!(kernel.hint.height, 0);
    log.borrow_mut().clear();
    kernel
        .set_setting(Instant(30), "core.rotation", settings::ROTATIONS[0])
        .unwrap();
    kernel.resize(Instant(40), 90, 90);
    assert_eq!(*log.borrow(), ["base made"]);
    assert_eq!(kernel.hint.height, row);
}

/// Turns the first refresh full, as a plugin that flashes on some page
/// turns does.
fn flash() -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    |r| {
        let seen = core::cell::Cell::new(0u32);
        r.add_refresh_filter(paperpaw_sdk::filters::RefreshFilterSpec::new(
            Id::new_static("flash.first"),
            "flash",
            Box::new(move |_, refresh| {
                seen.set(seen.get() + 1);
                if seen.get() == 2 && refresh.mode == RefreshMode::Partial {
                    paperpaw_sdk::filters::RefreshVerdict::Mode(RefreshMode::Full)
                } else {
                    paperpaw_sdk::filters::RefreshVerdict::Keep
                }
            }),
        ))
    }
}

/// What [`flash`] registers under.
fn flash_manifest() -> Manifest {
    Manifest::new(Id::new_static("flash"), "Flash", Version::new(1, 0, 0))
        .uses(paperpaw_sdk::Uses::NOTHING)
}

/// A full refresh a refresh filter made starts the count of partial
/// refreshes anew: the periodic full refresh comes as many refreshes
/// after it as after any other.
#[test]
fn a_full_refresh_from_a_filter_starts_the_count_anew() {
    let log = log();
    let builder = Kernel::builder(DEVICE)
        .items(flash_manifest(), flash())
        .setting_default(&settings::FULL_REFRESH_INTERVAL, 3);
    let mut kernel = frames_kernel(builder, &log);
    let mut modes = Vec::new();
    for i in 0..4u64 {
        kernel.apply(Command::Redraw(RefreshMode::Partial));
        let effects = kernel.finish();
        render(&mut kernel);
        modes.push(effects.refresh().map(|(_, mode)| mode).unwrap());
        kernel.tick(Instant(100 + i));
    }
    use RefreshMode::{Full, Partial};
    assert_eq!(modes, [Full, Partial, Partial, Full]);
}

/// How often the screen is refreshed in full may be a book's own (a
/// comic wants it more often than a novel).
#[test]
fn the_full_refresh_interval_may_be_a_books_own() {
    let kernel = Kernel::builder(DEVICE).build().unwrap();
    let spec = kernel
        .settings()
        .spec("core.full-refresh-interval")
        .unwrap();
    assert!(spec.scoped);
}

/// A frame of many bands is drawn in half the passes where the heap has the
/// room for a taller band and a picture's block after it, passed on in the
/// pieces of the bands there are, with the same pixels (#267).
#[test]
fn a_frame_of_many_bands_is_drawn_in_taller_bands_where_there_is_room() {
    static LARGEST: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    fn largest() -> Option<paperpaw_sdk::memory::MemoryUse> {
        let bytes = LARGEST.load(core::sync::atomic::Ordering::Relaxed);
        Some(paperpaw_sdk::memory::MemoryUse::new(bytes, 256 * 1024).with_largest_free(bytes))
    }
    let run = |bytes: u32| {
        LARGEST.store(bytes, core::sync::atomic::Ordering::Relaxed);
        let log = log();
        let mut kernel = Kernel::builder(DEVICE)
            .items(
                overlay_plugin_manifest(),
                overlay_plugin(OverlayPlugin { log: log.clone() }),
            )
            .frame_rows(16)
            .memory(largest)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        log.borrow_mut().clear();
        let mut copy = paperpaw_graphics::Framebuffer::new(90, 90, DEVICE.display.format).unwrap();
        let mut pieces = Vec::new();
        kernel.render(&mut |top, rows| {
            pieces.push(rows.height);
            copy.write_rows(top, rows);
        });
        let passes = log
            .borrow()
            .iter()
            .filter(|l| l.starts_with("draw base"))
            .count();
        assert_eq!(kernel.framebuffer.rows(), 16, "the band is as it was");
        (passes, pieces, copy.view().data.to_vec())
    };
    let (few, few_pieces, few_pixels) = run(10_000_000);
    let (many, many_pieces, many_pixels) = run(1_000);
    assert!(few < many, "{few} passes with room, {many} without");
    assert_eq!(few_pixels, many_pixels);
    for pieces in [&few_pieces, &many_pieces] {
        assert!(pieces.iter().all(|rows| *rows <= 16), "{pieces:?}");
        assert_eq!(pieces.iter().sum::<u16>(), 90);
    }
}

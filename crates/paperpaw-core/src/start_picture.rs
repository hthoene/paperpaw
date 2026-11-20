//! The start and restart screens plugins choose (#219).
//!
//! Plugins do not run yet while the device starts: the screen bound to the
//! slot [`slots::START`] is drawn ahead, as the device switches off or
//! restarts ([`Kernel::prepare_start_picture`]), into a 1-bit picture on the
//! card ([`PATH`], a binary PBM). The shell shows it at the next start before
//! the kernel is built ([`show`]), and its own screen where the file is
//! missing or bad. Nothing bound to the slot: the file is removed, and the
//! start looks as it always did.
//!
//! The restart screen (the slot [`slots::RESTART`]) is drawn by the kernel as
//! any frame ([`Kernel::restart_screen`]); the shell draws its own
//! *Restarting* where none is bound, the screen did not open or took longer
//! than [`LIMIT_MS`]. Neither asks anything of the power: the shell restarts
//! or switches off whatever the plugins did.

use super::*;
use alloc::rc::Rc;
use paperpaw_platform::PixelFormat;
use paperpaw_sdk::builtin::params;

/// The start picture on the card: a binary PBM (`P4`), 1 for black, as wide
/// and tall as the screen at its start.
pub const PATH: &str = ".paperpaw/start.pbm";
/// Where it is written first; left behind, a drawing that never ended.
const PART: &str = ".paperpaw/start.pbm.part";
/// How long a plugin's start or restart screen may take, from its opening
/// to its last row: then the device's own screen is shown instead.
pub const LIMIT_MS: u64 = 3_000;

/// What [`Kernel::prepare_start_picture`] did with the start picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Prepared {
    /// Drawn anew and saved.
    Written,
    /// No screen is bound to the slot: there is none (removed if there was).
    Removed,
    /// Left as it is: the battery is too low to draw it.
    Kept,
    /// Not drawn (the screen did not open, took too long, the card failed,
    /// or the last drawing never ended): removed, the device's own screen
    /// shows at the next start.
    Failed,
}

/// Why [`show`] showed no start picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Bad {
    /// There is none.
    Missing,
    /// Not a binary PBM of the screen's size, or cut short.
    Shape,
    /// No room for a band of it.
    Memory,
    /// The card failed while it was read.
    Read,
    /// The shell could not pass a band on to the panel.
    Shown,
}

/// The bytes of a row of `width` pixels, one bit each.
const fn stride(width: u16) -> usize {
    (width as usize).div_ceil(8)
}

/// The header [`PATH`] starts with for a screen of `width` × `height`.
fn header(width: u16, height: u16) -> String {
    format!("P4\n{width} {height}\n")
}

/// Shows the start picture of a screen of `width` × `height` pixels: checks
/// the file first (its header and its length) and passes nothing on for one
/// that is missing or of another shape; then passes it to `out` in bands of
/// `rows` rows (`out(top, band)`, as [`Kernel::render`] does, in
/// [`PixelFormat::Mono1`]), `out` saying whether the band was shown. Only a
/// card failing midway, or `out`, leaves rows passed and an error: the shell
/// draws its own screen over them then. Holds one band in memory.
pub fn show(
    storage: &dyn Storage,
    width: u16,
    height: u16,
    rows: u16,
    out: &mut dyn FnMut(u16, FrameView<'_>) -> bool,
) -> Result<(), Bad> {
    let head = header(width, height);
    let stride = stride(width);
    let body = stride as u64 * u64::from(height);
    match storage.size(PATH) {
        Ok(size) if size == head.len() as u64 + body => {}
        Ok(_) => return Err(Bad::Shape),
        Err(_) => return Err(Bad::Missing),
    }
    let mut first = [0; 16];
    let read = storage
        .read_at(PATH, 0, &mut first[..head.len().min(16)])
        .map_err(|_| Bad::Read)?;
    if first.get(..read) != Some(head.as_bytes()) {
        return Err(Bad::Shape);
    }
    let rows = rows.clamp(1, height.max(1));
    let mut band = Vec::new();
    band.try_reserve_exact(stride * usize::from(rows))
        .map_err(|_| Bad::Memory)?;
    band.resize(stride * usize::from(rows), 0);
    let mut top = 0;
    while top < height {
        let count = rows.min(height - top);
        let bytes = &mut band[..stride * usize::from(count)];
        let offset = head.len() as u64 + stride as u64 * u64::from(top);
        if storage.read_at(PATH, offset, bytes) != Ok(bytes.len()) {
            return Err(Bad::Read);
        }
        // PBM's 1 is black, the panel's white.
        for byte in bytes.iter_mut() {
            *byte = !*byte;
        }
        let view = FrameView::new(width, count, PixelFormat::Mono1, stride, bytes);
        if !out(top, view) {
            return Err(Bad::Shown);
        }
        top += count;
    }
    Ok(())
}

/// The rows of `view` as PBM rows (1 for black) onto `packed`.
fn pack(view: FrameView<'_>, packed: &mut Vec<u8>) {
    let stride = stride(view.width);
    packed.clear();
    for y in 0..view.height {
        let row = usize::from(y) * view.stride;
        match view.data.get(row..row + stride) {
            Some(bytes) if view.format == PixelFormat::Mono1 => {
                packed.extend(bytes.iter().map(|byte| !byte));
            }
            _ => {
                let start = packed.len();
                packed.resize(start + stride, 0);
                for x in 0..view.width {
                    let c = view.pixel(x, y);
                    let light = u32::from(c.r) * 77 + u32::from(c.g) * 150 + u32::from(c.b) * 29;
                    if light < 128 << 8 {
                        packed[start + usize::from(x / 8)] |= 0x80 >> (x % 8);
                    }
                }
            }
        }
        // The bits past the width are no pixels: 0.
        if let Some(last) = packed.last_mut()
            && !view.width.is_multiple_of(8)
        {
            *last &= 0xff << (8 - view.width % 8);
        }
    }
}

impl Kernel {
    /// Whether the screen shown came from `slot`.
    fn shows_slot(&self, slot: &Id) -> bool {
        self.stack.last().and_then(|s| s.slot.as_ref()) == Some(slot)
    }

    /// Draws the restart screen (the slot [`slots::RESTART`], told
    /// [`params::OFF_RESTART`]) for a shell about to restart, whole.
    /// `None` where no screen is bound, the screen did not open, or its
    /// opening took longer than [`LIMIT_MS`] by `clock`: the shell shows its
    /// own *Restarting*. The effects ask nothing of the power (the shell
    /// restarts after the frame, whatever the screen asked).
    pub fn restart_screen(&mut self, now: Instant, clock: &dyn Fn() -> Instant) -> Option<Effects> {
        if !self.shared.slots.contains(&slots::RESTART) {
            return None;
        }
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.now = now;
        self.output.commands.push(Ask::OpenSlot(
            slots::RESTART,
            Params::new().with(params::OFF_REASON, params::OFF_RESTART),
        ));
        self.output.commands.push(Ask::Redraw(RefreshMode::Full));
        self.run_commands();
        let mut effects = self.finish();
        if !self.shows_slot(&slots::RESTART) || clock().since(now) > LIMIT_MS {
            return None;
        }
        effects.restart = false;
        effects.sleep = false;
        effects.power_off = false;
        effects.usb_drive = false;
        Some(effects)
    }

    /// Draws the start screen (the slot [`slots::START`]) into [`PATH`] for
    /// the next start, the device about to switch off or restart; nothing is
    /// passed to the panel. Removes the picture where no screen is bound;
    /// leaves it on a battery too low; draws nothing where the last drawing
    /// never ended (a screen that hung). The screen gets [`LIMIT_MS`] by
    /// `clock` from its opening to its last row. The kernel is left showing
    /// it: called last, before the kernel stops.
    pub fn prepare_start_picture(&mut self, now: Instant, clock: &dyn Fn() -> Instant) -> Prepared {
        let storage = Rc::clone(&self.shared.storage);
        let bound = self.shared.slots.contains(&slots::START);
        let low = self.shared.status.battery.is_some_and(|battery| {
            !battery.charging && battery.percent < crate::awake::LOW_BATTERY_PERCENT
        });
        // The last drawing never ended: not tried again now.
        let hung = storage.exists(PART);
        if !bound || hung {
            for path in [PART, PATH] {
                if storage.exists(path) {
                    let _ = storage.remove(path);
                }
            }
            return if bound {
                Prepared::Failed
            } else {
                Prepared::Removed
            };
        }
        if low {
            return Prepared::Kept;
        }
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.now = now;
        let bounds = self.framebuffer.bounds();
        let (width, height) = (bounds.width, bounds.height);
        let written = storage
            .write(PART, header(width, height).as_bytes())
            .is_ok()
            && {
                self.output
                    .commands
                    .push(Ask::OpenSlot(slots::START, Params::new()));
                self.run_commands();
                let _ = self.finish();
                self.shows_slot(&slots::START) && self.draw_start(storage.as_ref(), now, clock)
            };
        // Drawn again whole for whoever shows the frame next.
        self.invalidate(bounds);
        let saved = written
            && (!storage.exists(PATH) || storage.remove(PATH).is_ok())
            && storage.rename(PART, PATH).is_ok();
        if saved {
            return Prepared::Written;
        }
        for path in [PART, PATH] {
            let _ = storage.remove(path);
        }
        Prepared::Failed
    }

    /// The frame drawn band by band onto [`PART`], within the time limit.
    fn draw_start(
        &mut self,
        storage: &dyn Storage,
        now: Instant,
        clock: &dyn Fn() -> Instant,
    ) -> bool {
        let bounds = self.framebuffer.bounds();
        let rows = self.framebuffer.rows().max(1);
        let mut packed = Vec::new();
        if packed
            .try_reserve_exact(stride(bounds.width) * usize::from(rows))
            .is_err()
        {
            return false;
        }
        self.sync_look();
        let mut top = 0;
        while top < bounds.height {
            if clock().since(now) > LIMIT_MS {
                return false;
            }
            self.framebuffer.set_top(top);
            let band = self.framebuffer.band();
            self.redraw(band);
            pack(self.framebuffer.view_rows(band), &mut packed);
            if storage.append(PART, &packed).is_err() {
                return false;
            }
            top = top.saturating_add(rows);
        }
        self.framebuffer.end_frame();
        clock().since(now) <= LIMIT_MS
    }
}

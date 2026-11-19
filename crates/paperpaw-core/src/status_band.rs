//! The host's status bar (#123, `docs/abi/ui.md` section 1.2): a band at the
//! top of the display, above every full screen whose resolved choice is
//! "shown". The screen gets the rest.
//!
//! - **Resolved** in this order: the user's (or another plugin's) line for the
//!   screen in `core.status-bar.screens` (`<screen id>=shown|hidden`, at most
//!   512 bytes read), then the screen's `screen.spec.flags` bits 11-12 (or
//!   what it set with `command.screen_policy`), then the device default:
//!   `core.status-bar.all`, off unless the user turned it on. Overlays and
//!   screens with all of the panel their own never get one.
//! - **What it shows**: `core.status-bar.line`, the kernel's line of items
//!   (the time and the battery, answered by the host as op 14 answers them)
//!   or another line the host draws itself; empty is no band at all.
//! - **Drawn** apart from the screen: a change of status (event 73) or of the
//!   minute marks only the band, so the panel refreshes that strip alone.
//!
//! Memory: no state kept but the minute last drawn; the line's layout lives
//! while the band is drawn.
use super::*;
use paperpaw_sdk::SettingSpec;
use paperpaw_sdk::builtin::{fonts, groups, status_items, status_lines};

/// The lines `<screen id>=shown|hidden` that choose for one screen.
pub(crate) const SCREENS: &str = "core.status-bar.screens";
/// The status line the bar shows; empty: none.
pub(crate) const LINE: &str = "core.status-bar.line";
/// The device default: the bar on every screen that does not say otherwise.
pub(crate) const ALL: &str = "core.status-bar.all";
/// How much of [`SCREENS`] is read.
const SCREENS_MAX: usize = 512;

/// The settings of the status bar.
pub(crate) static SETTINGS: [SettingSpec; 3] = [
    SettingSpec::switch(ALL, "core.setting.status-bar", false, groups::STATUS_LINES)
        .description("core.setting.status-bar.help"),
    SettingSpec::text(
        LINE,
        "core.setting.status-bar.line",
        status_lines::ITEMS.static_str(),
        groups::STATUS_LINES,
    )
    .description("core.setting.status-bar.line.help")
    .advanced(),
    SettingSpec::text(
        SCREENS,
        "core.setting.status-bar.screens",
        "",
        groups::STATUS_LINES,
    )
    .description("core.setting.status-bar.screens.help")
    .advanced(),
];

/// Whether the bar shows above screen `id` whose bits 11-12 are `declared`:
/// `screens` (the setting) first, then `declared`, then `all`.
pub(crate) fn shown(screens: &str, id: &str, declared: u32, all: bool) -> bool {
    let mut read = 0;
    for line in screens.split('\n') {
        read += line.len() + 1;
        if read > SCREENS_MAX + 1 {
            break;
        }
        if let Some((screen, value)) = line.split_once('=')
            && screen.trim() == id
        {
            match value.trim() {
                "shown" => return true,
                "hidden" => return false,
                _ => {}
            }
        }
    }
    match declared {
        1 => true,
        2 => false,
        _ => all,
    }
}

/// The kernel's line of items in the bar: the time, and the battery.
const ITEMS: [&str; 3] = [
    status_items::CLOCK.static_str(),
    "",
    status_items::BATTERY.static_str(),
];

impl Kernel {
    /// The height of the band above screen `index`, 0 where it has none.
    pub(crate) fn status_band(&self, index: usize) -> u16 {
        let Some(active) = self.stack.get(index) else {
            return 0;
        };
        if active.screen.mode().is_full_screen() || self.overlay(index).is_some() {
            return 0;
        }
        self.status_band_of(active.id.as_str(), active.status_bar())
    }

    /// The height of the band above full screen `id` with bits `declared`.
    pub(crate) fn status_band_of(&self, id: &str, declared: u32) -> u16 {
        let settings = &self.shared.settings;
        if settings.text(LINE).is_empty()
            || !shown(settings.text(SCREENS), id, declared, settings.bool(ALL))
        {
            return 0;
        }
        let env = self.shared.place(self.shared.now);
        let size = crate::kit::ui::UiStyle::new(&env).small();
        let panel = self.framebuffer.bounds().height;
        // 1.8 times the text size, the kernel line's height.
        ((size * 1.8) as u16).clamp(1, panel / 4)
    }

    /// The band shown now: above the topmost full screen, where it has one.
    pub(crate) fn status_band_area(&self) -> Option<Rect> {
        let active = self.stack.get(self.full_base())?;
        (active.area.y > 0 && self.overlay(self.full_base()).is_none())
            .then(|| Rect::new(0, 0, self.framebuffer.bounds().width, active.area.y))
    }

    /// Marks the band to be drawn again (a status changed, a minute went by):
    /// only it, so the panel refreshes that strip alone.
    pub(crate) fn status_band_changed(&mut self) {
        if let Some(band) = self.status_band_area() {
            self.invalidate(band);
        }
    }

    /// At a tick: the band again where the minute changed (its clock).
    pub(crate) fn status_band_tick(&mut self) {
        let minute = self
            .shared
            .status
            .clock
            .map_or(0, |c| c.unix_millis_at(self.shared.now) / 60_000);
        if self.frame.band_minute != minute {
            self.frame.band_minute = minute;
            self.status_band_changed();
        }
    }
}

/// Draws the band `band` of the bar: the line `core.status-bar.line` names,
/// the kernel's items where it names that or a line the host cannot draw
/// itself, and a rule below it.
#[inline(never)]
pub(crate) fn draw(canvas: &mut paperpaw_graphics::Canvas<'_>, env: &Place<'_>, band: Rect) {
    let style = crate::kit::ui::UiStyle::new(env);
    let size = style.small();
    let margin = style.margin().min(band.width / 4);
    let inner = Rect::new(
        margin,
        0,
        band.width - 2 * margin,
        band.height.saturating_sub(1),
    );
    let line = env.settings().text(LINE);
    let items = status_lines::ITEMS.static_str();
    let context = Params::default();
    let frame = crate::kit::status::StatusFrame::new(inner, true, size, Color::BLACK, &context);
    let drawn = line != items
        && line != "reader.status-line.items"
        && env.status_items().draw_line(line, canvas, env, &frame);
    if !drawn {
        let font = env
            .fonts()
            .role(fonts::STATUS.static_str())
            .or_else(|| env.fonts().role(fonts::UI.static_str()));
        if let Some(font) = font {
            crate::kit::status::StatusLine::new(inner, ITEMS, size, Color::BLACK)
                .draw(canvas, env, &font, &context);
        }
    }
    canvas.fill(
        Rect::new(0, band.height.saturating_sub(1), band.width, 1),
        Color::BLACK,
    );
}

#[cfg(test)]
#[path = "status_band_tests.rs"]
mod tests;

//! Hosting many native plugins as one batch (#306, row 9): each admission
//! leaves the refreshes that look at **all** plugins (the key map, the slots
//! plugins can ask for, the font choices and fonts in use, the widgets' font
//! settings, the sealed secrets) to the end of the batch, which makes them
//! once. Where each admission made them, hosting the 28 linked plugins
//! rebuilt the key map (which parses the user's configuration again) and the
//! font lists up to 28 times.
//!
//! The batch is the shell's call ([`Kernel::begin_native_batch`] and
//! [`Kernel::end_native_batch`]; `paperpaw_app::register_native_plugins`
//! makes both), so what the caller sees after it is what one-by-one hosting
//! gave.
use super::{flat::FlatDepth, shell};
use crate::Kernel;
use paperpaw_native::host_glue::Services;

/// What a batch counted at its start, to see at its end what the plugins
/// admitted since brought (as each admission did for itself).
#[derive(Debug, Clone, Copy)]
pub(crate) struct NativeBatch {
    fonts: usize,
    offers: usize,
    keys: usize,
}

/// Where the time of a batch went, in microseconds by the clock set with
/// [`Kernel::set_native_clock`] (zero without one).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct NativeTimes {
    /// All admissions together (the plugins' own `make` included).
    pub admit: u32,
    /// The key map, made anew.
    pub keymap: u32,
    /// The font choices and the reading and status fonts.
    pub fonts: u32,
    /// The widgets' font settings and the configuration values kept for them.
    pub widget_fonts: u32,
    /// [`Kernel::start`] before the start event: the reading apps, the
    /// reserves given back, the passwords and the memory notice (#403).
    pub start_prepare: u32,
    /// The start event and the commands it and the home slot gave.
    pub start_events: u32,
    /// The passes after them until nothing more is due (`finish_settled`).
    pub start_settle: u32,
}

impl Kernel {
    /// Sets the clock (microseconds, wrapping) the phases of hosting and of
    /// the start are timed with ([`Self::native_times`]). Shells that log start-up times
    /// set it; none, no time is taken.
    pub fn set_native_clock(&mut self, clock: fn() -> u32) {
        self.native_clock = Some(clock);
    }

    /// Where the time of hosting and of the start went so far ([`NativeTimes`]).
    #[must_use]
    pub fn native_times(&self) -> NativeTimes {
        self.native_times
    }

    /// The clock now, zero without one.
    pub(crate) fn native_now(&self) -> u32 {
        self.native_clock.map_or(0, |clock| clock())
    }

    /// Adds the time since `from` to the phase `of` points at.
    pub(crate) fn native_took(&mut self, from: u32, of: fn(&mut NativeTimes) -> &mut u32) {
        if self.native_clock.is_some() {
            let spent = self.native_now().wrapping_sub(from);
            let phase = of(&mut self.native_times);
            *phase = phase.saturating_add(spent);
        }
    }

    /// Room for `plugins` more plugins in the tables every admission adds a
    /// row to, asked for once: they do not grow by doubling and leave
    /// holes. Where the heap has no room the tables grow as they did.
    pub fn reserve_native_plugins(&mut self, plugins: usize) {
        let _ = self.shared.plugins.try_reserve(plugins);
        self.shared.native_instances.reserve_plugins(plugins);
    }

    /// The admission of one plugin of a batch: timed, and none of the
    /// refreshes [`Self::end_native_batch`] makes.
    pub(super) fn admit_in_batch<S: Services + 'static, M: 'static>(
        &mut self,
        host: shell::Hosted<S, M>,
        depth: &FlatDepth,
        folder: Option<&str>,
        report: fn(&S, &str, u32, i32),
        fixed: super::FixedTables,
        park: Option<super::park_linked::ParkArgs<S>>,
    ) -> Result<(), shell::Error> {
        let from = self.native_now();
        let admitted = self.admit_native_with(host, depth, folder, report, fixed, park);
        self.native_took(from, |times| &mut times.admit);
        admitted
    }

    /// Starts a batch: the plugins admitted from here to
    /// [`Self::end_native_batch`] are not refreshed for one by one. Calling
    /// it again while a batch is open changes nothing.
    pub fn begin_native_batch(&mut self) {
        if self.native_batch.is_none() {
            self.native_batch = Some(NativeBatch {
                fonts: self.shared.fonts.ids().count(),
                offers: self.shared.registry.offers(),
                keys: self.native_key_count(),
            });
        }
    }

    /// Ends the batch: the refreshes each admission would have made, made
    /// once for all of them, in the order an admission made them.
    pub fn end_native_batch(&mut self) {
        let Some(began) = self.native_batch.take() else {
            return;
        };
        self.open_sealed_secrets();
        if self.native_key_count() != began.keys {
            let from = self.native_now();
            self.rebuild_keymap();
            self.native_took(from, |times| &mut times.keymap);
        }
        if self.shared.registry.offers() != began.offers {
            self.refresh_slots();
        }
        let from = self.native_now();
        if self.shared.fonts.ids().count() != began.fonts {
            self.offer_font_choices();
            self.apply_reading_font();
            self.apply_status_font();
            self.apply_widget_fonts();
        }
        self.native_took(from, |times| &mut times.fonts);
        let from = self.native_now();
        self.widget_fonts_admitted();
        self.native_took(from, |times| &mut times.widget_fonts);
    }
}

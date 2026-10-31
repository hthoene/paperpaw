//! Render hooks: code that changes a finished band of the screen before
//! the shell gets it, for filters on pixels (a night or sepia tint, a
//! grain, the dither of the four-gray pass, an accessibility tint).
//!
//! **Check [`Capability::RENDER_HOOK`](crate::Capability::RENDER_HOOK)
//! first.** A hook is accepted by [`Registrar::add_render_hook`](crate::Registrar::add_render_hook) on every
//! PaperPaw, but runs only where the device says it can: without the
//! capability it is accepted and never called. Even then it runs only if
//! the reader allows display hooks and the hook's own switch is on.
//!
//! A hook sees **pixels, not coverage**: in a black-and-white band the edge
//! of a letter is already black or white, so anti-aliasing of letters is
//! done by the tone curve (`core.text-curve`), or in the four-gray pass.
//! A hook is for filters on finished pixels.
//!
//! The rules of a hook:
//!
//! - Looks, not metrics: it changes the band's pixels and its carry,
//!   nothing else.
//! - For the same band, rows above, carry and settings it gives the same
//!   rows and carry. It has no clock, storage or random to ask ([`HookCx`]),
//!   so a page drawn twice is the same picture.
//! - State that must cross bands (the error row of a dither) goes in the
//!   [`carry`](BandView::carry), which the kernel owns, zeroes at the
//!   start of each pass and keeps from band to band. Nothing is kept from
//!   one pass to the next.
//! - Memory is the kernel's: the carry is at most four bytes per pixel of
//!   the frame's width for one hook, and a hook that asks for more gets
//!   less (check [`BandView::carry`]'s length).
//! - A panic in a hook aborts the device. Hooks never run for the lock
//!   screen.
//!
//! The kernel orders the hooks of a stage by [`RenderHookSpec::order`], then
//! by the hook's id, then by registration.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use core::fmt;

pub use paperpaw_graphics::{BandView, FramePass, Region, RegionKind};

use crate::id::Id;
use crate::setting::{Setting, SettingType};
use crate::{Lang, Settings};

/// A hook: changes `band` (see the [module](self)).
pub type RenderHookFn = Box<dyn Fn(&mut BandView<'_>, &HookCx<'_>)>;

/// When in drawing a frame a hook runs: constants of one open type. The
/// numbers are identities, in the order of the stages, with gaps; 0 is no
/// stage, and a hook of a stage this release does not know is kept and
/// never run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HookStage(u16);

impl HookStage {
    /// Reserved, not run yet: before a band is drawn, so a hook may set
    /// its background (a paper texture).
    pub const BEFORE_BAND: Self = Self(16);
    /// A band is drawn, in the frame's own format, before it is passed on.
    pub const AFTER_BAND: Self = Self(32);
    /// Reserved, not run yet: a screen's own drawing is done.
    pub const AFTER_SCREEN: Self = Self(48);
    /// Reserved, not run yet: before the interface around a page.
    pub const BEFORE_CHROME: Self = Self(64);
    /// Reserved, not run yet: the whole frame is drawn.
    pub const AFTER_FRAME: Self = Self(80);

    /// The stage's number.
    pub const fn value(self) -> u16 {
        self.0
    }

    /// The stage of a number.
    pub const fn from_value(value: u16) -> Self {
        Self(value)
    }
}

/// What a hook may ask of the world: the settings (read only), the
/// language and script of the interface, and the version of the hook
/// interface the kernel runs. No storage, clock or network, no commands:
/// a hook cannot read the time or ask for a refresh.
pub struct HookCx<'a> {
    settings: &'a Settings,
    lang: Lang,
    revision: u16,
    /// Zero now; read through methods in a later release.
    spare: [u32; 4],
}

impl<'a> HookCx<'a> {
    /// For the kernel (and tests): the context a hook runs in.
    pub fn new(settings: &'a Settings, lang: Lang, revision: u16) -> Self {
        Self {
            settings,
            lang,
            revision,
            spare: [0; 4],
        }
    }

    /// The value of `setting`, read only (settings do not change while a
    /// pass is drawn): the type's empty value if it is unknown.
    pub fn setting<T: SettingType>(&self, setting: &Setting<T>) -> T::Value<'_> {
        self.settings.read(setting)
    }

    /// The language of the interface.
    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// The script of the interface's language (ISO 15924, `Arab`).
    pub fn script(&self) -> Option<&'static str> {
        self.lang.script()
    }

    /// The version of the hook interface the kernel runs (0 now), so a
    /// hook can adapt to what it offers.
    pub fn revision(&self) -> u16 {
        self.revision
    }
}

impl fmt::Debug for HookCx<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HookCx")
            .field("lang", &self.lang)
            .field("revision", &self.revision)
            .field("spare", &self.spare)
            .finish_non_exhaustive()
    }
}

/// A render hook and when and for what it runs (see the [module](self)).
pub struct RenderHookSpec {
    /// Its id, in the plugin's namespace.
    pub id: Id,
    /// Translation key of its name, in the plugin's own catalogue.
    pub title: Cow<'static, str>,
    /// When it runs.
    pub stage: HookStage,
    /// The passes it wants, a set (`FramePass::MONO | FramePass::GRAY`);
    /// others skip it.
    pub passes: FramePass,
    /// Lower runs first; at equal order by id, then by registration. 0.
    pub order: i16,
    /// The bool setting of the plugin that turns this hook on and off. A
    /// switch that names no registered setting counts as off (and is
    /// logged once). The reader can always turn the hook off.
    pub switch: Id,
    /// Rows of the band above that it needs to see, 0 to 4 (more count
    /// as 4). Rows below are not offered.
    pub overlap: u8,
    /// Bytes of kernel-owned scratch it wants for a pass (see
    /// [`BandView::carry`]); the kernel grants at most four bytes a pixel
    /// of the frame's width for one hook.
    pub carry: u16,
    /// Reserved, zero.
    pub flags: u32,
    /// Zero now.
    spare: [u32; 2],
    /// The hook.
    pub hook: RenderHookFn,
}

impl RenderHookSpec {
    /// A hook of `stage` for both passes, no overlap, no carry.
    pub fn new(
        id: Id,
        title: impl Into<Cow<'static, str>>,
        stage: HookStage,
        switch: Id,
        hook: RenderHookFn,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            stage,
            passes: FramePass::MONO | FramePass::GRAY,
            order: 0,
            switch,
            overlap: 0,
            carry: 0,
            flags: 0,
            spare: [0; 2],
            hook,
        }
    }

    /// Wanting `passes`.
    #[must_use]
    pub fn with_passes(mut self, passes: FramePass) -> Self {
        self.passes = passes;
        self
    }

    /// Running at `order` among the hooks of its stage.
    #[must_use]
    pub fn with_order(mut self, order: i16) -> Self {
        self.order = order;
        self
    }

    /// Seeing `rows` of the band above (at most 4).
    #[must_use]
    pub fn with_overlap(mut self, rows: u8) -> Self {
        self.overlap = rows.min(4);
        self
    }

    /// Wanting `bytes` of carry for a pass.
    #[must_use]
    pub fn with_carry(mut self, bytes: u16) -> Self {
        self.carry = bytes;
        self
    }
}

impl fmt::Debug for RenderHookSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderHookSpec")
            .field("id", &self.id)
            .field("stage", &self.stage)
            .field("passes", &self.passes)
            .field("order", &self.order)
            .field("switch", &self.switch)
            .field("overlap", &self.overlap)
            .field("carry", &self.carry)
            .field("spare", &self.spare)
            .finish_non_exhaustive()
    }
}

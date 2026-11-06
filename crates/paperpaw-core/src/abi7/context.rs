//! Scoped core views inside a stable host-owned context.
//!
//! The owner context may outlive an event's Env or Canvas. It must not retain
//! their Rust references. This slot admits a frame only for one synchronous
//! dispatch, clears it on every exit, and refuses nested installation.

use super::snapshot_wire::Snapshot;
use core::{cell::Cell, marker::PhantomPinned, ops::ControlFlow, pin::Pin, ptr::NonNull};
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::{ERR_NOT_FOUND, ERR_STATE};
use paperpaw_sdk::{Env, Plugins, Settings, usage::Usage, work::WorkStates};

#[allow(
    dead_code,
    reason = "service view fields are consumed by slots 17-19 encoders"
)]
#[path = "usage_views.rs"]
mod usage_views;
pub(crate) use usage_views::{DeviceUsageView, PluginRow};

/// Table position after a scoped visitor stops; counters are not snapshotted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RowsEnd {
    pub(crate) total: u32,
    pub(crate) generation: u32,
    pub(crate) next: u32,
}

/// Separate shared field borrows, assembled by core dispatch before foreign code.
/// This holds no mutable plugin state and never clones the meter's table.
#[allow(
    dead_code,
    reason = "core service frame awaits slots 17-19 host wiring"
)]
pub(crate) struct CoreFrame<'a> {
    pub(crate) env: &'a Env<'a>,
    pub(crate) meter: &'a crate::meter::Meter,
    pub(crate) usage: &'a Usage,
    pub(crate) plugins: &'a Plugins,
    pub(crate) work: &'a WorkStates,
}

impl Frame for CoreFrame<'_> {
    fn settings(&self) -> &Settings {
        self.env.settings()
    }
    fn translator(&self) -> &Translator {
        self.env.i18n()
    }
    fn snapshot(&self) -> Result<Snapshot, i32> {
        Frame::snapshot(self.env)
    }
    fn device_usage(&self) -> Result<DeviceUsageView, i32> {
        Ok(self.usage.into())
    }
    fn plugin_rows(
        &self,
        from: u32,
        visitor: &mut dyn FnMut(&PluginRow<'_>) -> ControlFlow<()>,
    ) -> Result<RowsEnd, i32> {
        self.meter.visit_rows(from, visitor)
    }
    fn plugin_name(&self, plugin: &str, visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        let id = if plugin == crate::meter::CORE {
            crate::meter::CORE
        } else {
            self.plugins
                .get(plugin)
                .ok_or(ERR_NOT_FOUND)?
                .manifest
                .id
                .as_str()
        };
        // A bounded stack key avoids PluginInfo::name's formatted allocation.
        let suffix = b".plugin.name";
        let mut key = [0u8; 128];
        let Some(len) = id
            .len()
            .checked_add(suffix.len())
            .filter(|&len| len <= key.len())
        else {
            visit(usage_views::clipped(id, 128));
            return Ok(());
        };
        key[..id.len()].copy_from_slice(id.as_bytes());
        key[id.len()..len].copy_from_slice(suffix);
        let key = core::str::from_utf8(&key[..len]).map_err(|_| ERR_STATE)?;
        visit(usage_views::clipped(
            self.env.i18n().get(key).unwrap_or(id),
            128,
        ));
        Ok(())
    }
    fn work_note(&self, plugin: &str, visit: &mut dyn FnMut(u64, &str)) -> Result<(), i32> {
        let info = self.plugins.get(plugin).ok_or(ERR_NOT_FOUND)?;
        let note = self.work.note(&info.manifest.id).ok_or(ERR_NOT_FOUND)?;
        visit(note.at.millis(), &note.text);
        Ok(())
    }
}

#[allow(dead_code, reason = "usage service methods await slots 17-19 adapters")]
pub(crate) trait Frame {
    fn settings(&self) -> &Settings;
    fn translator(&self) -> &Translator;
    fn device_usage(&self) -> Result<DeviceUsageView, i32> {
        Err(ERR_STATE)
    }
    /// Continue consumes the row; Break leaves it for the next page.
    /// The visitor is host-only and must never call plugin code while borrowing rows.
    fn plugin_rows(
        &self,
        _from: u32,
        _visitor: &mut dyn FnMut(&PluginRow<'_>) -> ControlFlow<()>,
    ) -> Result<RowsEnd, i32> {
        Err(ERR_STATE)
    }
    fn plugin_name(&self, _plugin: &str, _visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        Err(ERR_STATE)
    }
    fn work_note(&self, _plugin: &str, _visit: &mut dyn FnMut(u64, &str)) -> Result<(), i32> {
        Err(ERR_STATE)
    }
    /// Query-only frames need not provide an environment snapshot.
    fn snapshot(&self) -> Result<Snapshot, i32> {
        Err(paperpaw_native::host_glue::ERR_STATE)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    Busy,
    NoFrame,
}

pub(crate) struct FrameSlot {
    current: Cell<Option<NonNull<dyn Frame>>>,
    _pin: PhantomPinned,
}

struct Reset<'a>(&'a FrameSlot);

impl Drop for Reset<'_> {
    fn drop(&mut self) {
        self.0.current.set(None);
    }
}

impl FrameSlot {
    pub(crate) const fn new() -> Self {
        Self {
            current: Cell::new(None),
            _pin: PhantomPinned,
        }
    }

    /// The closure is synchronous. Neither it nor the raw ABI may keep a frame
    /// view after it returns. Pinning also keeps a surrounding owner context's
    /// address stable while foreign code holds its pointer.
    pub(crate) fn scope<R>(
        self: Pin<&Self>,
        frame: &dyn Frame,
        call: impl FnOnce() -> R,
    ) -> Result<R, Error> {
        if self.current.get().is_some() {
            return Err(Error::Busy);
        }
        let borrowed = NonNull::from(frame);
        // SAFETY: the erased lifetime is restored only inside `with`. The
        // Reset guard clears the pointer before this borrow ends, including
        // unwinding. Nested scope replacement is refused and Cell prevents
        // cross-thread sharing. No pointer or borrowed frame is returned.
        let erased = unsafe {
            core::mem::transmute::<NonNull<dyn Frame + '_>, NonNull<dyn Frame + 'static>>(borrowed)
        };
        self.current.set(Some(erased));
        let _reset = Reset(self.get_ref());
        Ok(call())
    }

    pub(crate) fn with<R>(&self, read: impl FnOnce(&dyn Frame) -> R) -> Result<R, Error> {
        let pointer = self.current.get().ok_or(Error::NoFrame)?;
        // SAFETY: a present pointer belongs to a live scope. Its shared frame
        // reference is limited to `read`; R cannot borrow that reference.
        Ok(read(unsafe { pointer.as_ref() }))
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;

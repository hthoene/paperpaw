//! Core field borrows for usage service callbacks, separate from fixture frames.
use super::context::{DeviceUsageView, Frame, PluginRow, RowsEnd, usage_views};
use super::snapshot_wire::Snapshot;
use core::ops::ControlFlow;
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::{ERR_NOT_FOUND, ERR_STATE};
use paperpaw_sdk::{Env, Plugins, Settings, usage::Usage, work::WorkStates};

/// Separate shared field borrows, assembled by core dispatch before foreign code.
/// This holds no mutable plugin state and never clones the meter's table.
pub(crate) struct CoreFrame<'a> {
    pub(crate) env: &'a Env<'a>,
    pub(crate) meter: &'a crate::meter::Meter,
    pub(crate) font_removals: &'a crate::native_host::font_removals::FontRemovals,
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
    fn fonts(&self) -> Option<&paperpaw_sdk::Fonts> {
        Some(self.env.fonts())
    }
    fn font_removals(&self) -> Option<&crate::native_host::font_removals::FontRemovals> {
        Some(self.font_removals)
    }
    fn storage(&self) -> Option<&dyn paperpaw_platform::Storage> {
        Some(self.env.storage())
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

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;

//! Core field borrows for usage service callbacks, separate from fixture frames.
use super::context::registry_views::{Asker, RegistryView, Row};
use super::context::{DeviceUsageView, Frame, PluginRow, RowsEnd, usage_views};
use super::snapshot_wire::Snapshot;
use core::ops::ControlFlow;
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::{ERR_NOT_FOUND, ERR_STATE};
use paperpaw_sdk::{Env, Plugins, Settings, usage::Usage, work::WorkStates};

/// Host-only callback identity, never decoded from plugin bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CallbackStamp {
    pub(crate) owner: u64,
    pub(crate) epoch: u64,
}

impl CallbackStamp {
    pub(crate) fn new(owner: u64, epoch: u64) -> Result<Self, i32> {
        if epoch == 0 {
            return Err(ERR_STATE);
        }
        Ok(Self { owner, epoch })
    }
}

/// Separate shared field borrows, assembled by core dispatch before foreign code.
/// This holds no mutable plugin state and never clones the meter's table.
pub(crate) struct CoreFrame<'a> {
    pub(crate) stack_rows: &'a crate::native_host::stack_rows::StackRows,
    pub(crate) stack_generation: u64,
    pub(crate) depth: Option<&'a crate::native_host::DispatchDepth>,
    pub(crate) directory: Option<&'a crate::native_host::native_declarations::NativeDeclarations>,
    pub(crate) callback: Option<CallbackStamp>,
    pub(crate) instance: Option<u32>,
    pub(crate) env: &'a Env<'a>,
    pub(crate) meter: &'a alloc::rc::Rc<crate::meter::Meter>,
    pub(crate) font_removals: &'a crate::native_host::font_removals::FontRemovals,
    pub(crate) registry_generation: &'a crate::native_host::registry_generation::RegistryGeneration,
    pub(crate) usage: &'a Usage,
    pub(crate) plugins: &'a Plugins,
    pub(crate) work: &'a WorkStates,
    /// The device's `core.ui` service: the widget handles slot 35 resolves.
    pub(crate) ui: Option<&'a crate::native_host::ui_service::UiServices>,
    /// The shell's device log, if it gave one (`Frame::log`).
    pub(crate) log: crate::native_host::device_log::DeviceLog,
}

impl Frame for CoreFrame<'_> {
    fn log(&self, line: core::fmt::Arguments<'_>) {
        self.log.line(line);
    }

    fn stage_run(
        &self,
        _caller_id: &str,
        full_stage: &str,
        cursor: u32,
        input: &[u8],
        out: &mut [core::mem::MaybeUninit<u8>],
    ) -> Result<usize, i32> {
        crate::native_host::stage_run::run(self, full_stage, cursor, input, out)
    }

    fn caller(&self) -> Result<u64, i32> {
        self.callback
            .filter(|stamp| stamp.epoch != 0)
            .map(|stamp| stamp.owner)
            .ok_or(ERR_STATE)
    }
    fn callback_epoch(&self) -> Result<u64, i32> {
        self.callback
            .filter(|stamp| stamp.epoch != 0)
            .map(|stamp| stamp.epoch)
            .ok_or(ERR_STATE)
    }

    fn calling_instance(&self) -> Result<u32, i32> {
        self.callback_epoch()?;
        let instance = self
            .instance
            .filter(|instance| *instance != 0)
            .ok_or(ERR_STATE)?;
        self.stack_rows
            .instance(instance, self.stack_generation)
            .filter(|row| !row.host && row.owner != 0 && self.caller() == Ok(row.owner))
            .map(|row| row.instance)
            .ok_or(ERR_STATE)
    }

    fn callback_depth(&self) -> Result<u8, i32> {
        self.callback_epoch()?;
        self.depth.map(|depth| depth.current()).ok_or(ERR_STATE)
    }

    fn kernel_env(&self) -> Option<&Env<'_>> {
        Some(self.env)
    }

    fn with_stack(
        &self,
        visit: &mut dyn FnMut(&dyn super::context::ui_guard::UiStackView),
    ) -> Result<(), i32> {
        self.callback_epoch()?;
        if !self.stack_rows.valid(self.stack_generation) {
            return Err(ERR_STATE);
        }
        visit(self.stack_rows);
        Ok(())
    }
    fn close_dialog(&self, instance: u32) -> Result<(), i32> {
        self.callback_epoch()?;
        self.stack_rows.defer_close(instance, self.stack_generation)
    }

    fn ui_x4(&self) -> Result<bool, i32> {
        self.callback_epoch()?;
        Ok(crate::native_host::ui_store::is_x4(
            self.env.device().memory,
        ))
    }

    fn ui_directory(&self) -> Result<&dyn crate::native_host::ui_service::UiDirectory, i32> {
        self.callback_epoch()?;
        self.directory
            .map(|directory| directory as &dyn crate::native_host::ui_service::UiDirectory)
            .ok_or(paperpaw_native::host_glue::ERR_UNSUPPORTED)
    }
    fn registry(&self) -> Result<&dyn RegistryView, i32> {
        Ok(self)
    }
    fn status_ask(
        &self,
        owner: u64,
        token: u32,
        items: &[&str],
        context: &[u8],
        out: &mut [u8],
    ) -> Result<usize, i32> {
        use crate::native_host::ui_service::StatusAsk;
        let directory = self
            .directory
            .ok_or(paperpaw_native::host_glue::ERR_UNSUPPORTED)?;
        crate::native_host::native_status::StatusCalls {
            frame: self,
            directory,
        }
        .ask(owner, token, items, context, out)
    }

    fn call_native(
        &self,
        owner: u64,
        token: u32,
        input: &[u8],
        reply: &mut [u8],
        accept: &mut dyn FnMut(&[u8]) -> Result<(), i32>,
    ) -> Result<usize, i32> {
        crate::native_host::native_call::call(self, owner, token, input, reply, accept)
    }

    fn with_declarations(&self, visit: &mut dyn FnMut(&dyn core::any::Any)) -> Result<(), i32> {
        self.callback_epoch()?;
        let directory = self
            .directory
            .ok_or(paperpaw_native::host_glue::ERR_UNSUPPORTED)?;
        let store = directory
            .store
            .try_borrow()
            .map_err(|_| paperpaw_native::host_glue::ERR_BUSY)?;
        visit(&*store);
        Ok(())
    }

    fn host_status(&self, item: &str, out: &mut [u8]) -> Option<super::context::HostStatus> {
        crate::native_host::ui_widget_ops::host_status::value(self.env, item, out)
    }

    fn widget_call(
        &self,
        owner: u64,
        token: u32,
        payload: &[u8],
        out: &mut [u8],
    ) -> Result<usize, i32> {
        crate::native_host::native_widget::call(self, owner, token, payload, out)
    }

    fn compose(
        &self,
        _caller_id: &str,
        request: &super::context::ComposeRequest,
        canvas: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<(), i32> {
        crate::native_host::ui_compose_host::compose_in(self, request, canvas)
    }

    fn settings(&self) -> &Settings {
        self.env.settings()
    }
    fn settings_editor(&self) -> Option<&str> {
        self.env
            .slot_screen(paperpaw_sdk::builtin::slots::SETTINGS.as_str())
            .map(paperpaw_sdk::Id::namespace)
    }
    fn translator(&self) -> &Translator {
        self.env.i18n()
    }
    fn fonts(&self) -> Option<&paperpaw_sdk::Fonts> {
        Some(self.env.fonts())
    }
    fn ink_threshold(&self) -> Result<u8, i32> {
        Ok(match self.env.device().display.format {
            paperpaw_platform::PixelFormat::Mono1 => {
                crate::config::mono_coverage(self.env.settings()).max(1)
            }
            _ => 128,
        })
    }
    fn font_generation(&self) -> Option<u64> {
        self.font_removals.generation()
    }
    fn font_valid(&self, provider: &str, acquired: u64) -> bool {
        self.font_removals.valid(provider, acquired)
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

impl RegistryView for CoreFrame<'_> {
    fn generation(&self) -> u32 {
        self.sources().generation()
    }
    fn rows(
        &self,
        op: u32,
        scope: &str,
        flags: u32,
        asker: &Asker<'_>,
        visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        self.sources().rows(op, scope, flags, asker, visit)
    }
    fn detail(
        &self,
        op: u32,
        key: &str,
        asker: &Asker<'_>,
        visit: &mut dyn FnMut(&dyn Row) -> Result<(), i32>,
    ) -> Result<(), i32> {
        self.sources().detail(op, key, asker, visit)
    }
}

impl CoreFrame<'_> {
    fn sources(&self) -> crate::native_host::registry_service::Sources<'_> {
        crate::native_host::registry_service::Sources {
            env: self.env,
            directory: self.directory,
            generation: self.registry_generation,
        }
    }
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;

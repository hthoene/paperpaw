//! The view over the kernel's tables: the environment (registries of actions,
//! screens, slots and setting groups, the plugin list, the language) and the
//! declaration store. Each family of ops lives in its own file and reads
//! through [`Sources`]; this file only routes and keeps the generation.
use super::{
    Asker, RegistryView, Row, bindings, items, menus, plugins, presets, schema, settings, setup,
    status, widgets,
};
use crate::Place;
use crate::native_host::{
    native_declarations::NativeDeclarations, registry_generation::RegistryGeneration,
};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

/// The hash a kept count or resume point names its scope by.
pub(super) fn scope_hash(scope: &str) -> u64 {
    paperpaw_digest::fnv1a64(scope.as_bytes())
}

/// What `core.registry` reads.
#[derive(Clone, Copy)]
pub(crate) struct Sources<'a> {
    pub(crate) env: &'a Place<'a>,
    /// The declaration store; `None` where the host keeps none.
    pub(crate) directory: Option<&'a NativeDeclarations>,
    /// The kernel's registry generation.
    pub(crate) generation: &'a RegistryGeneration,
}

impl RegistryView for Sources<'_> {
    fn generation(&self) -> u32 {
        self.generation.get()
    }

    fn rows(
        &self,
        op: u32,
        scope: &str,
        flags: u32,
        asker: &Asker<'_>,
        visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        match op {
            schema::OP_PLUGINS => plugins::rows(self, visit),
            schema::OP_SETTINGS | schema::OP_CHOICES => {
                settings::rows(self, op, scope, asker, visit)
            }
            schema::OP_PRESETS | schema::OP_VALUES => presets::rows(self, op, scope, asker, visit),
            schema::OP_GROUPS
            | schema::OP_ACTIONS
            | schema::OP_SCREENS
            | schema::OP_SLOTS
            | schema::OP_SLOT_OFFERS
            | schema::OP_INPUTS => items::rows(self, op, scope, visit),
            schema::OP_MENUS => menus::menus(self, visit),
            schema::OP_MENU => menus::menu(self, scope, flags, visit),
            schema::OP_BINDINGS => bindings::rows(self, scope, visit),
            schema::OP_SETUP => setup::rows(self, visit),
            schema::OP_WIDGETS => widgets::rows(self, scope, flags, visit),
            schema::OP_STATUS => status::rows(self, scope, visit),
            _ => Err(ERR_UNSUPPORTED),
        }
    }

    fn detail(
        &self,
        op: u32,
        key: &str,
        asker: &Asker<'_>,
        visit: &mut dyn FnMut(&dyn Row) -> Result<(), i32>,
    ) -> Result<(), i32> {
        match op {
            schema::OP_PLUGIN => plugins::detail(self, key, visit),
            schema::OP_SETTING => settings::detail(self, key, asker, visit),
            schema::OP_PRESET => presets::detail(self, key, visit),
            _ => Err(ERR_UNSUPPORTED),
        }
    }

    /// Op 1 alone: its rows are the settings specs (of a scope), which only
    /// change with the generation; whether a row frames is its key and group.
    fn counted(&self, op: u32, scope: &str) -> Option<(u32, bool)> {
        let memo = self.generation.memo()?;
        let (of, total, all_framed) = memo.total?;
        (op == schema::OP_SETTINGS && of == scope_hash(scope)).then_some((total, all_framed))
    }

    fn starts_at(&self, op: u32, scope: &str, from: u32) -> bool {
        op == schema::OP_SETTINGS && settings::resume_place(self, scope, from).is_some()
    }

    fn rows_from(
        &self,
        op: u32,
        scope: &str,
        flags: u32,
        asker: &Asker<'_>,
        from: u32,
        visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        if op == schema::OP_SETTINGS && from != 0 {
            return settings::settings_from(self, scope, asker, from, visit);
        }
        self.rows(op, scope, flags, asker, visit)
    }

    fn keep_count(&self, op: u32, scope: &str, total: u32, all_framed: bool) {
        if op != schema::OP_SETTINGS {
            return;
        }
        if let Some(mut memo) = self.generation.memo() {
            memo.total = Some((scope_hash(scope), total, all_framed));
        }
    }
}

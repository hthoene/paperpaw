//! The view over the kernel's tables: the environment (registries of actions,
//! screens, slots and setting groups, the plugin list, the language) and the
//! declaration store. Each family of ops lives in its own file and reads
//! through [`Sources`]; this file only routes and keeps the generation.
use super::{Asker, RegistryView, Row, items, plugins, schema};
use crate::native_host::{
    native_declarations::NativeDeclarations, registry_generation::RegistryGeneration,
};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::ERR_UNSUPPORTED;
use paperpaw_sdk::Env;

/// What `core.registry` reads.
#[derive(Clone, Copy)]
pub(crate) struct Sources<'a> {
    pub(crate) env: &'a Env<'a>,
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
        _asker: &Asker<'_>,
        visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        match op {
            schema::OP_PLUGINS => plugins::rows(self, visit),
            schema::OP_GROUPS
            | schema::OP_ACTIONS
            | schema::OP_SCREENS
            | schema::OP_SLOTS
            | schema::OP_SLOT_OFFERS
            | schema::OP_INPUTS => items::rows(self, op, scope, visit),
            _ => Err(ERR_UNSUPPORTED),
        }
    }

    fn detail(
        &self,
        op: u32,
        key: &str,
        _asker: &Asker<'_>,
        visit: &mut dyn FnMut(&dyn Row) -> Result<(), i32>,
    ) -> Result<(), i32> {
        match op {
            schema::OP_PLUGIN => plugins::detail(self, key, visit),
            _ => Err(ERR_UNSUPPORTED),
        }
    }
}

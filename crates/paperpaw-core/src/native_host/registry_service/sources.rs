//! The view over the kernel's tables: the environment (registries of actions,
//! screens, slots and setting groups, the plugin list, the language) and the
//! declaration store. Each family of ops lives in its own file and reads
//! through [`Sources`]; this file only routes and keeps the generation.
use super::{Asker, RegistryView, Row, items, plugins, schema};
use crate::native_host::native_declarations::NativeDeclarations;
use core::ops::ControlFlow;
use paperpaw_native::host_glue::ERR_UNSUPPORTED;
use paperpaw_sdk::Env;

/// What `core.registry` reads.
#[derive(Clone, Copy)]
pub(crate) struct Sources<'a> {
    pub(crate) env: &'a Env<'a>,
    /// The declaration store; `None` where the host keeps none.
    pub(crate) directory: Option<&'a NativeDeclarations>,
}

/// FNV-1a over 32 bits, for the generation.
struct Fingerprint(u32);

impl Fingerprint {
    const fn new() -> Self {
        Self(0x811c_9dc5)
    }
    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u32::from(*byte)).wrapping_mul(0x0100_0193);
        }
    }
    fn number(&mut self, number: usize) {
        self.bytes(&(number as u64).to_le_bytes());
    }
    /// Never 0: the host skips it.
    const fn finish(&self) -> u32 {
        if self.0 == 0 { 1 } else { self.0 }
    }
}

impl Sources<'_> {
    /// The registry generation: a fingerprint of what the lists are made of
    /// (the declaration generation, each plugin and its state, the bound
    /// slots, the numbers of actions, screens and groups). It changes when
    /// one of those lists changes its set or order; it is never 0.
    pub(crate) fn fingerprint(&self) -> u32 {
        let mut print = Fingerprint::new();
        if let Some(directory) = self.directory {
            print.bytes(&directory.catalog.generation().to_le_bytes());
        }
        for info in self.env.plugins().iter() {
            print.bytes(info.manifest.id.as_str().as_bytes());
            print.bytes(&[
                0xff,
                info.state as u8,
                u8::from(info.enabled),
                u8::from(info.update.is_some()),
            ]);
        }
        for slot in self.env.slots() {
            print.bytes(slot.as_str().as_bytes());
            print.bytes(&[0xff]);
        }
        print.number(self.env.actions().iter().count());
        print.number(self.env.screens().iter().count());
        print.number(self.env.settings().groups().count());
        print.finish()
    }
}

impl RegistryView for Sources<'_> {
    fn generation(&self) -> u32 {
        self.fingerprint()
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

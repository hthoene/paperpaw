//! A call the host makes to a format plugin on its own account, outside any
//! callback (the document preparing loop, `doc_pump`): the pump has checked
//! that no callback runs (`depth() == 0`) and that nothing staged waits, then
//! the format is entered through the retained instance registry
//! ([`format_call::call`]: epoch, lease, a started instance, a reply that is
//! checked and, if it is not whole, reported with `dropped`) and its run is
//! metered to the format plugin itself, as the callee of a nested call is
//! (`native_call`). The format's own negative status passes through.
use super::{fmt_call::Route, format_call, format_call::Target, native_instances::NativeInstances};
use crate::meter::Meter;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NOT_FOUND};
use paperpaw_sdk::Env;

/// The host's own route to a format: no caller, the callee metered to itself.
pub(crate) struct HostRoute<'a> {
    pub(crate) registry: &'a NativeInstances,
    pub(crate) env: &'a Env<'a>,
    pub(crate) meter: &'a Meter,
}

impl Route for HostRoute<'_> {
    #[inline(never)]
    fn call(&self, target: Target, payload: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        // Never inside a callback: the pump asks for another pass instead.
        if self.registry.depth() != 0 {
            return Err(ERR_BUSY);
        }
        let id = self.registry.id(target.plugin).ok_or(ERR_NOT_FOUND)?;
        let _running = self.meter.enter_scoped(id.as_str());
        format_call::call(self.registry, target, payload, out, self.env)
    }
    fn gave_up(&self, plugin: u64) -> bool {
        self.registry.format_gave_up(plugin)
    }
}

#[cfg(test)]
#[path = "native_host_call_tests.rs"]
mod tests;

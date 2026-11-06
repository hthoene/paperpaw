//! Copy only generated, bounded plugin fields. No full-table reference is
//! formed for a factory returning the original shorter lifecycle prefix.
use super::RuntimeError;
use paperpaw_native::host_glue::{
    self, PpBytes, PpPlugin, read_plugin_dispatch_event, read_plugin_draw, read_plugin_drop,
    read_plugin_size, read_plugin_state,
};
use std::ffi::c_void;

type Event = unsafe extern "C" fn(*mut c_void, u32, PpBytes) -> i32;
type Draw = unsafe extern "C" fn(*mut c_void, u32) -> i32;

#[derive(Clone, Copy)]
pub(super) struct PluginOutput {
    pub(super) state: *mut c_void,
    pub(super) destroy: unsafe extern "C" fn(*mut c_void),
    // Tail readers establish presence only, not executable-address validity.
    // Foreign dispatch remains gated on registration and event contracts.
    #[allow(dead_code, reason = "foreign callback dispatch remains inactive")]
    pub(super) event: Option<Event>,
    #[allow(dead_code, reason = "foreign callback dispatch remains inactive")]
    pub(super) draw: Option<Draw>,
}

fn optional<T>(callback: Result<T, i32>) -> Result<Option<T>, RuntimeError> {
    match callback {
        Ok(callback) => Ok(Some(callback)),
        Err(host_glue::ERR_UNSUPPORTED) => Ok(None),
        Err(_) => Err(RuntimeError::Plugin),
    }
}

/// The authorized factory must provide an aligned, readable size word and a
/// stable readable allocation covering its declared size, with valid pointer
/// representations. Numeric checks cannot establish those memory guarantees.
/// The caller retains the module and owns exactly-once destruction on success.
pub(super) unsafe fn read(out: *const PpPlugin) -> Result<PluginOutput, RuntimeError> {
    if out.is_null() || !(out as usize).is_multiple_of(align_of::<PpPlugin>()) {
        return Err(RuntimeError::Plugin);
    }
    // SAFETY: the successful factory promises a readable size prefix. The
    // declared span is the factory's contract, not an independent memory probe.
    let accessible = unsafe { out.cast::<u32>().read() } as usize;
    let table = out.cast::<u8>();
    // SAFETY: the authorized factory guarantees this live, initialized span;
    // generated readers check numeric bounds before copying individual fields.
    unsafe {
        read_plugin_size(table, accessible).map_err(|_| RuntimeError::Plugin)?;
        Ok(PluginOutput {
            state: read_plugin_state(table, accessible).map_err(|_| RuntimeError::Plugin)?,
            destroy: read_plugin_drop(table, accessible).map_err(|_| RuntimeError::Plugin)?,
            event: optional(read_plugin_dispatch_event(table, accessible))?,
            draw: optional(read_plugin_draw(table, accessible))?,
        })
    }
}

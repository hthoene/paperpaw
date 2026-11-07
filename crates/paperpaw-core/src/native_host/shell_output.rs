//! Checked plugin output copying after a successful native factory.
use super::*;

fn optional<T>(callback: Result<T, i32>) -> Result<Option<T>, Error> {
    match callback {
        Ok(callback) => Ok(Some(callback)),
        Err(host_glue::ERR_UNSUPPORTED) => Ok(None),
        Err(_) => Err(Error::Plugin),
    }
}

/// Reads the output of a successful `make`.
///
/// # Safety
///
/// The factory promises an aligned, readable size word and a stable readable
/// allocation covering its declared size. Numeric checks cannot establish
/// those memory guarantees.
pub(super) unsafe fn read_output(out: *const PpPlugin) -> Result<PluginOutput, Error> {
    if out.is_null() || !(out as usize).is_multiple_of(align_of::<PpPlugin>()) {
        return Err(Error::Plugin);
    }
    // SAFETY: the successful factory promises a readable size prefix.
    let accessible = unsafe { out.cast::<u32>().read() } as usize;
    let table = out.cast::<u8>();
    // SAFETY: the factory guarantees this live, initialized span; the
    // generated readers check numeric bounds before copying single fields.
    unsafe {
        read_plugin_size(table, accessible).map_err(|_| Error::Plugin)?;
        Ok(PluginOutput {
            state: read_plugin_state(table, accessible).map_err(|_| Error::Plugin)?,
            destroy: read_plugin_drop(table, accessible).map_err(|_| Error::Plugin)?,
            event: optional(read_plugin_dispatch_event(table, accessible))?,
            draw: optional(read_plugin_draw(table, accessible))?,
            input: optional(read_plugin_input(table, accessible))?,
        })
    }
}

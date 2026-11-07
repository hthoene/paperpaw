//! One declaration producer retaining the same lease as native callbacks.
use super::{
    declaration_store::{Allocations, ImageOwner, PendingDeclarations},
    native_instances::NativeLease,
    native_screen::Dispatch,
    raw_declarations::RawDeclarations,
};
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_STATE};
use paperpaw_sdk::Id;

struct Source {
    lease: NativeLease,
    plugin: Id,
    owner: u64,
}
impl ImageOwner for Source {
    fn plugin_id(&self) -> &str {
        self.plugin.as_str()
    }
    fn generation(&self) -> u32 {
        u32::from(self.lease.live().is_ok())
    }
    fn owner_id(&self) -> Result<u64, i32> {
        Ok(self.owner)
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        match self.lease.live() {
            Ok(instance) => instance.with_image(visit),
            Err(_) => visit(None),
        }
    }
    fn invalid_declaration(&self, error: i32) {
        self.lease.dropped(0, error);
    }
}

pub(crate) fn pending(
    plugin: &Id,
    owner: u64,
    lease: NativeLease,
    raw: RawDeclarations,
    allocations: &mut Allocations,
) -> Result<PendingDeclarations, i32> {
    lease.live()?;
    let id = plugin.try_clone().ok_or(ERR_NO_MEMORY)?;
    let source = Source {
        lease,
        plugin: plugin.try_clone().ok_or(ERR_NO_MEMORY)?,
        owner,
    };
    if source.generation() == 0 {
        return Err(ERR_STATE);
    }
    let mut pending = PendingDeclarations::native(id, source)?;
    for batch in raw.batches {
        pending.stage_committed(batch, allocations)?;
    }
    Ok(pending)
}

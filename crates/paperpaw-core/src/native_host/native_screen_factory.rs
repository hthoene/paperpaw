//! Fallible native screen factories retain the admitted owner, never plugin pointers.
use super::{
    native_instances::NativeLease,
    native_screen::{Dispatch, NativeScreen},
    native_values::Callback,
    shell,
};
use alloc::{borrow::Cow, boxed::Box};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED};
use paperpaw_sdk::{Id, Instances, Screen};

pub(crate) struct NativeScreenSpec {
    pub(crate) id: Id,
    pub(crate) title: Cow<'static, str>,
    pub(crate) made: NativeMade,
}

/// What makes a native screen; its id is the registry's ([`crate::registry`]
/// keeps it once, with the title, #248). How many instances it has is read
/// from `flags` (no field of its own: the registry's row is smaller, #248 B).
pub(crate) struct NativeMade {
    token: u32,
    flags: u32,
    instance: NativeLease,
}

/// How many instances of a screen with `flags` there may be; `None` for the
/// value no screen declares.
fn instances_of(flags: u32) -> Option<Instances> {
    match (flags >> 6) & 3 {
        0 => Some(Instances::PER_PARAMS),
        1 => Some(Instances::SINGLE),
        2 => Some(Instances::MANY),
        _ => None,
    }
}

impl NativeScreenSpec {
    pub(crate) fn new(callback: Callback, instance: NativeLease) -> Result<Self, i32> {
        instances_of(callback.flags).ok_or(ERR_UNSUPPORTED)?;
        Ok(Self {
            id: callback.id,
            title: callback.title,
            made: NativeMade {
                token: callback.token,
                flags: callback.flags,
                instance,
            },
        })
    }
}

impl NativeMade {
    /// How many instances the screen may have (checked when it was made).
    pub(crate) fn instances(&self) -> Instances {
        instances_of(self.flags).unwrap_or(Instances::PER_PARAMS)
    }

    pub(crate) fn refresh(&self) -> Option<paperpaw_sdk::RefreshMode> {
        (self.flags & 1 != 0).then_some(paperpaw_sdk::RefreshMode::Full)
    }

    /// The screen `id` (this one's) for `params`.
    pub(crate) fn make(
        &self,
        id: &Id,
        params: &paperpaw_sdk::Params,
    ) -> Result<Box<dyn Screen>, i32> {
        let result = (|| {
            self.instance.check_running()?;
            // A parked plugin is made here at a safe point, so that one
            // without room fails the open as any plugin then does (no
            // screen it never heard of); elsewhere at its first callback.
            self.instance.slot().make_if_safe().map_err(|status| {
                if status == ERR_BUSY {
                    ERR_NO_MEMORY
                } else {
                    status
                }
            })?;
            let count = u32::try_from(params.iter().count()).map_err(|_| ERR_INVALID)?;
            let id = id.try_clone().ok_or(ERR_NO_MEMORY)?;
            let instance = self.instance.try_clone()?;
            let screen = NativeScreen::new(id, self.token, instance)
                .with_flags(self.flags)
                .with_params_count(count);
            let screen: Box<dyn Screen> = shell::try_box(screen).map_err(|_| ERR_NO_MEMORY)?;
            Ok(screen)
        })();
        if let Err(status) = result {
            self.instance.dropped(self.token, status);
        }
        result
    }
}

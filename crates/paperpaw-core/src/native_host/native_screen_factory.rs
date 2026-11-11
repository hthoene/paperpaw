//! Fallible native screen factories retain the admitted owner, never plugin pointers.
use super::{
    native_instances::NativeLease,
    native_screen::{Dispatch, NativeScreen},
    native_values::Callback,
    shell,
};
use alloc::{borrow::Cow, boxed::Box};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED};
use paperpaw_sdk::{Id, Instances, Screen};

pub(crate) struct NativeScreenSpec {
    pub(crate) id: Id,
    pub(crate) title: Cow<'static, str>,
    pub(crate) made: NativeMade,
}

pub(crate) struct NativeMade {
    id: Id,
    token: u32,
    flags: u32,
    instance: NativeLease,
    pub(crate) instances: Instances,
}

impl NativeScreenSpec {
    pub(crate) fn new(callback: Callback, instance: NativeLease) -> Result<Self, i32> {
        let instances = match (callback.flags >> 6) & 3 {
            0 => Instances::PER_PARAMS,
            1 => Instances::SINGLE,
            2 => Instances::MANY,
            _ => return Err(ERR_UNSUPPORTED),
        };
        let id = callback.id.try_clone().ok_or(ERR_NO_MEMORY)?;
        Ok(Self {
            id: callback.id,
            title: callback.title,
            made: NativeMade {
                id,
                token: callback.token,
                flags: callback.flags,
                instance,
                instances,
            },
        })
    }
}

impl NativeMade {
    pub(crate) fn refresh(&self) -> Option<paperpaw_sdk::RefreshMode> {
        (self.flags & 1 != 0).then_some(paperpaw_sdk::RefreshMode::Full)
    }

    pub(crate) fn make(&self, params: &paperpaw_sdk::Params) -> Result<Box<dyn Screen>, i32> {
        let result = (|| {
            self.instance.live()?;
            let count = u32::try_from(params.iter().count()).map_err(|_| ERR_INVALID)?;
            let id = self.id.try_clone().ok_or(ERR_NO_MEMORY)?;
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

//! Publish screen-only native declarations after complete mapping and owner retention.
use super::{
    FlatHost,
    flat::FlatDepth,
    native_dispatch::NativeDispatch,
    native_instances::NativeLease,
    native_screen_factory::NativeScreenSpec,
    native_values::{self, Value},
    retained::Retained,
    shell,
};
use alloc::{rc::Rc, vec::Vec};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED, Services};
use paperpaw_sdk::{Id, IdError, RegisterError};

impl crate::Kernel {
    /// Admit a screen-only native owner and publish its screen factories together.
    /// The shell supplies its existing device-wide callback depth and diagnostic
    /// function. Other declaration families are refused as a whole; their host
    /// adapters must register them through the same declaration transaction.
    ///
    /// # Errors
    /// Returns admission, allocation or declaration errors without publishing a
    /// partial screen set. The consumed host is destroyed on every failed install.
    pub fn register_native_screens<S: Services + 'static, M: 'static>(
        &mut self,
        host: shell::Hosted<S, M>,
        depth: FlatDepth,
        report: fn(&S, u32, i32),
    ) -> Result<(), shell::Error> {
        let (retained, (owner, callbacks)) = Retained::make_native(host, |view| {
            let owner = Id::try_parse(view.plugin).map_err(|error| match error {
                IdError::NoMemory => ERR_NO_MEMORY,
                _ => ERR_INVALID,
            })?;
            let mut callbacks = Vec::new();
            let mut result = Ok(());
            view.visit(&mut |record| {
                if result.is_err() {
                    return;
                }
                result = (|| {
                    let Value::Screen(callback) = native_values::from_record(view.plugin, record)?
                    else {
                        return Err(ERR_UNSUPPORTED);
                    };
                    callbacks.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
                    callbacks.push(callback);
                    Ok(())
                })();
            })?;
            result?;
            Ok((owner, callbacks))
        })?;
        let flat_host = FlatHost {
            meter: Rc::clone(&self.shared.meter),
            font_removals: Rc::clone(&self.shared.font_removals),
        };
        let instance =
            NativeLease::try_new(NativeDispatch::new(retained, depth, flat_host, report))
                .map_err(shell::Error::Status)?;
        let mut staging = crate::registry::Staging::default();
        for callback in callbacks {
            let screen = instance
                .try_clone()
                .and_then(|lease| NativeScreenSpec::new(callback, lease))
                .map_err(shell::Error::Status)?;
            staging
                .native_screen(screen)
                .map_err(shell::Error::Status)?;
        }
        self.merge(&owner, staging).map_err(|error| {
            shell::Error::Status(match error {
                RegisterError::Invalid { .. } => ERR_NO_MEMORY,
                _ => ERR_INVALID,
            })
        })
    }
}

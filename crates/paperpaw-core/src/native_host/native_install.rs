//! One native declaration entry, with owned mapping before binding and publication.
use super::{
    FlatHost,
    flat::FlatDepth,
    native_dispatch::NativeDispatch,
    native_instances::NativeLease,
    native_screen_factory::NativeScreenSpec,
    native_values::{self, Callback, Value},
    registrations::Record,
    retained::Retained,
    shell,
};
use alloc::{rc::Rc, vec::Vec};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED, Services};
use paperpaw_sdk::{Id, IdError, RegisterError};

/// Owned mapping results; future declaration families extend this one dispatch.
enum Owned {
    Screen(Callback),
}

fn map_record(owner: &str, record: Record<'_>) -> Result<Owned, i32> {
    match record {
        Record::ScreenSpec(_) => match native_values::from_record(owner, record)? {
            Value::Screen(callback) => Ok(Owned::Screen(callback)),
            _ => Err(ERR_INVALID),
        },
        _ => Err(ERR_UNSUPPORTED),
    }
}

impl crate::Kernel {
    /// Admit a native owner and publish all supported declarations together.
    /// The shell supplies its existing device-wide callback depth and diagnostic
    /// function. Other declaration families are refused as a whole; their host
    /// adapters must register them through the same declaration transaction.
    ///
    /// # Errors
    /// Returns admission, allocation or declaration errors without publishing a
    /// partial screen set. The consumed host is destroyed on every failed install.
    pub fn register_native<S: Services + 'static, M: 'static>(
        &mut self,
        host: shell::Hosted<S, M>,
        depth: &FlatDepth,
        report: fn(&S, u32, i32),
    ) -> Result<(), shell::Error> {
        let depth = depth.try_clone().ok_or(shell::Error::Allocation)?;
        let (retained, (owner, owned)) = Retained::make_native(host, |view| {
            let owner = Id::try_parse(view.plugin).map_err(|error| match error {
                IdError::NoMemory => ERR_NO_MEMORY,
                _ => ERR_INVALID,
            })?;
            let mut owned = Vec::new();
            let mut result: Result<(), i32> = Ok(());
            view.visit(&mut |record| {
                if result.is_err() {
                    return;
                }
                result = (|| {
                    let value = map_record(view.plugin, record)?;
                    owned.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
                    owned.push(value);
                    Ok(())
                })();
            })?;
            result?;
            Ok((owner, owned))
        })?;
        let flat_host = FlatHost {
            meter: Rc::clone(&self.shared.meter),
            font_removals: Rc::clone(&self.shared.font_removals),
        };
        let instance =
            NativeLease::try_new(NativeDispatch::new(retained, depth, flat_host, report))
                .map_err(shell::Error::Status)?;
        let mut staging = crate::registry::Staging::default();
        for value in owned {
            match value {
                Owned::Screen(callback) => {
                    let screen = instance
                        .try_clone()
                        .and_then(|lease| NativeScreenSpec::new(callback, lease))
                        .map_err(shell::Error::Status)?;
                    staging
                        .native_screen(screen)
                        .map_err(shell::Error::Status)?;
                }
            }
        }
        self.merge(&owner, staging).map_err(|error| {
            shell::Error::Status(match error {
                RegisterError::Invalid { .. } => ERR_NO_MEMORY,
                _ => ERR_INVALID,
            })
        })
    }
}

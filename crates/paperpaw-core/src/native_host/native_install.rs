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
        let (retained, (owner, owned)) = Retained::make_native(host, map_declarations)?;
        // Callback registrations need their own mapping arm. Never accept an
        // owner while silently leaving any committed contribution unrouted.
        if !retained.batch().map_err(shell::Error::from)?.is_empty() {
            return Err(shell::Error::Status(ERR_UNSUPPORTED));
        }
        let key = self
            .shared
            .native_instances
            .insert(
                owner
                    .try_clone()
                    .ok_or(shell::Error::Status(ERR_NO_MEMORY))?,
                None,
                true,
            )
            .map_err(shell::Error::Status)?;
        let result = self.bind_native(key, retained, depth, owned, &owner, report);
        if result.is_err() {
            self.shared
                .native_instances
                .abandon(key)
                .map_err(shell::Error::Status)?;
        }
        result
    }

    fn bind_native<S: Services + 'static, M: 'static>(
        &mut self,
        key: usize,
        retained: Retained<S, M>,
        depth: FlatDepth,
        owned: Vec<Owned>,
        owner: &Id,
        report: fn(&S, u32, i32),
    ) -> Result<(), shell::Error> {
        let flat_host = FlatHost {
            meter: Rc::clone(&self.shared.meter),
            font_removals: Rc::clone(&self.shared.font_removals),
        };
        let instance = NativeLease::try_new(NativeDispatch::new(
            retained,
            depth,
            flat_host,
            report,
            self.shared
                .native_instances
                .identity(key)
                .map_err(shell::Error::Status)?,
        ))
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
                        .native_screen(owner, screen)
                        .map_err(shell::Error::Status)?;
                }
            }
        }
        self.shared
            .native_instances
            .install(key, instance.try_clone().map_err(shell::Error::Status)?)
            .map_err(shell::Error::Status)?;
        // Only screen factories are supported here. Publish through the reserved
        // staging transaction without the legacy merge's allocating action sweep.
        staging
            .merge_into(&mut self.shared.registry, &mut self.shared.actions)
            .map_err(|error| shell::Error::Status(merge_status(&error)))
    }
}

fn map_declarations(view: &super::declaration_checks::View<'_>) -> Result<(Id, Vec<Owned>), i32> {
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
    if owned.is_empty() {
        return Err(ERR_UNSUPPORTED);
    }
    Ok((owner, owned))
}

fn merge_status(error: &RegisterError) -> i32 {
    match error {
        RegisterError::Invalid { message }
            if message.as_ref() == crate::registry::NATIVE_CAPACITY_OVERFLOW
                || message.as_ref() == crate::registry::NATIVE_CAPACITY_UNAVAILABLE =>
        {
            ERR_NO_MEMORY
        }
        _ => ERR_INVALID,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_native_capacity_errors_become_no_memory() {
        for message in [
            crate::registry::NATIVE_CAPACITY_OVERFLOW,
            crate::registry::NATIVE_CAPACITY_UNAVAILABLE,
        ] {
            assert_eq!(
                merge_status(&RegisterError::Invalid {
                    message: message.into()
                }),
                ERR_NO_MEMORY
            );
        }
        assert_eq!(
            merge_status(&RegisterError::Invalid {
                message: "invalid native declaration".into()
            }),
            ERR_INVALID
        );
    }
}

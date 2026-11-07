//! Own the committed callback lease and its pinned host in one movable holder.
//! The lease is always released before the pinned allocation can be freed.

use super::{DispatchDepth, Error, registrations::Registration, shell};
use core::mem::ManuallyDrop;
use paperpaw_native::host_glue::Services;

pub(crate) struct Retained<S: Services + 'static, M: 'static> {
    batch: Option<shell::Batch<'static, S, M, Registration>>,
    host: ManuallyDrop<shell::Hosted<S, M>>,
}

impl<S: Services + 'static, M: 'static> Retained<S, M> {
    pub(crate) fn make(host: shell::Hosted<S, M>) -> Result<Self, shell::Error> {
        let transaction = host.begin()?;
        let batch = host.make_registered(transaction)?.commit()?;
        // SAFETY: this holder retains the pinned owner and drops the raw
        // batch lease before freeing it. Only &self exposes borrowed views.
        let batch = unsafe { batch.into_retained() };
        Ok(Self {
            batch: Some(batch),
            host: ManuallyDrop::new(host),
        })
    }

    pub(crate) fn make_native<R>(
        host: shell::Hosted<S, M>,
        map: impl FnOnce(&super::declaration_checks::View<'_>) -> Result<R, i32>,
    ) -> Result<(Self, R), shell::Error> {
        let transaction = host.begin()?;
        let (batch, staging) = host.make_registered(transaction)?.commit_native(map)?;
        // SAFETY: as in make, only raw pointers and lifetime markers are
        // retained; no reference into the owned allocation survives the move.
        let batch = unsafe { batch.into_retained() };
        Ok((
            Self {
                batch: Some(batch),
                host: ManuallyDrop::new(host),
            },
            staging,
        ))
    }

    pub(crate) fn batch(&self) -> Result<&shell::Batch<'_, S, M, Registration>, Error> {
        self.batch.as_ref().ok_or(Error::State)
    }

    pub(crate) fn host(&self) -> &shell::Hosted<S, M> {
        &self.host
    }

    pub(crate) fn registrations(
        &self,
        depth: &DispatchDepth,
        visit: &mut dyn FnMut(usize, Registration),
    ) -> Result<(), Error> {
        let batch = self.batch()?;
        for index in 0..batch.len() {
            batch.dispatch(index, depth, |_, record| visit(index, *record))?;
        }
        Ok(())
    }
}

impl<S: Services + 'static, M: 'static> Drop for Retained<S, M> {
    fn drop(&mut self) {
        // Revocation prevents new calls. A safe borrower cannot destroy this
        // holder during a call; if unsafe code violates that rule, retain the
        // pinned owner instead of freeing memory still used by plugin code.
        let owner = self.host.owner();
        owner.revoke();
        if owner.prepare_teardown().is_err() {
            return;
        }
        drop(self.batch.take());
        if owner.finish().is_ok() {
            // SAFETY: all leases drained, state destroyed, host dropped once.
            unsafe { ManuallyDrop::drop(&mut self.host) };
        }
    }
}

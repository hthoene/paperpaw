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
        // SAFETY: Hosted owns a pinned heap allocation. Moving Hosted moves
        // only its owning handle, never that allocation. This holder releases
        // the batch before finishing or freeing the host, including on unwind.
        // No extended reference escapes the private holder's borrow.
        let batch = unsafe {
            core::mem::transmute::<
                shell::Batch<'_, S, M, Registration>,
                shell::Batch<'static, S, M, Registration>,
            >(batch)
        };
        Ok(Self {
            batch: Some(batch),
            host: ManuallyDrop::new(host),
        })
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

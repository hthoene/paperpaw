//! Native admission transfers only validated construction output.
use super::*;
use crate::native_host::{declaration_checks, raw_declarations::RawDeclarations};

type StoredNative<'a, S, M, T, R> = (Batch<'a, S, M, T>, R, RawDeclarations);

impl<'a, S, M, T> PendingInstance<'a, S, M, T> {
    pub(crate) fn commit_native<R>(
        self,
        map: impl FnOnce(&declaration_checks::View<'_>) -> Result<R, i32>,
    ) -> Result<(Batch<'a, S, M, T>, R), Error> {
        let (batch, value, raw) = self.commit_native_stored(map, |_, _| {})?;
        drop(raw);
        Ok((batch, value))
    }

    pub(crate) fn commit_native_stored<R>(
        mut self,
        map: impl FnOnce(&declaration_checks::View<'_>) -> Result<R, i32>,
        refused: impl FnOnce(&str, declaration_checks::Cap),
    ) -> Result<StoredNative<'a, S, M, T, R>, Error> {
        let transaction = self.transaction.take().ok_or(Error::Attempt)?;
        let result = transaction
            .staging
            .commit_native_stored(
                crate::native_host::Teardown {
                    report: report_context::<S>,
                    cancel: cancel_context::<S>,
                    release: release_context::<S>,
                    destroy: destroy_context::<S>,
                },
                map,
                refused,
            )
            .map_err(Error::Status)?;
        self.armed = false;
        Ok(result)
    }
}

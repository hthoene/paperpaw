//! `command.lookup_ready` (1593, `docs/abi/document.md` 7): a provider of
//! `core.lookup` kept its result for (`language`, `text`). Checked at submit
//! (a non-empty text, a language as the ask takes it, else `ERR_INVALID`),
//! kept as the ask's hash only (8 bytes, no text copied) and applied after the
//! callback with the other commands: the host ends the pending asks it
//! matches and queues `event.lookup_ready` for each asker
//! ([`Kernel::native_apply_lookup_readies`]). One that matches no ask is still
//! accepted (counted and logged): never an error to the provider.
//!
//! [`Kernel::native_apply_lookup_readies`]: crate::Kernel::native_apply_lookup_readies
use super::{CommandBatch, CommandStage, WireCommand};
use crate::native_host::lookup_service;
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY};

impl CommandStage<'_> {
    /// Whether `command` says a lookup result is ready.
    pub(super) fn is_lookup_ready(command: &WireCommand<'_>) -> bool {
        matches!(command, WireCommand::LookupReady { .. })
    }

    /// Takes the admitted ready commands (the asks' hashes), in order, for the
    /// host to apply after the callback.
    #[must_use]
    pub(crate) fn take_lookup_readies(&mut self) -> Vec<u64> {
        core::mem::take(&mut self.staged_lookups)
    }

    /// The ready commands of `batch`, checked, as the hashes of their asks.
    #[inline(never)]
    pub(super) fn validate_lookup_readies(
        &self,
        batch: &CommandBatch<'_>,
    ) -> Result<Vec<u64>, i32> {
        let mut hashes = Vec::new();
        let count = batch.iter().filter(Self::is_lookup_ready).count();
        if count == 0 {
            return Ok(hashes);
        }
        hashes.try_reserve_exact(count).map_err(|_| ERR_NO_MEMORY)?;
        for command in batch.iter() {
            let WireCommand::LookupReady { text, language } = command else {
                continue;
            };
            if text.is_empty() || !lookup_service::language_ok(language) {
                return Err(ERR_INVALID);
            }
            hashes.push(lookup_service::ask_hash(language, text));
        }
        Ok(hashes)
    }
}

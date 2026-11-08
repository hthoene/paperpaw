//! Stages and their subscribers, read from the declaration store: what the
//! broker (`stage_broker.rs`) is fed by. A stage and a subscription live and
//! die with their plugin's declarations; there is no second registry.
#![allow(
    dead_code,
    reason = "the broker's adapter (slot 32, Core's N4 hand-off) reads these when it is wired"
)]
use super::{DeclarationStore, Kind};
use crate::native_host::registrations::Record;
use alloc::vec::Vec;
use paperpaw_native::host_glue::ERR_NO_MEMORY;

/// One subscription of a live plugin, as it was declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StageSubscriber<'a> {
    /// The subscribing plugin's host owner id.
    pub(crate) owner: u64,
    /// The subscribing plugin's id.
    pub(crate) plugin: &'a str,
    pub(crate) token: u32,
    pub(crate) order: i16,
    /// Where in the plugin's declarations: ascending in declaration order.
    pub(crate) index: u32,
    pub(crate) signature: u64,
}

/// A stage's bounds, as its owner declared them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StageLimits {
    /// The owning plugin's host owner id.
    pub(crate) owner: u64,
    pub(crate) schema: u32,
    pub(crate) flags: u32,
    pub(crate) max_input: u32,
    pub(crate) max_reply: u32,
    pub(crate) max_records: u32,
}

impl DeclarationStore {
    /// The stage `<plugin>.<name>` of a live plugin, if declared.
    pub(crate) fn stage(&self, full_id: &str) -> Option<StageLimits> {
        let mut found = None;
        for owner in self.owners.iter().flatten() {
            if owner.source.generation() != owner.generation {
                continue;
            }
            let Some(name) = full_id
                .strip_prefix(owner.id.as_str())
                .and_then(|rest| rest.strip_prefix('.'))
            else {
                continue;
            };
            // A stage name is one segment, so `a` + `b.c` is nothing.
            if name.contains('.') {
                continue;
            }
            let Ok(host) = owner.source.owner_id() else {
                continue;
            };
            if self.find(owner.id.as_str(), Kind::Stage, name, &mut |record| {
                if let Record::StageSpec(spec) = record {
                    found = Some(StageLimits {
                        owner: host,
                        schema: spec.schema,
                        flags: spec.flags,
                        max_input: spec.max_input,
                        max_reply: spec.max_reply,
                        max_records: spec.max_records,
                    });
                }
            }) {
                break;
            }
        }
        found
    }

    /// Every live subscription to `stage` (a full id), in no order: the
    /// broker orders them. The list is reserved fallibly and complete or
    /// not at all.
    ///
    /// # Errors
    /// `ERR_NO_MEMORY`.
    pub(crate) fn stage_subscribers<'a>(
        &'a self,
        stage: &str,
    ) -> Result<Vec<StageSubscriber<'a>>, i32> {
        let mut found: Vec<StageSubscriber<'a>> = Vec::new();
        let mut failure = None;
        self.keys(Kind::Subscription, &mut |key| {
            let Some(owner) = self
                .owners
                .get(usize::from(key.owner))
                .and_then(Option::as_ref)
            else {
                return;
            };
            let Ok(host) = owner.source.owner_id() else {
                return;
            };
            self.with_record(key, &mut |record| {
                let Record::RegistrationStage(registration) = record else {
                    return;
                };
                if registration.stage != stage || failure.is_some() {
                    return;
                }
                if found.try_reserve(1).is_err() {
                    failure = Some(ERR_NO_MEMORY);
                    return;
                }
                found.push(StageSubscriber {
                    owner: host,
                    plugin: owner.id.as_str(),
                    token: registration.token,
                    order: i16::try_from(registration.order).unwrap_or(0),
                    index: u32::from(key.batch) << 16 | u32::from(key.record),
                    signature: registration.signature,
                });
            });
        });
        match failure {
            Some(error) => Err(error),
            None => Ok(found),
        }
    }
}

#[cfg(test)]
#[path = "declaration_stages_tests.rs"]
mod tests;

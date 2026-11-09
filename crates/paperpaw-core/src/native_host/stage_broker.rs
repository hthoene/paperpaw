//! Bounded host-run stages over the existing retained instance registry.
//! Replies stage changes in a private transaction; only a completed run commits.
use super::{
    native_instances::{CallbackEpoch, InstanceState, NativeInstances, NativeLease},
    native_screen::Dispatch,
    registrations::{self as wire, Record},
};
use crate::native_host::codec;
use alloc::vec::Vec;
use paperpaw_native::host_glue::{
    ERR_CANCELLED, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_STATE,
};
use paperpaw_sdk::{Env, Id};

const SUBSCRIBERS: usize = 32;
const CALL_BYTES: usize = 4096;
const PLUGIN_ID_BYTES: usize = 128;

/// Host-selected payload codec. A plugin never supplies these function pointers.
pub(crate) struct Schema {
    pub(crate) key: &'static str,
    pub(crate) major: u32,
    pub(crate) max_subscribers: usize,
    pub(crate) max_input: usize,
    pub(crate) max_reply: usize,
    pub(crate) reply_rows: u32,
    pub(crate) input: fn(&[u8]) -> Result<(), i32>,
    pub(crate) reply: fn(&[u8]) -> Result<(), i32>,
}

impl Schema {
    /// The schema of a plugin-defined stage: the host sees only the framing of
    /// its input and reply (up to 64 records each way, within the byte bounds),
    /// never the payload, which belongs to the stage's own schema.
    #[allow(
        dead_code,
        reason = "plugin-defined stages join the broker when the production stage path is wired"
    )]
    pub(crate) fn plugin_defined(
        key: &'static str,
        major: u32,
        max_subscribers: usize,
        max_input: usize,
        max_reply: usize,
        reply_rows: u32,
    ) -> Self {
        Self {
            key,
            major,
            max_subscribers,
            max_input,
            max_reply,
            reply_rows,
            input: super::stage_wire::validate_plugin_input,
            reply: |_| Ok(()),
        }
    }
}

/// One committed registration, with identity resolved through NativeInstances.
#[derive(Clone, Copy)]
pub(crate) struct Subscription<'a> {
    pub(crate) stage: &'a str,
    pub(crate) plugin: u64,
    pub(crate) token: u32,
    pub(crate) order: i16,
    pub(crate) index: u32,
    pub(crate) signature: u64,
    pub(crate) enabled: bool,
    pub(crate) style_granted: bool,
}

struct Subscriber {
    id: Id,
    registration: Subscription<'static>,
    instance: Option<NativeLease>,
    lazy: bool,
}

/// Callback-local membership and two scratch buffers; never a second registry.
pub(crate) struct Run<'a> {
    schema: &'a Schema,
    depth: &'a super::DispatchDepth,
    _epoch: CallbackEpoch<'a>,
    subscribers: Vec<Subscriber>,
    call: Vec<u8>,
    reply: Vec<u8>,
    missing: usize,
}

/// Validated subscriber output. All bytes are borrowed only for the apply call.
pub(crate) struct Reply<'a> {
    pub(crate) plugin: &'a Id,
    pub(crate) index: usize,
    pub(crate) signature: u64,
    pub(crate) style_granted: bool,
    pub(crate) bytes: &'a [u8],
}

#[derive(Default, Debug, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub(crate) applied: usize,
    pub(crate) refused: usize,
}

fn scratch(cap: usize) -> Result<Vec<u8>, i32> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(cap).map_err(|_| ERR_NO_MEMORY)?;
    bytes.resize(cap, 0);
    Ok(bytes)
}

impl<'a> Run<'a> {
    /// Freeze membership before foreign code. Disabled rows do not consume slots.
    pub(crate) fn prepare<'s>(
        schema: &'a Schema,
        depth: &'a super::DispatchDepth,
        input: &[u8],
        registry: &'a NativeInstances,
        registrations: impl IntoIterator<Item = Subscription<'s>>,
    ) -> Result<Self, i32> {
        if schema.major == 0
            || !(1..=SUBSCRIBERS).contains(&schema.max_subscribers)
            || !(1..=3840).contains(&schema.max_input)
            || !(1..=3840).contains(&schema.max_reply)
            || !(1..=64).contains(&schema.reply_rows)
            || schema.key.len() > 128
        {
            return Err(ERR_INVALID);
        }
        if input.len() > schema.max_input {
            return Err(ERR_NO_SPACE);
        }
        super::declarations::stage_registration_check(&wire::RegistrationStage {
            stage: schema.key,
            token: 0,
            order: 0,
            signature: 0,
        })?;
        if registry.depth() != 0 || depth.0.current() != 0 {
            return Err(ERR_STATE);
        }
        (schema.input)(input)?;
        let epoch = registry.callback()?;
        let (selected, missing) = select(schema, registry, registrations)?;
        let mut subscribers = Vec::new();
        subscribers
            .try_reserve_exact(selected.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        for registration in selected {
            let id = registry.id(registration.plugin).ok_or(ERR_STATE)?;
            if id.as_str().len() > PLUGIN_ID_BYTES {
                return Err(ERR_INVALID);
            }
            let (instance, lazy) = match registry.state(registration.plugin)? {
                InstanceState::Running(lease) => (Some(lease), false),
                InstanceState::Lazy => (None, true),
                InstanceState::Stopped => (None, false),
            };
            subscribers.push(Subscriber {
                id: id.try_clone().ok_or(ERR_NO_MEMORY)?,
                // The stage key is already selected; keep only the static host key.
                registration: Subscription {
                    stage: schema.key,
                    ..registration
                },
                instance,
                lazy,
            });
        }
        let record = Record::StageCall(wire::StageCall {
            stage: schema.key,
            schema: schema.major,
            payload: input,
        });
        // A one-record batch has a one-byte count followed by its framed record.
        let capacity = codec::framed_len(&record)?
            .checked_add(1)
            .filter(|length| *length <= CALL_BYTES)
            .ok_or(ERR_NO_SPACE)?;
        let mut call = scratch(capacity)?;
        if codec::encode_batch(&[record], &mut call)? != capacity {
            return Err(ERR_INVALID);
        }
        let reply = scratch(schema.max_reply)?;
        // Publish deduplicated lazy starts only after every allocation succeeded.
        // The retained callback epoch prevents flushing before the run drops.
        for subscriber in &subscribers {
            if subscriber.lazy {
                registry.request_start(subscriber.registration.plugin)?;
            }
        }
        Ok(Self {
            schema,
            depth,
            _epoch: epoch,
            subscribers,
            call,
            reply,
            missing,
        })
    }

    /// Prepare target state fallibly, apply valid replies privately, then commit.
    /// Refusal or a revoked owner skips only that subscriber. A transaction error
    /// aborts the whole run and leaves the caller's target unchanged.
    pub(crate) fn apply<T>(
        mut self,
        target: &mut T,
        env: &Env<'_>,
        prepare: impl FnOnce(&T) -> Result<T, i32>,
        mut apply: impl FnMut(&mut T, Reply<'_>) -> Result<(), i32>,
    ) -> Result<Outcome, i32> {
        if self.depth.0.current() != 0 {
            return Err(ERR_STATE);
        }
        let host = crate::abi7::calls::Instance::new();
        let _host_call = host.enter(&self.depth.0).map_err(|_| ERR_STATE)?;
        let mut staged = prepare(target)?;
        let mut outcome = Outcome {
            refused: self.missing,
            ..Outcome::default()
        };
        for (index, subscriber) in self.subscribers.iter().enumerate() {
            // Foreign callbacks may report bytes they never wrote. Clear the
            // full offered span so no previous subscriber can supply those bytes.
            self.reply.fill(0);
            let result = subscriber
                .instance
                .as_ref()
                .ok_or(ERR_CANCELLED)
                .and_then(|instance| {
                    instance.stage(
                        subscriber.registration.token,
                        &self.call,
                        &mut self.reply,
                        env,
                    )
                });
            let result = result.and_then(|length| {
                let length = usize::try_from(length).map_err(|_| length)?;
                let bytes = self.reply.get(..length).ok_or(ERR_INVALID)?;
                super::stage_wire::validate_reply(
                    bytes,
                    self.schema.max_reply,
                    self.schema.reply_rows,
                )?;
                (self.schema.reply)(bytes)?;
                Ok(bytes)
            });
            match result {
                Ok(bytes) => {
                    apply(
                        &mut staged,
                        Reply {
                            plugin: &subscriber.id,
                            index,
                            signature: subscriber.registration.signature,
                            style_granted: subscriber.registration.style_granted,
                            bytes,
                        },
                    )?;
                    outcome.applied += 1;
                }
                Err(status) => {
                    if let Some(instance) = &subscriber.instance {
                        instance.dropped(subscriber.registration.token, status);
                    }
                    outcome.refused += 1;
                }
            }
        }
        *target = staged;
        Ok(outcome)
    }
}

fn select<'s>(
    schema: &Schema,
    registry: &NativeInstances,
    registrations: impl IntoIterator<Item = Subscription<'s>>,
) -> Result<(Vec<Subscription<'s>>, usize), i32> {
    let mut selected: Vec<Subscription<'s>> = Vec::new();
    selected
        .try_reserve_exact(schema.max_subscribers)
        .map_err(|_| ERR_NO_MEMORY)?;
    let mut missing = 0_usize;
    for registration in registrations {
        if registration.stage != schema.key || !registration.enabled {
            continue;
        }
        let Some(id) = registry.id(registration.plugin) else {
            missing = missing.saturating_add(1);
            continue;
        };
        let position = selected
            .iter()
            .position(|before| {
                let before_id = registry.id(before.plugin).map(Id::as_str).unwrap_or("");
                (registration.order, id.as_str(), registration.index)
                    < (before.order, before_id, before.index)
            })
            .unwrap_or(selected.len());
        if position < schema.max_subscribers {
            if selected.len() == schema.max_subscribers {
                selected.pop();
            }
            selected.insert(position, registration);
        }
    }
    Ok((selected, missing))
}

#[cfg(test)]
#[path = "stage_broker_tests.rs"]
pub(crate) mod tests;

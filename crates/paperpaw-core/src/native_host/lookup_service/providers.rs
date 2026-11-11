//! The production host of `core.lookup` (`docs/abi/document.md` 7): the
//! providers the plugins declared (`lookup.spec`, in the declaration store),
//! filtered for the ask, and the directed stage call to the one the cursor
//! names ([`Frame::call_native`]).
//!
//! The providers are called in load order (no user order for 1.0). A provider
//! is left out when
//!
//! - it is the asker's own (its instance is the caller's, which a directed
//!   call refuses: a plugin that asks and provides never asks itself),
//! - it needs the network (`flags` bit 0) and none is connected: the device
//!   can reach one and, where PaperPaw manages Wi-Fi, it is online,
//! - its languages do not take the ask's: Beta 3's rule, an empty list or an
//!   empty ask takes all, otherwise a tag equals the ask's language or its
//!   base (`de-AT` is German too), ASCII case ignored.
//!
//! The cursor counts the providers that are left, on every ask: a plugin that
//! loads or stops, or a network that comes or goes, between two pages can make
//! the asker skip or see a provider twice (the asker's loop is best effort).
//! The cable serves a computer, not the internet, so it never counts as a
//! network here: only Wi-Fi (and the device's ability) say "connected". A provider that is stopped is not
//! started for a lookup, and one that fails or answers badly has the empty
//! entry for its page (see [`call`]).
//!
//! Memory: the page keeps the chosen provider's owner, token and name (about
//! 90 bytes); the call has the framed ask (at most 2.2 KiB) and the stage
//! reply (at most 3,840 bytes) on the heap, reserved fallibly before the
//! provider is entered and freed on return. Nothing else is held.
use super::{LookupHost, broker, wire};
use crate::abi7::context::Frame;
use crate::native_host::stages_wire::core_lookup_v1 as stage;
use crate::native_host::{codec, declaration_store::DeclarationStore, stage_wire};
use alloc::vec::Vec;
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE,
};

/// `lookup.spec.flags` bit 0: shown only while a network is up.
const NETWORK: u32 = 1;
/// The longest provider name (`lookup_entry.provider`).
const NAME_MAX: usize = 64;
/// The framed call: one `stage.call` record and the batch count byte.
const CALL_MAX: usize = 4096;

/// The providers of the declaration store. Keeps nothing: every ask reads the
/// store through the caller's frame.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Providers;

/// The provider an ask's cursor names.
struct Chosen {
    owner: u64,
    token: u32,
    name: [u8; NAME_MAX],
    len: usize,
    /// Providers after this one are left.
    more: bool,
}

impl Chosen {
    fn name(&self) -> &str {
        self.name
            .get(..self.len)
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
            .unwrap_or("")
    }
}

/// Whether a network is connected now: the device can reach one and, where
/// PaperPaw manages Wi-Fi, it is online. A frame without an environment says
/// no.
fn network_up(frame: &dyn Frame) -> bool {
    // The cable serves a computer, not the internet: it does not count.
    frame.kernel_env().is_some_and(|env| {
        env.caps().network && env.wifi().is_none_or(|wifi| wifi.state.is_online())
    })
}

/// Whether a provider of `languages` (one tag per line) takes `language`.
fn fits(languages: &str, language: &str) -> bool {
    if language.is_empty() || languages.is_empty() {
        return true;
    }
    let base = language.split(['-', '_']).next().unwrap_or(language);
    languages
        .split('\n')
        .any(|tag| tag.eq_ignore_ascii_case(language) || tag.eq_ignore_ascii_case(base))
}

/// The `cursor`th provider left for `asker`'s ask in `language`, and whether
/// more follow; `None` past the last.
fn choose(
    frame: &dyn Frame,
    asker: u64,
    language: &str,
    cursor: u32,
) -> Result<Option<Chosen>, i32> {
    let network = network_up(frame);
    let mut left: u32 = 0;
    let mut chosen: Option<Chosen> = None;
    let mut found = Err(ERR_STATE);
    frame.with_declarations(&mut |any| {
        let Some(store) = any.downcast_ref::<DeclarationStore>() else {
            return;
        };
        found = Ok(());
        store.each_lookup(&mut |decl| {
            let spec = decl.spec;
            if decl.owner == asker
                || (spec.flags & NETWORK != 0 && !network)
                || !fits(spec.languages, language)
            {
                return;
            }
            if left == cursor {
                let bytes = spec.name.as_bytes();
                let mut name = [0; NAME_MAX];
                if let Some(room) = name.get_mut(..bytes.len()) {
                    room.copy_from_slice(bytes);
                    chosen = Some(Chosen {
                        owner: decl.owner,
                        token: spec.token,
                        name,
                        len: bytes.len(),
                        more: false,
                    });
                }
            } else if left == cursor.saturating_add(1)
                && let Some(chosen) = chosen.as_mut()
            {
                chosen.more = true;
            }
            left = left.saturating_add(1);
        });
    })?;
    found?;
    Ok(chosen)
}

fn scratch(capacity: usize) -> Result<Vec<u8>, i32> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| ERR_NO_MEMORY)?;
    bytes.resize(capacity, 0);
    Ok(bytes)
}

/// The ask as the one `stage.call` record the provider's `core.lookup` token
/// takes, framed as a batch. The provider's cursor is always 0: the host pages
/// across providers, one page each, and a provider's paging stays inside its
/// page (`document.md` 7).
fn framed(ask: &wire::ServiceLookupAsk<'_>) -> Result<Vec<u8>, i32> {
    // The payload: one `lookup_ask` in the stage's own tag.
    let payload_len = ask
        .text
        .len()
        .checked_add(ask.language.len())
        .and_then(|n| n.checked_add(ask.context.len()))
        .and_then(|n| n.checked_add(32))
        .ok_or(ERR_NO_SPACE)?;
    let mut payload = scratch(payload_len)?;
    let used =
        stage::LookupAsk::new(ask.text, ask.language, ask.context, 0).encode(&mut payload)?;
    let record = wire::Record::StageCall(wire::StageCall {
        stage: stage::KEY,
        schema: u32::from(stage::MAJOR),
        payload: payload.get(..used).ok_or(ERR_INVALID)?,
    });
    let capacity = codec::framed_len(&record)?
        .checked_add(1)
        .filter(|length| *length <= CALL_MAX)
        .ok_or(ERR_NO_SPACE)?;
    let mut framed = scratch(capacity)?;
    if codec::encode_batch(&[record], &mut framed)? != capacity {
        return Err(ERR_INVALID);
    }
    Ok(framed)
}

/// Runs the chosen provider and writes its entry, as the service's
/// `lookup_entry` naming it, at the front of `out`.
///
/// The host's own failures (the scratch it cannot get, an ask that does not
/// fit, an `out` that is too small) are the status. Whatever the provider does
/// wrong is **its** page only: its own refusal (`ERR_NOT_FOUND`, `ERR_BUSY`,
/// `ERR_NO_MEMORY`, a trap) and a reply the stage check or the decoder refuses
/// all answer `ERR_NOT_FOUND`, which the broker turns into the empty entry with
/// `more`, so one broken provider never hides the ones after it.
fn call(
    frame: &dyn Frame,
    chosen: &Chosen,
    ask: &wire::ServiceLookupAsk<'_>,
    out: &mut [u8],
) -> Result<usize, i32> {
    let input = framed(ask)?;
    let mut reply = scratch(stage::MAX_REPLY)?;
    let Ok(length) = frame.call_native(
        chosen.owner,
        chosen.token,
        &input,
        &mut reply,
        &mut |bytes| stage_wire::validate_reply(bytes, stage::MAX_REPLY, 1),
    ) else {
        return Err(ERR_NOT_FOUND);
    };
    let entry = reply
        .get(..length)
        .and_then(|bytes| stage::LookupEntry::decode(bytes).ok())
        .ok_or(ERR_NOT_FOUND)?;
    wire::record_encode_one(
        &wire::Record::ServiceLookupEntry(wire::ServiceLookupEntry {
            provider: chosen.name(),
            title: entry.title,
            text: entry.text,
            flags: entry.flags,
        }),
        out,
    )
}

impl LookupHost for Providers {
    fn ask(
        &self,
        frame: &dyn Frame,
        owner: u64,
        ask: &wire::ServiceLookupAsk<'_>,
        out: &mut [u8],
    ) -> Result<usize, i32> {
        let chosen = choose(frame, owner, ask.language, ask.cursor)?.ok_or(ERR_NOT_FOUND)?;
        let result = call(frame, &chosen, ask, out);
        broker::finish(chosen.name(), chosen.more, result, out)
    }
}

#[cfg(test)]
#[path = "providers_tests.rs"]
mod tests;

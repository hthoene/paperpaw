//! Host side of the service `core.lookup` (`docs/abi/document.md` 7).
//!
//! One op, `lookup`: the asker names a text and gets the entry of one
//! provider per call, `cursor` the index of the provider page (0 first). The
//! host walks the providers in the user's order through its stage
//! `core.lookup`, one provider per call, no provider seeing another's answer;
//! the broker, the user's order and the language and network filters are the
//! [`LookupHost`]'s. This file owns the input rules and the check of what a
//! provider answered: exactly one `lookup_entry`, its flags known, its fields
//! within the contract. A malformed provider answer is `ERR_INVALID`; running
//! out of providers is `ERR_NOT_FOUND`, which ends the asker's loop.
//!
//! Memory: nothing is kept; the call runs in the dispatcher's 4,096-byte
//! scratch (a `lookup_entry` is at most 2,384 bytes).
use super::registrations::{self as wire, Record};
use crate::abi7::{context::Frame, services_wire::ServiceOp};
use alloc::rc::Rc;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_UNSUPPORTED};

/// `lookup_entry.flags`: bit 0 more pages follow, bit 1 pending (the provider
/// answers later).
const ENTRY_FLAGS: u32 = 0b11;

/// What the shell provides under the key `core.lookup`.
pub(crate) trait LookupHost {
    /// The entry of the `cursor`th provider page for this ask, in the user's
    /// order, written into `out` as one `lookup_entry`; `ERR_NOT_FOUND` past the
    /// last provider. A provider that fails answers its own status.
    fn ask(
        &self,
        owner: u64,
        ask: &wire::ServiceLookupAsk<'_>,
        out: &mut [u8],
    ) -> Result<usize, i32>;
}

/// What the shell hands the frame under the key `core.lookup`.
pub(crate) struct LookupServices(pub(crate) Rc<dyn LookupHost>);

/// The `core.lookup` backend.
pub(crate) fn backend(
    frame: &dyn Frame,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let services = frame
        .host_service("core.lookup")
        .and_then(<dyn core::any::Any>::downcast_ref::<LookupServices>)
        .ok_or(ERR_UNSUPPORTED)?;
    if op.id != 1 {
        return Err(ERR_UNSUPPORTED);
    }
    let Record::ServiceLookupAsk(ask) = wire::records_decode_single(input)? else {
        return Err(ERR_INVALID);
    };
    // The text is what is looked up; the language is a BCP 47 tag or empty.
    if ask.text.is_empty() || !language_ok(ask.language) {
        return Err(ERR_INVALID);
    }
    let length = services.0.ask(frame.caller()?, &ask, out)?;
    let reply = out.get(..length).ok_or(ERR_INVALID)?;
    check(reply)?;
    Ok(length)
}

/// A BCP 47 tag as the contract spells it (`en`, `pt-BR`, `zh-Hant-TW`):
/// 2 to 35 bytes of ASCII letters and digits in hyphen-separated subtags of
/// 1 to 8, or empty for "any".
fn language_ok(tag: &str) -> bool {
    tag.is_empty()
        || (tag.len() >= 2
            && tag.split('-').all(|subtag| {
                (1..=8).contains(&subtag.len()) && subtag.bytes().all(|b| b.is_ascii_alphanumeric())
            }))
}

fn check(reply: &[u8]) -> Result<(), i32> {
    let (record, used) = wire::record_decode_prefix(reply).map_err(|_| ERR_INVALID)?;
    match record {
        Record::ServiceLookupEntry(entry)
            if used == reply.len()
                && entry.flags & !ENTRY_FLAGS == 0
                && !entry.provider.is_empty() =>
        {
            Ok(())
        }
        _ => Err(ERR_INVALID),
    }
}

#[cfg(test)]
#[path = "lookup_service/tests.rs"]
mod tests;

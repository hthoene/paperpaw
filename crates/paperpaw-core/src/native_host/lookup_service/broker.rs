//! The page of the lookup broker: the provider at the cursor answers, and its
//! refusal "not found" (it has no entry for the text) is an **empty entry**,
//! never the status: `ERR_NOT_FOUND` out of this service means "no providers
//! left" and ends the asker's loop, so one provider without an entry must not
//! hide the providers after it (`docs/abi/document.md` 7).
//!
//! Nothing is kept: the empty entry is built in the caller's scratch, and the
//! providers' own answers pass untouched (the service checks them).
use crate::native_host::codec;
use crate::native_host::registrations::{self as wire, Record};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NOT_FOUND};

/// What runs a provider: its name and the scratch to write its `lookup_entry`
/// into; the length written, or its status.
pub(super) type Call<'a> = dyn FnMut(&str, &mut [u8]) -> Result<usize, i32> + 'a;

/// `lookup_entry.flags` bit 0: more provider pages follow.
const MORE: u32 = 1;
/// The longest provider name an entry carries.
const NAME_MAX: usize = 64;

/// The entry of the provider at `cursor` of `providers` (in the user's
/// order, already filtered), written into `out` as one `lookup_entry`:
/// `call` runs the provider's stage with the name and gives its answer's
/// length. `ERR_NOT_FOUND` past the last provider; a provider's own
/// `ERR_NOT_FOUND` is an entry with its name, no title, no text and no flags.
///
/// # Errors
/// `ERR_NOT_FOUND` for a cursor past the last provider; a provider's other
/// statuses as they are; `ERR_INVALID` for a name the record cannot carry;
/// `ERR_NO_SPACE` for a scratch that cannot hold the entry.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the list form of the page; the production host uses `finish` (todo 109)"
    )
)]
pub(super) fn page(
    providers: &[&str],
    cursor: u32,
    call: &mut Call<'_>,
    out: &mut [u8],
) -> Result<usize, i32> {
    let at = usize::try_from(cursor).map_err(|_| ERR_NOT_FOUND)?;
    let name = providers.get(at).ok_or(ERR_NOT_FOUND)?;
    if name.is_empty() || name.len() > NAME_MAX {
        return Err(ERR_INVALID);
    }
    let more = at + 1 < providers.len();
    finish(name, more, call(name, out), out)
}

/// What a provider's call `result` is as the page of provider `name`: its
/// own entry (`out` holds it) with `more` as the broker says, or the empty
/// entry for its `ERR_NOT_FOUND`; any other status as it is. For a host that
/// finds its provider without a list of names ([`page`] is the list's form).
pub(super) fn finish(
    name: &str,
    more: bool,
    result: Result<usize, i32>,
    out: &mut [u8],
) -> Result<usize, i32> {
    if name.is_empty() || name.len() > NAME_MAX {
        return Err(ERR_INVALID);
    }
    match result {
        Err(ERR_NOT_FOUND) => empty(name, more, out),
        Ok(length) => set_more(out, length, more),
        Err(status) => Err(status),
    }
}

/// A provider's own entry (`length` bytes of `out`) with `more` as the
/// broker says. The flags (a little-endian `u32`) come right before the last
/// field, `opens` (a varint length and its bytes); they are patched where
/// they are, after checking that they are what the record decodes to, so that
/// no copy of the texts is made.
fn set_more(out: &mut [u8], length: usize, more: bool) -> Result<usize, i32> {
    let reply = out.get(..length).ok_or(ERR_INVALID)?;
    let (entry, used) =
        codec::prefix::<wire::ServiceLookupEntry<'_>>(reply).map_err(|_| ERR_INVALID)?;
    let tail = varint_len(entry.opens.len()) + entry.opens.len();
    let at = length
        .checked_sub(tail)
        .and_then(|end| end.checked_sub(4))
        .filter(|_| used == length)
        .ok_or(ERR_INVALID)?;
    let flags = entry.flags;
    let wanted = (flags & !MORE) | if more { MORE } else { 0 };
    if wanted == flags {
        return Ok(length);
    }
    if reply[at..at + 4] != flags.to_le_bytes() {
        return Err(ERR_INVALID);
    }
    out[at..at + 4].copy_from_slice(&wanted.to_le_bytes());
    Ok(length)
}

/// The entry of a provider that has none for the text.
pub(super) fn empty(provider: &str, more: bool, out: &mut [u8]) -> Result<usize, i32> {
    codec::encode_one(
        &Record::ServiceLookupEntry(wire::ServiceLookupEntry {
            provider,
            title: "",
            text: "",
            flags: if more { MORE } else { 0 },
            opens: "",
        }),
        out,
    )
}

/// The bytes of the varint length prefix of `len` (7 bits a byte).
const fn varint_len(len: usize) -> usize {
    let mut bytes = 1;
    let mut rest = len >> 7;
    while rest != 0 {
        bytes += 1;
        rest >>= 7;
    }
    bytes
}

#[cfg(test)]
#[path = "broker_tests.rs"]
mod tests;

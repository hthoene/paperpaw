//! The page of the lookup broker: the provider at the cursor answers, and its
//! refusal "not found" (it has no entry for the text) is an **empty entry**,
//! never the status: `ERR_NOT_FOUND` out of this service means "no providers
//! left" and ends the asker's loop, so one provider without an entry must not
//! hide the providers after it (`docs/abi/document.md` 7).
//!
//! Nothing is kept: the empty entry is built in the caller's scratch, and the
//! providers' own answers pass untouched (the service checks them).
use crate::native_host::registrations::{self as wire, Record};
use paperpaw_native::host_glue::ERR_NOT_FOUND;

/// What runs a provider: its name and the scratch to write its `lookup_entry`
/// into; the length written, or its status.
pub(super) type Call<'a> = dyn FnMut(&str, &mut [u8]) -> Result<usize, i32> + 'a;

/// `lookup_entry.flags` bit 0: more provider pages follow.
const MORE: u32 = 1;

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
        reason = "the production LookupHost over the stage broker calls it (todo 109)"
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
    match call(name, out) {
        Err(ERR_NOT_FOUND) => empty(name, at + 1 < providers.len(), out),
        other => other,
    }
}

/// The entry of a provider that has none for the text.
fn empty(provider: &str, more: bool, out: &mut [u8]) -> Result<usize, i32> {
    wire::record_encode_one(
        &Record::ServiceLookupEntry(wire::ServiceLookupEntry {
            provider,
            title: "",
            text: "",
            flags: if more { MORE } else { 0 },
        }),
        out,
    )
}

#[cfg(test)]
#[path = "broker_tests.rs"]
mod tests;

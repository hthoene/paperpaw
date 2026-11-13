//! Where a word may break: host slot 38 `text.hyphenate`
//! (`docs/abi/document.md` 6.1). The host holds the tables (data plugins'
//! `[hyphenation]` files on the card); a plugin asks one word at a time, as
//! its layout wants to break it, and keeps no table.
use super::{Error, Host};
use paperpaw_native::host_glue::{PpBuffer, PpBytes};

/// The longest word asked about, in bytes.
pub const WORD_MAX: usize = 192;
/// The most break places one word has (a word of at most 48 characters):
/// an `out` of this many bytes holds every answer.
pub const BREAKS_MAX: usize = 47;
/// The longest language tag, in bytes.
pub const LANGUAGE_MAX: usize = 35;
/// The most characters `left` and `right` may ask for.
pub const MINIMUM_MAX: u32 = 16;

/// Where `word` may break in `language`: writes into `out` the byte offset
/// into `word` before which a hyphen goes, one byte each, ascending, each at
/// a character boundary, and returns how many. `left` and `right` are the
/// fewest characters before and after a break (1 to [`MINIMUM_MAX`], or 0
/// for the table's own: the reader's `core.hyphen-min-left` / `-right`).
///
/// `language` is a tag (`[A-Za-z0-9-]`, 1 to 35 bytes); the host tries it,
/// then drops its last `-` part until one is left. `word` is letters only by
/// the caller's choice (no punctuation, no soft hyphens), matched without
/// regard to case. `Ok(0)`: the word may not break (empty, too short for
/// `left` + `right`, over 48 characters, no pattern allows it) or the table
/// cannot be read at this moment; leave it whole. An empty `word` asks
/// whether the language has a table at all (`Ok(0)` or `NotFound`). An `out`
/// of [`BREAKS_MAX`] bytes always fits. Allocates nothing; works in any
/// callback.
///
/// # Errors
/// `Invalid` for a malformed tag, a word over [`WORD_MAX`] bytes or a
/// minimum over [`MINIMUM_MAX`] (nothing is asked), and for an answer that
/// breaks the contract; `NotFound` where no table serves the language;
/// `NoSpace` for an `out` too small for every place (nothing written); the
/// host's other refusals, `Unsupported` on a host that has not wired the slot.
pub fn hyphenate(
    host: &Host<'_>,
    language: &str,
    word: &str,
    left: u32,
    right: u32,
    out: &mut [u8],
) -> Result<usize, Error> {
    let tag_ok = !language.is_empty()
        && language.len() <= LANGUAGE_MAX
        && language
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if !tag_ok || word.len() > WORD_MAX || left > MINIMUM_MAX || right > MINIMUM_MAX {
        return Err(Error::Invalid);
    }
    // SAFETY: the host binding is live for this call; the tag, the word and
    // the output buffer are the caller's, live and disjoint for the call.
    let result = unsafe {
        (host.table.text_hyphenate)(
            host.ctx,
            PpBytes {
                ptr: language.as_ptr(),
                len: language.len(),
            },
            PpBytes {
                ptr: word.as_ptr(),
                len: word.len(),
            },
            left,
            right,
            PpBuffer {
                ptr: out.as_mut_ptr(),
                cap: out.len(),
            },
        )
    };
    let Ok(count) = usize::try_from(result) else {
        return Err(Error::from_status(result));
    };
    let places = out.get(..count).ok_or(Error::Invalid)?;
    if count > BREAKS_MAX || !places_ok(word, places) {
        return Err(Error::Invalid);
    }
    Ok(count)
}

/// Whether `places` are break places of `word` as the contract has them:
/// inside it, ascending, each at a character boundary.
fn places_ok(word: &str, places: &[u8]) -> bool {
    let mut last = 0;
    places.iter().all(|&place| {
        let at = usize::from(place);
        let ok = at > last && at < word.len() && word.is_char_boundary(at);
        last = at;
        ok
    })
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;

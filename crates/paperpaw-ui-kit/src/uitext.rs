//! Whole host texts: a text card is read part by part within a stated limit.
use alloc::string::String;
use paperpaw_sdk::native::{
    Error, Host,
    ui::{TextId, TextWrite, Ui},
};

use crate::mem::{SERVICE_BUFFER, buffer};

/// The whole text `text` of `ui`, read part by part into one string.
///
/// # Errors
/// [`Error::NoSpace`] when the text is longer than `limit` bytes (nothing is
/// returned of it), [`Error::Invalid`] for a reply that does not progress, the
/// host's refusals, and [`Error::NoMemory`].
pub fn read_all(ui: &Ui, host: &Host<'_>, text: TextId, limit: usize) -> Result<String, Error> {
    let mut input = buffer(SERVICE_BUFFER)?;
    let mut output = buffer(SERVICE_BUFFER)?;
    let mut result = String::new();
    let mut offset = 0usize;
    loop {
        let at = u32::try_from(offset).map_err(|_| Error::NoSpace)?;
        let part = ui.text_read(host, text, at, &mut input, &mut output)?;
        let total = usize::try_from(part.total()).map_err(|_| Error::NoSpace)?;
        if total > limit {
            return Err(Error::NoSpace);
        }
        let data = part.data();
        result
            .try_reserve(data.len())
            .map_err(|_| Error::NoMemory)?;
        result.push_str(data);
        offset = offset.checked_add(data.len()).ok_or(Error::NoSpace)?;
        if offset >= total {
            return Ok(result);
        }
        if data.is_empty() {
            return Err(Error::Invalid);
        }
    }
}

/// The most one part of a host text holds.
pub const PART_BYTES: usize = 3072;

/// Where the part of `text` that starts at `offset` (a character boundary) ends: at most
/// [`PART_BYTES`] on, cut back to a character boundary.
fn part_end(text: &str, offset: usize) -> usize {
    let mut end = text.len().min(offset + PART_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// Writes `text` to the host text `text_id` in parts of at most
/// [`PART_BYTES`] (cut at character boundaries), the last committing it. An
/// empty text is one write of no bytes.
///
/// # Errors
/// [`Error::NoSpace`] for an offset past `u32`, [`Error::NoMemory`], and the
/// host's refusals; a text begun is not committed then.
pub fn write_all(ui: &Ui, host: &Host<'_>, text_id: TextId, text: &str) -> Result<(), Error> {
    let mut input = buffer(SERVICE_BUFFER)?;
    let mut output = buffer(SERVICE_BUFFER)?;
    let mut offset = 0usize;
    loop {
        let end = part_end(text, offset);
        let last = end == text.len();
        let part = text.get(offset..end);
        debug_assert!(part.is_some(), "a part is cut at character boundaries");
        ui.text_write(
            host,
            text_id,
            u32::try_from(offset).map_err(|_| Error::NoSpace)?,
            part.unwrap_or_default(),
            if last {
                TextWrite::Last
            } else {
                TextWrite::Continue
            },
            &mut input,
            &mut output,
        )?;
        offset = end;
        if last {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{string::String, vec::Vec};

    fn parts(text: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut offset = 0;
        loop {
            let end = part_end(text, offset);
            out.push(text.get(offset..end).expect("a character boundary"));
            if end == text.len() {
                return out;
            }
            offset = end;
        }
    }

    /// Characters of 2, 3 and 4 bytes fall across the 3072-byte limit at every
    /// offset: the parts are whole characters, none is over the limit, and together
    /// they are the text.
    #[test]
    fn parts_are_cut_at_character_boundaries_across_the_limit() {
        for lead in 0..4 {
            for ch in ['é', '€', '😀'] {
                let mut text = String::new();
                text.extend(core::iter::repeat_n('a', lead));
                text.extend(core::iter::repeat_n(ch, 3 * PART_BYTES / ch.len_utf8() + 5));
                let parts = parts(&text);
                assert!(parts.len() >= 3);
                assert!(parts.iter().all(|p| p.len() <= PART_BYTES));
                assert!(
                    parts[..parts.len() - 1]
                        .iter()
                        .all(|p| p.len() > PART_BYTES - 4)
                );
                assert_eq!(parts.concat(), text);
            }
        }
    }

    #[test]
    fn an_empty_text_is_one_empty_part() {
        assert_eq!(parts(""), [""]);
    }

    #[test]
    fn a_text_of_exactly_one_part_is_one_part() {
        let text: String = core::iter::repeat_n('x', PART_BYTES).collect();
        assert_eq!(parts(&text), [text.as_str()]);
    }
}

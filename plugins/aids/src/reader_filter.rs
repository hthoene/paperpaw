//! Experimental reader-owned rules, not connected to the legacy registrations.
//! The entire payload is validated before any document block is changed.
use alloc::{string::ToString, vec::Vec};
use paperpaw_sdk::Translator;
use paperpaw_sdk::document::Block;

#[allow(
    dead_code,
    reason = "generated framing helpers are shared with other stage codecs"
)]
#[rustfmt::skip]
mod wire;

/// Configuration cannot be represented or the incoming reply is invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A rule violates schema semantics.
    Invalid,
    /// Canonical encoder/decoder status, including aggregate size refusal.
    Wire(i32),
}

/// Encode a complete configuration. Refusal never writes a partial reply.
/// The caller must surface refusal before adopting a changed configuration.
///
/// # Errors
/// Refuses more than 32 pairs, strings over 96 bytes or a reply over 3840 bytes.
pub fn encode(
    bold: bool,
    replacements: &str,
    pictures: &str,
    caption_word: &str,
    output: &mut [u8],
) -> Result<usize, Error> {
    let mut pairs = Vec::new();
    for pair in replacements.split(';') {
        let Some((from, to)) = pair.split_once('=') else {
            continue;
        };
        let (from, to) = (from.trim(), to.trim());
        if from.is_empty() {
            continue;
        }
        if pairs.len() == 32 || from.len() > 96 || to.len() > 96 {
            return Err(Error::Invalid);
        }
        pairs.push((from, to));
    }
    let policy = match pictures {
        super::pictures::CAPTION => 1,
        super::pictures::HIDE => 2,
        _ => 0,
    };
    let mut records = Vec::with_capacity(pairs.len() + 1);
    records.push(wire::Record::FilterRules(wire::FilterRules {
        signature: 0,
        bold: u32::from(bold),
        pictures: policy,
        caption_word,
    }));
    for (from, to) in &pairs {
        records.push(wire::Record::FilterPair(wire::FilterPair {
            from,
            to,
            whole_word: 1,
        }));
    }
    // Hash the canonical rules with the signature zeroed. The final encode is
    // preflighted separately, so the caller's output stays untouched on refusal.
    let mut scratch = alloc::vec![0; wire::RECORD_MAX_BYTES];
    let len = wire::records_encode(&records, &mut scratch).map_err(Error::Wire)?;
    let signature = scratch[..len]
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    if let Some(wire::Record::FilterRules(rules)) = records.first_mut() {
        rules.signature = signature;
    }
    wire::records_encode(&records, output).map_err(Error::Wire)
}

/// Apply validated rules in the existing order: bold, replacements, pictures.
/// This operates on a complete reader block, not arbitrary byte windows.
///
/// # Errors
/// Malformed or semantically invalid replies leave the block unchanged.
pub fn apply(
    bytes: &[u8],
    path: &str,
    block: &mut Block,
) -> Result<(u64, paperpaw_sdk::filters::Verdict), Error> {
    let batch = wire::records_decode(bytes).map_err(Error::Wire)?;
    let mut rows = batch.iter();
    let Some(wire::Record::FilterRules(rules)) = rows.next() else {
        return Err(Error::Invalid);
    };
    if rules.bold > 1 || rules.pictures > 2 {
        return Err(Error::Invalid);
    }
    let mut pairs = Vec::new();
    for row in rows {
        let wire::Record::FilterPair(pair) = row else {
            return Err(Error::Invalid);
        };
        if pair.from.is_empty() || pair.whole_word != 1 {
            return Err(Error::Invalid);
        }
        pairs.push((pair.from.to_string(), pair.to.to_string()));
    }
    if rules.bold == 1 {
        super::bold_beginnings(block);
    }
    super::replace(block, &pairs);
    let verdict = super::pictures::apply_word(rules.pictures, path, rules.caption_word, block);
    Ok((rules.signature, verdict))
}

/// Encode using the same localized caption label as the existing filter.
///
/// # Errors
/// Returns the same bounded configuration refusals as [`encode`].
pub fn encode_localized(
    bold: bool,
    replacements: &str,
    pictures: &str,
    i18n: &Translator,
    output: &mut [u8],
) -> Result<usize, Error> {
    encode(
        bold,
        replacements,
        pictures,
        i18n.tr("aids.picture"),
        output,
    )
}

#[cfg(test)]
mod tests;

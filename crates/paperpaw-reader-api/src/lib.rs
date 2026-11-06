//! Versioned, bounded reader-owned stage payloads. No host ABI types cross here.
#![no_std]

#[allow(dead_code, clippy::all, reason = "canonical generated codec")]
#[rustfmt::skip]
pub mod wire;

use wire::Record;

pub const VERSION: u32 = 1;
pub const MAX_CHAPTER_INSERTS: u32 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Insert,
    Style,
    Decor,
    Layer,
    Status,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Wire(i32),
    Invalid,
    WrongStage,
    Limit,
}

impl Stage {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Insert => "reader.insert",
            Self::Style => "reader.style",
            Self::Decor => "reader.decor",
            Self::Layer => "reader.layer",
            Self::Status => "reader.status",
        }
    }
}

fn valid(record: Record<'_>) -> bool {
    match record {
        Record::InsertQuery(q) => {
            q.start <= q.end
                && q.width > 0
                && q.page_height > 0
                && q.page_height <= u32::MAX / 4
                && q.font_size > 0
        }
        Record::InsertItem(i) => i.side <= 1 && i.height > 0 && i.flags & !7 == 0,
        Record::StyleQuery(q) => {
            q.start <= q.end && q.end - q.start == q.text.len() as u32 && q.final_chunk <= 1
        }
        Record::StyleBlock(s) => {
            s.align.is_none_or(|v| v <= 3)
                && s.size.is_none_or(|v| (1..=1000).contains(&v))
                && s.line_height.is_none_or(|v| (1..=1000).contains(&v))
                && s.drop_cap.is_none_or(|v| v <= 16)
        }
        Record::StyleRun(s) => {
            s.start <= s.end
                && s.set & s.clear == 0
                && s.color.is_none_or(|v| v >> 24 == 0)
                && s.weight.is_none_or(|v| (1..=1000).contains(&v))
        }
        Record::DecorQuery(q) => {
            q.start <= q.end && q.end - q.start == q.text.len() as u32 && q.final_chunk <= 1
        }
        Record::DecorRange(r) => r.start < r.end && !r.role.is_empty() && r.flags & !1 == 0,
        Record::LayerQuery(q) => q.width > 0 && q.height > 0 && q.final_chunk <= 1,
        Record::LayerItem(i) => i.side <= 3 && i.flags & !3 == 0,
        Record::StatusQuery(q) => {
            q.progress_ppm <= 1_000_000
                && q.page != Some(0)
                && q.pages != Some(0)
                && !matches!((q.page,q.pages), (Some(p),Some(n)) if p>n)
        }
        Record::StatusItem(i) => i.width > 0 && i.height > 0 && i.flags & !3 == 0,
        Record::PageLine(l) => l.start <= l.end && l.width > 0 && l.height > 0,
        Record::PageWord(w) => w.start < w.end && w.width > 0 && w.height > 0,
        Record::DrawItem(_) | Record::InputTap(_) => true,
    }
}

fn allowed(stage: Stage, reply: bool, r: Record<'_>) -> bool {
    matches!(
        (stage, reply, r),
        (Stage::Insert, false, Record::InsertQuery(_))
            | (Stage::Insert, true, Record::InsertItem(_))
            | (Stage::Style, false, Record::StyleQuery(_))
            | (Stage::Style, _, Record::StyleBlock(_) | Record::StyleRun(_))
            | (
                Stage::Decor,
                false,
                Record::DecorQuery(_) | Record::PageWord(_)
            )
            | (Stage::Decor, true, Record::DecorRange(_))
            | (
                Stage::Layer,
                false,
                Record::LayerQuery(_) | Record::PageLine(_) | Record::PageWord(_)
            )
            | (Stage::Layer, true, Record::LayerItem(_))
            | (Stage::Status, false, Record::StatusQuery(_))
            | (Stage::Status, true, Record::StatusItem(_))
    )
}

fn validate<'a>(
    stage: Stage,
    reply: bool,
    records: impl Iterator<Item = Record<'a>>,
) -> Result<(), Error> {
    let mut count = 0;
    for record in records {
        if !reply {
            let query = matches!(
                record,
                Record::InsertQuery(_)
                    | Record::StyleQuery(_)
                    | Record::DecorQuery(_)
                    | Record::LayerQuery(_)
                    | Record::StatusQuery(_)
            );
            if query != (count == 0) {
                return Err(Error::Invalid);
            }
        }
        if !allowed(stage, reply, record) {
            return Err(Error::WrongStage);
        }
        if !valid(record) {
            return Err(Error::Invalid);
        }
        count += 1;
    }
    if count > 8 || (stage == Stage::Insert && reply && count > 4) {
        return Err(Error::Limit);
    }
    if !reply && count == 0 {
        return Err(Error::Invalid);
    }
    Ok(())
}

pub fn decode(stage: Stage, reply: bool, bytes: &[u8]) -> Result<wire::RecordBatch<'_>, Error> {
    let batch = wire::records_decode(bytes).map_err(Error::Wire)?;
    validate(stage, reply, batch.iter())?;
    Ok(batch)
}

pub fn encode(
    stage: Stage,
    reply: bool,
    records: &[Record<'_>],
    out: &mut [u8],
) -> Result<usize, Error> {
    validate(stage, reply, records.iter().copied())?;
    wire::records_encode(records, out).map_err(Error::Wire)
}

/// Breakable pieces get distinct reader-assigned ordinals, never one ambiguous locator.
pub fn insert_pieces(height: u32, page_height: u32, breakable: bool) -> Result<u32, Error> {
    let cap = page_height.checked_mul(4).ok_or(Error::Invalid)?;
    if page_height == 0 || height == 0 || height > cap {
        return Err(Error::Invalid);
    }
    if !breakable {
        return Ok(1);
    }
    let step = (page_height / 8).max(1);
    Ok(height.div_ceil(step))
}

/// Admit fragments atomically against the chapter budget. Zero is reserved for text.
/// The caller commits the returned total only after admitting the complete item.
pub fn admit_insert(
    current: u32,
    height: u32,
    page_height: u32,
    breakable: bool,
) -> Result<u32, Error> {
    let pieces = insert_pieces(height, page_height, breakable)?;
    let total = current.checked_add(pieces).ok_or(Error::Limit)?;
    if total > MAX_CHAPTER_INSERTS {
        return Err(Error::Limit);
    }
    Ok(total)
}

/// A fragment has a unique page locator and contributes no source-text progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertPiece {
    pub ordinal: u32,
    pub offset: u32,
    pub height: u32,
    pub progress_ppm: u32,
}

/// Validate the whole item before exposing pieces. Preserve source-anchor progress.
pub fn pieces(
    current: u32,
    height: u32,
    page_height: u32,
    breakable: bool,
    progress_ppm: u32,
) -> Result<impl Iterator<Item = InsertPiece>, Error> {
    if progress_ppm > 1_000_000 {
        return Err(Error::Invalid);
    }
    let end = admit_insert(current, height, page_height, breakable)?;
    let step = if breakable {
        (page_height / 8).max(1)
    } else {
        height
    };
    Ok((current + 1..=end)
        .enumerate()
        .map(move |(index, ordinal)| {
            // Admission bounded the count and height; this product is below height.
            let offset = index as u32 * step;
            InsertPiece {
                ordinal,
                offset,
                height: step.min(height - offset),
                progress_ppm,
            }
        }))
}

/// Check a source-byte range against the callback's UTF-8 chunk, without slicing it.
pub fn source_range(chunk_start: u32, text: &str, start: u32, end: u32) -> Result<(), Error> {
    let start = start.checked_sub(chunk_start).ok_or(Error::Invalid)? as usize;
    let end = end.checked_sub(chunk_start).ok_or(Error::Invalid)? as usize;
    if start > end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// Decision 131: only a user grant can let this subscriber replace a protected value.
pub const fn may_override(user_protected: bool, user_granted: bool) -> bool {
    !user_protected || user_granted
}

#[cfg(test)]
mod tests;

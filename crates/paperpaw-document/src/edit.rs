//! Fallible edits of a block, all or nothing: [`Block::try_transform`]
//! hands a filter a working copy of the block's text, offset map and runs
//! ([`TextEdit`]) under a byte limit, and puts it back only where the
//! filter succeeds. Every step reserves what it needs, after checking the
//! limit, before it changes anything, so the block stays exactly as it was
//! when memory runs out, the limit is reached or the filter refuses
//! (#1028). The copy never leaves the call, so it can only go back to the
//! block it came from.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use core::ops::Range;

use crate::{Align, Block, BlockKind, OffsetMap, Run, TextStyle};

/// Why an edit of a block's text was refused; the text is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditError {
    /// Memory for the edit could not be reserved.
    NoMemory,
    /// A range or run off character boundaries, outside the text, out of
    /// order, or a text or source offset past what `u32` names.
    Range,
    /// The edit would hold more than its byte limit.
    Limit,
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            EditError::NoMemory => "not enough memory for the edit",
            EditError::Range => "edit range or run out of place",
            EditError::Limit => "edit over its byte limit",
        })
    }
}

/// A working copy of a block's text, offset map and runs, and the
/// block's kind, alignment and picture (see the module documentation).
/// Other fields of the block are not copied.
#[derive(Debug, PartialEq, Eq)]
pub struct TextEdit {
    start: u32,
    end: u32,
    limit: usize,
    text: String,
    map: OffsetMap,
    runs: Vec<Run>,
    kind: BlockKind,
    align: Align,
    drop_image: bool,
}

const CHECKPOINT: usize = core::mem::size_of::<(u32, u32)>();
const RUN: usize = core::mem::size_of::<Run>();

fn copy_of<T: Copy>(items: &[T]) -> Result<Vec<T>, EditError> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(items.len())
        .map_err(|_| EditError::NoMemory)?;
    copy.extend_from_slice(items);
    Ok(copy)
}

impl Block {
    /// Runs `filter` on a working copy of this block and puts the copy in
    /// place of the block's text, offset map, runs, kind, alignment and
    /// picture only where it returns `Ok`; the copy holds at most `limit`
    /// bytes of heap at any moment of the edit ([`TextEdit::heap_bytes`]).
    /// Putting it back allocates nothing.
    ///
    /// # Errors
    /// [`EditError::NoMemory`] or [`EditError::Limit`] where the copy
    /// cannot be made, and whatever `filter` returns (an [`EditError`] of
    /// a step converts into `E`). The block is then unchanged.
    pub fn try_transform<T, E: From<EditError>>(
        &mut self,
        limit: usize,
        filter: impl FnOnce(&mut TextEdit) -> Result<T, E>,
    ) -> Result<T, E> {
        let mut edit = self.try_edit(limit)?;
        let value = filter(&mut edit)?;
        self.text = edit.text;
        self.map = edit.map;
        self.runs = edit.runs;
        self.kind = edit.kind;
        self.align = edit.align;
        if edit.drop_image {
            self.image = None;
        }
        Ok(value)
    }

    fn try_edit(&self, limit: usize) -> Result<TextEdit, EditError> {
        let need = self
            .text
            .len()
            .saturating_add(self.map.checkpoints.len().saturating_mul(CHECKPOINT))
            .saturating_add(self.runs.len().saturating_mul(RUN));
        if need > limit {
            return Err(EditError::Limit);
        }
        let mut text = String::new();
        text.try_reserve_exact(self.text.len())
            .map_err(|_| EditError::NoMemory)?;
        text.push_str(&self.text);
        Ok(TextEdit {
            start: self.start,
            end: self.end,
            limit,
            text,
            map: OffsetMap {
                checkpoints: copy_of(&self.map.checkpoints)?,
            },
            runs: copy_of(&self.runs)?,
            kind: self.kind,
            align: self.align,
            drop_image: false,
        })
    }

    /// Replaces bytes `range` of `text` (on character boundaries) with
    /// `with`, e.g. in a text filter: styles move along, and positions in
    /// the source stay right around it (the new text maps to where the old
    /// one was). A range off character boundaries, and an edit memory
    /// cannot be found for, leave the block unchanged; filters that must
    /// know use [`Block::try_transform`].
    pub fn replace(&mut self, range: Range<usize>, with: &str) {
        let range = range.start..range.end.min(self.text.len());
        let _ = replace_in(
            Parts {
                start: self.start,
                end: self.end,
                limit: usize::MAX,
                text: &mut self.text,
                map: &mut self.map,
                runs: &mut self.runs,
            },
            range,
            with,
        );
    }
}

impl TextEdit {
    /// The text as edited so far.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The runs as edited so far, sorted and not overlapping.
    pub fn runs(&self) -> &[Run] {
        &self.runs
    }

    /// The style of the character at byte `index` of the text, as
    /// [`Block::style_at`].
    pub fn style_at(&self, index: usize) -> TextStyle {
        style_at(&self.runs, index)
    }

    /// The source offset of byte `index` of the text, as
    /// [`Block::source_offset`].
    pub fn source_offset(&self, index: usize) -> u32 {
        source_offset(self.start, self.end, &self.text, &self.map, index)
    }

    /// The heap this copy holds, in bytes (capacities). No step lets it,
    /// or what a step holds while it runs, pass the limit.
    pub fn heap_bytes(&self) -> usize {
        self.text.capacity()
            + self.map.checkpoints.capacity() * CHECKPOINT
            + self.runs.capacity() * RUN
    }

    /// The byte limit of this edit.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// The block's kind as edited so far.
    pub fn kind(&self) -> BlockKind {
        self.kind
    }

    /// Makes the block a block of `kind` (e.g. a picture's caption a
    /// paragraph).
    pub fn set_kind(&mut self, kind: BlockKind) {
        self.kind = kind;
    }

    /// The block's alignment as edited so far.
    pub fn align(&self) -> Align {
        self.align
    }

    /// Aligns the block's lines `align`.
    pub fn set_align(&mut self, align: Align) {
        self.align = align;
    }

    /// Takes the block's picture away (a caption in its place).
    pub fn drop_image(&mut self) {
        self.drop_image = true;
    }

    /// Whether the picture goes.
    pub fn drops_image(&self) -> bool {
        self.drop_image
    }

    /// [`Block::replace`] on the copy, all or nothing.
    ///
    /// # Errors
    /// [`EditError::Range`] for a range outside the text or off character
    /// boundaries, a result longer than `u32` offsets name, or one whose
    /// source offsets would pass `u32::MAX`;
    /// [`EditError::Limit`] where the copy would pass its limit, checked
    /// before anything is reserved; [`EditError::NoMemory`] where the
    /// edit's memory cannot be reserved. Either way the copy is unchanged.
    pub fn try_replace(&mut self, range: Range<usize>, with: &str) -> Result<(), EditError> {
        replace_in(
            Parts {
                start: self.start,
                end: self.end,
                limit: self.limit,
                text: &mut self.text,
                map: &mut self.map,
                runs: &mut self.runs,
            },
            range,
            with,
        )
    }

    /// Adds `run` after the last one.
    ///
    /// # Errors
    /// [`EditError::Range`] for an empty run, one past the text, off
    /// character boundaries or starting before the last run ends;
    /// [`EditError::Limit`], [`EditError::NoMemory`]. The copy is
    /// unchanged.
    pub fn try_push_run(&mut self, run: Run) -> Result<(), EditError> {
        let after = self.runs.last().map_or(0, |last| last.end);
        if !self.fits(run) || run.start < after {
            return Err(EditError::Range);
        }
        // A growth may double the runs' room, the old room held meanwhile.
        if self.runs.len() == self.runs.capacity() {
            let grown = self.runs.capacity().max(2) * 3 * RUN;
            if self.heap_bytes() - self.runs.capacity() * RUN + grown > self.limit {
                return Err(EditError::Limit);
            }
        }
        self.runs.try_reserve(1).map_err(|_| EditError::NoMemory)?;
        self.runs.push(run);
        Ok(())
    }

    /// Puts `runs` in place of the copy's runs.
    ///
    /// # Errors
    /// [`EditError::Range`] where `runs` are not sorted, overlap, are
    /// empty or lie off the text's character boundaries;
    /// [`EditError::Limit`], [`EditError::NoMemory`]. The copy is
    /// unchanged.
    pub fn try_set_runs(&mut self, runs: &[Run]) -> Result<(), EditError> {
        let mut after = 0;
        for &run in runs {
            if !self.fits(run) || run.start < after {
                return Err(EditError::Range);
            }
            after = run.end;
        }
        // The new runs are made while the old ones are still held.
        if self.heap_bytes() + runs.len() * RUN > self.limit {
            return Err(EditError::Limit);
        }
        let mut new = copy_of(runs)?;
        core::mem::swap(&mut self.runs, &mut new);
        Ok(())
    }

    fn fits(&self, run: Run) -> bool {
        run.start < run.end
            && run.end as usize <= self.text.len()
            && self.text.is_char_boundary(run.start as usize)
            && self.text.is_char_boundary(run.end as usize)
    }
}

pub(crate) fn style_at(runs: &[Run], index: usize) -> TextStyle {
    let index = index as u32;
    let at = runs.partition_point(|run| run.end <= index);
    match runs.get(at) {
        Some(run) if run.start <= index => run.style,
        _ => TextStyle::REGULAR,
    }
}

pub(crate) fn source_offset(
    start: u32,
    end: u32,
    text: &str,
    map: &OffsetMap,
    index: usize,
) -> u32 {
    if index >= text.len() {
        return end;
    }
    map.source_offset(start, index)
}

/// What a replacement changes, borrowed from a block or a copy.
struct Parts<'a> {
    start: u32,
    end: u32,
    limit: usize,
    text: &'a mut String,
    map: &'a mut OffsetMap,
    runs: &'a mut Vec<Run>,
}

/// The source offset of text index `index`, `end` from the text's end on;
/// `None` past what `u32` names.
fn checked_source(start: u32, end: u32, text: &str, map: &OffsetMap, index: usize) -> Option<u32> {
    if index >= text.len() {
        return Some(end);
    }
    map.checked_source_offset(start, index)
}

/// Whether every text index below `len` maps to a source offset `u32`
/// names, for a map starting at `start` with checkpoints `points` (text
/// index, source offset) in order.
fn offsets_fit(start: u32, len: usize, points: impl Iterator<Item = (usize, u32)>) -> bool {
    // Text indices `text..next` map to `source..source + (next - text)`.
    let fits = |(text, source): (usize, u32), next: usize| {
        next <= text || (next - text - 1) as u64 <= u64::from(u32::MAX - source)
    };
    let mut from = (0, start);
    for point in points {
        if !fits(from, point.0) {
            return false;
        }
        from = point;
    }
    fits(from, len)
}

/// The replacement: everything it needs is checked and reserved first,
/// then nothing it does can fail or allocate.
fn replace_in(parts: Parts<'_>, range: Range<usize>, with: &str) -> Result<(), EditError> {
    let Parts {
        start,
        end,
        limit,
        text,
        map,
        runs,
    } = parts;
    let (from, to) = (range.start, range.end);
    if from > to || to > text.len() || !text.is_char_boundary(from) || !text.is_char_boundary(to) {
        return Err(EditError::Range);
    }
    let new_len = (text.len() - (to - from))
        .checked_add(with.len())
        .filter(|&len| u32::try_from(len).is_ok())
        .ok_or(EditError::Range)?;
    if limit != usize::MAX && peak_of(text, map, runs, new_len) > limit {
        return Err(EditError::Limit);
    }
    // Every source offset of the new map must stay in `u32`, also where
    // the new text is longer than the old one (#277).
    let (Some(source_from), Some(source_to)) = (
        checked_source(start, end, text, map, from),
        checked_source(start, end, text, map, to),
    ) else {
        return Err(EditError::Range);
    };
    let shift = |i: u32| -> u32 { (i as usize + with.len() - (to - from)) as u32 };
    let old = &map.checkpoints;
    let points = || {
        old.iter()
            .filter(|&&(t, _)| (t as usize) < from)
            .map(|&(t, s)| (t as usize, s))
            .chain([(from, source_from), (from + with.len(), source_to)])
            .chain(
                old.iter()
                    .filter(|&&(t, _)| t as usize > to)
                    .map(|&(t, s)| (shift(t) as usize, s)),
            )
    };
    if !offsets_fit(start, new_len, points()) {
        return Err(EditError::Range);
    }
    let mut checkpoints: Vec<(u32, u32)> = Vec::new();
    checkpoints
        .try_reserve_exact(map.checkpoints.len() + 2)
        .map_err(|_| EditError::NoMemory)?;
    let more = new_len.saturating_sub(text.len());
    if limit == usize::MAX {
        text.try_reserve(more)
    } else {
        text.try_reserve_exact(more)
    }
    .map_err(|_| EditError::NoMemory)?;
    // Nothing below allocates: the checkpoints fit what was reserved, the
    // runs change in place, and the text has room for its new length.
    let mut new = OffsetMap { checkpoints };
    for (t, s) in points() {
        new.checkpoint(t, s);
    }
    *map = new;
    // The new text takes the style of the first run reaching into the
    // replaced text; runs after it that reach in too start behind it.
    let stop_new = (from + with.len()) as u32;
    let mut claimed = false;
    for run in runs.iter_mut() {
        let (run_start, run_stop) = (run.start as usize, run.end as usize);
        if run_stop <= from {
            continue;
        }
        if run_start >= to {
            run.start = shift(run.start);
        } else if claimed {
            run.start = stop_new;
        } else {
            run.start = run.start.min(from as u32);
            claimed = true;
        }
        run.end = if run_stop >= to {
            shift(run.end)
        } else {
            stop_new
        };
    }
    runs.retain(|run| run.start < run.end);
    text.replace_range(from..to, with);
    Ok(())
}

/// What a replacement under a limit holds at its peak: the text at its new
/// length (a text that grows moves into room of exactly that length, the
/// old room held meanwhile), the old and the new checkpoints, the runs.
/// Saturating: a sum past `usize` passes every limit.
#[allow(
    clippy::ptr_arg,
    reason = "the capacities count, not only the contents"
)]
fn peak_of(text: &String, map: &OffsetMap, runs: &Vec<Run>, new_len: usize) -> usize {
    let text_peak = if new_len > text.capacity() {
        text.capacity().saturating_add(new_len)
    } else {
        text.capacity()
    };
    let checkpoints = map
        .checkpoints
        .capacity()
        .saturating_add(map.checkpoints.len())
        .saturating_add(2)
        .saturating_mul(CHECKPOINT);
    text_peak
        .saturating_add(checkpoints)
        .saturating_add(runs.capacity().saturating_mul(RUN))
}

//! Checks of a format's answer before the host passes it on
//! (`docs/abi/document.md` 3: "the host passes them on after checking them").
//! A failed check is `ERR_INVALID` (a malformed answer) or `ERR_STATE`
//! (a changed generation); the caller counts the first as a strike.
use super::super::registrations::{self as wire, Record};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_STATE};

/// `doc_state.flags`: bits 0-6 are defined.
const STATE_FLAGS: u32 = 0x7f;
/// `doc_block.flags`: bit 0 continues, bit 1 continued; a table row's bits 8-10
/// say header, first, last row (`document.md` 4).
const BLOCK_FLAGS: u32 = 0b11 | (0b111 << 8);
/// `doc_meta.flags`: bit 0 title cut, bit 1 series.
const META_FLAGS: u32 = 0b11;
/// `doc_toc.flags`: bit 0 approximate, bit 1 title cut.
const TOC_FLAGS: u32 = 0b11;
/// `doc_toc.level`.
const TOC_LEVEL_MAX: u32 = 15;
/// 31 families, regular, bold, italic, bold italic.
const FONT_FACES: u32 = 124;
const PPM: u32 = 1_000_000;
/// The stage payload: a block fragment is never larger.
const FRAGMENT_MAX: usize = 3_840;
/// Interface 6 `MAX_COLUMNS` and `MAX_INLINE_LANGS`.
const MAX_CELLS: usize = 16;
const MAX_INLINE_LANGS: usize = 15;

/// The one record of a detail answer, with nothing after it.
pub(super) fn only(bytes: &[u8]) -> Result<Record<'_>, i32> {
    let (record, used) = wire::record_decode_prefix(bytes).map_err(|_| ERR_INVALID)?;
    if used == bytes.len() {
        Ok(record)
    } else {
        Err(ERR_INVALID)
    }
}

/// Rows of a paged answer, then exactly one `end`, last. Each row goes to
/// `row`; more than `max_rows` rows is `ERR_INVALID`.
pub(super) fn rows<'a>(
    bytes: &'a [u8],
    max_rows: usize,
    mut row: impl FnMut(&Record<'a>) -> Result<(), i32>,
) -> Result<wire::ServiceEnd, i32> {
    let mut at = 0;
    let mut count = 0;
    while at < bytes.len() {
        let (record, used) = wire::record_decode_prefix(&bytes[at..]).map_err(|_| ERR_INVALID)?;
        at += used;
        if let Record::ServiceEnd(end) = record {
            return if at == bytes.len() {
                Ok(end)
            } else {
                Err(ERR_INVALID)
            };
        }
        count += 1;
        if count > max_rows {
            return Err(ERR_INVALID);
        }
        row(&record)?;
    }
    Err(ERR_INVALID)
}

/// The paging record of a list: the generation is never 0 and, once the
/// caller named one, never another (`ERR_STATE`); a page that is not the last
/// moves the cursor forward, so a loop over pages ends.
pub(super) fn page_end(end: &wire::ServiceEnd, cursor: u32, generation: u32) -> Result<(), i32> {
    if end.generation == 0 || end.flags & !1 != 0 {
        return Err(ERR_INVALID);
    }
    if generation != 0 && end.generation != generation {
        return Err(ERR_STATE);
    }
    if end.flags & 1 == 0 && end.next <= cursor {
        return Err(ERR_INVALID);
    }
    Ok(())
}

fn require(ok: bool) -> Result<(), i32> {
    if ok { Ok(()) } else { Err(ERR_INVALID) }
}

pub(super) fn state(state: &wire::ServiceDocState<'_>) -> Result<(), i32> {
    require(
        state.state <= super::table::FAILED
            && state.progress_ppm <= PPM
            && state.flags & !STATE_FLAGS == 0
            // A failure carries its `ERR_*`; nothing else carries a status.
            && (state.state == super::table::FAILED) == (state.status < 0),
    )
}

pub(super) fn meta(meta: &wire::ServiceDocMeta<'_>) -> Result<(), i32> {
    require(meta.direction <= 1 && meta.writing_mode <= 2 && meta.flags & !META_FLAGS == 0)
}

/// `kind`: 0 title, 1 author, 2 subject, 3 description, 4 series.
pub(super) fn text(
    text: &wire::ServiceDocText<'_>,
    kind: u32,
    index: u32,
    offset: u32,
) -> Result<(), i32> {
    let rest = text.total.checked_sub(offset).ok_or(ERR_INVALID)?;
    let len = u32::try_from(text.text.len()).map_err(|_| ERR_INVALID)?;
    // At most 1,024 bytes from `offset`, cut only at a UTF-8 boundary (up to
    // three bytes short of the cap); the whole rest when it fits.
    let want = rest.min(1024);
    require(
        text.kind == kind
            && text.index == index
            && len <= want
            && len + 3 >= want
            && (rest > 1024 || len == rest),
    )
}

pub(super) fn section(row: &wire::ServiceDocSection, index: u32) -> Result<(), i32> {
    require(row.flags & !1 == 0 && row.index >= index)
}

pub(super) fn toc(row: &wire::ServiceDocToc<'_>, index: u32) -> Result<(), i32> {
    require(row.level <= TOC_LEVEL_MAX && row.flags & !TOC_FLAGS == 0 && row.index >= index)
}

pub(super) fn link(row: &wire::ServiceDocLink<'_>) -> Result<(), i32> {
    require(row.kind <= 2 && row.start <= row.end)
}

pub(super) fn font(row: &wire::ServiceDocFont) -> Result<(), i32> {
    require(row.index < FONT_FACES && (1..=31).contains(&row.family) && row.flags & !0b11 == 0)
}

/// An answer that names a position: this document, a plain position.
pub(super) fn at(at: &wire::ServiceDocAt, document: u32) -> Result<(), i32> {
    require(at.document == document && at.insert == 0 && at.cursor == 0 && at.generation == 0)
}

pub(super) fn progress(progress: &wire::ServiceDocProgress) -> Result<(), i32> {
    require(progress.progress_ppm <= PPM)
}

/// What a fragment of a block stream said, to be checked against the block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Fragment {
    /// The header, when this fragment holds it.
    pub(super) kind: Option<u32>,
    pub(super) text_bytes: u32,
    pub(super) parts: u64,
    pub(super) images: usize,
    pub(super) columns: usize,
    pub(super) cells: usize,
    langs: usize,
    run_end: u32,
    rank: u8,
}

/// The place of a record in the list of `document.md` 4: header, text parts,
/// runs, maps, image, columns, cells, rubies, languages.
pub(super) fn block_rank(record: &Record<'_>) -> Option<u8> {
    Some(match record {
        Record::ServiceDocBlock(_) => 0,
        Record::ServiceDocTextPart(_) => 1,
        Record::ServiceDocRun(_) => 2,
        Record::ServiceDocMap(_) => 3,
        Record::ServiceDocImage(_) => 4,
        Record::ServiceDocColumns(_) => 5,
        Record::ServiceDocCell(_) => 6,
        Record::ServiceDocRuby(_) => 7,
        Record::ServiceDocInlineLang(_) => 8,
        _ => return None,
    })
}

impl Fragment {
    /// One record of the list, in order. A fragment after the first starts
    /// anywhere in the list, so only its internal order is known.
    pub(super) fn feed(&mut self, record: &Record<'_>, first: bool, at: usize) -> Result<(), i32> {
        let rank = block_rank(record).ok_or(ERR_INVALID)?;
        if rank < self.rank || (first && at == 0) != (rank == 0) {
            return Err(ERR_INVALID);
        }
        self.rank = rank;
        match record {
            Record::ServiceDocBlock(block) => {
                let heading_ok = block.kind != 1 || (1..=6).contains(&block.level);
                require(heading_ok && block.flags & !BLOCK_FLAGS == 0)?;
                require(block.dir <= 1 && block.writing_mode <= 2)?;
                self.kind = Some(block.kind);
                self.text_bytes = block.text_bytes;
            }
            Record::ServiceDocTextPart(part) => {
                self.parts += part.text.len() as u64;
                // Never past the header's whole length, across fragments.
                require(self.parts <= u64::from(self.text_bytes))?;
            }
            Record::ServiceDocRun(run) => {
                // Sorted, not overlapping; inside the text where it is known.
                require(run.start <= run.end && run.start >= self.run_end)?;
                self.run_end = run.end;
                if self.kind.is_some() {
                    require(run.end <= self.text_bytes)?;
                }
            }
            Record::ServiceDocImage(_) => {
                self.images += 1;
                require(self.images <= 1)?;
            }
            Record::ServiceDocColumns(columns) => {
                // Up to 16 little-endian u16 weights.
                self.columns += 1;
                // One per table row, whatever the fragments.
                require(self.columns <= 1)?;
                require(columns.weights.len() % 2 == 0 && columns.weights.len() <= 32)?;
            }
            Record::ServiceDocCell(_) => {
                self.cells += 1;
                require(self.cells <= MAX_CELLS)?;
            }
            Record::ServiceDocInlineLang(_) => {
                self.langs += 1;
                require(self.langs <= MAX_INLINE_LANGS)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// A block that goes no further (`more` clear), seen from its header: its
    /// parts add up, and its kind has the records it needs and no others.
    pub(super) fn complete(&self) -> Result<(), i32> {
        // The header opened fragment 0 and the totals ran on across the rest.
        let kind = self.kind.ok_or(ERR_INVALID)?;
        require(
            self.parts == u64::from(self.text_bytes)
                && (self.images == 1) == (kind == 3)
                && self.images <= 1
                && (self.columns == 1) == (kind == 4)
                && (kind == 4 || self.cells == 0),
        )
    }
}

/// A fragment of a block stream: the records, then `end`. Returns the `end`
/// and the totals so far. `cursor` is the record index the caller asked from,
/// `generation` the one it named; `resume` what the earlier fragments said
/// (`Some` exactly when `cursor` is not 0).
pub(super) fn block<'a>(
    bytes: &'a [u8],
    cursor: u32,
    generation: u32,
    resume: Option<Fragment>,
    mut each: impl FnMut(&Record<'a>, usize) -> Result<(), i32>,
) -> Result<(wire::ServiceEnd, Fragment), i32> {
    // A fragment is a stage payload at most.
    require(bytes.len() <= FRAGMENT_MAX)?;
    require((cursor == 0) == resume.is_none())?;
    let mut fragment = resume.unwrap_or_default();
    let mut index = 0;
    let end = rows(bytes, usize::MAX, |record| {
        fragment.feed(record, cursor == 0, index)?;
        each(record, index)?;
        index += 1;
        Ok(())
    })?;
    let sent = u32::try_from(index).map_err(|_| ERR_INVALID)?;
    // Every fragment holds records, so a walk over fragments ends.
    if end.flags & !0b11 != 0 || sent == 0 {
        return Err(ERR_INVALID);
    }
    // The block's identity holds to its last fragment: a different or missing
    // one is a changed block.
    if cursor != 0 && end.generation != generation {
        return Err(ERR_STATE);
    }
    if generation != 0 && end.generation != 0 && end.generation != generation {
        return Err(ERR_STATE);
    }
    if end.flags & 2 != 0 {
        // The rest is asked for by record index, under the block's identity.
        if end.generation == 0 || Some(end.next) != cursor.checked_add(sent) {
            return Err(ERR_INVALID);
        }
    } else {
        fragment.complete()?;
    }
    Ok((end, fragment))
}

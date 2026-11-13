//! The open book as the layout reads it: a [`Document`] over the document
//! service (`core.document`, op 6 for the blocks). It lives for one callback,
//! because it holds that callback's host; the layout asks it for blocks and it
//! joins the fragments the host sends ([`crate::native_block`]). The buffers of
//! one read are on the heap, made once and kept (the stack of the X4 has no
//! room for a 4 KiB reply). Section lengths (op 5) and the picture and table
//! records are not read yet (`PORTING-7.md`).
use crate::native_block::{Assembler, pieces};
use crate::native_filter::{self, Filter, Shown};
use alloc::{borrow::Cow, vec::Vec};
use core::cell::RefCell;
use paperpaw_sdk::Storage;
use paperpaw_sdk::{
    document::{Block, Document, DocumentError, Locator, Metadata},
    native::{
        Error, Host,
        document::{BlockCursor, DocId, DocPosition, Documents},
    },
};

/// The most fragments one block may come in (a block of the longest text in
/// parts of 3,072 bytes needs about twenty).
pub const MAX_FRAGMENTS: usize = 64;
/// How often a block is read again from its start when the host says it changed.
pub const MAX_RESTARTS: usize = 3;
/// The size of the ask of one read.
pub const INPUT_BYTES: usize = 256;
/// The size of the reply of one read.
pub const REPLY_BYTES: usize = 4096;
/// Most blocks the text filters may leave out in a row (Beta 3's limit).
const MOST_DROPPED: usize = 10_000;

/// What one step of reading a block ended in.
#[derive(Debug)]
#[non_exhaustive]
#[allow(
    clippy::large_enum_variant,
    reason = "made once per block and moved out at once; a box would be an infallible allocation"
)]
pub enum Next {
    /// The block goes on: ask again with this cursor.
    More(BlockCursor),
    /// The block is whole.
    Done(Block),
}

/// Reads one block by asking `step` for its fragments in turn: each step
/// hands the pieces of one fragment to the assembler and says whether the
/// block goes on. A block the host says changed (`Error::State`) is read again
/// from its start, a few times at most.
///
/// # Errors
/// What a step or the assembler refuses; `NoSpace` for a block of more than
/// [`MAX_FRAGMENTS`] fragments or one that keeps changing.
pub fn read_block(
    mut step: impl FnMut(BlockCursor, &mut Assembler) -> Result<Next, Error>,
) -> Result<Block, Error> {
    let mut assembler = Assembler::new();
    let mut restarts = 0;
    let mut cursor = BlockCursor::start();
    let mut fragments = 0;
    loop {
        match step(cursor, &mut assembler) {
            Ok(Next::Done(block)) => return Ok(block),
            Ok(Next::More(next)) => {
                fragments += 1;
                if fragments >= MAX_FRAGMENTS {
                    assembler.restart();
                    return Err(Error::NoSpace);
                }
                cursor = next;
            }
            Err(Error::State) => {
                assembler.restart();
                restarts += 1;
                if restarts > MAX_RESTARTS {
                    return Err(Error::NoSpace);
                }
                cursor = BlockCursor::start();
                fragments = 0;
            }
            // Only the first ask of a block can mean the section has no further block.
            Err(Error::NotFound) if fragments > 0 || restarts > 0 => {
                assembler.restart();
                return Err(Error::Invalid);
            }
            Err(error) => {
                assembler.restart();
                return Err(error);
            }
        }
    }
}

/// What the layout is told of a refusal of the host.
#[must_use]
pub fn document_error(error: Error) -> DocumentError {
    match error {
        Error::NoMemory => DocumentError::OutOfMemory,
        Error::Busy => DocumentError::NotReady,
        Error::Unsupported => DocumentError::UnsupportedFormat,
        Error::Invalid | Error::NoSpace => DocumentError::Malformed(alloc::borrow::Cow::Borrowed(
            "a block of the document service",
        )),
        _ => DocumentError::Other(0),
    }
}

/// What the layout is given for the end of a read: a section's end
/// (`NotFound` of a first ask) is no block, not an error.
pub fn block_result(read: Result<Block, Error>) -> Result<Option<Block>, DocumentError> {
    match read {
        Ok(block) => Ok(Some(block)),
        Err(Error::NotFound) => Ok(None),
        Err(error) => Err(document_error(error)),
    }
}

/// The document service's book, read as a [`Document`].
pub struct HostDocument<'h> {
    host: &'h Host<'h>,
    documents: &'h Documents,
    id: DocId,
    sections: u32,
    whole_pages: bool,
    metadata: Metadata,
    buffers: RefCell<(Vec<u8>, Vec<u8>)>,
    /// The text filters a block goes through, and the book's path they see.
    filters: (&'h [Filter], &'h str),
}

impl<'h> HostDocument<'h> {
    /// The book `id`, which has `sections` sections.
    ///
    /// # Errors
    /// `NoMemory` for the buffers.
    pub fn new(
        host: &'h Host<'h>,
        documents: &'h Documents,
        id: DocId,
        sections: u32,
        whole_pages: bool,
    ) -> Result<Self, Error> {
        let (mut input, mut out) = (Vec::new(), Vec::new());
        input
            .try_reserve_exact(INPUT_BYTES)
            .map_err(|_| Error::NoMemory)?;
        out.try_reserve_exact(REPLY_BYTES)
            .map_err(|_| Error::NoMemory)?;
        input.resize(INPUT_BYTES, 0);
        out.resize(REPLY_BYTES, 0);
        Ok(Self {
            host,
            documents,
            id,
            sections,
            whole_pages,
            metadata: Metadata::default(),
            buffers: RefCell::new((input, out)),
            filters: (&[], ""),
        })
    }

    /// The same book with its blocks through `filters` (the book at `path`):
    /// a block they leave out gives the next one kept, as Beta 3's filtered
    /// book did.
    #[must_use]
    pub fn filtered(mut self, filters: &'h [Filter], path: &'h str) -> Self {
        self.filters = (filters, path);
        self
    }
}

impl core::fmt::Debug for HostDocument<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HostDocument")
            .field("id", &self.id)
            .field("sections", &self.sections)
            .finish_non_exhaustive()
    }
}

impl Document for HostDocument<'_> {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn sections(&self) -> u32 {
        self.sections
    }

    fn whole_pages(&self) -> bool {
        self.whole_pages
    }

    /// Not read yet: progress needs the sections' lengths (op 5).
    fn section_len(&self, _section: u32) -> u32 {
        0
    }

    fn block_at(
        &self,
        _storage: &dyn Storage,
        at: Locator,
    ) -> Result<Option<Block>, DocumentError> {
        let (filters, path) = self.filters;
        filtered_block(|at| block_result(self.read_at(at)), (filters, path), at)
    }
}

/// The block at `at` that `read` gives, through `filters` (the book at
/// `path`): a block they leave out gives the next one kept, none at the end
/// of the section, as Beta 3's filtered book did. A rule that cannot be
/// applied leaves the block as the book has it.
///
/// # Errors
/// What `read` refuses; a block that does not advance; too many left out.
pub fn filtered_block(
    mut read: impl FnMut(Locator) -> Result<Option<Block>, DocumentError>,
    (filters, path): (&[Filter], &str),
    at: Locator,
) -> Result<Option<Block>, DocumentError> {
    let mut at = at;
    for _ in 0..MOST_DROPPED {
        let Some(mut block) = read(at)? else {
            return Ok(None);
        };
        if filters.is_empty() {
            return Ok(Some(block));
        }
        if block.end <= at.offset {
            return Err(DocumentError::Malformed(Cow::Borrowed(
                "a block that does not advance",
            )));
        }
        match native_filter::apply(filters, path, &mut block) {
            Ok(Shown::Drop) => at = Locator::new(at.section, block.end),
            Ok(Shown::Keep) | Err(_) => return Ok(Some(block)),
        }
    }
    Err(DocumentError::Malformed(Cow::Borrowed(
        "the text filters left out too many blocks in a row",
    )))
}

impl HostDocument<'_> {
    /// The block at `at`, with the host's refusal as it is (the search scan
    /// tells the end of a section from a refusal by it).
    ///
    /// # Errors
    /// What the host refuses; `State` while another read holds the buffers.
    pub fn read_at(&self, at: Locator) -> Result<Block, Error> {
        let place = DocPosition::new(at.section, at.offset);
        let mut buffers = self.buffers.try_borrow_mut().map_err(|_| Error::State)?;
        let (input, out) = &mut *buffers;
        read_block(|cursor, assembler| {
            let fragment = self
                .documents
                .block_at(self.host, self.id, place, cursor, input, out)?;
            let more = fragment.continuation();
            match assembler.push(pieces(&fragment), more.is_none())? {
                Some(block) => Ok(Next::Done(block)),
                None => more.map(Next::More).ok_or(Error::Invalid),
            }
        })
    }
}

#[cfg(test)]
mod tests;

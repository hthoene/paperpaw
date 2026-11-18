//! Tables and block quotes.
//!
//! A `<tr>` is one record of kind [`BlockKind::TableRow`]: its cells' texts
//! one after the other, the second and later after [`SEPARATOR`] (so the
//! row reads as its text where it is shown as a paragraph), and after its
//! runs and languages the table's data: the number of its cells and of the
//! table's columns, its flags (header, first, last row), [`MAX_COLUMNS`]
//! column weights as u16 and, for each cell, where its text ends (u16), the
//! columns it spans and its alignment (u8 each). Block elements in a cell
//! are lines of it, tables in a cell its text; a row too long for one
//! record is read as paragraphs.
//!
//! The weights are the same in all rows of a table: from what its cells
//! hold (the most characters of a cell, at most [`CAP`], and never fewer
//! than its longest word), as far as its rows are still in the buffer when
//! it is written out or the table ends; rows written so far get them then,
//! later rows as they are.
//!
//! The paragraphs of a `<blockquote>` are of kind [`QUOTE`], those of a
//! callout (a quote of class `callout`) of kind [`ASIDE`].

use alloc::boxed::Box;
use alloc::vec::Vec;

use paperpaw_sdk::document::{Align, BlockKind, DocumentError, MAX_COLUMNS, TableCell, TableRow};
use paperpaw_sdk::xml::Attrs;

use super::Converter;
use crate::fallible;

/// The most characters a column's weight counts of a cell's text: longer
/// text is broken in its column rather than taking the table.
const CAP: u16 = 30;
/// Bytes of a row's table data before its cells, and of each cell.
const HEAD: usize = 4 + 2 * MAX_COLUMNS;
const CELL: usize = 4;
/// The most bytes of a row's table data.
pub(crate) const MAX_DATA: usize = HEAD + CELL * MAX_COLUMNS;
/// The flags of a row.
const HEADER_ROW: u8 = 1;
const FIRST_ROW: u8 = 2;
const LAST_ROW: u8 = 4;
/// What sets a row's cells apart in its text.
const SEPARATOR: &str = " \u{b7} ";
/// The kind numbers of an aside (a callout) and of a block quote's
/// paragraph (see `BlockKind::Extended`).
const ASIDE: u16 = 5;
const QUOTE: u16 = 8;

/// What the paragraphs in an element are: of a block quote, of a callout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Quoted {
    #[default]
    None,
    Quote,
    Callout,
}

/// The table being read.
pub(super) struct Table {
    /// The depth of its `<table>` in the stack.
    depth: usize,
    /// No row of it is written yet.
    first: bool,
    /// The columns of its rows so far.
    columns: u8,
    /// For each column, the most characters of a cell's text and of a word
    /// in it.
    sizes: [(u16, u16); MAX_COLUMNS],
    /// Where the data of the rows still without weights is in `out`.
    held: Vec<usize>,
    /// Its weights, once known.
    weights: Option<[u16; MAX_COLUMNS]>,
    /// Where the last row's data is in `out`, while it is there.
    last: Option<usize>,
}

/// The row being read.
pub(super) struct Row {
    /// The depth of its `<tr>` in the stack.
    depth: usize,
    cells: [TableCell; MAX_COLUMNS],
    count: usize,
    /// In a `<thead>`, or of `<th>` cells only so far.
    head: bool,
    any_td: bool,
    /// Tables open in its cells.
    nested: u32,
    /// Where the text of its last cell starts.
    cell_start: usize,
    /// Its text was too long for one record: read as paragraphs.
    broken: bool,
}

impl Quoted {
    /// What the paragraphs of element `tag` are, in one whose are `parent`.
    pub(super) fn of(
        tag: &str,
        attrs: &Attrs<'_>,
        parent: Quoted,
    ) -> Result<Quoted, DocumentError> {
        if tag != "blockquote" {
            return Ok(parent);
        }
        let class = attrs.try_get("class").map_err(|_| fallible::OOM)?;
        let callout = class.is_some_and(|c| {
            c.split_whitespace()
                .any(|w| w.eq_ignore_ascii_case("callout"))
        });
        Ok(if callout {
            Quoted::Callout
        } else {
            Quoted::Quote
        })
    }
}

impl Converter<'_> {
    /// Whether the content read now is in a table's row.
    pub(super) fn in_row(&self) -> bool {
        self.row.is_some()
    }

    /// Whether closing the elements from depth `open` on leaves a row's
    /// cell, not the row.
    pub(super) fn in_cell(&self, open: usize) -> bool {
        self.row.as_ref().is_some_and(|r| r.depth < open)
    }

    /// A block element `tag` begins in a row's cell: a line of it.
    pub(super) fn line_in_cell(&mut self) -> Result<(), DocumentError> {
        let start = self.row.as_ref().map_or(usize::MAX, |r| r.cell_start);
        if self.text.len() > start && !self.text.ends_with('\n') {
            self.push('\n')?;
        }
        self.space = false;
        Ok(())
    }

    /// Element `tag` of a table entered (its frame on the stack): a table,
    /// a row or a cell begins. Whether it was one of them.
    pub(super) fn enter_table(
        &mut self,
        tag: &str,
        attrs: &Attrs<'_>,
    ) -> Result<bool, DocumentError> {
        let depth = self.stack.len().saturating_sub(1);
        match (tag, self.row.as_mut()) {
            ("table", None) => {
                self.end_table();
                self.table = Some(fallible::try_box(Table::new(depth))?);
            }
            ("table", Some(row)) => row.nested += 1,
            ("tr", None) => {
                if self.table.is_none() {
                    self.table = Some(fallible::try_box(Table::new(usize::MAX))?);
                }
                let head = self.stack.iter().any(|f| f.tag == "thead");
                self.row = Some(fallible::try_box(Row::new(depth, head))?);
            }
            ("td" | "th", Some(row)) if row.nested == 0 => {
                let span = attrs
                    .try_get("colspan")
                    .map_err(|_| fallible::OOM)?
                    .and_then(|s| s.trim().parse::<u8>().ok())
                    .unwrap_or(1)
                    .clamp(1, MAX_COLUMNS as u8);
                let align = self.stack.last().map_or(Align::Auto, |f| f.align);
                row.any_td |= tag == "td";
                let next = row.count < MAX_COLUMNS;
                if next && row.count > 0 {
                    row.cells[row.count - 1].end = self.text.len() as u32;
                }
                if next {
                    row.cells[row.count] = TableCell::new(0).span(span).align(align);
                    row.count += 1;
                }
                self.set_cell_apart()?;
            }
            ("td" | "th", _) => self.set_cell_apart()?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// A cell's text set apart from the one before.
    fn set_cell_apart(&mut self) -> Result<(), DocumentError> {
        if !self.text.is_empty() {
            self.push_str(SEPARATOR)?;
        }
        self.space = false;
        self.space_origin = None;
        if let Some(row) = self.row.as_mut().filter(|r| r.nested == 0) {
            row.cell_start = self.text.len();
        }
        Ok(())
    }

    /// The elements from depth `open` on were closed (`tag` the outermost):
    /// the row and the table they end.
    pub(super) fn left_table(&mut self, tag: &str, open: usize) {
        if let Some(row) = self.row.as_mut() {
            if row.depth >= open {
                self.row = None;
            } else if tag == "table" {
                row.nested = row.nested.saturating_sub(1);
            }
        }
        if self.row.is_none() && self.table.as_ref().is_some_and(|t| t.depth >= open) {
            self.end_table();
        }
    }

    /// The table ends: its rows still held get their weights, its last one
    /// its flag.
    fn end_table(&mut self) {
        self.weigh();
        if let Some(at) = self.table.take().and_then(|t| t.last)
            && let Some(flags) = self.out.get_mut(at + 2)
        {
            *flags |= LAST_ROW;
        }
    }

    /// The kind of the block being written, of kind `kind` as collected: a
    /// row's while a row's cells are in it, a quote's or an aside's where
    /// a paragraph is in one.
    fn written_kind(&self, kind: BlockKind, piece: bool) -> BlockKind {
        let row = self.row.as_ref().is_some_and(|r| r.count > 0 && !r.broken);
        match (kind, self.quote) {
            (BlockKind::Image, _) => BlockKind::Image,
            _ if row && !piece => BlockKind::TableRow,
            (BlockKind::Paragraph, Quoted::Quote) => BlockKind::Extended(QUOTE),
            (BlockKind::Paragraph, Quoted::Callout) => BlockKind::Extended(ASIDE),
            (BlockKind::TableRow, _) => BlockKind::Paragraph,
            (kind, _) => kind,
        }
    }

    /// What a block of kind `kind` (as collected) is written as, its text
    /// `len` bytes long or a `piece` of a longer one, and the bytes of its
    /// table data.
    pub(super) fn written(
        &mut self,
        kind: BlockKind,
        len: usize,
        piece: bool,
    ) -> (BlockKind, usize) {
        let kind = self.written_kind(kind, piece);
        match kind {
            BlockKind::TableRow => (kind, self.row_data_len(len)),
            _ => (kind, 0),
        }
    }

    /// The row's text is cut into pieces: from now on paragraphs.
    pub(super) fn break_row(&mut self) {
        if let Some(row) = self.row.as_mut() {
            row.broken = true;
        }
    }

    /// The bytes of the table data of a row written now, its text `len`
    /// bytes long; its cells noted in the table's sizes.
    fn row_data_len(&mut self, len: usize) -> usize {
        let (Some(row), Some(table)) = (self.row.as_mut(), self.table.as_mut()) else {
            return 0;
        };
        if row.count == 0 || row.broken {
            return 0;
        }
        row.cells[row.count - 1].end = len as u32;
        if table.weights.is_none() {
            table.note(&self.text[..len], &row.cells[..row.count]);
        }
        HEAD + CELL * row.count
    }

    /// The table data of the row being written, `len` bytes (none for a
    /// block that is no row), after its runs and languages: its weights as
    /// they are known, else left to fill.
    pub(super) fn write_row_data(&mut self, len: usize) {
        let (Some(row), Some(table)) = (self.row.as_mut(), self.table.as_mut()) else {
            return;
        };
        if len == 0 {
            return;
        }
        self.length += len as u32;
        let at = self.out.len();
        let mut flags = 0;
        if row.head || !row.any_td {
            flags |= HEADER_ROW;
        }
        if table.first {
            flags |= FIRST_ROW;
        }
        self.out
            .extend_from_slice(&[row.count as u8, table.columns, flags, 0]);
        let weights = table.weights.unwrap_or_default();
        for weight in weights {
            self.out.extend_from_slice(&weight.to_le_bytes());
        }
        for cell in &row.cells[..row.count] {
            self.out
                .extend_from_slice(&(cell.end.min(u32::from(u16::MAX)) as u16).to_le_bytes());
            self.out.extend_from_slice(&[cell.span, cell.align.bits()]);
        }
        table.first = false;
        table.last = Some(at);
        // The rest of the row is read as cells of their own.
        row.count = 0;
        row.cell_start = 0;
        // Rows held are bounded by the buffer they are in.
        if table.weights.is_none() && fallible::push(&mut table.held, at).is_err() {
            self.weigh();
        }
    }

    /// The table's weights are settled (its rows are written out, or it
    /// ends): those held get them.
    pub(super) fn weigh(&mut self) {
        let Some(table) = self.table.as_mut() else {
            return;
        };
        let weights = *table.weights.get_or_insert_with(|| {
            let mut weights = [0u16; MAX_COLUMNS];
            for (weight, (most, word)) in weights.iter_mut().zip(table.sizes) {
                *weight = most.min(CAP).max(word).max(1);
            }
            weights
        });
        for at in table.held.drain(..) {
            if let Some(data) = self.out.get_mut(at..at + HEAD) {
                data[1] = table.columns;
                for (bytes, weight) in data[4..].as_chunks_mut::<2>().0.iter_mut().zip(weights) {
                    *bytes = weight.to_le_bytes();
                }
            }
        }
    }

    /// `out` is written out: its rows' places in it are gone.
    pub(super) fn rows_written_out(&mut self) {
        if let Some(table) = self.table.as_mut() {
            table.last = None;
        }
    }
}

impl Table {
    fn new(depth: usize) -> Table {
        Table {
            depth,
            first: true,
            columns: 0,
            sizes: [(0, 0); MAX_COLUMNS],
            held: Vec::new(),
            weights: None,
            last: None,
        }
    }

    /// The cells of a row of `text` noted: its columns and their sizes.
    fn note(&mut self, text: &str, cells: &[TableCell]) {
        let mut start = 0usize;
        let mut column = 0usize;
        for cell in cells {
            let end = (cell.end as usize).clamp(start, text.len());
            let own = text.get(start..end).unwrap_or_default();
            let own = own.strip_prefix(SEPARATOR).unwrap_or(own);
            let span = usize::from(cell.span.max(1));
            if span == 1
                && let Some(size) = self.sizes.get_mut(column)
            {
                let count = |t: &str| u16::try_from(t.chars().count()).unwrap_or(u16::MAX);
                let word = own.split_whitespace().map(count).max().unwrap_or(0);
                *size = (size.0.max(count(own.trim())), size.1.max(word));
            }
            column += span;
            start = end;
        }
        self.columns = self.columns.max(column.min(MAX_COLUMNS) as u8);
    }
}

impl Row {
    fn new(depth: usize, head: bool) -> Row {
        Row {
            depth,
            cells: [TableCell::new(0); MAX_COLUMNS],
            count: 0,
            head,
            any_td: false,
            nested: 0,
            cell_start: 0,
            broken: false,
        }
    }
}

/// The row a record's table `data` describes, its text `len` bytes long;
/// `None` where the data is not a row's (the record is read as a
/// paragraph).
pub(crate) fn read_row(data: &[u8], len: usize) -> Result<Option<Box<TableRow>>, DocumentError> {
    let [count, columns, flags, _, ..] = *data else {
        return Ok(None);
    };
    let (count, columns) = (usize::from(count), usize::from(columns));
    if count == 0
        || count > MAX_COLUMNS
        || columns > MAX_COLUMNS
        || data.len() != HEAD + CELL * count
    {
        return Ok(None);
    }
    let mut weights = Vec::new();
    fallible::reserve_exact(&mut weights, columns)?;
    weights.extend(
        data[4..4 + 2 * columns]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b)),
    );
    let mut cells = Vec::new();
    fallible::reserve_exact(&mut cells, count)?;
    let mut before = 0;
    for cell in data[HEAD..].as_chunks::<CELL>().0 {
        let end = usize::from(u16::from_le_bytes([cell[0], cell[1]]));
        if end < before || end > len {
            return Ok(None);
        }
        before = end;
        cells.push(
            TableCell::new(end as u32)
                .span(cell[2])
                .align(Align::from_bits(cell[3])),
        );
    }
    let mut row = TableRow::new(cells.into_boxed_slice());
    row.columns = weights.into_boxed_slice();
    row.header = flags & HEADER_ROW != 0;
    row.first = flags & FIRST_ROW != 0;
    row.last = flags & LAST_ROW != 0;
    Ok(Some(fallible::try_box(row)?))
}

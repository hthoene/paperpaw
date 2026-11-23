//! A table's row ([`BlockKind::TableRow`](paperpaw_document::BlockKind)) set
//! as a row of cells: the table's columns as wide as their weights say
//! (each at least [`MIN_COLUMN`]), each cell's text broken into lines in
//! its column, rules around and between the cells. A row is set whole on a
//! page; one whose columns cannot be that wide, or that is taller than a
//! page, is set as the paragraph of its text instead (its cells set apart
//! by [`SEPARATOR`]), as are the pieces of a row a page goes on with.
//! Everything is the row's own: laid out from any row, a table's rows come
//! out the same.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ops::Range;

use paperpaw_document::{Align, Block, TableRow, VerticalAlign};
use paperpaw_text::wrap_lines_kerned_opts;

use crate::breaking::{advance_at, kern_at};
use crate::decor::RULE;
use crate::levels::BlockLevels;
use crate::measure::Measure;
use crate::{
    BookStyles, Face, LayoutContext, Page, PageLine, PageRule, TextStyle, block_em, block_line,
    book_size, line_bidi, line_runs, word_sources,
};

/// Room between a cell's rules and its text, in ems (half of it above and
/// below the text).
const PADDING: f32 = 0.3;
/// The least width of a column's text, in ems: a row whose columns cannot
/// all be as wide is set as a paragraph.
const MIN_COLUMN: f32 = 2.0;
/// The most columns a row is set in.
const MAX_GRID: usize = 64;
/// What a format puts between cells where the row is read as a paragraph:
/// no text of the cell after it.
pub(crate) const SEPARATOR: &str = " \u{b7} ";

/// What became of a table's row on the page.
pub(crate) enum RowSet {
    /// Set in columns, below what came before.
    Set,
    /// Set in columns, it does not fit: the page ends before it.
    Next,
    /// It is set as a paragraph of its text.
    Paragraph,
}

/// Sets the row `block` on `page` from `y` down, in columns where it can be
/// (see the module), and moves `y` below it: the book's space and a rule
/// above it where it starts a table (it is the table's first row, or does not
/// come right after a row set in columns: `after_row`), the space only on a
/// page that has something.
pub(crate) fn set_row(
    cx: &LayoutContext<'_>,
    block: &Block,
    page: &mut Page,
    (y, height): (&mut f32, f32),
    (section, after_row): (u32, bool),
) -> RowSet {
    let style = cx.style;
    let starts = !after_row || block.table.as_ref().is_some_and(|table| table.first);
    let space = match page.is_past_end() || !starts {
        true => 0.0,
        false => block
            .space
            .filter(|_| style.book.has(BookStyles::SPACING))
            .map_or(style.paragraph_spacing, |s| {
                f32::from(s) / 100.0 * block_em(block, style)
            }),
    };
    let top = *y + space;
    let Some(row) = row(cx, block, section, top, starts) else {
        return RowSet::Paragraph;
    };
    match top + row.height <= height {
        true => {
            page.lines.extend(row.lines);
            page.rules.extend(row.rules);
            *y = top + row.height;
            RowSet::Set
        }
        // Taller than a page: as a paragraph, which goes on.
        false if page.is_past_end() => RowSet::Paragraph,
        false => RowSet::Next,
    }
}

/// A row set: its lines and rules, and how tall it is.
pub(crate) struct Row {
    pub(crate) lines: Vec<PageLine>,
    pub(crate) rules: Vec<PageRule>,
    pub(crate) height: f32,
}

/// A cell to set: its text and its lines, its columns, how it is aligned.
struct Cell {
    text: Range<usize>,
    lines: Vec<paperpaw_text::Line>,
    column: usize,
    span: usize,
    align: Align,
    vertical: VerticalAlign,
}

/// The row of `block` set from `top` down, with a rule above it where
/// `rule_above`; `None` where it is set as a paragraph (see the module).
pub(crate) fn row(
    cx: &LayoutContext<'_>,
    block: &Block,
    section: u32,
    top: f32,
    rule_above: bool,
) -> Option<Row> {
    let table = block.table.as_deref()?;
    let style = cx.style;
    let em = block_em(block, style);
    let (x0, total) = Measure::of(block, style, cx.width, 0.0).rest;
    let pad = (PADDING * em).max(1.0);
    let weights = grid(table)?;
    let n = weights.len();
    let room = total - (n + 1) as f32 * RULE - n as f32 * 2.0 * pad;
    let widths = widths(&weights, room, MIN_COLUMN * em)?;
    // Where column `j`'s left rule is, and the right one at `n`.
    let mut edges = Vec::with_capacity(n + 1);
    let mut x = x0;
    for j in 0..=n {
        edges.push(x);
        x += RULE + widths.get(j).map_or(0.0, |w| w + 2.0 * pad);
    }
    let levels = BlockLevels::in_context(block, cx, section);
    let base = Face::new(TextStyle::REGULAR, 0, book_size(block, style).unwrap_or(0));
    let (ascent, line_height) = block_line(block, style, cx.typesetter.line(base), em);
    let cells = cells(cx, block, table, (&edges, pad), (base, &levels))?;
    let most = cells
        .iter()
        .map(|c| c.lines.len())
        .max()
        .unwrap_or(0)
        .max(1);
    let content = most as f32 * line_height;
    let above = if rule_above { RULE } else { 0.0 };
    let text_top = top + above + pad / 2.0;
    let height = above + pad + content + RULE;
    let rtl = levels.paragraph.is_rtl();
    // Mirrored within the table in a right-to-left one.
    let place = |x: f32, w: f32| if rtl { 2.0 * x0 + total - x - w } else { x };
    let mut lines = Vec::new();
    for cell in &cells {
        let inner = edges[cell.column + cell.span] - edges[cell.column] - RULE - 2.0 * pad;
        let left = edges[cell.column] + RULE + pad;
        let free_height = content - cell.lines.len() as f32 * line_height;
        let down = match cell.vertical {
            VerticalAlign::Middle => free_height / 2.0,
            VerticalAlign::Bottom => free_height,
            _ => 0.0,
        };
        let align = cell_align(cell.align, table.header, rtl);
        for (k, line) in cell.lines.iter().enumerate() {
            let range = cell.text.start + line.range.start..cell.text.start + line.range.end;
            let mut text = String::from(block.text.get(range.clone())?);
            if line.hyphen {
                text.push('-');
            }
            let free = (inner - line.width).max(0.0);
            let x = left
                + match align {
                    Align::Center => free / 2.0,
                    Align::End => free,
                    _ => 0.0,
                };
            let runs = line_runs(
                block,
                base,
                range.clone(),
                usize::from(line.hyphen),
                style.book,
                &levels,
            );
            let bidi = line_bidi(&levels, &runs, k == 0);
            lines.push(PageLine {
                x: place(x, line.width),
                baseline: text_top + down + k as f32 * line_height + ascent,
                text,
                word_spacing: 0.0,
                runs,
                section,
                words: word_sources(block, range),
                leading: 0.0,
                inserted: false,
                annotations: Vec::new(),
                bidi,
                hyphen: line.hyphen,
            });
        }
    }
    let rules = rules(&cells, &edges, (top, height, rule_above), &place);
    Some(Row {
        lines,
        rules,
        height,
    })
}

/// The rules of a row of `cells` in columns of `edges`, `height` high from
/// `top`, with one above it where `above`: placed by `place`.
fn rules(
    cells: &[Cell],
    edges: &[f32],
    (top, height, above): (f32, f32, bool),
    place: &dyn Fn(f32, f32) -> f32,
) -> Vec<PageRule> {
    let (x0, n) = (edges[0], edges.len() - 1);
    let mut rules = Vec::new();
    let across = edges[n] + RULE - x0;
    if above {
        rules.push(PageRule::new(place(x0, across), top, across, RULE));
    }
    rules.push(PageRule::new(
        place(x0, across),
        top + height - RULE,
        across,
        RULE,
    ));
    for (j, edge) in edges.iter().enumerate() {
        // Not inside a cell that spans columns.
        if !cells.iter().any(|c| c.column < j && j < c.column + c.span) {
            rules.push(PageRule::new(place(*edge, RULE), top, RULE, height));
        }
    }
    rules
}

/// The cells of `table` in `block`, each broken into lines in its columns
/// (`edges` and `pad` as [`row`] has them); `None` where a cell's text is
/// not where the row says.
fn cells(
    cx: &LayoutContext<'_>,
    block: &Block,
    table: &TableRow,
    (edges, pad): (&[f32], f32),
    (base, levels): (Face, &BlockLevels),
) -> Option<Vec<Cell>> {
    let n = edges.len() - 1;
    let mut cells = Vec::with_capacity(table.cells.len());
    let mut column = 0;
    for (cell, range) in table.cells.iter().zip(table.ranges(block.text.len())) {
        let span = usize::from(cell.span.max(1)).min(n.saturating_sub(column));
        if span == 0 {
            break;
        }
        let text = cell_text(&block.text, range)?;
        let inner = edges[column + span] - edges[column] - RULE - 2.0 * pad;
        let start = text.start;
        let lines = wrap_lines_kerned_opts(
            block.text.get(text.clone())?,
            |_| inner,
            |i, c| advance_at(block, base, cx, levels, start + i, c),
            |i, left, right| kern_at(block, base, cx, levels, start + i, left, right),
            cx.hyphenate,
            cx.style.script_options.bits(),
        );
        cells.push(Cell {
            text,
            lines: lines.into_iter().filter(|l| !l.range.is_empty()).collect(),
            column,
            span,
            align: cell.align,
            vertical: cell.vertical,
        });
        column += span;
    }
    Some(cells)
}

/// A cell's text in `range` of the row's: without the separator before it
/// and the spaces and line breaks around it. `None` where the range is not
/// on characters of the text.
fn cell_text(text: &str, mut range: Range<usize>) -> Option<Range<usize>> {
    if text.get(range.clone())?.starts_with(SEPARATOR) {
        range.start += SEPARATOR.len();
    }
    let own = text.get(range.clone())?;
    let blank = [' ', '\n'];
    let start = range.start + (own.len() - own.trim_start_matches(blank).len());
    let end = range.end - (own.len() - own.trim_end_matches(blank).len());
    Some(start..end.max(start))
}

/// The weight of each column of the grid a row is set in: the table's
/// columns where they hold for the row's cells, else as many alike as its
/// cells span. `None` for a row without cells or of too many columns.
fn grid(table: &TableRow) -> Option<Vec<f32>> {
    let spans: usize = table.cells.iter().map(|c| usize::from(c.span.max(1))).sum();
    // Told before the weights are made, so that they are never longer.
    if spans == 0 || spans > MAX_GRID || table.columns.len() > MAX_GRID {
        return None;
    }
    let weights = match spans <= table.columns.len() {
        true => table
            .columns
            .iter()
            .map(|w| f32::from((*w).max(1)))
            .collect(),
        false => vec![1.0; spans],
    };
    Some(weights)
}

/// The widths of columns of `weights` sharing `room`, each at least
/// `least`: those that would be narrower are that wide, the others share
/// what is left. `None` where not all can be.
fn widths(weights: &[f32], room: f32, least: f32) -> Option<Vec<f32>> {
    if room < least * weights.len() as f32 {
        return None;
    }
    let mut widths = vec![0.0f32; weights.len()];
    let mut fixed = vec![false; weights.len()];
    // Each round fixes a column or ends.
    for _ in 0..=weights.len() {
        let left = room
            - widths
                .iter()
                .zip(&fixed)
                .filter(|(_, f)| **f)
                .map(|(w, _)| w)
                .sum::<f32>();
        let shared: f32 = weights
            .iter()
            .zip(&fixed)
            .filter(|(_, f)| !**f)
            .map(|(w, _)| w)
            .sum();
        let mut narrow = false;
        for (j, weight) in weights.iter().enumerate() {
            if fixed[j] {
                continue;
            }
            widths[j] = left * weight / shared.max(1.0);
            if widths[j] < least {
                (widths[j], fixed[j], narrow) = (least, true, true);
            }
        }
        if !narrow {
            break;
        }
    }
    Some(widths)
}

/// How a cell's lines are set in a table set left to right (one right to
/// left is mirrored after): the book's alignment, a header's centred.
fn cell_align(align: Align, header: bool, rtl: bool) -> Align {
    match (align, rtl) {
        (Align::Center, _) => Align::Center,
        (Align::Left, false) | (Align::Right, true) | (Align::Start, _) => Align::Start,
        (Align::Right, false) | (Align::Left, true) | (Align::End, _) => Align::End,
        _ if header => Align::Center,
        _ => Align::Start,
    }
}

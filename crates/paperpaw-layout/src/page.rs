//! Laying out one page, block after block ([`crate::layout_page`]).

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_document::{Align, Block, BlockKind, DocumentError, ImageRef, Locator};
use paperpaw_text::Line;

use crate::breaking::{SetLines, block_lines};
use crate::drop_cap::{self, Cap};
use crate::levels::BlockLevels;
use crate::{
    BookStyles, Face, HEADING_SPACE, LayoutContext, MAX_JUSTIFY_GAP, MIN_LINE_HEIGHT, Page,
    PageImage, PageLine, ScriptOptions, annotate, block_align, block_em, block_face, block_leading,
    block_line, decor, ended, first_indent, hanging, inserted, line_bidi, line_runs, malformed,
    measure, placed_x, table, word_sources,
};

/// [`crate::layout_page`], noting in `marks` the lines that have rules
/// beside them.
pub(crate) fn lay_page(
    cx: &LayoutContext<'_>,
    start: Locator,
    height: f32,
    marks: &mut Vec<decor::Mark>,
) -> Result<Page, DocumentError> {
    let mut run = Run {
        cx,
        height,
        marks,
        page: Page {
            start,
            end: start,
            lines: Vec::new(),
            images: Vec::new(),
            notes: Vec::new(),
            rules: Vec::new(),
        },
        at: start,
        y: 0.0,
        body_line: cx.typesetter.line(Face::default()).1.max(MIN_LINE_HEIGHT),
        heading_before: None,
        in_table: false,
        carried: Vec::new(),
    };
    // Room for as many lines as body text fills the page with, so that the
    // list does not double past them; what is left is given back.
    let lines = (height / run.body_line) as usize + 1;
    let _ = run
        .page
        .lines
        .try_reserve_exact(lines.min(MAX_RESERVED_LINES));
    let mut page = lay_blocks(run)?;
    page.lines.shrink_to_fit();
    Ok(page)
}

/// The most lines a page reserves room for before it is laid out.
const MAX_RESERVED_LINES: usize = 128;

/// The blocks of `run` from its start, until the page is full.
fn lay_blocks(mut run: Run<'_, '_>) -> Result<Page, DocumentError> {
    let cx = run.cx;
    loop {
        let Some(block) = cx.document.block_at(cx.storage, run.at)? else {
            if run.next_section() {
                continue;
            }
            break;
        };
        match run.block(&block)? {
            Flow::Go => run.at = Locator::new(run.at.section, block.end),
            Flow::End(end) => return ended(run.page, end),
            Flow::Done => return Ok(run.page),
        }
    }
    run.page.end = run.at;
    Ok(run.page)
}

/// What comes after a block.
enum Flow {
    /// The next block, on this page.
    Go,
    /// The page ends here.
    End(Locator),
    /// The page is finished, its end set.
    Done,
}

/// A page being laid out.
struct Run<'c, 'm> {
    cx: &'c LayoutContext<'c>,
    height: f32,
    marks: &'m mut Vec<decor::Mark>,
    page: Page,
    /// Where the next block starts.
    at: Locator,
    /// The height of what is on the page so far.
    y: f32,
    body_line: f32,
    /// The heading laid out last, while nothing else followed it on the
    /// page: the lines before it, where it starts, and whether anything
    /// came before it. A paragraph that goes on whole takes it along.
    heading_before: Option<(usize, Locator, bool)>,
    /// Whether the block laid out last was a table's row set in columns.
    in_table: bool,
    /// The rests of annotations that go on under the next line.
    carried: Vec<annotate::Pending>,
}

/// A block of text being set, with what all of its lines share.
struct Paragraph<'b> {
    block: &'b Block,
    /// The byte of its text the page starts at.
    from: usize,
    base: Face,
    heading: u8,
    ascent: f32,
    line_height: f32,
    /// The line height without the room a plugin asked for.
    letters: f32,
    leading: f32,
    /// The faces and rooms of its annotations, where it has any.
    annotated: Option<annotate::Rooms>,
    /// The page's lines before this block.
    lines_before: usize,
    had_content: bool,
    after_heading: Option<(usize, Locator, bool)>,
    levels: BlockLevels,
    sides: Option<(decor::Kind, f32, f32)>,
    align: Align,
    indent: f32,
    lines: SetLines,
    cap: Option<Cap>,
}

impl Run<'_, '_> {
    /// At the end of a section: whether the next one starts this page.
    /// Chapters start on a new page.
    fn next_section(&mut self) -> bool {
        let next = self
            .at
            .section
            .checked_add(1)
            .filter(|&n| n < self.cx.document.sections());
        if let Some(next) = next {
            self.at = Locator::new(next, 0);
            if self.page.is_past_end() {
                self.page.start = self.at;
                return true;
            }
        }
        false
    }

    fn block(&mut self, block: &Block) -> Result<Flow, DocumentError> {
        let at = self.at;
        // Every block takes the layout further, or it would never end.
        if block.end <= at.offset {
            return Err(malformed());
        }
        let from = if at.offset > block.start {
            block.text_index(at.offset)
        } else {
            0
        };
        if self.page.is_past_end() && from == 0 && at.offset < block.start {
            self.page.start = Locator::new(at.section, block.start);
        }
        // What follows a heading, if this block does.
        let after_heading = self.heading_before.take();
        let after_row = core::mem::replace(&mut self.in_table, false);
        if block.kind == BlockKind::TableRow && from == 0 {
            match table::set_row(
                self.cx,
                block,
                &mut self.page,
                (&mut self.y, self.height),
                (at.section, after_row),
            ) {
                table::RowSet::Set => {
                    self.in_table = true;
                    return Ok(Flow::Go);
                }
                table::RowSet::Next => {
                    return Ok(match after_heading {
                        // The heading just before goes along.
                        Some((kept, start, true)) => {
                            self.page.lines.truncate(kept);
                            Flow::End(start)
                        }
                        _ => Flow::End(Locator::new(at.section, block.start)),
                    });
                }
                table::RowSet::Paragraph => {}
            }
        }
        if block.kind == BlockKind::Image
            && let Some(image) = &block.image
        {
            return Ok(self.image(block, image));
        }
        let paragraph = self.paragraph(block, from, after_heading)?;
        for n in 0..paragraph.lines.len() {
            if let Some(end) = self.line(&paragraph, n) {
                return Ok(Flow::End(end));
            }
        }
        Ok(self.after_lines(&paragraph))
    }

    fn image(&mut self, block: &Block, image: &ImageRef) -> Flow {
        let (at, height) = (self.at, self.height);
        // A picture without size has nothing to show, and no scale.
        if image.width == 0 || image.height == 0 {
            return Flow::Go;
        }
        // Scaled to fit the content box, keeping its proportions.
        let width = self.cx.width;
        let scale = (width / image.width as f32).min(height / image.height as f32);
        let (w, h) = (image.width as f32 * scale, image.height as f32 * scale);
        let spacing = if self.page.is_past_end() {
            0.0
        } else {
            self.cx.style.paragraph_spacing.max(self.body_line * 0.5)
        };
        if !self.page.is_past_end() && self.y + spacing + h > height {
            return Flow::End(Locator::new(at.section, block.start));
        }
        if self.page.is_past_end() {
            self.page.start = Locator::new(at.section, block.start);
        }
        self.y += spacing;
        self.page.images.push(PageImage {
            x: (width - w) / 2.0,
            y: self.y,
            width: w,
            height: h,
            image: image.clone(),
        });
        self.y += h + spacing;
        Flow::Go
    }

    /// Sets up a block of text from byte `from` of its text: the space
    /// above it, its face, measure and lines.
    fn paragraph<'b>(
        &mut self,
        block: &'b Block,
        from: usize,
        after_heading: Option<(usize, Locator, bool)>,
    ) -> Result<Paragraph<'b>, DocumentError> {
        let cx = self.cx;
        let (style, at) = (cx.style, self.at);
        let (heading, base) = block_face(block, style);
        let em = block_em(block, style);
        let (ascent, line_height) = block_line(block, style, cx.typesetter.line(base), em);
        // Room a plugin asked for above and below every line, and what its
        // annotations take. It is part of each line's height, so the page
        // breaks with it.
        let annotated = annotate::rooms(cx, block, em);
        self.carried.clear();
        let (above, below) = annotated.as_ref().map_or((0.0, 0.0), annotate::Rooms::room);
        let leading = block_leading(block, em).max(above);
        let had_content = !self.page.is_past_end();
        let at_block_start = from == 0 && !block.continues;
        // The book's space where it sets one. At the top of a page only
        // where a chapter starts, for example a title set lower.
        let book_space = block
            .space
            .filter(|_| style.book.has(BookStyles::SPACING))
            .map(|s| f32::from(s) / 100.0 * em);
        if !had_content && at_block_start && book_space.is_some() {
            let first = cx
                .document
                .block_at(cx.storage, Locator::new(at.section, 0))?;
            if first.is_some_and(|f| f.start == block.start) {
                self.y += book_space.unwrap_or(0.0).min(self.height / 3.0);
            }
        }
        if had_content && at_block_start {
            self.y += book_space.unwrap_or(style.paragraph_spacing);
            if heading > 0 {
                self.y += self.body_line * HEADING_SPACE;
            }
        }
        let levels = BlockLevels::in_context(block, cx, at.section);
        let sides = decor::sides(
            block,
            measure::Measure::of(block, style, cx.width, 0.0).rest,
            (cx.width, em, levels.paragraph.is_rtl()),
        );
        let align = block_align(block, style, heading, &levels);
        // Balanced, the whole block is broken from its start, its first
        // line indented as there.
        let indent = if at_block_start || style.balanced {
            first_indent(block, style, align)
        } else {
            0.0
        };
        let justify_limit = (align == Align::Justify).then_some(line_height * MAX_JUSTIFY_GAP);
        let (lines, cap) = block_lines(
            block,
            from,
            base,
            cx,
            heading,
            indent,
            justify_limit,
            &levels,
        );
        Ok(Paragraph {
            block,
            from,
            base,
            heading,
            ascent: ascent + leading,
            line_height: line_height + leading + below,
            letters: line_height,
            leading,
            annotated,
            lines_before: self.page.lines.len(),
            had_content,
            after_heading,
            levels,
            sides,
            align,
            indent,
            lines,
            cap,
        })
    }

    /// Sets line `n` of `p`. The page's end, where it does not fit.
    fn line(&mut self, p: &Paragraph<'_>, n: usize) -> Option<Locator> {
        let (block, cx, style) = (p.block, self.cx, self.cx.style);
        let (line, (first_x, available)) = &p.lines[n];
        let (first_x, available) = (*first_x, *available);
        let fits = self.y + p.line_height <= self.height;
        if !fits && !self.page.is_past_end() {
            return Some(self.break_in(p, n));
        }
        // Punctuation hung out of the measure, where asked for.
        let (hang_start, hang_end) =
            match style.script_options.has(ScriptOptions::HANGING_PUNCTUATION) {
                true => {
                    let beside = p.cap.is_some_and(|cap| p.from == 0 && n < cap.lines);
                    let indented =
                        beside || (line.range.start == 0 && !block.continues && p.indent > 0.0);
                    hanging::hanging(cx, block, p.base, line, &p.levels, p.align, indented)
                }
                false => (0.0, 0.0),
            };
        let (first_x, available) = (first_x - hang_start, available + hang_start + hang_end);
        let free = (available - line.width).max(0.0);
        let (x, word_spacing) = match p.align {
            Align::Center => (first_x + free / 2.0, 0.0),
            Align::End => (first_x + free, 0.0),
            Align::Justify if !line.last && line.spaces > 0 => {
                let extra = free / line.spaces as f32;
                // Bound wide gaps continuously. A line that cannot
                // reach the edge keeps a ragged end instead of
                // suddenly losing all of its justification.
                (first_x, extra.min(p.letters * MAX_JUSTIFY_GAP))
            }
            _ => (first_x, 0.0),
        };
        let drawn = line.width + word_spacing * line.spaces as f32;
        let x = placed_x(&p.levels, cx.width, x, drawn);
        let range = self.drop_cap(p, line, first_x);
        self.push_line(p, line, range, (x, word_spacing));
        None
    }

    /// Where the page ends when line `n` of `p` does not fit on it.
    fn break_in(&mut self, p: &Paragraph<'_>, n: usize) -> Locator {
        let (block, style, section) = (p.block, self.cx.style, self.at.section);
        // Widows and orphans: as many lines go on as the next page needs.
        // Where too few would stay here, the paragraph goes whole.
        let left = p.lines.len() - n;
        let back = usize::from(style.widows).saturating_sub(left).min(n);
        let stay = n - back;
        // A drop capital's lines go together.
        let spanned = p.cap.filter(|_| p.from == 0).map_or(0, |cap| cap.lines);
        let orphans = usize::from(style.orphans.max(1)).max(spanned);
        let alone = stay < orphans;
        let line_start =
            |i: usize| Locator::new(section, block.source_offset(p.lines[i].0.range.start));
        if alone && p.had_content {
            match p.after_heading {
                // The heading just before goes along.
                Some((kept, start, true)) => {
                    self.page.lines.truncate(kept);
                    return start;
                }
                // The paragraph goes whole.
                None => {
                    self.page.lines.truncate(p.lines_before);
                    return line_start(0);
                }
                // Under a heading that starts the page nothing can go. It
                // stays as it falls.
                Some(_) => {}
            }
        } else if back > 0 && !alone {
            self.page.lines.truncate(self.page.lines.len() - back);
            return line_start(stay);
        }
        // Starting the page, it cannot go whole. Then it stays as it falls
        // rather than leaving an orphan here.
        line_start(n)
    }

    /// Places the drop capital beside `line` where it starts the block.
    /// Its baseline is that of the lines it spans. The part of `line`
    /// without it.
    fn drop_cap(
        &mut self,
        p: &Paragraph<'_>,
        line: &Line,
        first_x: f32,
    ) -> core::ops::Range<usize> {
        match p.cap.filter(|_| drop_cap::BUILT && line.range.start == 0) {
            Some(cap) => {
                // On a page too short for the lines it spans, as large as
                // the lines it has there.
                let fit = ((self.height - self.y) / p.line_height).max(1.0) as usize;
                let spans = cap.lines.min(fit);
                let face = drop_cap::face(self.cx, p.block, p.base, spans as u8);
                let baseline = self.y + p.ascent + (spans - 1) as f32 * p.line_height;
                let place = (first_x - cap.room, baseline);
                let shown = drop_cap::Cap { face, ..cap };
                self.page
                    .lines
                    .push(drop_cap::line(p.block, shown, place, self.at.section));
                cap.len..line.range.end
            }
            None => line.range.clone(),
        }
    }

    /// Puts `range` of `line` on the page at `x`, and its rules beside it.
    fn push_line(
        &mut self,
        p: &Paragraph<'_>,
        line: &Line,
        range: core::ops::Range<usize>,
        (x, word_spacing): (f32, f32),
    ) {
        let block = p.block;
        let mut line_text = String::from(&block.text[range.clone()]);
        if line.hyphen {
            line_text.push('-');
        }
        let words = word_sources(block, range.clone());
        let runs = line_runs(
            block,
            p.base,
            range.clone(),
            usize::from(line.hyphen),
            self.cx.style.book,
            &p.levels,
        );
        let bidi = line_bidi(&p.levels, &runs, line.range.start == 0 && !block.continues);
        let annotations = annotate::place(
            self.cx,
            p.annotated.as_ref(),
            (block, range.clone(), &runs, bidi),
            (x, word_spacing),
            (
                self.y + p.leading,
                self.y + p.ascent + p.letters - (p.ascent - p.leading),
            ),
            (&mut self.carried, line.range.end >= block.text.len()),
        );
        self.page.lines.push(PageLine {
            x,
            baseline: self.y + p.ascent,
            text: line_text,
            word_spacing,
            runs,
            section: self.at.section,
            words,
            leading: p.leading,
            inserted: false,
            annotations,
            bidi,
            hyphen: line.hyphen,
        });
        if let Some((kind, left, right)) = p.sides {
            self.marks.push(decor::Mark {
                line: self.page.lines.len() - 1,
                kind,
                top: self.y,
                bottom: self.y + p.line_height,
                left,
                right,
            });
        }
        self.y += p.line_height;
    }

    /// What follows the lines of `p`: lines a filter inserted, and a
    /// heading kept with what comes after it.
    fn after_lines(&mut self, p: &Paragraph<'_>) -> Flow {
        let (block, cx, section, height) = (p.block, self.cx, self.at.section, self.height);
        // What a filter inserted after the block goes below its last line,
        // never apart from it.
        if !block.inserted.is_empty() {
            let (placed, below, whole) = inserted::lines(block, cx, section, self.y, height);
            if !whole {
                // Not all of it fits. The page ends before the block's last
                // line, which goes on the next page with it, if all of it
                // fits there. Where it would not fit on any page, or that
                // line starts the page, what fits stays here and the rest
                // goes: what is inserted has no place to go on from.
                let last = self.page.lines.len().saturating_sub(1);
                let (_, tall, _) = inserted::lines(block, cx, section, 0.0, f32::INFINITY);
                if self.page.lines.len() > p.lines_before
                    && last > 0
                    && p.line_height + tall <= height
                    && let Some((line, _)) = p.lines.last()
                {
                    self.page.lines.truncate(last);
                    return Flow::End(Locator::new(section, block.source_offset(line.range.start)));
                }
                self.page.lines.extend(placed);
                self.page.end = Locator::new(section, block.end);
                return Flow::Done;
            }
            self.page.lines.extend(placed);
            self.y = below;
        }
        // A heading needs at least one line of what follows on its page.
        let heading_start = Locator::new(section, block.source_offset(p.from));
        if p.heading > 0 && p.had_content && self.y + self.body_line > height {
            self.page.lines.truncate(p.lines_before);
            return Flow::End(heading_start);
        }
        if p.heading > 0 {
            self.heading_before = Some((p.lines_before, heading_start, p.had_content));
        }
        Flow::Go
    }
}

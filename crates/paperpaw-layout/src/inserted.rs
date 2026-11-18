//! The lines of what a text filter inserted after a block
//! ([`Block::inserted`]).

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_document::{Align, Block, BlockKind};

use crate::breaking::block_lines;
use crate::levels::BlockLevels;
use crate::{
    LayoutContext, PageLine, block_align, block_em, block_face, block_leading, block_line,
    first_indent, line_bidi, line_runs, placed_x,
};

/// The lines of the blocks inserted after `block` ([`Block::inserted`]),
/// set from `y` down on a page `height` high, as many as fit; where they
/// end, and whether all fit. They take the paragraph spacing before
/// each, their own face, size and alignment, and no places.
pub(crate) fn lines(
    block: &Block,
    cx: &LayoutContext<'_>,
    section: u32,
    mut y: f32,
    height: f32,
) -> (Vec<PageLine>, f32, bool) {
    let (style, typesetter) = (cx.style, cx.typesetter);
    let mut placed = Vec::new();
    for inserted in &block.inserted {
        if inserted.kind == BlockKind::Image || inserted.text.is_empty() {
            continue;
        }
        let (heading, base) = block_face(inserted, style);
        let em = block_em(inserted, style);
        let (ascent, line_height) = block_line(inserted, style, typesetter.line(base), em);
        let leading = block_leading(inserted, em);
        let (ascent, line_height) = (ascent + leading, line_height + leading);
        let levels = BlockLevels::of(inserted);
        let align = block_align(inserted, style, heading, &levels);
        y += style.paragraph_spacing;
        let indent = first_indent(inserted, style, align);
        for (line, (first_x, available)) in
            block_lines(inserted, 0, base, cx, heading, indent, None, &levels).0
        {
            if y + line_height > height {
                return (placed, y, false);
            }
            let free = (available - line.width).max(0.0);
            let x = match align {
                Align::Center => first_x + free / 2.0,
                Align::End => first_x + free,
                _ => first_x,
            };
            let x = placed_x(&levels, cx.width, x, line.width);
            let mut text = String::from(&inserted.text[line.range.clone()]);
            if line.hyphen {
                text.push('-');
            }
            let runs = line_runs(
                inserted,
                base,
                line.range.clone(),
                usize::from(line.hyphen),
                style.book,
                &levels,
            );
            let bidi = line_bidi(&levels, &runs, line.range.start == 0);
            placed.push(PageLine {
                x,
                baseline: y + ascent,
                text,
                word_spacing: 0.0,
                runs,
                section,
                words: Vec::new(),
                leading,
                inserted: true,
                annotations: Vec::new(),
                bidi,
                hyphen: false,
            });
            y += line_height;
        }
    }
    (placed, y, true)
}

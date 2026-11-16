//! Drop capitals ([`Block::drop_cap`]): a paragraph's first letter set
//! large over its first lines, which are set in beside it. The letter
//! takes no room in the lines' text as they are broken (its width is in
//! their measure instead), and is drawn as a line of its own whose
//! baseline is that of the last line it spans.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use paperpaw_document::{Block, BlockKind};

use crate::breaking::SetLines;
use crate::levels::BlockLevels;
use crate::{
    Face, LayoutContext, LineBidi, LineRun, PageLine, block_em, block_leading, block_line, face_at,
    word_sources,
};

/// Whether drop capitals are set: only where fonts can be drawn large
/// (`drop-caps`, which the SDK's `outline` turns on). A device of bitmap
/// fonts alone could never draw one, so its firmware leaves the code out
/// and lays such blocks out as without, as it would anyway.
pub(crate) const BUILT: bool = cfg!(feature = "drop-caps");

/// The most lines a drop capital spans.
pub(crate) const MAX_LINES: u8 = 5;
/// The height of a capital letter, in ems of its text (a usual serif's,
/// about 0.7): what the capital's size is reckoned from, so its top
/// stands about at the first line's capitals.
const CAP_HEIGHT: f32 = 0.7;
/// The room between the capital and the lines beside it, in ems of the
/// block's text.
const GAP: f32 = 0.15;
/// The most characters before the letter a capital takes along (an
/// opening quote).
const MAX_BEFORE: usize = 2;
/// How much of the size asked for a capital's letter must grow by, as
/// measured: less, and the font cannot be drawn that large.
const MIN_GROWTH: f32 = 0.85;
/// The largest size a capital is set in, in percent of body text.
const MAX_PERCENT: f32 = 2000.0;

/// A drop capital as laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Cap {
    /// The bytes of the block's text it takes: its letter and any
    /// punctuation before it.
    pub(crate) len: usize,
    /// The lines it spans, at least two.
    pub(crate) lines: usize,
    /// Its face: the block's at its start, larger.
    pub(crate) face: Face,
    /// The room the lines beside it leave: its width and a gap.
    pub(crate) room: f32,
}

/// Whether the layout may set `block`'s drop capital: a paragraph's
/// (never a heading's, a continued piece's or a right-to-left one's)
/// whose book asks for one of two lines or more.
pub(crate) fn wanted(block: &Block, heading: u8) -> bool {
    block.drop_cap >= 2 && heading == 0 && block.kind == BlockKind::Paragraph && !block.continues
}

/// The drop capital of `block` spanning `lines` lines (clamped to
/// [`MAX_LINES`]), in a block set in `base`, taking at most `max_room` of
/// the lines' width; `None` where it has none:
/// the book asks for none, the paragraph runs right to left, or no
/// letter starts it (after at most [`MAX_BEFORE`] opening quotes or
/// brackets, and with text after it), the font cannot be drawn large
/// enough for two lines, or the capital would take more than `max_room`.
/// A font drawn no larger than for fewer lines spans fewer.
pub(crate) fn of(
    cx: &LayoutContext<'_>,
    block: &Block,
    base: Face,
    heading: u8,
    levels: &BlockLevels,
    lines: u8,
    max_room: f32,
) -> Option<Cap> {
    let lines = lines.min(MAX_LINES);
    if !wanted(block, heading) || lines < 2 || levels.paragraph.is_rtl() {
        return None;
    }
    let (i, letter) = block
        .text
        .char_indices()
        .take(MAX_BEFORE + 1)
        .find(|(_, c)| c.is_alphanumeric())?;
    let len = i + letter.len_utf8();
    if !block.text[..i].chars().all(opens)
        || (0..len).any(|i| levels.is_rtl_at(i))
        || block.text[len..].trim().is_empty()
    {
        return None;
    }
    // As many lines as the font can be drawn large for: a font of
    // fixed sizes (a bitmap pack) is no larger past its largest.
    let body = face_at(block, base, 0, cx.style.book);
    let body_size = f32::from(if body.size == 0 { 100 } else { body.size });
    let at_body = cx.typesetter.advance(body, letter);
    let (lines, face) = (2..=lines)
        .rev()
        .map(|n| (n, face(cx, block, base, n)))
        .find(|&(_, face)| {
            let wanted = f32::from(face.size) / body_size;
            cx.typesetter.advance(face, letter) >= at_body * wanted * MIN_GROWTH
        })?;
    let em = block_em(block, cx.style);
    let width: f32 = block.text[..len]
        .chars()
        .map(|c| cx.typesetter.advance(face, c))
        .sum();
    Some(Cap {
        len,
        lines: usize::from(lines),
        face,
        room: width.max(0.0) + GAP * em,
    })
    // One wider than it may be is none: the lines would cover it.
    .filter(|cap| cap.room <= max_room)
}

/// Whether `c` may stand before a capital's letter: an opening quote or
/// bracket.
fn opens(c: char) -> bool {
    matches!(
        c,
        '"' | '\'' | '(' | '[' | '\u{ab}' | '\u{bb}' | '\u{2018}'
            ..='\u{201f}' | '\u{2039}' | '\u{203a}'
    )
}

/// The face of `block`'s capital spanning `lines` lines (one: as large as
/// its text), in a block set in `base`.
pub(crate) fn face(cx: &LayoutContext<'_>, block: &Block, base: Face, lines: u8) -> Face {
    let style = cx.style;
    let em = block_em(block, style);
    let (_, line_height) = block_line(block, style, cx.typesetter.line(base), em);
    let line_height = line_height + block_leading(block, em);
    // From the first line's capitals down to the last line's baseline.
    let height = f32::from(lines.max(1) - 1) * line_height + CAP_HEIGHT * em;
    let percent = (height / CAP_HEIGHT / style.em.max(1.0) * 100.0).clamp(1.0, MAX_PERCENT);
    let mut face = face_at(block, base, 0, style.book);
    face.size = percent as u16;
    face
}

/// The lines `lay` breaks beside the capital `capital` gives for `wanted`
/// lines, and that capital: one that would span more lines than its
/// paragraph has spans as many as it has (and none beside a paragraph of
/// one line), and none where the first line would hold nothing but the
/// capital (a word too long beside it).
pub(crate) fn fitted(
    lay: impl Fn(Option<Cap>) -> SetLines,
    capital: impl Fn(u8) -> Option<Cap>,
    wanted: u8,
) -> (SetLines, Option<Cap>) {
    if !BUILT {
        return (lay(None), None);
    }
    let mut cap = capital(wanted);
    let mut lines = lay(cap);
    if let Some(spans) = cap
        && lines.len() < spans.lines
    {
        cap = capital(u8::try_from(lines.len()).unwrap_or(0));
        lines = lay(cap);
    }
    if let Some(cap) = cap
        && lines
            .first()
            .is_none_or(|(line, _)| line.range.end <= cap.len)
    {
        return (lay(None), None);
    }
    (lines, cap)
}

/// The line `cap` of `block` is drawn as, its left edge and baseline at
/// `(x, baseline)`, in section `section`: the capital's text in its face,
/// a word of the book's.
pub(crate) fn line(block: &Block, cap: Cap, (x, baseline): (f32, f32), section: u32) -> PageLine {
    let run = LineRun::new(0, cap.len, cap.face).with_lang(block.lang_of(cap.face.style));
    PageLine {
        x,
        baseline,
        text: String::from(&block.text[..cap.len]),
        word_spacing: 0.0,
        runs: vec![run],
        section,
        words: word_sources(block, 0..cap.len),
        leading: 0.0,
        inserted: false,
        annotations: Vec::new(),
        bidi: LineBidi::new(0, LineBidi::DROP_CAP),
        hyphen: false,
    }
}

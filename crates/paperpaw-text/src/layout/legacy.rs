//! A verbatim copy of the two wrapping functions as they were before
//! Unicode line breaking (A5), kept as the oracle of the tests: a text
//! without a flagged character must give the same lines (design, section
//! 9.2). Only the names, the visibility and the paths of what they use are
//! changed.

use alloc::vec::Vec;
use core::ops::Range;

use super::{
    Break, Hyphenate, Line, MAX_BREAKS, Measures, cheapest, count_spaces, end_line, hyphen_break,
    hyphen_points, push,
};

pub(super) fn legacy_wrap_kerned_in(
    text: &str,
    width: &dyn Fn(usize) -> f32,
    advance: &dyn Fn(usize, char) -> f32,
    kern: &dyn Fn(usize, char, char) -> f32,
    hyphenate: Option<Hyphenate<'_>>,
) -> Vec<Line> {
    let measures = Measures { advance, kern };
    let mut lines = Vec::new();
    let mut line_start = 0;
    let mut line_width = 0.0f32;
    // Width of the line without the spaces at its end.
    let mut trimmed_width = 0.0f32;
    let mut spaces = 0usize;
    // Where the word being read began.
    let mut word_start = 0;
    // Last break opportunity: (end of line text, start of next line, width, spaces).
    let mut last_break: Option<(usize, usize, f32, usize)> = None;
    // Byte offset of the next character to read.
    let mut at = 0;

    while let Some(c) = text[at..].chars().next() {
        let i = at;
        at += c.len_utf8();
        if c == '\n' {
            push(&mut lines, text, line_start..i, trimmed_width, spaces, true);
            line_start = at;
            word_start = line_start;
            (line_width, trimmed_width, spaces, last_break) = (0.0, 0.0, 0, None);
            continue;
        }
        if c == ' ' && i == line_start {
            // Spaces at the start of a continuation line are dropped.
            line_start = at;
            word_start = line_start;
            continue;
        }
        let w = measures.add(text, line_start, i, c);
        let available = width(lines.len());
        if c != ' ' && line_width + w > available && i > line_start {
            let measure = |range: Range<usize>| -> f32 {
                let start = range.start;
                let add = |(k, c)| measures.add(text, line_start, start + k, c);
                text[range].char_indices().map(add).sum()
            };
            let hyphen = |cut: usize| measures.hyphen(text, line_start, cut);
            let before = line_width - measure(word_start..i);
            let hyphenated = hyphenate.and_then(|hyphenate| {
                hyphen_break(
                    text, word_start, line_start, i, before, available, &measure, &hyphen,
                    hyphenate,
                )
            });
            // Break at the last opportunity, or inside the word if none.
            let fallback = last_break.unwrap_or((i, i, line_width, spaces));
            let next = end_line(&mut lines, text, (line_start, spaces), hyphenated, fallback);
            // Read again from the start of the new line, so that what
            // moved there is broken against its width like any other text
            // and the line comes out as it would laid out from its start.
            // Every break moves past the old line start, so this ends.
            line_start = next;
            word_start = next;
            at = next;
            (line_width, trimmed_width, spaces, last_break) = (0.0, 0.0, 0, None);
            continue;
        }
        line_width += w;
        match c {
            ' ' => {
                let trimmed = text[line_start..i].trim_end_matches(' ');
                let end = line_start + trimmed.len();
                let end_spaces = spaces.saturating_sub(i - end);
                last_break = Some((end, at, trimmed_width, end_spaces));
                spaces += 1;
                word_start = at;
            }
            '-' | '\u{2010}' | '\u{2013}' | '\u{2014}' => {
                trimmed_width = line_width;
                if text[at..].chars().next().is_some_and(|n| n != ' ') {
                    last_break = Some((at, at, line_width, spaces));
                    word_start = at;
                }
            }
            _ => trimmed_width = line_width,
        }
    }
    if line_start < text.len() || lines.is_empty() || text.ends_with('\n') {
        push(
            &mut lines,
            text,
            line_start..text.len(),
            trimmed_width,
            spaces,
            true,
        );
    }
    lines
}

pub(super) fn legacy_wrap_balanced_in(
    text: &str,
    width: &dyn Fn(usize) -> f32,
    advance: &dyn Fn(usize, char) -> f32,
    kern: &dyn Fn(usize, char, char) -> f32,
    hyphenate: Option<Hyphenate<'_>>,
    extra_cost: &dyn Fn(f32, usize) -> f32,
) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    loop {
        let end = text[start..].find('\n').map_or(text.len(), |k| start + k);
        let widths = (width(lines.len()), width(lines.len() + 1));
        let measures = Measures { advance, kern };
        let balanced =
            legacy_balanced_paragraph(text, start..end, widths, &measures, hyphenate, extra_cost);
        match balanced {
            Some(mut paragraph) => lines.append(&mut paragraph),
            None => {
                let offset = lines.len();
                let greedy = legacy_wrap_kerned(
                    &text[start..end],
                    |n| width(offset + n),
                    |i, c| advance(start + i, c),
                    |i, left, c| kern(start + i, left, c),
                    hyphenate,
                );
                lines.extend(greedy.into_iter().map(|line| Line {
                    range: start + line.range.start..start + line.range.end,
                    ..line
                }));
            }
        }
        if end == text.len() {
            break;
        }
        start = end + 1;
    }
    lines
}

/// The lines of the paragraph `range` of `text`, broken over all of it
/// (see [`wrap_lines_balanced`]); `widths` of its first line and of the
/// others. `None` where it is broken greedily.
fn legacy_balanced_paragraph(
    text: &str,
    range: Range<usize>,
    (first, rest): (f32, f32),
    measures: &Measures<'_>,
    hyphenate: Option<Hyphenate<'_>>,
    extra_cost: &dyn Fn(f32, usize) -> f32,
) -> Option<Vec<Line>> {
    if text.len() >= usize::from(u16::MAX) {
        return None;
    }
    // Spaces at the start are dropped, as a line's are.
    let paragraph = &text[range.clone()];
    let start = range.start + (paragraph.len() - paragraph.trim_start_matches(' ').len());
    // A break after each run of spaces, the first and the last: room for
    // most, without growing the list step by step.
    let spaces = paragraph.bytes().filter(|&b| b == b' ').count();
    let mut breaks: Vec<Break> = Vec::with_capacity(spaces.min(MAX_BREAKS) + 2);
    let at = |end: usize, next: usize, to_end: f32, to_next: f32, hyphen: f32| Break {
        end: end as u16,
        next: next as u16,
        to_end,
        to_next,
        hyphen,
        cost: f32::INFINITY,
        prev: 0,
        spaces: 0,
    };
    breaks.push(Break {
        cost: 0.0,
        ..at(start, start, 0.0, 0.0, 0.0)
    });
    // The places a word may be hyphenated, `x` the width before it.
    let points = |breaks: &mut Vec<Break>, word: Range<usize>, x: f32| {
        if let Some(hyphenate) = hyphenate {
            hyphen_points(text, word, start, x, measures, hyphenate, breaks, &at);
        }
    };
    // The places to break, with the width of the text up to them.
    let mut x = 0.0f32;
    let mut word = start;
    let mut i = start;
    let mut x_word = 0.0f32;
    while i < range.end {
        let c = text[i..].chars().next()?;
        let len = c.len_utf8();
        if c == ' ' {
            if i > word {
                points(&mut breaks, word..i, x_word);
            }
            let end = i;
            let to_end = x;
            while text[i..].starts_with(' ') && i < range.end {
                x += (measures.advance)(i, ' ');
                i += 1;
            }
            if i < range.end {
                breaks.push(at(end, i, to_end, x, 0.0));
            }
            word = i;
            x_word = x;
        } else {
            x += measures.add(text, start, i, c);
            i += len;
            let dash = matches!(c, '-' | '\u{2010}' | '\u{2013}' | '\u{2014}');
            if dash && i < range.end && !text[i..].starts_with(' ') {
                // The next line starts with the character after it,
                // without its pair with the dash.
                breaks.push(at(i, i, x, x + measures.cut(text, i), 0.0));
            }
        }
        if breaks.len() > MAX_BREAKS {
            return None;
        }
    }
    if range.end > word {
        points(&mut breaks, word..range.end, x_word);
    }
    // Hyphenation points came after the word's start; keep the order.
    // No two breaks share `(end, next)`: a hyphenation point is between
    // two letters, a dash's break after the dash, and a space's break
    // ends before its spaces and goes on after them. A point given twice
    // makes two equal breaks. So sorting in place gives the same order.
    breaks.sort_unstable_by_key(|b| (b.end, b.next));
    let last = breaks.len();
    breaks.push(at(range.end, range.end, x, x, 0.0));
    if breaks.len() > MAX_BREAKS {
        return None;
    }
    count_spaces(text, start, &mut breaks);
    cheapest(&mut breaks, (first, rest), extra_cost);
    if breaks[last].cost.is_infinite() {
        return None;
    }
    // The lines, from the last break back.
    let mut ends = Vec::new();
    let mut j = last;
    while j > 0 {
        ends.push(j);
        j = usize::from(breaks[j].prev);
    }
    let mut lines = Vec::with_capacity(ends.len());
    let mut from = 0;
    for &j in ends.iter().rev() {
        let (b, e) = (breaks[from], breaks[j]);
        let line_range = usize::from(b.next)..usize::from(e.end);
        lines.push(Line {
            spaces: text[line_range.clone()].matches(' ').count(),
            range: line_range,
            width: e.to_end - b.to_next + e.hyphen,
            last: j == last,
            hyphen: e.hyphen > 0.0,
        });
        from = j;
    }
    Some(lines)
}

fn legacy_wrap_kerned(
    text: &str,
    width: impl Fn(usize) -> f32,
    advance: impl Fn(usize, char) -> f32,
    kern: impl Fn(usize, char, char) -> f32,
    hyphenate: Option<Hyphenate<'_>>,
) -> Vec<Line> {
    legacy_wrap_kerned_in(text, &width, &advance, &kern, hyphenate)
}

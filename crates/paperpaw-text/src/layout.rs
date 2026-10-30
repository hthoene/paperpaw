//! Line breaking.

use alloc::vec::Vec;
use core::ops::Range;

/// A line produced by [`wrap_lines`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Line {
    /// Byte range of the line's text, without the whitespace it broke at.
    pub range: Range<usize>,
    /// Width of the text in `range`, plus the hyphen if `hyphen` is set.
    pub width: f32,
    /// Number of spaces inside `range` (for justification).
    pub spaces: usize,
    /// Whether the line ends its paragraph (or a forced break).
    pub last: bool,
    /// Whether the line ends inside a hyphenated word: draw a hyphen
    /// after the text.
    pub hyphen: bool,
}

impl Line {
    /// Made of what it holds.
    pub fn new(range: Range<usize>, width: f32, spaces: usize, last: bool, hyphen: bool) -> Self {
        Self {
            range,
            width,
            spaces,
            last,
            hyphen,
        }
    }
}

/// Finds hyphenation points: calls the second argument with the byte
/// offset of every position where the word (letters only) may be broken.
pub type Hyphenate<'a> = &'a dyn Fn(&str, &mut dyn FnMut(usize));

/// Greedy line breaking in a single pass over `text`.
///
/// Breaks at spaces and after hyphens and dashes; explicit newlines always
/// break; words longer than a line are split by character. `width(n)` is the
/// available width of line `n` (allowing a first-line indent), `advance(c)`
/// the width of a character.
pub fn wrap_lines(
    text: &str,
    width: impl Fn(usize) -> f32,
    advance: impl Fn(char) -> f32,
) -> Vec<Line> {
    wrap_lines_hyphenated(text, width, |_, c| advance(c), None)
}

/// Like [`wrap_lines`], but `advance(index, c)` also gets the byte index of
/// the character (for text in several styles), and a word that does not fit
/// at the end of a line is hyphenated where `hyphenate` allows, as far as it
/// fits.
///
/// Lines only depend on the text from their start, so laying out from the
/// start of any line gives the same lines as laying out the whole text.
pub fn wrap_lines_hyphenated(
    text: &str,
    width: impl Fn(usize) -> f32,
    advance: impl Fn(usize, char) -> f32,
    hyphenate: Option<Hyphenate<'_>>,
) -> Vec<Line> {
    wrap_lines_kerned(text, width, advance, |_, _, _| 0.0, hyphenate)
}

/// The adjustment `kern` makes between `c`, set at byte `i` of `text`,
/// and the character before it; none where either is a space or the
/// line breaks between them (`i` at `line_start` or before).
fn pair(
    text: &str,
    kern: &dyn Fn(usize, char, char) -> f32,
    line_start: usize,
    i: usize,
    c: char,
) -> f32 {
    if i <= line_start || c == ' ' {
        return 0.0;
    }
    match text[..i].chars().next_back() {
        Some(left) if left != ' ' && left != '\n' => kern(i, left, c),
        _ => 0.0,
    }
}

/// Like [`wrap_lines_hyphenated`], with pairs of characters kerned:
/// `kern(index, left, right)` is how much `right`, set at byte `index`
/// after `left`, moves (negative: closer); for the hyphen at the end of
/// a hyphenated line, `index` is where the line ends. Pairs are kerned
/// within a line only, never across a space or a break, so a line's
/// width is that of its text drawn alone with the same pairs.
pub fn wrap_lines_kerned(
    text: &str,
    width: impl Fn(usize) -> f32,
    advance: impl Fn(usize, char) -> f32,
    kern: impl Fn(usize, char, char) -> f32,
    hyphenate: Option<Hyphenate<'_>>,
) -> Vec<Line> {
    wrap_lines_kerned_in(text, &width, &advance, &kern, hyphenate)
}

paperpaw_native::shared! {
/// [`wrap_lines_kerned`] without generics, the one function of it that is
/// shared with the firmware: the caller's closures come as `dyn`.
#[doc(hidden)]
pub fn wrap_lines_kerned_in(
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
} @ LAYOUT_WRAP_KERNED
}

/// The most places to break a paragraph (between newlines) that
/// [`wrap_lines_balanced`] weighs; one with more is broken greedily, so
/// the memory it takes stays bounded (some 20 KB).
pub const MAX_BREAKS: usize = 1024;

/// Words shorter than this are not hyphenated by [`wrap_lines_balanced`]:
/// looking up the patterns of every word costs, and short words gain
/// little.
const BALANCED_HYPHEN_MIN: usize = 7;

/// A place where a line may end, for [`wrap_lines_balanced`].
#[derive(Debug, Clone, Copy)]
struct Break {
    /// Where the line before it ends and the next starts.
    end: u16,
    next: u16,
    /// The width of the text from the paragraph's start to `end` and to
    /// `next`; with a hyphen, its width in `hyphen`.
    to_end: f32,
    to_next: f32,
    hyphen: f32,
    /// The least cost of lines up to here, and the break before.
    cost: f32,
    prev: u16,
    /// Spaces before `end`, counted once for the whole paragraph.
    spaces: u16,
}

/// Line breaking over whole paragraphs: of the ways to break each, the
/// one whose lines (all but the last) leave the least room unfilled, as
/// the sum of the squares of what each leaves, a hyphen counting as a
/// tenth of a line left (minimum raggedness, with a bounded look back:
/// a line is never longer than its width). Lines then end more evenly
/// than greedily, and justified text has fewer wide gaps.
///
/// `width(n)` must be the same for every line after the first. A
/// paragraph with a word longer than a line, or with more than
/// [`MAX_BREAKS`] places to break, is broken greedily, as
/// [`wrap_lines_hyphenated`] does; so are paragraphs of 64 KB and more.
///
/// The lines of a paragraph depend on all of it: laid out from a line's
/// start, they come out other than in the whole paragraph. Callers lay
/// out a paragraph from its start and take the lines they need.
pub fn wrap_lines_balanced(
    text: &str,
    width: impl Fn(usize) -> f32,
    advance: impl Fn(usize, char) -> f32,
    hyphenate: Option<Hyphenate<'_>>,
) -> Vec<Line> {
    wrap_lines_balanced_kerned(text, width, advance, |_, _, _| 0.0, hyphenate)
}

/// Like [`wrap_lines_balanced`], with pairs of characters kerned as
/// [`wrap_lines_kerned`] kerns them.
pub fn wrap_lines_balanced_kerned(
    text: &str,
    width: impl Fn(usize) -> f32,
    advance: impl Fn(usize, char) -> f32,
    kern: impl Fn(usize, char, char) -> f32,
    hyphenate: Option<Hyphenate<'_>>,
) -> Vec<Line> {
    wrap_lines_balanced_kerned_with_cost(text, width, advance, kern, hyphenate, |_, _| 0.0)
}

/// Like [`wrap_lines_balanced_kerned`], with an additional cost for each
/// non-final line. `extra_cost` receives its unused width and the number
/// of spaces inside it, excluding spaces at the break. It can account
/// for how those spaces will be stretched when the line is drawn.
///
/// The ordinary raggedness and hyphen costs still apply. Negative or NaN
/// additions count as zero. An infinite cost rules out that candidate;
/// if no complete layout remains, the paragraph is broken greedily.
/// The callback must return the same cost for the same arguments. It is
/// not called for final lines. Returning zero preserves the ordinary
/// balanced layout.
pub fn wrap_lines_balanced_kerned_with_cost(
    text: &str,
    width: impl Fn(usize) -> f32,
    advance: impl Fn(usize, char) -> f32,
    kern: impl Fn(usize, char, char) -> f32,
    hyphenate: Option<Hyphenate<'_>>,
    extra_cost: impl Fn(f32, usize) -> f32,
) -> Vec<Line> {
    wrap_lines_balanced_in(text, &width, &advance, &kern, hyphenate, &extra_cost)
}

paperpaw_native::shared! {
/// [`wrap_lines_balanced_kerned_with_cost`] without generics, the one
/// function of it that is shared with the firmware.
#[doc(hidden)]
pub fn wrap_lines_balanced_in(
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
            balanced_paragraph(text, start..end, widths, &measures, hyphenate, extra_cost);
        match balanced {
            Some(mut paragraph) => lines.append(&mut paragraph),
            None => {
                let offset = lines.len();
                let greedy = wrap_lines_kerned(
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
} @ LAYOUT_WRAP_BALANCED
}

/// How characters are measured for [`balanced_paragraph`]: their
/// advances and their pairs (see [`wrap_lines_kerned`]).
struct Measures<'a> {
    advance: &'a dyn Fn(usize, char) -> f32,
    kern: &'a dyn Fn(usize, char, char) -> f32,
}

impl Measures<'_> {
    /// The width `c` at byte `i` of `text` adds to a line that starts
    /// at `line_start`: its advance and its pair with the one before.
    fn add(&self, text: &str, line_start: usize, i: usize, c: char) -> f32 {
        (self.advance)(i, c) + pair(text, self.kern, line_start, i, c)
    }

    /// The width of the hyphen that ends a line at `cut`, set like the
    /// character before it, with their pair.
    fn hyphen(&self, text: &str, line_start: usize, cut: usize) -> f32 {
        (self.advance)(cut - 1, '-') + pair(text, self.kern, line_start, cut, '-')
    }

    /// What a line from `i` on does not count of what the text up to
    /// it does: the pair of the character at `i` with the one before.
    fn cut(&self, text: &str, i: usize) -> f32 {
        let c = text[i..].chars().next().unwrap_or(' ');
        pair(text, self.kern, 0, i, c)
    }
}

/// The lines of the paragraph `range` of `text`, broken over all of it
/// (see [`wrap_lines_balanced`]); `widths` of its first line and of the
/// others. `None` where it is broken greedily.
fn balanced_paragraph(
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

/// Prefix counts fit in the padding of each break. Counting once keeps
/// candidate scoring independent of the length of its text.
fn count_spaces(text: &str, start: usize, breaks: &mut [Break]) {
    let mut at = start;
    let mut spaces = 0u16;
    for point in breaks {
        let end = usize::from(point.end);
        spaces += text[at..end].bytes().filter(|&b| b == b' ').count() as u16;
        point.spaces = spaces;
        at = end;
    }
}

#[cfg(test)]
#[path = "layout/cost_tests.rs"]
mod cost_tests;

/// The least cost of lines up to each of `breaks` (the last ends the
/// paragraph, free of cost), and the break before it on that way,
/// looking back no further than a line goes; `widths` of the first line
/// and of the others.
fn cheapest(
    breaks: &mut [Break],
    (first, rest): (f32, f32),
    extra_cost: &dyn Fn(f32, usize) -> f32,
) {
    let last = breaks.len() - 1;
    let hyphen_cost = (rest * 0.1) * (rest * 0.1);
    for j in 1..breaks.len() {
        let mut best = (f32::INFINITY, 0usize);
        for i in (0..j).rev() {
            let (b, e) = (breaks[i], breaks[j]);
            if usize::from(e.end) < usize::from(b.next) {
                continue;
            }
            let line = e.to_end - b.to_next + e.hyphen;
            let available = if i == 0 { first } else { rest };
            if line > available {
                // Further back, lines only get longer.
                if e.to_end - b.to_next > available {
                    break;
                }
                continue;
            }
            if b.cost.is_infinite() {
                continue;
            }
            let left = available - line;
            let spaces = usize::from(e.spaces - b.spaces - (b.next - b.end));
            let cost = b.cost
                + match j == last {
                    true => 0.0,
                    false => {
                        left * left
                            + if e.hyphen > 0.0 { hyphen_cost } else { 0.0 }
                            + extra_cost(left, spaces).max(0.0)
                    }
                };
            if cost < best.0 {
                best = (cost, i);
            }
        }
        breaks[j].cost = best.0;
        breaks[j].prev = best.1 as u16;
    }
}

/// Adds the places the word `word` of `text` may be hyphenated, as
/// [`hyphen_break`] takes them, `x` the width before it, in a paragraph
/// from `start`.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
fn hyphen_points(
    text: &str,
    word: Range<usize>,
    start: usize,
    x: f32,
    measures: &Measures<'_>,
    hyphenate: Hyphenate<'_>,
    breaks: &mut Vec<Break>,
    at: &dyn Fn(usize, usize, f32, f32, f32) -> Break,
) {
    let w = &text[word.clone()];
    let Some(first) = w.find(char::is_alphabetic) else {
        return;
    };
    let letters = w[first..]
        .find(|c: char| !c.is_alphabetic())
        .map_or(w.len(), |k| first + k);
    let core = &w[first..letters];
    if core.chars().count() < BALANCED_HYPHEN_MIN || !core.chars().any(char::is_lowercase) {
        return;
    }
    hyphenate(core, &mut |cut| {
        if !core.is_char_boundary(cut) || cut == 0 || cut >= core.len() {
            return;
        }
        let end = word.start + first + cut;
        let before: f32 = text[word.start..end]
            .char_indices()
            .map(|(k, c)| measures.add(text, start, word.start + k, c))
            .sum();
        let to = x + before;
        // The hyphen with its pair, and the next line without the pair
        // it breaks.
        let hyphen = measures.hyphen(text, start, end);
        breaks.push(at(end, end, to, to + measures.cut(text, end), hyphen));
    });
}

/// The last hyphenation point of the word at `word_start` that fits on the
/// line before `overflow`, the character that does not: `(byte offset,
/// line width with the hyphen)`. The letters the word starts with are
/// hyphenated (with at least one lowercase letter among them); punctuation
/// before them and whatever follows them, such as "(s)" or "'s", stays
/// attached.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
fn hyphen_break(
    text: &str,
    word_start: usize,
    line_start: usize,
    overflow: usize,
    before: f32,
    available: f32,
    measure: &dyn Fn(Range<usize>) -> f32,
    hyphen: &dyn Fn(usize) -> f32,
    hyphenate: Hyphenate<'_>,
) -> Option<(usize, f32)> {
    let end = text[word_start..]
        .find([' ', '\n'])
        .map_or(text.len(), |k| word_start + k);
    let word = &text[word_start..end];
    let first = word.find(char::is_alphabetic)?;
    let letters = word[first..]
        .find(|c: char| !c.is_alphabetic())
        .map_or(word.len(), |k| first + k);
    let core = &word[first..letters];
    // Words in capitals (abbreviations, headings set in capitals) are
    // not hyphenated.
    if !core.chars().any(char::is_lowercase) {
        return None;
    }
    let mut best = None;
    hyphenate(core, &mut |at| {
        // Points between characters of the word only; none past the
        // character that overflows, which a negative width behind it
        // could make seem to fit.
        if !core.is_char_boundary(at) {
            return;
        }
        let cut = word_start + first + at;
        if cut <= line_start || cut > overflow {
            return;
        }
        // The hyphen is set like the letter before it.
        let width = before + measure(word_start..cut) + hyphen(cut);
        if width <= available {
            best = Some((cut, width));
        }
    });
    best
}

/// Ends the line from `line_start` with `spaces` that overflows: at the
/// hyphenation point found (and its width), else at `fallback` (end of
/// the line's text, start of the next, width, spaces). Where the next
/// line starts.
fn end_line(
    lines: &mut Vec<Line>,
    text: &str,
    (line_start, spaces): (usize, usize),
    hyphenated: Option<(usize, f32)>,
    fallback: (usize, usize, f32, usize),
) -> usize {
    if let Some((cut, width)) = hyphenated {
        lines.push(Line {
            range: line_start..cut,
            width,
            spaces,
            last: false,
            hyphen: true,
        });
        return cut;
    }
    let (end, next, width, spaces) = fallback;
    push(lines, text, line_start..end, width, spaces, false);
    next
}

fn push(
    lines: &mut Vec<Line>,
    text: &str,
    range: Range<usize>,
    width: f32,
    spaces: usize,
    last: bool,
) {
    let trimmed = text[range.clone()].trim_end_matches(' ');
    let removed = range.len() - trimmed.len();
    lines.push(Line {
        range: range.start..range.start + trimmed.len(),
        width,
        spaces: spaces.saturating_sub(removed),
        last,
        hyphen: false,
    });
}

/// Breaks `text` into lines no wider than `max_width`, using `measure` to
/// get the width of a slice. Convenience wrapper around [`wrap_lines`] for
/// short UI texts.
pub fn wrap(text: &str, max_width: f32, measure: impl Fn(&str) -> f32) -> Vec<Range<usize>> {
    wrap_lines(
        text,
        |_| max_width,
        |c| measure(c.encode_utf8(&mut [0u8; 4])),
    )
    .into_iter()
    .map(|line| line.range)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// Every character is one unit wide.
    fn lines(text: &str, width: f32) -> Vec<&str> {
        wrap(text, width, |s| s.chars().count() as f32)
            .into_iter()
            .map(|r| &text[r])
            .collect()
    }

    #[test]
    fn breaks_at_spaces() {
        assert_eq!(
            lines("the quick brown fox", 10.0),
            vec!["the quick", "brown fox"]
        );
        assert_eq!(lines("a  b", 1.0), vec!["a", "b"]);
    }

    #[test]
    fn breaks_after_hyphens_and_at_newlines() {
        assert_eq!(
            lines("well-known\nfact", 6.0),
            vec!["well-", "known", "fact"]
        );
        assert_eq!(lines("a\n\nb", 6.0), vec!["a", "", "b"]);
    }

    #[test]
    fn splits_overlong_words() {
        assert_eq!(lines("abcdefgh", 3.0), vec!["abc", "def", "gh"]);
    }

    #[test]
    fn handles_multibyte_text() {
        assert_eq!(
            lines("Größe über Äpfel", 6.0),
            vec!["Größe", "über", "Äpfel"]
        );
    }

    #[test]
    fn reports_widths_spaces_and_first_line_indent() {
        let text = "aa bb cc dd";
        let result = wrap_lines(text, |n| if n == 0 { 6.0 } else { 8.0 }, |_| 1.0);
        let got: Vec<(&str, f32, usize, bool)> = result
            .iter()
            .map(|l| (&text[l.range.clone()], l.width, l.spaces, l.last))
            .collect();
        assert_eq!(got, vec![("aa bb", 5.0, 1, false), ("cc dd", 5.0, 1, true)]);
    }

    #[test]
    fn hyphenates_words_that_do_not_fit() {
        // Every word may be broken after its third and sixth letter.
        let hyphenate = |word: &str, out: &mut dyn FnMut(usize)| {
            for at in [3, 6] {
                if at < word.len() {
                    out(at);
                }
            }
        };
        let text = "ab abcdefghi, xy";
        let result = wrap_lines_hyphenated(text, |_| 9.0, |_, _| 1.0, Some(&hyphenate));
        let got: Vec<(&str, f32, bool)> = result
            .iter()
            .map(|l| (&text[l.range.clone()], l.width, l.hyphen))
            .collect();
        // "ab abc-" fits 9 units with the hyphen, "ab abcdef-" would not.
        assert_eq!(
            got,
            vec![
                ("ab abc", 7.0, true),
                ("defghi,", 7.0, false),
                ("xy", 2.0, false)
            ]
        );
        // The letters before a bracket or an apostrophe are hyphenated.
        let bracketed = "ab abcdefghi(s) xy";
        let result = wrap_lines_hyphenated(bracketed, |_| 9.0, |_, _| 1.0, Some(&hyphenate));
        assert_eq!(&bracketed[result[1].range.clone()], "defghi(s)");
        assert!(result[0].hyphen);
        // Words in capitals are not hyphenated.
        let caps = wrap_lines_hyphenated("ab ABCDEFGHI", |_| 9.0, |_, _| 1.0, Some(&hyphenate));
        assert!(caps.iter().all(|l| !l.hyphen));
        // Without hyphenation the word moves to the next line.
        assert_eq!(lines(text, 9.0), vec!["ab", "abcdefghi", ", xy"]);
    }

    #[test]
    fn hyphenation_points_past_the_overflow_are_ignored() {
        // A character after the one that overflows has a negative width,
        // so the line up to a cut behind it would seem to fit.
        let hyphenate = |word: &str, out: &mut dyn FnMut(usize)| out(word.len() - 1);
        let advance = |_, c| if c == 'z' { -10.0 } else { 1.0 };
        let text = "abcdefgzk";
        let result = wrap_lines_hyphenated(text, |_| 4.0, advance, Some(&hyphenate));
        assert!(result.iter().all(|l| l.range.start <= l.range.end));
        assert_eq!(&text[result[0].range.clone()], "abcd");
        // Points inside a character or past the word are ignored too.
        let wild = |_: &str, out: &mut dyn FnMut(usize)| {
            out(1);
            out(2);
            out(99);
        };
        let result = wrap_lines_hyphenated("ağbcdef", |_| 4.0, |_, _| 1.0, Some(&wild));
        assert_eq!(result[0].range, 0..1);
        assert!(result[0].hyphen);
    }

    #[test]
    fn hyphenation_is_stable_from_any_line_start() {
        let hyphenate = |word: &str, out: &mut dyn FnMut(usize)| {
            for at in (2..word.len().saturating_sub(1)).step_by(2) {
                out(at);
            }
        };
        let text = "Lorem ipsum dolor sitametconsectetur adipiscing elit sed doeiusmod";
        let all = wrap_lines_hyphenated(text, |_| 12.0, |_, _| 1.0, Some(&hyphenate));
        for line in &all[1..] {
            let from = line.range.start;
            let rest = wrap_lines_hyphenated(&text[from..], |_| 12.0, |_, _| 1.0, Some(&hyphenate));
            assert_eq!(
                text[from + rest[0].range.start..from + rest[0].range.end],
                text[line.range.clone()]
            );
        }
    }

    /// The lines of `text` broken over whole paragraphs, one unit a
    /// character.
    fn balanced(text: &str, width: f32) -> Vec<&str> {
        wrap_lines_balanced(text, |_| width, |_, _| 1.0, None)
            .into_iter()
            .map(|l| &text[l.range])
            .collect()
    }

    #[test]
    fn balanced_lines_end_more_evenly_than_greedy_ones() {
        let text = "aaa bb cc ddddd eee ff ggggg";
        // Greedy fills the first line and leaves the second short.
        assert_eq!(
            lines(text, 10.0),
            vec!["aaa bb cc", "ddddd eee", "ff ggggg"]
        );
        let even = balanced(text, 10.0);
        assert_eq!(even.concat().replace(' ', ""), text.replace(' ', ""));
        let slack = |lines: &[&str]| -> f32 {
            lines[..lines.len() - 1]
                .iter()
                .map(|l| (10.0 - l.len() as f32).powi(2))
                .sum()
        };
        assert!(slack(&even) <= slack(&lines(text, 10.0)), "{even:?}");
        assert!(even.iter().all(|l| l.len() <= 10), "{even:?}");
    }

    #[test]
    fn balanced_breaking_keeps_what_greedy_keeps() {
        // Newlines break, the last line of each paragraph says so,
        // spaces are counted, widths and the first line's own width.
        let text = "aa bb cc dd
ee ff";
        let result =
            wrap_lines_balanced(text, |n| if n == 0 { 6.0 } else { 8.0 }, |_, _| 1.0, None);
        let got: Vec<(&str, f32, usize, bool)> = result
            .iter()
            .map(|l| (&text[l.range.clone()], l.width, l.spaces, l.last))
            .collect();
        assert_eq!(
            got,
            vec![
                ("aa bb", 5.0, 1, false),
                ("cc dd", 5.0, 1, true),
                ("ee ff", 5.0, 1, true)
            ]
        );
        // A word longer than a line: broken as greedy breaks it.
        assert_eq!(
            balanced("ab abcdefgh cd", 4.0),
            lines("ab abcdefgh cd", 4.0)
        );
        assert_eq!(balanced("", 5.0), vec![""]);
        assert_eq!(
            balanced("well-known fact", 6.0),
            vec!["well-", "known", "fact"]
        );
    }

    #[test]
    fn balanced_breaking_hyphenates_long_words() {
        let hyphenate = |word: &str, out: &mut dyn FnMut(usize)| {
            for at in (2..word.len().saturating_sub(1)).step_by(2) {
                out(at);
            }
        };
        let text = "ab cdefghijkl mn opqrstuvwx yz";
        let result = wrap_lines_balanced(text, |_| 9.0, |_, _| 1.0, Some(&hyphenate));
        for line in &result {
            assert!(line.width <= 9.0, "{line:?}");
            let shown = text[line.range.clone()].len() + usize::from(line.hyphen);
            assert_eq!(shown as f32, line.width, "{line:?}");
        }
        assert!(result.iter().any(|l| l.hyphen), "{result:?}");
    }

    #[test]
    fn empty_text_is_one_empty_line() {
        assert_eq!(lines("", 5.0), vec![""]);
    }
}

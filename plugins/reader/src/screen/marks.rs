//! Marks on the page's words, all drawn the same way (ADR 0021): the
//! search hit gone to, the sentence read aloud, one's own highlights,
//! what plugins' decorators mark, links. Each is a span of the page in a
//! role's look; where spans meet on a word their looks are combined, the
//! earlier source winning an aspect both set.

use paperpaw_sdk::builtin::roles;
use paperpaw_sdk::decorations::{self, Decoration, PageText, PageWord};

use super::*;

/// A part of the page, in a look.
struct Span {
    start: Locator,
    end: Locator,
    look: Decoration,
}

/// The marks of a page, ready to draw: the boxes of words that look the
/// same (one per line), and each word's ink.
pub(super) struct PageMarks {
    groups: Vec<(Decoration, Vec<Rect>)>,
    pub(super) inks: Vec<Option<Color>>,
}

impl PageMarks {
    /// Backgrounds, before the text.
    pub(super) fn draw_behind(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        for (look, boxes) in &self.groups {
            decorations::draw_behind(canvas, env, boxes, look);
        }
    }

    /// Lines, frames and strikes, after the text.
    pub(super) fn draw_over(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        for (look, boxes) in &self.groups {
            decorations::draw_over(canvas, env, boxes, look);
        }
    }
}

/// The whole word for the parts of a word split over two lines: both
/// parts get it, so a decorator finds `Kaninchen` in `Kan-` and `inchen`.
/// A part ends with `-` and the next one goes on at once in the book, on
/// another line; the hyphen is the layout's (left out) where the book has
/// none there, the book's own (kept) where it has.
fn whole_words(words: &[WordBox]) -> Vec<Option<String>> {
    let mut whole: Vec<Option<String>> = alloc::vec![None; words.len()];
    for (i, pair) in words.windows(2).enumerate() {
        let (first, next) = (&pair[0], &pair[1]);
        let split =
            first.text.ends_with('-') && next.start == first.end && first.area.y != next.area.y;
        if !split {
            continue;
        }
        let in_book = first.end.offset.saturating_sub(first.start.offset) as usize;
        let head = if in_book < first.text.len() {
            &first.text[..first.text.len() - 1]
        } else {
            first.text.as_str()
        };
        let word = format!("{head}{}", next.text);
        whole[i] = Some(word.clone());
        whole[i + 1] = Some(word);
    }
    whole
}

impl Book {
    /// Asks the decorators for this page's marks if the page or what their
    /// marks depend on changed since they were asked.
    pub(super) fn update_decorations(&mut self, env: &Env<'_>) {
        if env.decorators().is_empty() {
            self.decorated.clear();
            return;
        }
        let key = (self.page.start, env.decorators().signature(env));
        if self.decorated_for == Some(key) {
            return;
        }
        self.decorated = self.decorate(env);
        self.decorated_for = Some(key);
    }

    /// What the decorators mark on the page.
    fn decorate(&self, env: &Env<'_>) -> Vec<decorations::Mark> {
        let words = self.words(env);
        let whole = whole_words(words);
        let page_words: Vec<PageWord<'_>> = words
            .iter()
            .zip(&whole)
            .map(|(w, whole)| PageWord::new(whole.as_deref().unwrap_or(&w.text), w.start, w.end))
            .collect();
        let page = PageText::new(&self.path, &self.language, &page_words);
        env.decorators().decorate(env, &page)
    }

    /// The decorator's mark that takes taps at word `index`, if any.
    pub(super) fn tappable_at(&self, words: &[WordBox], index: usize) -> Option<usize> {
        let word = words.get(index)?;
        self.decorated
            .iter()
            .position(|m| m.taps && m.overlaps(word.start, word.end))
    }

    /// Tells the decorator of mark `index` that it was tapped (or chosen
    /// with the buttons).
    pub(super) fn tap_mark(&self, words: &[WordBox], index: usize, cx: &mut Cx<'_, '_>) {
        let Some(mark) = self.decorated.get(index) else {
            return;
        };
        let text: Vec<&str> = words
            .iter()
            .filter(|w| mark.overlaps(w.start, w.end))
            .map(|w| w.text.as_str())
            .collect();
        cx.command(
            events::DECORATION_TAPPED.emit(events::DecorationTapped::new(
                mark.decorator.clone(),
                mark.role.clone(),
                text.join(" "),
                self.path.clone(),
                mark.start.section,
                mark.start.offset,
                mark.end.section,
                mark.end.offset,
            )),
        );
    }

    /// Every mark of the page, by priority, as spans in their looks.
    fn spans(&self, env: &Env<'_>, aloud: Option<(usize, usize)>, words: &[WordBox]) -> Vec<Span> {
        let mut looks: Vec<(Id, Decoration)> = Vec::new();
        let mut look = |role: &Id| match looks.iter().find(|(r, _)| r == role) {
            Some((_, look)) => *look,
            None => {
                let found = Decoration::of_role(env, role);
                looks.push((role.clone(), found));
                found
            }
        };
        let mut spans = Vec::new();
        if let Some((start, end)) = self.found {
            spans.push(Span {
                start,
                end,
                look: look(&roles::SEARCH_HIT),
            });
        }
        if let Some((from, to)) = aloud
            && let (Some(first), Some(last)) = (words.get(from), words.get(to.saturating_sub(1)))
            && from < to
        {
            spans.push(Span {
                start: first.start,
                end: last.end,
                look: look(&roles::ALOUD),
            });
        }
        let (page_start, page_end) = (self.page.start, self.page.end);
        for mark in self
            .marks
            .iter()
            .filter(|m| m.kind == Kind::Highlight && m.overlaps(page_start, page_end))
        {
            spans.push(Span {
                start: mark.start,
                end: mark.end,
                look: look(&roles::HIGHLIGHT),
            });
        }
        for mark in &self.decorated {
            spans.push(Span {
                start: mark.start,
                end: mark.end,
                look: look(&mark.role),
            });
        }
        let web = env.has_slot(&slots::BROWSER);
        let section = self.page.start.section;
        for link in self
            .links
            .iter()
            .filter(|l| web || !paperpaw_sdk::document::is_external(&l.href))
        {
            spans.push(Span {
                start: Locator::new(section, link.start),
                end: Locator::new(section, link.end),
                look: look(&roles::LINK),
            });
        }
        spans
    }

    /// The marks of the page as they are drawn: words that look the same
    /// next to each other in one box per line, and each word's ink.
    /// `aloud`: the words read aloud now, if on this page.
    pub(super) fn page_marks(
        &self,
        env: &Env<'_>,
        aloud: Option<(usize, usize)>,
        words: &[WordBox],
    ) -> PageMarks {
        let spans = self.spans(env, aloud, words);
        let mut inks = Vec::with_capacity(words.len());
        let mut groups: Vec<(Decoration, Vec<Rect>)> = Vec::new();
        let looks: Vec<Decoration> = words
            .iter()
            .map(|word| {
                decorations::combine(
                    spans
                        .iter()
                        .filter(|s| s.start < word.end && word.start < s.end)
                        .map(|s| s.look),
                )
            })
            .collect();
        let mut run: Option<(Decoration, Vec<(usize, Rect)>)> = None;
        for (index, (word, &look)) in words.iter().zip(&looks).enumerate() {
            inks.push(look.ink_color());
            // A run of one look goes on with a word drawn next to the
            // run's last, or with only words of its look between (see
            // `drawn_together`).
            let together = |members: &Vec<(usize, Rect)>| {
                members.last().is_none_or(|(last, _)| {
                    crate::selection::drawn_together(words, *last, index, |i| {
                        looks.get(i) == Some(&look)
                    })
                })
            };
            match &mut run {
                Some((current, members)) if *current == look && together(members) => {
                    members.push((index, word.area));
                }
                _ => {
                    if let Some((done, members)) = run.take()
                        && !done.is_none()
                    {
                        groups.push((done, decorations::join_boxes(members)));
                    }
                    run = Some((look, Vec::from([(index, word.area)])));
                }
            }
        }
        if let Some((done, members)) = run
            && !done.is_none()
        {
            groups.push((done, decorations::join_boxes(members)));
        }
        PageMarks { groups, inks }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, y: u16, start: u32, end: u32) -> WordBox {
        WordBox::new(
            Rect::new(0, y, 10, 10),
            text,
            Locator::new(0, start),
            Locator::new(0, end),
        )
    }

    #[test]
    fn words_split_over_lines_are_whole_for_decorators() {
        let words = [
            // `Kan-` and `inchen`: the hyphen is the layout's.
            word("Kan-", 0, 10, 13),
            word("inchen", 20, 13, 19),
            // `waistcoat-` and `pocket`: the book's own hyphen.
            word("waistcoat-", 20, 20, 30),
            word("pocket", 40, 30, 36),
            // On one line, or with a space between: two words.
            word("well-", 40, 37, 42),
            word("known", 40, 42, 47),
            word("end-", 40, 48, 52),
            word("start", 60, 53, 58),
        ];
        let whole = whole_words(&words);
        assert_eq!(whole[0].as_deref(), Some("Kaninchen"));
        assert_eq!(whole[1].as_deref(), Some("Kaninchen"));
        assert_eq!(whole[2].as_deref(), Some("waistcoat-pocket"));
        assert_eq!(whole[3].as_deref(), Some("waistcoat-pocket"));
        assert!(whole[4..].iter().all(Option::is_none));
    }
}

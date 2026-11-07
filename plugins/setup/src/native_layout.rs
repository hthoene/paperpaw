//! Where things are on a page of the guide: the dots, the question and its
//! explanation, the answer's card and the two buttons. Pure geometry over a
//! way of measuring text, so it is tested without the host.
use paperpaw_sdk::Rect;

/// Lines of the explanation at most; at the end, with no card, more.
pub const HELP_LINES: usize = 6;
pub const END_LINES: usize = 12;

/// The sizes the page is laid out in, in pixels.
#[derive(Debug, Clone, Copy)]
pub struct Sizes {
    pub width: u16,
    pub height: u16,
    pub margin: u16,
    pub gap: u16,
    /// A button's height, roomy and close.
    pub row: u16,
    pub row_small: u16,
    pub title: f32,
    pub small: f32,
}

/// How text is measured: the host's font in the plugin, a fixed one in tests.
pub trait Measure {
    /// The height of a line at `size`.
    fn line(&self, size: f32) -> u16;
    /// The lines `text` wraps to in `width`, at most `max`, at least one.
    fn lines(&self, size: f32, text: &str, width: u16, max: usize) -> usize;
    /// The height of the sample text at `size`, wrapped to `width`.
    fn sample(&self, size: f32, width: u16) -> u16;
}

/// What a page asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Answer {
    /// The last page: no card.
    None,
    /// A row: a value, a switch or an editor.
    Row,
    /// Two options side by side.
    Two,
    /// A number: − value +, and below it a sample of the text at that size.
    Number { sample: Option<f32> },
}

/// The page's texts and answer.
#[derive(Debug, Clone, Copy)]
pub struct Content<'a> {
    pub title: &'a str,
    pub help: &'a str,
    pub help_lines: usize,
    pub answer: Answer,
}

/// Where things are on a page.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub dots: Rect,
    pub title: Rect,
    pub help: Rect,
    pub card: Option<Rect>,
    /// − and + of a number, in the card.
    pub stepper: Option<(Rect, Rect)>,
    /// The two options of a choice of two, side by side in the card.
    pub segments: Option<(Rect, Rect)>,
    pub next: Rect,
    pub other: Rect,
    /// Laid out close, where the page did not fit with its roomy margins.
    pub tight: bool,
    /// Whether the page fits at all.
    pub fits: bool,
}

/// The page laid out roomy, or close where it would not fit so.
#[must_use]
pub fn layout(sizes: &Sizes, m: &impl Measure, content: &Content<'_>) -> Layout {
    let roomy = layout_with(sizes, m, content, false);
    if roomy.fits {
        roomy
    } else {
        layout_with(sizes, m, content, true)
    }
}

fn card_height(
    sizes: &Sizes,
    m: &impl Measure,
    answer: Answer,
    width: u16,
    button: u16,
) -> Option<u16> {
    let gap = sizes.gap;
    match answer {
        Answer::None => None,
        Answer::Row | Answer::Two => Some(button.saturating_add(gap.saturating_mul(2))),
        Answer::Number { sample } => {
            let below = match sample {
                Some(size) => m.sample(size, width.saturating_sub(2 * sizes.margin)),
                None => m.line(sizes.small) * 2,
            };
            Some(
                button
                    .saturating_add(gap.saturating_mul(4))
                    .saturating_add(below),
            )
        }
    }
}

fn layout_with(sizes: &Sizes, m: &impl Measure, content: &Content<'_>, tight: bool) -> Layout {
    let (w, h, gap) = (sizes.width, sizes.height, sizes.gap);
    let margin = if tight {
        sizes.margin
    } else {
        sizes.margin.saturating_add(gap)
    };
    let (after_dots, after_help) = if tight { (2, 1) } else { (4, 3) };
    let width = w.saturating_sub(margin.saturating_mul(2));
    let text_height = |size: f32, text: &str, max: usize| -> u16 {
        m.line(size).saturating_mul(
            u16::try_from(m.lines(size, text, width, max).clamp(1, max)).unwrap_or(u16::MAX),
        )
    };
    let mut y = margin;
    let dots = Rect::new(margin, y, width, gap);
    y = y
        .saturating_add(dots.height)
        .saturating_add(gap.saturating_mul(after_dots));
    let title = Rect::new(
        margin,
        y,
        width,
        text_height(sizes.title * 1.3, content.title, 2),
    );
    y = y.saturating_add(title.height).saturating_add(gap);
    let help = Rect::new(
        margin,
        y,
        width,
        text_height(sizes.small, content.help, content.help_lines),
    );
    y = y
        .saturating_add(help.height)
        .saturating_add(gap.saturating_mul(after_help));
    let wanted = |button: u16| card_height(sizes, m, content.answer, width, button);
    // The buttons a row high, one above the other at the bottom; close, as
    // high as buttons are elsewhere and side by side where one above the other
    // does not fit.
    let fits = |button: u16, beside: bool| {
        let rows = if beside { 1 } else { 2 };
        let used = margin
            .saturating_add(rows.saturating_mul(button))
            .saturating_add((rows - 1).saturating_mul(gap).saturating_mul(2));
        let top = h.saturating_sub(used);
        u32::from(y) + u32::from(wanted(button).unwrap_or(0)) + u32::from(gap) <= u32::from(top)
    };
    let small = sizes.row_small;
    let (button, beside) = if tight {
        (small, !fits(small, false))
    } else {
        (sizes.row, false)
    };
    let fit = fits(button, beside);
    let bottom = h.saturating_sub(margin.saturating_add(button));
    let (next, other) = if beside {
        let half = width.saturating_sub(gap) / 2;
        (
            Rect::new(
                margin.saturating_add(half).saturating_add(gap),
                bottom,
                width.saturating_sub(half).saturating_sub(gap),
                button,
            ),
            Rect::new(margin, bottom, half, button),
        )
    } else {
        let above = bottom.saturating_sub(gap.saturating_mul(2).saturating_add(button));
        (
            Rect::new(margin, above, width, button),
            Rect::new(margin, bottom, width, button),
        )
    };
    let (card, stepper, segments) = match wanted(button) {
        None => (None, None, None),
        Some(height) => {
            let room = next.y.saturating_sub(y.saturating_add(gap));
            let card = Rect::new(margin, y, width, height.min(room));
            let stepper = matches!(content.answer, Answer::Number { .. }).then(|| {
                let top = card.y.saturating_add(gap);
                let inset = sizes.margin;
                (
                    Rect::new(card.x.saturating_add(inset), top, button, button),
                    Rect::new(
                        (card.x.saturating_add(card.width))
                            .saturating_sub(inset)
                            .saturating_sub(button),
                        top,
                        button,
                        button,
                    ),
                )
            });
            let segments = matches!(content.answer, Answer::Two).then(|| {
                let inset = if tight { 0 } else { sizes.margin / 2 };
                let above = inset.min(gap).min(card.height / 6);
                let row = Rect::new(
                    card.x.saturating_add(inset),
                    card.y.saturating_add(above),
                    card.width.saturating_sub(inset.saturating_mul(2)),
                    card.height.saturating_sub(above.saturating_mul(2)),
                );
                let space = inset.max(gap / 2);
                let each = row.width.saturating_sub(space) / 2;
                (
                    Rect::new(row.x, row.y, each, row.height),
                    Rect::new(
                        row.x.saturating_add(each).saturating_add(space),
                        row.y,
                        each,
                        row.height,
                    ),
                )
            });
            (Some(card), stepper, segments)
        }
    };
    Layout {
        dots,
        title,
        help,
        card,
        stepper,
        segments,
        next,
        other,
        tight,
        fits: fit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A font of fixed widths: a character is 8 px at size 16 and scales.
    struct Fixed;
    impl Measure for Fixed {
        fn line(&self, size: f32) -> u16 {
            (size * 1.3) as u16
        }
        fn lines(&self, size: f32, text: &str, width: u16, max: usize) -> usize {
            let chars = (f32::from(width) / (size / 2.0)) as usize;
            text.len().div_ceil(chars.max(1)).clamp(1, max)
        }
        fn sample(&self, size: f32, _: u16) -> u16 {
            (size * 1.3) as u16 * 2
        }
    }

    fn sizes(w: u16, h: u16) -> Sizes {
        Sizes {
            width: w,
            height: h,
            margin: 16,
            gap: 8,
            row: 56,
            row_small: 40,
            title: 24.0,
            small: 16.0,
        }
    }

    fn content(answer: Answer) -> Content<'static> {
        Content {
            title: "Language",
            help: "The language of the menus and messages.",
            help_lines: HELP_LINES,
            answer,
        }
    }

    #[test]
    fn a_roomy_page_has_its_parts_in_order_inside_the_screen() {
        let l = layout(&sizes(480, 800), &Fixed, &content(Answer::Row));
        assert!(l.fits && !l.tight);
        let card = l.card.unwrap();
        assert!(l.title.y > l.dots.y && l.help.y > l.title.y && card.y > l.help.y);
        assert!(card.y + card.height < l.next.y);
        assert!(l.next.y + l.next.height < l.other.y);
        for r in [l.dots, l.title, l.help, card, l.next, l.other] {
            assert!(r.x + r.width <= 480 && r.y + r.height <= 800, "{r:?}");
        }
    }

    #[test]
    fn a_short_screen_lays_the_page_out_close_with_the_buttons_side_by_side() {
        let l = layout(
            &sizes(480, 330),
            &Fixed,
            &content(Answer::Number { sample: Some(40.0) }),
        );
        assert!(l.tight);
        assert_eq!(l.next.y, l.other.y, "side by side");
        assert!(l.next.x > l.other.x, "Next at the right");
    }

    #[test]
    fn the_last_page_has_no_card() {
        let l = layout(&sizes(480, 800), &Fixed, &content(Answer::None));
        assert!(l.card.is_none() && l.stepper.is_none() && l.segments.is_none());
    }

    #[test]
    fn a_number_has_less_and_more_at_the_sides_of_its_card() {
        let l = layout(
            &sizes(480, 800),
            &Fixed,
            &content(Answer::Number { sample: None }),
        );
        let (less, more) = l.stepper.unwrap();
        let card = l.card.unwrap();
        assert!(less.x < more.x && less.x >= card.x && more.x + more.width <= card.x + card.width);
    }

    #[test]
    fn two_options_share_the_card_without_overlapping() {
        let l = layout(&sizes(480, 800), &Fixed, &content(Answer::Two));
        let (a, b) = l.segments.unwrap();
        assert!(a.x + a.width <= b.x);
        let card = l.card.unwrap();
        assert!(b.x + b.width <= card.x + card.width);
    }

    #[test]
    fn nothing_overflows_on_tiny_screens() {
        for (w, h) in [(0, 0), (1, 1), (10, 10), (100, 60)] {
            let _ = layout(
                &sizes(w, h),
                &Fixed,
                &content(Answer::Number {
                    sample: Some(300.0),
                }),
            );
        }
    }
}

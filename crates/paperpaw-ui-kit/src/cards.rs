//! Beta 3's list-section cards: open at a page cut, with the focus inside.
use crate::{
    Paint, Style,
    items::{Item, carried_heading},
    paint, style,
};
use alloc::vec::Vec;
use core::ops::Range;
use paperpaw_sdk::{
    Color, Rect,
    native::{
        Error, Host,
        callbacks::FrameRect,
        settings::{self, SettingValue},
        snapshot::Snapshot,
    },
};

#[derive(Clone, Copy)]
pub(crate) struct Cards {
    fill: Option<Color>,
}

impl Cards {
    pub(crate) fn read(host: &Host<'_>, display: &Snapshot) -> Result<Option<Self>, Error> {
        let mut bytes = [0; 64];
        if !matches!(
            settings::read_setting(host, "core.ui-groups", &mut bytes)?,
            SettingValue::Text("core.groups.cards")
        ) {
            return Ok(None);
        }
        let fill = if display
            .display_gray_levels()
            .is_none_or(|grays| grays >= 16)
        {
            let palette = match style::setting_text(host, "core.ui-palette") {
                Err(Error::NoSpace) => alloc::string::String::new(),
                other => other?,
            };
            Some(style::palette_color(&palette, "section", Color::gray(238)))
        } else {
            None
        };
        Ok(Some(Self { fill }))
    }

    pub(crate) fn run(
        self,
        ops: &mut Vec<Paint>,
        s: &Style,
        shown: Rect,
        open: (bool, bool),
    ) -> Result<(), Error> {
        let side = (s.margin / 2).min(shown.width / 8);
        let clip = Rect::new(
            shown.x.saturating_add(side),
            shown.y,
            shown.width.saturating_sub(2 * side),
            shown.height,
        );
        let reach = s
            .corner
            .saturating_add(u32::from(s.line))
            .saturating_add(1)
            .min(u16::MAX.into()) as i32;
        let y = i32::from(shown.y) - if open.0 { reach } else { 0 };
        let end = i32::from(shown.y) + i32::from(shown.height) + if open.1 { reach } else { 0 };
        let rect = FrameRect::new(
            i32::from(clip.x),
            y,
            u32::from(clip.width),
            (end - y) as u32,
        );
        paint::push(
            ops,
            Paint::Section {
                rect,
                clip,
                radius: s.corner,
                thickness: self
                    .fill
                    .is_none()
                    .then_some(u32::from((s.line / 2).max(1))),
                color: self.fill.unwrap_or(Color::BLACK),
            },
        )
    }
}

/// Every visible run between headings. The previous/next items decide whether
/// a page cuts a card, exactly as Beta 3's Rows did.
pub(crate) fn page(
    ops: &mut Vec<Paint>,
    s: &Style,
    cards: Option<Cards>,
    model: (&[Item], &[u16]),
    range: Range<usize>,
    area: Rect,
) -> Result<(), Error> {
    let Some(cards) = cards else {
        return Ok(());
    };
    let (items, heights) = model;
    if items.len() != heights.len() || range.start > range.end || range.end > items.len() {
        return Err(Error::Invalid);
    }
    let mut at = range.start;
    let mut y = area.y;
    while at < range.end {
        let start = at;
        if matches!(items[at], Item::Heading(_)) {
            y = y.saturating_add(heights[at]);
            at += 1;
            continue;
        }
        let top = y;
        while at < range.end && matches!(items[at], Item::Row(_)) {
            y = y.saturating_add(heights[at]);
            at += 1;
        }
        cards.run(
            ops,
            s,
            Rect::new(area.x, top, area.width, y.saturating_sub(top)),
            (
                // Open above only where the card is the first thing on the
                // page; the heading a page carries over closes it, as Beta 3.
                start > 0
                    && matches!(items[start - 1], Item::Row(_))
                    && carried_heading(items, start).is_none(),
                matches!(items.get(at), Some(Item::Row(_))),
            ),
        )?;
    }
    Ok(())
}

/// A card's focus is inset from its frame, with the same text margin.
pub(crate) fn backdrop(
    ops: &mut Vec<Paint>,
    s: &Style,
    rect: Rect,
    focused: bool,
    more: bool,
) -> Result<(), Error> {
    if focused {
        let inset = (s.margin / 6)
            .max(s.line.saturating_add(1))
            .min(s.margin / 2);
        let edge = s.line.min(rect.height / 8);
        let rect = rect.inset(2 * inset, inset.saturating_sub(edge).min(rect.height / 4));
        // Equivalent to Beta 3's draw_focus with the remaining margin.
        let margin = s.margin.saturating_sub(2 * inset);
        let rect = rect.inset(
            (margin / 2).min(rect.width / 8),
            s.line.min(rect.height / 8),
        );
        let radius = if rect.width < rect.height || !s.pill || rect.height > s.row {
            s.corner.saturating_sub(u32::from(inset))
        } else {
            u32::from(rect.height) / 2
        };
        paint::push(
            ops,
            Paint::Focus(
                rect,
                radius,
                s.ring.then_some((u32::from(s.line) * 2).max(3)),
                s.bar(),
            ),
        )
    } else if more {
        crate::list::backdrop(ops, s, rect, false)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn section_cards_are_open_only_at_page_cuts() {
        let mut s = Style::themed(Color::BLACK, Color::WHITE);
        s.margin = 12;
        s.corner = 4;
        s.line = 2;
        let items = [
            Item::heading("Books").unwrap(),
            Item::row("A", "").unwrap(),
            Item::row("B", "").unwrap(),
            Item::row("C", "").unwrap(),
            Item::heading("Device").unwrap(),
            Item::row("D", "").unwrap(),
        ];
        let heights = [10, 20, 20, 20, 10, 20];
        let mut ops = Vec::new();
        page(
            &mut ops,
            &s,
            Some(Cards { fill: None }),
            (&items, &heights),
            0..3,
            Rect::new(0, 0, 100, 50),
        )
        .unwrap();
        assert!(
            matches!(ops.as_slice(), [Paint::Section {rect, clip, thickness: Some(1), ..}] if *rect == FrameRect::new(6,10,88,47) && *clip == Rect::new(6,10,88,40))
        );
        ops.clear();
        page(
            &mut ops,
            &s,
            Some(Cards { fill: None }),
            (&items, &heights),
            3..6,
            Rect::new(0, 0, 100, 50),
        )
        .unwrap();
        // The page carries the heading "Books" over above "C": the card is
        // closed under it, as Beta 3 drew it.
        assert!(
            matches!(ops.first(), Some(Paint::Section {rect, clip, ..}) if *rect == FrameRect::new(6,0,88,20) && *clip == Rect::new(6,0,88,20))
        );
        assert!(
            matches!(ops.get(1), Some(Paint::Section {rect, ..}) if *rect == FrameRect::new(6,30,88,20))
        );
        ops.clear();
        // Without a heading to carry over, the card the page cuts is open
        // above.
        page(
            &mut ops,
            &s,
            Some(Cards { fill: None }),
            (&items[1..4], &heights[1..4]),
            2..3,
            Rect::new(0, 0, 100, 50),
        )
        .unwrap();
        assert!(
            matches!(ops.as_slice(), [Paint::Section {rect, clip, ..}] if *rect == FrameRect::new(6,-7,88,27) && *clip == Rect::new(6,0,88,20))
        );
        ops.clear();
        page(
            &mut ops,
            &s,
            None,
            (&items, &heights),
            0..3,
            Rect::new(0, 0, 100, 50),
        )
        .unwrap();
        assert!(ops.is_empty(), "default list drawing stays unchanged");
    }
}

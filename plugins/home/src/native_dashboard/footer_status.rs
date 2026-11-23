//! The footer's status items (`home.footer.items`, *More below the page*): a
//! status line at the footer's left, where *Add* goes while editing, in the
//! note ink, as Beta 3's `draw_footer` drew them. The line itself is
//! `paperpaw_ui_kit::status_line`, which the status line widget draws with.
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;
use paperpaw_sdk::{
    Rect,
    native::{Error, Host, callbacks::DrawFrame, fonts::Font, ui::Ui},
};
use paperpaw_ui_kit::{
    Style, paint,
    status_line::{self, Face, Lists},
    style::setting_text,
    text::copy,
};

use super::{Dashboard, Laid};

/// The setting that lists the items.
const ITEMS: &str = "home.footer.items";

/// What the footer shows at its left: the items listed, read when the page is
/// laid out, and what they showed at the last minute.
#[derive(Debug, Default)]
pub(super) struct FooterStatus {
    items: String,
    shown: Cell<Option<u64>>,
}

impl FooterStatus {
    /// The items the setting lists now; none where it cannot be read (the
    /// footer then shows none, and nothing is written).
    pub(super) fn read(host: &Host<'_>) -> Self {
        Self {
            items: setting_text(host, ITEMS).unwrap_or_default(),
            shown: Cell::new(None),
        }
    }

    fn lists(&self) -> Result<Lists, Error> {
        Ok(Lists {
            left: copy(&self.items)?,
            center: String::new(),
            right: String::new(),
            line: String::new(),
        })
    }

    /// Whether the minute changed what the items show (the first minute after
    /// the page was laid out counts as changed); one that cannot tell waits.
    pub(super) fn ticked(&self, host: &Host<'_>, ui: &Ui) -> bool {
        if self.items.is_empty() {
            return false;
        }
        let Ok(now) = self.lists().and_then(|lists| status_line::shows(host, &lists, ui)) else {
            return false;
        };
        self.shown.replace(Some(now)) != Some(now)
    }
}

impl Laid {
    /// Where the footer's status items go: its left half, short of the dots in
    /// the middle (Beta 3's place).
    pub(super) fn footer_status_area(&self, style: &Style) -> Rect {
        let footer = self.footer_area();
        let width = (footer.width / 2).saturating_sub(style.margin + style.gap * 3);
        Rect::new(
            footer.x.saturating_add(style.margin),
            footer.y,
            width,
            footer.height,
        )
    }
}

impl Dashboard {
    /// Draws the footer's status items where they lie in the band of `frame`
    /// (not while editing: *Add* is there). The values their owners draw are
    /// composed in; a host that does not answer the status calls leaves the
    /// place empty.
    ///
    /// # Errors
    /// Font and host refusals, [`Error::NoMemory`].
    pub(super) fn draw_footer_status(
        &self,
        host: &Host<'_>,
        ui: &Ui,
        frame: &mut DrawFrame<'_>,
        (font, style, laid): (&Font<'_>, &Style, &Laid),
    ) -> Result<(), Error> {
        if self.editing || laid.footer_status.items.is_empty() {
            return Ok(());
        }
        let area = laid.footer_status_area(style);
        let (top, bottom) = (i32::from(area.y), i32::from(area.y) + i32::from(area.height));
        let band = (
            frame.clip_top.saturating_sub(frame.rect.y),
            frame.clip_bottom.saturating_sub(frame.rect.y),
        );
        if bottom <= band.0 || top >= band.1 {
            return Ok(());
        }
        let lists = laid.footer_status.lists()?;
        let face = Face {
            host,
            font,
            small: style.small,
        };
        let (mut out, mut placed, mut buffers) = (Vec::new(), Vec::new(), [Vec::new(), Vec::new()]);
        let drawn = status_line::draw(
            (&face, &lists),
            ui,
            &mut out,
            &mut placed,
            &mut buffers,
            (area, style.note),
        );
        match drawn {
            Err(Error::Unsupported) => return Ok(()),
            other => other?,
        }
        paint::replay(frame, font, out.iter())?;
        for item in &placed {
            // An owner that cannot draw now (busy, gone) leaves its place empty.
            Ui::compose(host, item.target, item.rect).ok();
        }
        Ok(())
    }
}

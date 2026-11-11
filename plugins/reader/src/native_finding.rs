//! The "Finding the chapters" screen's text and model (R6b): what the table of
//! contents shows while the headings scan ([`crate::native_headings`]) runs, as
//! Beta 3's `toc.rs` showed `reader.toc.finding`. Here it also says how far the
//! scan is, in steps of [`STEP`] percent so that a slow e-paper screen is not
//! drawn again for every block read. The caller (the table-of-contents screen)
//! draws it in place of the list and asks for a redraw when [`Finding::update`]
//! says the shown number moved.
use alloc::string::String;
use paperpaw_sdk::native::{Error, Host, callbacks::DrawFrame, formatting::Arg};
use paperpaw_ui_kit::{Footer, ItemList, Messages};

/// The shown percentage moves in steps of this many.
pub const STEP: u32 = 5;

/// The screen's key of its title.
pub const TITLE: &str = "reader.toc";
/// The key of the text before the scan has a number to show.
pub const FINDING: &str = "reader.toc.finding";
/// The key of the text with the percentage.
pub const FINDING_PERCENT: &str = "reader.toc.finding-percent";

/// What the screen shows while the headings are found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Finding {
    /// The percentage shown, a multiple of [`STEP`]; `None` before the first
    /// step of the scan is told.
    shown: Option<u32>,
}

impl Finding {
    /// A screen that has not been told how far the scan is.
    #[must_use]
    pub const fn new() -> Self {
        Self { shown: None }
    }

    /// Takes the scan's `percent` (0 to 100); `true` when the number shown
    /// moved and the screen is to be drawn again.
    pub const fn update(&mut self, percent: u32) -> bool {
        let shown = if percent >= 100 {
            100
        } else {
            percent / STEP * STEP
        };
        let moved = match self.shown {
            Some(was) => was != shown,
            None => true,
        };
        self.shown = Some(shown);
        moved
    }

    /// The percentage shown, if one is.
    #[must_use]
    pub const fn shown(&self) -> Option<u32> {
        self.shown
    }

    /// The key of the text to show: with the percentage once there is one.
    #[must_use]
    pub const fn key(&self) -> &'static str {
        if self.shown.is_some() {
            FINDING_PERCENT
        } else {
            FINDING
        }
    }

    /// The text to show.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals.
    pub fn text(&self, host: &Host<'_>, messages: &mut Messages) -> Result<String, Error> {
        match self.shown {
            Some(percent) => messages.format(
                host,
                FINDING_PERCENT,
                &[Arg::Int {
                    name: "percent",
                    value: i64::from(percent),
                }],
            ),
            None => messages.copy(host, FINDING),
        }
    }

    /// Draws the screen: the title and the text where the list would be.
    ///
    /// # Errors
    /// Allocation, font and drawing errors.
    pub fn draw(&self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let title = messages.copy(host, TITLE)?;
        let text = self.text(host, &mut messages)?;
        ItemList::default().draw_with(
            host,
            frame,
            &title,
            true,
            Footer::Empty {
                title: &text,
                text: "",
            },
        )
    }
}

#[cfg(test)]
mod tests;

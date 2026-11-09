//! What the picture home screen shows now, read from the device: the settings
//! and registry snapshot, the metrics of the style on this display, the frame of
//! the screen as it is and the pages. The dashboard lays them out.
use alloc::vec::Vec;
use paperpaw_sdk::native::{Error, Host, fonts::Font, snapshot::Snapshot};
use paperpaw_ui_kit::Style;

use crate::native_dash::{self as dash, Frame, Metrics};
use crate::native_pages::{self, Page};
use crate::native_picture::Visible;
use crate::native_source::HostSource;

/// The setting of the gap between widgets.
const GAP: &str = "core.widgets.gap";

/// The picture home screen as it shows now.
#[derive(Debug)]
#[non_exhaustive]
pub struct Shown {
    /// What is read of the device.
    pub src: HostSource,
    /// The style's metrics on this display.
    pub metrics: Metrics,
    /// Whether the screen is on its side.
    pub wide: bool,
    /// The bar's strip, the grid and the footer.
    pub frame: Frame,
    /// Every page, as kept (a screen on its side places the items anew).
    pub pages: Vec<Page>,
}

impl Shown {
    /// Reads the snapshot and lays the frame out.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    pub fn read(host: &Host<'_>, display: &Snapshot, font: &Font<'_>) -> Result<Self, Error> {
        let style = Style::read(host, display, font)?;
        let src = HostSource::read(host)?;
        let metrics = metrics(host, display, &style, font)?;
        let wide = dash::landscape(&metrics);
        // The strip is kept free, shown or not, so that the page does not move
        // when the bar comes.
        let strip = (src.bar != Visible::Off).then_some(src.bar_bottom);
        let frame = dash::frame(&metrics, strip, src.columns(), src.rows);
        let pages = native_pages::pages(&src)?;
        Ok(Self {
            src,
            metrics,
            wide,
            frame,
            pages,
        })
    }

    /// Reads the device now, with the font and display of the callback.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    pub fn current(host: &Host<'_>) -> Result<Self, Error> {
        let display = paperpaw_sdk::native::snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, paperpaw_sdk::native::fonts::FontRole::Ui)?;
        let read = Self::read(host, &display, &font);
        let released = font.release();
        let shown = read?;
        released?;
        Ok(shown)
    }
}

/// The metrics of the style on this display.
fn metrics(
    host: &Host<'_>,
    display: &Snapshot,
    style: &Style,
    font: &Font<'_>,
) -> Result<Metrics, Error> {
    let gap = match paperpaw_ui_kit::style::setting_text(host, GAP)?.as_str() {
        "core.gap.small" => style.gap / 2,
        "core.gap.large" => style.gap.saturating_mul(2),
        _ => style.gap,
    };
    let line = paperpaw_ui_kit::geometry::line_px(font, style.small)?;
    Ok(Metrics {
        width: display.display_width(),
        height: display.display_height(),
        margin: style.margin,
        row: style.row,
        gap,
        style_gap: style.gap,
        // A line of small text with room round it, as interface 6 had the bar.
        bar: line.saturating_add(style.gap),
    })
}

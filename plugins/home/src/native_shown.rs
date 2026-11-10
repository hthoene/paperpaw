//! What the home screen shows now, read from the device: the settings and
//! registry snapshot, the metrics of the style on this display, the grid of
//! the screen as it is and the pages in it. The dashboard lays them out; the
//! *Add* screen changes them.
use alloc::vec::Vec;
use paperpaw_sdk::{
    native::{Error, Host, fonts::Font, snapshot::Snapshot},
    widgets::grid::Geometry,
};
use paperpaw_ui_kit::Style;

use crate::native_dash::{self as dash, Frame, Metrics};
use crate::native_pages::{self, Page, Source};
use crate::native_source::HostSource;

/// The setting of the gap between widgets.
const GAP: &str = "core.widgets.gap";

/// The home screen as it shows now.
#[derive(Debug)]
pub struct Shown {
    /// What is read of the device.
    pub src: HostSource,
    /// The style's metrics on this display.
    pub metrics: Metrics,
    /// Whether the screen is on its side.
    pub wide: bool,
    /// The grid's columns upright.
    pub columns: u8,
    /// The status line, grid and footer.
    pub frame: Frame,
    /// Every page, in the layout shown.
    pub pages: Vec<Page>,
}

impl Shown {
    /// Reads the snapshot and lays the frame out.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    pub fn read(host: &Host<'_>, display: &Snapshot, font: &Font<'_>) -> Result<Self, Error> {
        let style = Style::read(host, display, font)?;
        let src = HostSource::read(host, display.display_dpi())?;
        let metrics = metrics(host, display, &style, font)?;
        let wide = dash::landscape(&metrics);
        let columns = native_pages::columns_upright(&src);
        let frame = dash::frame(&metrics, wide, src.status_bar(), columns, src.rows);
        let pages = if wide {
            native_pages::pages_wide(&src, &frame.geometry)?
        } else {
            native_pages::pages(&src)?
        };
        Ok(Self {
            src,
            metrics,
            wide,
            columns,
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

    /// The grid of a screen on its side.
    #[must_use]
    pub fn wide_grid(&self) -> Geometry {
        self.grid(true)
    }

    /// The grid of a screen on its side (`wide`) or one upright, whichever the
    /// screen is now.
    #[must_use]
    pub fn grid(&self, wide: bool) -> Geometry {
        dash::frame(
            &self.metrics,
            wide,
            self.src.status_bar(),
            self.columns,
            self.src.rows,
        )
        .geometry
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
        status: line.saturating_add(style.gap / 2),
    })
}

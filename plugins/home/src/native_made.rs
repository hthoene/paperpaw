//! A widget as a screen of home shows it. Another plugin's widget is an
//! instance the host made, which the host measures, places, composes into the
//! screen and forwards taps to. A widget of this plugin's own (the date, the
//! clock, the card, the shelf, the status line) has no instance: the host never
//! calls a plugin back from its own draw (`ERR_BUSY`, `docs/abi/ui.md` 5.1), so
//! the screen asks the widget directly ([`Mine`]) and draws it into its own
//! frame. One type for both, so the dashboard and the *Add* gallery treat them
//! alike.
use alloc::vec::Vec;
use paperpaw_sdk::{
    Point, Rect,
    native::{
        Error, Host,
        callbacks::DrawFrame,
        ui::{Compose, MadeWidget, Ui, WidgetHandle, WidgetInputKind},
    },
    widgets::grid::Geometry,
};
use paperpaw_ui_kit::{
    geometry::clamp_u16,
    mem::{SERVICE_BUFFER, buffer, push},
    paint,
};

use crate::native_measure as measure;
use crate::native_widgets::Mine;

/// A widget made for a screen of home.
#[derive(Debug)]
pub enum Made {
    /// Another plugin's: an instance of the host's.
    Host(WidgetHandle),
    /// This plugin's own.
    Mine(Mine),
}

impl Made {
    /// Makes widget `id` for the screen on top, at `place`. One of this
    /// plugin's own is made by the plugin (the host makes nothing for it).
    ///
    /// # Errors
    /// The host's refusals (gone, not for this screen, full) and
    /// [`Error::NoMemory`].
    pub fn make(host: &Host<'_>, ui: &Ui, id: &str, place: &str) -> Result<Self, Error> {
        if let Some(mine) = Mine::new(id)? {
            return Ok(Self::Mine(mine));
        }
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        match ui.widget_make(host, id, place, &mut input, &mut out)? {
            MadeWidget::Widget(widget) => Ok(Self::Host(widget)),
            // The host says it is this plugin's, but home has no such widget
            // of its own to draw: not shown, as a widget that is gone.
            _ => Err(Error::NotFound),
        }
    }

    /// Tells it where it sits (before the first draw there and when that
    /// changes).
    ///
    /// # Errors
    /// The host's refusals and [`Error::NoMemory`].
    pub fn place(&self, host: &Host<'_>, ui: &Ui, rect: Rect) -> Result<(), Error> {
        match self {
            Self::Host(widget) => {
                let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
                ui.widget_place(host, *widget, paint::frame_rect(rect), &mut input, &mut out)
            }
            Self::Mine(mine) => {
                mine.place(rect);
                Ok(())
            }
        }
    }

    /// Its rows and its fewest rows at the grid's width.
    ///
    /// # Errors
    /// Allocation and the host's refusals.
    pub fn measure(
        &self,
        host: &Host<'_>,
        ui: &Ui,
        geometry: &Geometry,
    ) -> Result<(u8, u8), Error> {
        let (height, least) = match self {
            Self::Host(widget) => {
                let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
                let size = ui.widget_measure(
                    host,
                    *widget,
                    measure::grid_width(geometry),
                    &mut input,
                    &mut out,
                )?;
                (size.height(), size.min_height())
            }
            Self::Mine(mine) => {
                let (height, least) = mine.measure(host, measure::grid_width(geometry))?;
                (u32::from(height), u32::from(least))
            }
        };
        Ok(measure::rows_of(geometry, height, least))
    }

    /// Draws it into `rect` of the screen's `frame`: the host composes another
    /// plugin's; this plugin's own draws itself.
    ///
    /// # Errors
    /// What the draw or the host refuses.
    pub fn draw(
        &self,
        host: &Host<'_>,
        ui: &Ui,
        frame: &mut DrawFrame<'_>,
        rect: Rect,
    ) -> Result<(), Error> {
        match self {
            Self::Host(widget) => {
                Ui::compose(host, Compose::Widget(*widget), paint::frame_rect(rect))
            }
            Self::Mine(mine) => mine.draw(host, ui, frame, rect),
        }
    }

    /// What a tap can reach in `rect`, in reading order: the target's number and
    /// where it is.
    ///
    /// # Errors
    /// The host's refusals and [`Error::NoMemory`].
    pub fn targets(&self, host: &Host<'_>, ui: &Ui, rect: Rect) -> Result<Vec<(u32, Rect)>, Error> {
        let mut found = Vec::new();
        match self {
            Self::Host(widget) => {
                let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
                let page = ui.widget_targets(
                    host,
                    *widget,
                    paint::frame_rect(rect),
                    &mut input,
                    &mut out,
                )?;
                for target in page {
                    let target = target?;
                    push(&mut found, (target.target(), from_frame(target.rect())))?;
                }
            }
            Self::Mine(mine) => {
                for target in mine.targets(host, ui, rect)? {
                    push(&mut found, (target.target(), from_frame(target.rect())))?;
                }
            }
        }
        Ok(found)
    }

    /// A tap or long press on one of its targets (0: the widget as a whole).
    ///
    /// # Errors
    /// The host's refusals and [`Error::NoMemory`].
    pub fn input(
        &self,
        host: &Host<'_>,
        ui: &Ui,
        target: u32,
        at: Point,
        kind: WidgetInputKind,
    ) -> Result<(), Error> {
        match self {
            Self::Host(widget) => {
                let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
                ui.widget_input(host, *widget, target, at, kind, &mut input, &mut out)
            }
            Self::Mine(mine) => mine.tap(host, ui, target, kind).map(drop),
        }
    }

    /// Lets it go; a refusal is of no matter (the host drops them when the
    /// screen closes).
    pub fn release(self, host: &Host<'_>, ui: &Ui) {
        if let Self::Host(widget) = self
            && let (Ok(mut input), Ok(mut out)) = (buffer(SERVICE_BUFFER), buffer(64))
        {
            let _ = ui.widget_drop(host, widget, &mut input, &mut out);
        }
    }
}

/// A rectangle of the host's wire as the kit's.
fn from_frame(rect: paperpaw_sdk::native::callbacks::FrameRect) -> Rect {
    Rect::new(
        clamp_u16(i64::from(rect.x)),
        clamp_u16(i64::from(rect.y)),
        clamp_u16(i64::from(rect.width)),
        clamp_u16(i64::from(rect.height)),
    )
}

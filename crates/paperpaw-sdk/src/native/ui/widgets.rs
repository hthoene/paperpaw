//! Opaque composition handles and the widget service operations.
use super::{Error, Host, Instance, Ui, UiPage, expect, one, schema, wire};
use crate::native::callbacks::FrameRect as Rect;

/// A widget instance made by the host for another plugin's widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WidgetHandle {
    instance: Instance,
}
impl WidgetHandle {
    /// The instance used by its widget_made/dropped events.
    #[must_use]
    pub const fn instance(self) -> Instance {
        self.instance
    }
}

/// The answer to making a widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MadeWidget {
    /// The widget belongs to the caller, which measures, places and draws it
    /// itself: the host never re-enters the caller, so there is no instance.
    Own,
    /// A host-made instance of another plugin's widget.
    Widget(WidgetHandle),
}

/// A status draw handle decoded from a status_values reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusHandle<'cb> {
    pub(super) handle: core::num::NonZeroU32,
    pub(super) callback: core::marker::PhantomData<&'cb Host<'cb>>,
}
/// A composed status line returned by status_line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusLineHandle<'cb> {
    pub(super) instance: Instance,
    pub(super) callback: core::marker::PhantomData<&'cb Host<'cb>>,
}

/// A composition target obtained from the host. Raw handles cannot construct it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Compose<'cb> {
    /// A host-made widget instance.
    Widget(WidgetHandle),
    /// A host-made drawn status value.
    StatusItem(StatusHandle<'cb>),
    /// A host-made status line.
    StatusLine(StatusLineHandle<'cb>),
}

/// Input forwarded to a widget owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WidgetInputKind {
    /// A tap.
    Tap,
    /// A long press.
    LongPress,
}

/// The widget's natural and minimum heights for a measured width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WidgetSize {
    height: u32,
    min_height: u32,
}
impl WidgetSize {
    /// Natural height in pixels; zero means nothing is shown.
    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }
    /// Minimum height in pixels.
    #[must_use]
    pub const fn min_height(self) -> u32 {
        self.min_height
    }
}

/// A current widget input target in reading order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WidgetTarget {
    target: u32,
    rect: Rect,
}
impl WidgetTarget {
    /// An owner-defined target identifier and rectangle for a stage reply.
    #[must_use]
    pub const fn new(target: u32, rect: Rect) -> Self {
        Self { target, rect }
    }

    /// The target identifier forwarded by widget_input; zero means no target.
    #[must_use]
    pub const fn target(self) -> u32 {
        self.target
    }
    /// The target's rectangle.
    #[must_use]
    pub const fn rect(self) -> Rect {
        self.rect
    }
    fn decode(record: wire::RawRecord<'_>) -> Result<Self, Error> {
        let row: wire::ServiceUiTarget = expect(record)?;
        Ok(Self {
            target: row.target,
            rect: Rect::new(row.x, row.y, row.width, row.height),
        })
    }
}

impl Ui {
    /// Make a widget for the caller's own top screen instance. A widget of
    /// the caller's own plugin answers [`MadeWidget::Own`] without an instance.
    ///
    /// # Errors
    /// Returns invalid id syntax, `Invalid` for an own marker with an
    /// instance or an instance without the marker, or the host's
    /// ownership/resource refusal.
    pub fn widget_make(
        &self,
        host: &Host<'_>,
        widget: &str,
        place: &str,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<MadeWidget, Error> {
        let count = self.call(
            host,
            schema::OP_WIDGET_MAKE,
            &[&wire::ServiceUiWidgetRef { widget, place }],
            input,
            out,
        )?;
        let row: wire::ServiceUiInstance = expect(one(&out[..count])?)?;
        if row.flags & 1 != 0 {
            if row.instance != 0 {
                return Err(Error::Invalid);
            }
            return Ok(MadeWidget::Own);
        }
        Ok(MadeWidget::Widget(WidgetHandle {
            instance: Instance::new(row.instance).ok_or(Error::Invalid)?,
        }))
    }
    /// Drop a widget made by this screen.
    ///
    /// # Errors
    /// Returns the host's ownership/refusal or State for an unexpected reply.
    pub fn widget_drop(
        &self,
        host: &Host<'_>,
        widget: WidgetHandle,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        let count = self.call(
            host,
            schema::OP_WIDGET_DROP,
            &[&wire::ServiceUiInstance {
                instance: widget.instance.get(),
                flags: 0,
            }],
            input,
            out,
        )?;
        empty(count)
    }
    /// Forward a tap or long press to this widget's owner.
    ///
    /// # Errors
    /// Returns the host's ownership/refusal or State for an unexpected reply.
    #[allow(
        clippy::too_many_arguments,
        reason = "typed widget input plus current host and caller buffers"
    )]
    pub fn widget_input(
        &self,
        host: &Host<'_>,
        widget: WidgetHandle,
        target: u32,
        point: paperpaw_platform::Point,
        kind: WidgetInputKind,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        let kind = match kind {
            WidgetInputKind::Tap => 0,
            WidgetInputKind::LongPress => 1,
        };
        let count = self.call(
            host,
            schema::OP_WIDGET_INPUT,
            &[&wire::ServiceUiWidgetInput {
                instance: widget.instance.get(),
                target,
                x: point.x,
                y: point.y,
                kind,
            }],
            input,
            out,
        )?;
        empty(count)
    }
    /// Measure a widget's natural and minimum heights at this width.
    ///
    /// # Errors
    /// Returns the host's depth/ownership refusal or a malformed reply.
    pub fn widget_measure(
        &self,
        host: &Host<'_>,
        widget: WidgetHandle,
        width: u32,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<WidgetSize, Error> {
        let count = self.call(
            host,
            schema::OP_WIDGET_MEASURE,
            &[&area(widget, Rect::new(0, 0, width, 0))],
            input,
            out,
        )?;
        let row: wire::ServiceUiSize = expect(one(&out[..count])?)?;
        Ok(WidgetSize {
            height: row.height,
            min_height: row.min_height,
        })
    }
    /// Set the widget's rectangle before its first draw or after a layout change.
    ///
    /// # Errors
    /// Returns the host's depth/ownership refusal or State for an unexpected reply.
    pub fn widget_place(
        &self,
        host: &Host<'_>,
        widget: WidgetHandle,
        rect: Rect,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        let count = self.call(
            host,
            schema::OP_WIDGET_PLACE,
            &[&area(widget, rect)],
            input,
            out,
        )?;
        empty(count)
    }
    /// Read all current widget targets in reading order, without a cursor.
    /// A widget change affects the next call; no target is truncated.
    ///
    /// # Errors
    /// Returns the host's depth/ownership refusal or malformed page metadata.
    pub fn widget_targets<'a>(
        &self,
        host: &Host<'_>,
        widget: WidgetHandle,
        rect: Rect,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<UiPage<'a, WidgetTarget>, Error> {
        let count = self.call(
            host,
            schema::OP_WIDGET_TARGETS,
            &[&area(widget, rect)],
            input,
            out,
        )?;
        UiPage::terminal(
            &out[..count],
            self.operation(schema::OP_WIDGET_TARGETS)?,
            WidgetTarget::decode,
        )
    }
    /// Compose a host-issued target during the caller's own draw callback.
    /// Geometry is forwarded unchanged; the host clips it to the canvas.
    ///
    /// # Errors
    /// Maps Unsupported without a fallback, and every other host refusal.
    pub fn compose<'cb>(
        host: &'cb Host<'cb>,
        target: Compose<'cb>,
        rect: Rect,
    ) -> Result<(), Error> {
        let (kind, handle) = match target {
            Compose::Widget(widget) => (0, widget.instance.get()),
            Compose::StatusItem(status) => (1, status.handle.get()),
            Compose::StatusLine(line) => (2, line.instance.get()),
        };
        // SAFETY: the callback host is live and this slot carries scalars only.
        super::super::binding::status(unsafe {
            (host.table.ui_compose)(
                host.ctx,
                kind,
                handle,
                rect.x,
                rect.y,
                rect.width,
                rect.height,
            )
        })
    }
}
fn area(widget: WidgetHandle, rect: Rect) -> wire::ServiceUiWidgetArea {
    wire::ServiceUiWidgetArea {
        instance: widget.instance.get(),
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    }
}
pub(super) fn empty(count: usize) -> Result<(), Error> {
    if count == 0 {
        Ok(())
    } else {
        Err(Error::State)
    }
}

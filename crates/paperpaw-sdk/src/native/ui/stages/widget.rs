use super::super::{Error, Instance, WidgetTarget};
use crate::native::stage_schemas::core_widget_v1 as schema;
use alloc::vec::Vec;
use paperpaw_platform::Rect;

/// An owner-side widget request. Instance ids are decoded from host records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WidgetCall {
    /// Measure natural and minimum height at this width.
    Measure { instance: Instance, width: u32 },
    /// Keep the widget's current rectangle.
    Place { instance: Instance, rect: Rect },
    /// Return targets in reading order.
    Targets { instance: Instance, rect: Rect },
}
impl WidgetCall {
    /// Decode exactly one generated core.widget stage input.
    ///
    /// # Errors
    /// Returns malformed framing, zero instance or mismatched input errors.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut rows = schema::input_decode(bytes).map_err(Error::from_status)?;
        let row = rows
            .next()
            .transpose()
            .map_err(Error::from_status)?
            .ok_or(Error::Invalid)?;
        if rows.next().is_some() {
            return Err(Error::Invalid);
        }
        Ok(match row {
            schema::Record::WidgetMeasure(r) => Self::Measure {
                instance: Instance::new(r.instance).ok_or(Error::Invalid)?,
                width: r.width,
            },
            schema::Record::WidgetPlace(r) => Self::Place {
                instance: Instance::new(r.instance).ok_or(Error::Invalid)?,
                rect: Rect::new(r.x, r.y, r.width, r.height),
            },
            schema::Record::WidgetTargets(r) => Self::Targets {
                instance: Instance::new(r.instance).ok_or(Error::Invalid)?,
                rect: Rect::new(r.x, r.y, r.width, r.height),
            },
            _ => return Err(Error::State),
        })
    }
    /// Reply to a measure request using the generated size record.
    ///
    /// # Errors
    /// State for a different request kind; NoSpace for short scratch.
    pub fn reply_size(self, height: u32, min_height: u32, out: &mut [u8]) -> Result<usize, Error> {
        if !matches!(self, Self::Measure { .. }) {
            return Err(Error::State);
        }
        schema::reply_encode(
            &[schema::Record::WidgetSize(schema::WidgetSize::new(
                height, min_height,
            ))],
            out,
        )
        .map_err(Error::from_status)
    }
    /// Acknowledge a placed rectangle with the generated empty reply.
    ///
    /// # Errors
    /// State for a different request kind; NoSpace for short scratch.
    pub fn reply_place(self, out: &mut [u8]) -> Result<usize, Error> {
        if !matches!(self, Self::Place { .. }) {
            return Err(Error::State);
        }
        schema::reply_encode(&[], out).map_err(Error::from_status)
    }
    /// Reply with every target in reading order. The bounded record list is
    /// allocated fallibly, never placed as a large array on the callback stack.
    ///
    /// # Errors
    /// State for the wrong request, NoSpace above generated row/byte caps,
    /// NoMemory if the temporary records cannot be held.
    pub fn reply_targets(self, targets: &[WidgetTarget], out: &mut [u8]) -> Result<usize, Error> {
        if !matches!(self, Self::Targets { .. }) {
            return Err(Error::State);
        }
        if targets.len() > schema::REPLY_ROWS {
            return Err(Error::NoSpace);
        }
        let mut rows = Vec::new();
        rows.try_reserve_exact(targets.len())
            .map_err(|_| Error::NoMemory)?;
        for target in targets {
            let rect = target.rect();
            rows.push(schema::Record::WidgetTarget(schema::WidgetTarget::new(
                target.target(),
                rect.x,
                rect.y,
                rect.width,
                rect.height,
            )));
        }
        schema::reply_encode(&rows, out).map_err(Error::from_status)
    }
}

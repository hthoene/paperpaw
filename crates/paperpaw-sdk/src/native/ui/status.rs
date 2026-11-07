//! Status values and draw handles are valid only in the current callback.
use super::{
    Error, Host, Instance, Param, StatusHandle, StatusLineHandle, Ui, UiPage, one, params, schema,
    wire,
};
use core::{marker::PhantomData, num::NonZeroU32};

/// One status value. Own and hidden values carry no text or draw handle.
#[derive(Clone, Copy, Debug)]
pub struct StatusValue<'a> {
    item: &'a str,
    text: &'a str,
    flags: u32,
    handle: Option<StatusHandle<'a>>,
}
impl<'a> StatusValue<'a> {
    /// The requested full item id.
    #[must_use]
    pub const fn item(self) -> &'a str {
        self.item
    }
    /// Complete display text, empty for drawn, own or hidden values.
    #[must_use]
    pub const fn text(self) -> &'a str {
        self.text
    }
    /// The caller owns this item and must render its value itself.
    #[must_use]
    pub const fn is_own(self) -> bool {
        self.flags & 4 != 0
    }
    /// A guard hid this private value.
    #[must_use]
    pub const fn is_hidden(self) -> bool {
        self.flags & 2 != 0
    }
    /// Compose a drawn value inside this same draw callback.
    #[must_use]
    pub const fn draw_handle(self) -> Option<StatusHandle<'a>> {
        self.handle
    }
    fn decode(record: wire::Record<'a>) -> Result<Self, Error> {
        let wire::Record::ServiceUiStatusValue(row) = record else {
            return Err(Error::State);
        };
        let suppressed = row.flags & 6 != 0;
        if suppressed && (!row.text.is_empty() || row.handle != 0) {
            return Err(Error::Invalid);
        }
        let handle = if !suppressed && row.flags & 1 != 0 {
            Some(StatusHandle {
                handle: NonZeroU32::new(row.handle).ok_or(Error::Invalid)?,
                callback: PhantomData,
            })
        } else {
            if row.handle != 0 {
                return Err(Error::Invalid);
            }
            None
        };
        Ok(Self {
            item: row.item,
            text: row.text,
            flags: row.flags,
            handle,
        })
    }
}

/// A status-line result. An own line is drawn directly without host reentry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StatusLine<'cb> {
    /// The caller owns the line and draws it itself.
    Own,
    /// Compose this callback-scoped host handle.
    Draw(StatusLineHandle<'cb>),
}
impl Ui {
    /// Read up to eight status items using a typed parameter context. Draw
    /// handles borrow this callback's host and cannot escape into a later one.
    ///
    /// # Errors
    /// Returns local caps, a mismatched reply, or the host's depth/guard refusal.
    #[allow(
        clippy::too_many_arguments,
        reason = "typed status context needs a separate caller buffer before outer encoding"
    )]
    pub fn status_values<'a>(
        &self,
        host: &'a Host<'a>,
        items: &str,
        context: &[Param<'_>],
        context_scratch: &mut [u8],
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<UiPage<'a, StatusValue<'a>>, Error> {
        let expected = check_items(items)?;
        let context = context_bytes(context, context_scratch)?;
        let count = self.call(
            host,
            schema::OP_STATUS_VALUES,
            &[wire::Record::ServiceUiStatusAsk(wire::ServiceUiStatusAsk {
                items,
                context,
            })],
            input,
            out,
        )?;
        let page = UiPage::terminal(
            &out[..count],
            self.operation(schema::OP_STATUS_VALUES)?,
            StatusValue::decode,
        )?;
        let mut seen = [""; 8];
        let mut rows = 0;
        for row in page.clone() {
            let row = row?;
            if rows == seen.len() {
                return Err(Error::Invalid);
            }
            if !items.split('\n').any(|item| item == row.item) || seen[..rows].contains(&row.item) {
                return Err(Error::State);
            }
            seen[rows] = row.item;
            rows += 1;
        }
        if rows != expected {
            return Err(Error::State);
        }
        Ok(page)
    }
    /// Resolve a status line during draw, preserving the own-line marker.
    ///
    /// # Errors
    /// Returns caps, a mismatched/zero handle or host depth/guard refusals.
    #[allow(
        clippy::too_many_arguments,
        reason = "typed status context needs a separate caller buffer before outer encoding"
    )]
    pub fn status_line<'a>(
        &self,
        host: &'a Host<'a>,
        line: &str,
        context: &[Param<'_>],
        context_scratch: &mut [u8],
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<StatusLine<'a>, Error> {
        super::navigation::full_id(line)?;
        let context = context_bytes(context, context_scratch)?;
        let count = self.call(
            host,
            schema::OP_STATUS_LINE,
            &[wire::Record::ServiceUiStatusAsk(wire::ServiceUiStatusAsk {
                items: line,
                context,
            })],
            input,
            out,
        )?;
        let wire::Record::ServiceUiInstance(row) = one(&out[..count])? else {
            return Err(Error::State);
        };
        if row.flags & 1 != 0 {
            if row.instance != 0 {
                return Err(Error::Invalid);
            }
            return Ok(StatusLine::Own);
        }
        Ok(StatusLine::Draw(StatusLineHandle {
            instance: Instance::new(row.instance).ok_or(Error::Invalid)?,
            callback: PhantomData,
        }))
    }
}
fn check_items(items: &str) -> Result<usize, Error> {
    if items.len() > wire::SERVICE_UI_STATUS_ASK_ITEMS_MAX_BYTES {
        return Err(Error::NoSpace);
    }
    if items.is_empty() {
        return Ok(0);
    }
    let mut count = 0;
    for item in items.split('\n') {
        super::navigation::full_id(item)?;
        count += 1;
        if count > 8 {
            return Err(Error::NoSpace);
        }
    }
    Ok(count)
}
fn context_bytes<'a>(values: &[Param<'_>], out: &'a mut [u8]) -> Result<&'a [u8], Error> {
    params::validate_content(values)?;
    let mut used = 0;
    for value in values {
        let record = value.record();
        let length = wire::record_framed_len(&record).map_err(Error::from_status)?;
        if used + length > wire::SERVICE_UI_STATUS_ASK_CONTEXT_MAX_BYTES {
            return Err(Error::NoSpace);
        }
        used += wire::record_encode_one(&record, out.get_mut(used..).ok_or(Error::NoSpace)?)
            .map_err(Error::from_status)?;
    }
    Ok(&out[..used])
}

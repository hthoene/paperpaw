use super::super::{Error, Param, navigation::full_id, wire};
use crate::native::stage_schemas::core_status_v1 as schema;

/// A borrowed status context, decoded one param at a time without allocation.
#[derive(Clone, Debug)]
pub struct ContextParams<'a> {
    bytes: &'a [u8],
}
impl<'a> Iterator for ContextParams<'a> {
    type Item = Result<Param<'a>, Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.bytes.is_empty() {
            return None;
        }
        match wire::RawRecord::prefix(self.bytes) {
            Ok((record, count)) => {
                self.bytes = &self.bytes[count..];
                Some(Param::decode(record))
            }
            Err(error) => {
                self.bytes = &[];
                Some(Err(Error::from_status(error)))
            }
        }
    }
}
/// A generated core.status request, retaining only borrowed items and params.
#[derive(Clone, Copy, Debug)]
pub struct StatusCall<'a> {
    items: &'a str,
    context: &'a [u8],
}
impl<'a> StatusCall<'a> {
    /// Decode and validate a status request and its entire typed context.
    ///
    /// # Errors
    /// Returns framing/cap errors, invalid ids or a mismatched record.
    pub fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        let mut rows = schema::input_decode(bytes).map_err(Error::from_status)?;
        let row = rows
            .next()
            .transpose()
            .map_err(Error::from_status)?
            .ok_or(Error::Invalid)?;
        if rows.next().is_some() {
            return Err(Error::Invalid);
        }
        let schema::Record::StatusAsk(row) = row else {
            return Err(Error::State);
        };
        if !row.items.is_empty() {
            if row.items.split('\n').count() > schema::STATUS_ASK_ITEMS_MAX_ITEMS {
                return Err(Error::NoSpace);
            }
            for item in row.items.split('\n') {
                full_id(item)?;
            }
        }
        let context = ContextParams { bytes: row.context };
        for param in context {
            param?;
        }
        Ok(Self {
            items: row.items,
            context: row.context,
        })
    }
    /// Requested item ids, in order, without allocating a list.
    pub fn items(self) -> impl Iterator<Item = &'a str> {
        self.items.split('\n').filter(|item| !item.is_empty())
    }
    /// Parameters supplied by the screen showing these values.
    #[must_use]
    pub const fn context(self) -> ContextParams<'a> {
        ContextParams {
            bytes: self.context,
        }
    }
    /// Encode values from this owner using bounded stack scratch; display text
    /// stays borrowed and is never copied into owned strings.
    ///
    /// # Errors
    /// State for an unrequested item, Invalid for duplicate items, NoSpace for
    /// generated row/byte caps.
    pub fn reply(self, values: &[StatusAnswer<'_>], out: &mut [u8]) -> Result<usize, Error> {
        if values.len() > schema::REPLY_ROWS {
            return Err(Error::NoSpace);
        }
        let mut rows =
            [schema::Record::StatusValue(schema::StatusValue::new("", 0, "")); schema::REPLY_ROWS];
        for (index, value) in values.iter().enumerate() {
            if !self.items().any(|item| item == value.item) {
                return Err(Error::State);
            }
            if values[..index]
                .iter()
                .any(|earlier| earlier.item == value.item)
            {
                return Err(Error::Invalid);
            }
            rows[index] = schema::Record::StatusValue(schema::StatusValue::new(
                value.item,
                value.flags,
                value.text,
            ));
        }
        schema::reply_encode(&rows[..values.len()], out).map_err(Error::from_status)
    }
}
/// One typed status-stage answer: complete text, or an owner-drawn value.
#[derive(Clone, Copy, Debug)]
pub struct StatusAnswer<'a> {
    item: &'a str,
    flags: u32,
    text: &'a str,
}
impl<'a> StatusAnswer<'a> {
    /// Supply complete display text, without truncation.
    ///
    /// # Errors
    /// Invalid for malformed item ids; NoSpace above generated field caps.
    pub fn text(item: &'a str, text: &'a str) -> Result<Self, Error> {
        full_id(item)?;
        if item.len() > schema::STATUS_VALUE_ITEM_MAX_BYTES
            || text.len() > schema::STATUS_VALUE_TEXT_MAX_BYTES
        {
            return Err(Error::NoSpace);
        }
        Ok(Self {
            item,
            flags: 0,
            text,
        })
    }
    /// Declare that the owner draws this value; the host supplies its handle.
    ///
    /// # Errors
    /// Invalid for malformed item ids; NoSpace above generated field caps.
    pub fn drawn(item: &'a str) -> Result<Self, Error> {
        let mut value = Self::text(item, "")?;
        value.flags = 1;
        Ok(value)
    }
}

// Keep the bounded codec scratch below one KiB on supported device profiles.
const _: () =
    assert!(core::mem::size_of::<[schema::Record<'static>; schema::REPLY_ROWS]>() <= 1024);

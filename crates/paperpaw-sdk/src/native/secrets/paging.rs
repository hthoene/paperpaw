//! Borrowed bare-stream pages over Kernel's typed per-record codec.
use super::{Error, SecretList, SecretName, SecretWrite, schema, wire};

/// A listed name and label. The secret value is never present in a list.
#[derive(Clone, Copy, Debug)]
pub struct SecretEntry<'a>(wire::ServiceSecEntry<'a>);
impl<'a> SecretEntry<'a> {
    fn decode(raw: wire::RawRecord<'a>) -> Result<Self, Error> {
        if raw.tag() != wire::SERVICE_SEC_ENTRY_TAG {
            return Err(Error::Invalid);
        }
        let row = raw
            .decode::<wire::ServiceSecEntry<'a>>()
            .map_err(Error::from_status)?;
        super::types::plugin_id(row.plugin)?;
        SecretName::new(row.name)?;
        super::types::label(row.label, wire::SERVICE_SEC_ENTRY_LABEL_MAX_BYTES)?;
        if row.flags & !1 != 0 {
            return Err(Error::Invalid);
        }
        Ok(Self(row))
    }
    /// The plugin that owns this secret.
    #[must_use]
    pub const fn plugin(self) -> &'a str {
        self.0.plugin
    }
    /// Exact unprefixed name, including allowed SSID whitespace.
    #[must_use]
    pub const fn name(self) -> &'a str {
        self.0.name
    }
    /// Owner's catalog label, or plain text if there is no translation.
    #[must_use]
    pub const fn label(self) -> &'a str {
        self.0.label
    }
    /// Whether the settings editor may replace the value.
    #[must_use]
    pub const fn changeable(self) -> bool {
        self.0.flags & 1 != 0
    }
    /// The explicit owner/name suitable for editor removal.
    #[must_use]
    pub const fn target(self) -> SecretName<'a> {
        SecretName {
            plugin: self.0.plugin,
            name: self.0.name,
        }
    }
    /// Build a replacement while preserving the owner's changeable flag.
    ///
    /// # Errors
    /// Denied if not changeable; NoSpace if the value exceeds 1,024 bytes.
    pub fn replacement(self, value: &'a str) -> Result<SecretWrite<'a>, Error> {
        if !self.changeable() {
            return Err(Error::Denied);
        }
        SecretWrite::new(self.target(), value).map(SecretWrite::changeable)
    }
}
/// Row count and continuation information for one secret list page.
#[derive(Clone, Copy, Debug)]
pub struct SecretPageEnd {
    total: u32,
    next: Option<u32>,
}
impl SecretPageEnd {
    /// Current visible total; concurrent edits may change it between pages.
    #[must_use]
    pub const fn total(self) -> u32 {
        self.total
    }
    /// A row cursor for the next page, or none for the terminal page.
    #[must_use]
    pub const fn next(self) -> Option<u32> {
        self.next
    }
}
/// A prevalidated borrowed page. Iteration never exposes the terminal marker.
#[derive(Clone, Debug)]
pub struct SecretPage<'a> {
    bytes: &'a [u8],
    end: SecretPageEnd,
}
impl<'a> SecretPage<'a> {
    pub(super) fn decode(bytes: &'a [u8], query: SecretList) -> Result<Self, Error> {
        let max_reply =
            crate::native::wire::services_wire::service_schema(schema::KEY, schema::MAJOR)
                .and_then(|service| service.op(schema::OP_LIST))
                .ok_or(Error::Unsupported)?
                .max_reply;
        if bytes.len() > max_reply {
            return Err(Error::NoSpace);
        }
        let mut rest = bytes;
        let mut rows = 0_u32;
        while !rest.is_empty() {
            let (raw, n) = wire::RawRecord::prefix(rest).map_err(Error::from_status)?;
            let tail = &rest[n..];
            if raw.tag() == wire::SERVICE_END_TAG {
                let end = raw
                    .decode::<wire::ServiceEnd>()
                    .map_err(Error::from_status)?;
                let next = query.cursor.checked_add(rows).ok_or(Error::Invalid)?;
                let last = end.flags & 1 != 0;
                if !tail.is_empty()
                    || end.flags & !1 != 0
                    || end.generation != 0
                    || end.cursor.is_some()
                    || end.next != next
                    || (!last && (rows == 0 || next >= end.total))
                    || (last && (next < end.total || (rows != 0 && next != end.total)))
                    || (!query.includes_all() && end.total > 32)
                {
                    return Err(Error::Invalid);
                }
                let row_bytes = bytes.len() - rest.len();
                return Ok(Self {
                    bytes: &bytes[..row_bytes],
                    end: SecretPageEnd {
                        total: end.total,
                        next: if last { None } else { Some(next) },
                    },
                });
            }
            SecretEntry::decode(raw)?;
            rows += 1;
            if rows as usize > schema::PAGE_MAX_ROWS {
                return Err(Error::Invalid);
            }
            rest = tail;
        }
        Err(Error::Invalid)
    }
    /// Paging information, available before or after consuming the rows.
    #[must_use]
    pub const fn end(&self) -> SecretPageEnd {
        self.end
    }
}
impl<'a> Iterator for SecretPage<'a> {
    type Item = Result<SecretEntry<'a>, Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.bytes.is_empty() {
            return None;
        }
        let result = wire::RawRecord::prefix(self.bytes)
            .map_err(Error::from_status)
            .and_then(|(raw, used)| {
                self.bytes = &self.bytes[used..];
                SecretEntry::decode(raw)
            });
        if result.is_err() {
            self.bytes = &[];
        }
        Some(result)
    }
}

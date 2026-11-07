//! Typed discovery of registered status items and lines (REG op 21).
use super::{Error, Host, ListQuery, PageCursor, Record, Registry, RegistryPage, schema, wire};

/// Which status declarations to enumerate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StatusScope {
    /// Every status item and line.
    All,
    /// Status items only.
    Items,
    /// Status lines only.
    Lines,
}

/// The kind of a registered status declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StatusKind {
    /// One status item.
    Item,
    /// A complete status line.
    Line,
}

/// A status declaration, borrowing translated text from the reply buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegistryStatus<'a> {
    row: wire::ServiceRegStatus<'a>,
    kind: StatusKind,
}
impl<'a> RegistryStatus<'a> {
    fn decode(record: Record<'a>) -> Result<Self, Error> {
        let Record::ServiceRegStatus(row) = record else {
            return Err(Error::Invalid);
        };
        let kind = match row.kind {
            0 => StatusKind::Item,
            1 => StatusKind::Line,
            _ => return Err(Error::Invalid),
        };
        if row.id.is_empty() || row.plugin.is_empty() || row.priority > 255 {
            return Err(Error::Invalid);
        }
        if (row.flags & 1 != 0) != (kind == StatusKind::Line)
            || kind == StatusKind::Line && (row.priority != 0 || !row.tap.is_empty())
        {
            return Err(Error::Invalid);
        }
        Ok(Self { row, kind })
    }
    /// Full status declaration id.
    #[must_use]
    pub const fn id(self) -> &'a str {
        self.row.id
    }
    /// Host-stamped plugin identity.
    #[must_use]
    pub const fn plugin(self) -> &'a str {
        self.row.plugin
    }
    /// Whether this declaration is an item or a line.
    #[must_use]
    pub const fn kind(self) -> StatusKind {
        self.kind
    }
    /// The owner draws this declaration.
    #[must_use]
    pub const fn drawn_by_owner(self) -> bool {
        self.row.flags & 1 != 0
    }
    /// The declaration is private while another plugin guards the screen.
    #[must_use]
    pub const fn private(self) -> bool {
        self.row.flags & 2 != 0
    }
    /// Item priority from 0 to 255; zero for a line.
    #[must_use]
    pub const fn priority(self) -> u32 {
        self.row.priority
    }
    /// Minimum item width or line height, in thousandths of the text size.
    #[must_use]
    pub const fn size(self) -> u32 {
        self.row.size
    }
    /// Full action id, or empty when no action is declared.
    #[must_use]
    pub const fn tap(self) -> &'a str {
        self.row.tap
    }
    /// Title translated into the host's language.
    #[must_use]
    pub const fn title(self) -> &'a str {
        self.row.title
    }
}
impl Registry {
    /// Enumerate registered status items and lines, without making instances.
    ///
    /// # Errors
    /// Returns Invalid for malformed rows or pages, State for a stale cursor,
    /// or a host refusal. An empty registry returns a terminal empty page.
    pub fn status<'a>(
        &self,
        host: &Host<'_>,
        scope: StatusScope,
        cursor: PageCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<RegistryPage<'a, RegistryStatus<'a>>, Error> {
        let (scope, decode): (&str, super::Decode<'a, RegistryStatus<'a>>) = match scope {
            StatusScope::All => ("", RegistryStatus::decode),
            StatusScope::Items => ("item", |record| {
                let row = RegistryStatus::decode(record)?;
                if row.kind() != StatusKind::Item {
                    return Err(Error::Invalid);
                }
                Ok(row)
            }),
            StatusScope::Lines => ("line", |record| {
                let row = RegistryStatus::decode(record)?;
                if row.kind() != StatusKind::Line {
                    return Err(Error::Invalid);
                }
                Ok(row)
            }),
        };
        self.page(
            host,
            ListQuery {
                op: schema::OP_STATUS,
                scope,
                cursor,
                flags: 0,
            },
            input,
            out,
            decode,
        )
    }
}

//! Query-only UI rows, and callback-scoped clipboard text.
use super::{Error, Host, Instance, TextPart, TextWrite, Ui, UiCursor, UiPage, schema, wire};

/// One visible screen instance, ordered from the stack's top down.
#[derive(Clone, Copy, Debug)]
pub struct UiScreen<'a> {
    screen: &'a str,
    instance: Instance,
    slot: &'a str,
    owner: &'a str,
    flags: u32,
}
impl<'a> UiScreen<'a> {
    /// Full screen id.
    #[must_use]
    pub const fn screen(self) -> &'a str {
        self.screen
    }
    /// The host-issued instance.
    #[must_use]
    pub const fn instance(self) -> Instance {
        self.instance
    }
    /// Full slot id, or empty for an ordinary screen.
    #[must_use]
    pub const fn slot(self) -> &'a str {
        self.slot
    }
    /// Host-stamped plugin identity.
    #[must_use]
    pub const fn owner(self) -> &'a str {
        self.owner
    }
    /// The instance is an overlay.
    #[must_use]
    pub const fn is_overlay(self) -> bool {
        self.flags & 1 != 0
    }
    /// The instance guards what is below it.
    #[must_use]
    pub const fn guards(self) -> bool {
        self.flags & 2 != 0
    }
    /// Another instance is above it.
    #[must_use]
    pub const fn is_covered(self) -> bool {
        self.flags & 4 != 0
    }
    /// Its first page is not ready yet.
    #[must_use]
    pub const fn waits_ready(self) -> bool {
        self.flags & 8 != 0
    }
    /// Its content must not be drawn under another plugin's overlay.
    #[must_use]
    pub const fn is_secret(self) -> bool {
        self.flags & 16 != 0
    }
    fn decode(record: wire::Record<'a>) -> Result<Self, Error> {
        let wire::Record::ServiceUiScreen(row) = record else {
            return Err(Error::State);
        };
        Ok(Self {
            screen: row.screen,
            instance: Instance::new(row.instance).ok_or(Error::Invalid)?,
            slot: row.slot,
            owner: row.owner,
            flags: row.flags,
        })
    }
}

/// Which input style a help hint applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HintInput {
    /// Every input style.
    Any,
    /// Touch input.
    Touch,
    /// Buttons.
    Buttons,
}
/// A help hint borrowed from the reply; text is a key in its owner's catalog.
#[derive(Clone, Copy, Debug)]
pub struct UiHint<'a> {
    input: HintInput,
    zone: paperpaw_platform::Rect,
    text: &'a str,
    owner: &'a str,
}
impl<'a> UiHint<'a> {
    /// Applicable input style.
    #[must_use]
    pub const fn input(self) -> HintInput {
        self.input
    }
    /// Zone in thousandths of the screen, or None for a hint in the box.
    #[must_use]
    pub fn zone(self) -> Option<paperpaw_platform::Rect> {
        (self.zone.x != 0 || self.zone.y != 0 || self.zone.width != 0 || self.zone.height != 0)
            .then_some(self.zone)
    }
    /// Catalog key, unchanged.
    #[must_use]
    pub const fn text(self) -> &'a str {
        self.text
    }
    /// The plugin whose catalog owns the text key.
    #[must_use]
    pub const fn owner(self) -> &'a str {
        self.owner
    }
    fn decode(record: wire::Record<'a>) -> Result<Self, Error> {
        let wire::Record::ServiceUiHint(row) = record else {
            return Err(Error::State);
        };
        let input = match row.input {
            0 => HintInput::Any,
            1 => HintInput::Touch,
            2 => HintInput::Buttons,
            _ => return Err(Error::Unsupported),
        };
        Ok(Self {
            input,
            zone: paperpaw_platform::Rect::new(
                i32::try_from(row.x).map_err(|_| Error::Invalid)?,
                i32::try_from(row.y).map_err(|_| Error::Invalid)?,
                row.width,
                row.height,
            ),
            text: row.text,
            owner: row.owner,
        })
    }
}
impl Ui {
    /// Read a stack page. Guarded rows may be omitted, as its end metadata says.
    ///
    /// # Errors
    /// Returns State for a changed generation or a mismatched reply; otherwise
    /// malformed-page errors or host refusals.
    pub fn stack<'a>(
        &self,
        host: &Host<'_>,
        cursor: UiCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<UiPage<'a, UiScreen<'a>>, Error> {
        self.query(
            host,
            schema::OP_STACK,
            "",
            cursor,
            input,
            out,
            UiScreen::decode,
        )
    }
    /// Read a screen's help hints from every declaring plugin.
    ///
    /// # Errors
    /// Returns invalid full-id syntax, stale pages or host refusals.
    pub fn hints<'a>(
        &self,
        host: &Host<'_>,
        screen: &str,
        cursor: UiCursor,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<UiPage<'a, UiHint<'a>>, Error> {
        super::navigation::full_id(screen)?;
        self.query(
            host,
            schema::OP_HINTS,
            screen,
            cursor,
            input,
            out,
            UiHint::decode,
        )
    }
    /// Append a clipboard text part. Offset zero starts a replacement and Last
    /// publishes it atomically; a failed replacement leaves the old clipboard.
    ///
    /// # Errors
    /// Returns part limits or host storage errors, including IO without a card.
    pub fn clipboard_write(
        &self,
        host: &Host<'_>,
        offset: u32,
        data: &str,
        finish: TextWrite,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        self.write(
            host,
            schema::OP_CLIPBOARD_WRITE,
            0,
            offset,
            data,
            finish,
            input,
            out,
        )
    }
    /// Read a committed clipboard text part; zero total means an empty clipboard.
    ///
    /// # Errors
    /// Returns State for a mismatched reply or host storage/part errors.
    pub fn clipboard_read<'a>(
        &self,
        host: &Host<'_>,
        offset: u32,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<TextPart<'a>, Error> {
        self.read(host, schema::OP_CLIPBOARD_READ, 0, offset, input, out)
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "shared typed paged query transport"
    )]
    fn query<'a, T>(
        &self,
        host: &Host<'_>,
        op: u32,
        scope: &str,
        cursor: UiCursor,
        input: &mut [u8],
        out: &'a mut [u8],
        decode: fn(wire::Record<'a>) -> Result<T, Error>,
    ) -> Result<UiPage<'a, T>, Error> {
        let count = self.call(
            host,
            op,
            &[wire::Record::ServiceRegQuery(wire::ServiceRegQuery {
                scope,
                cursor: cursor.offset,
                generation: cursor.generation,
                flags: 0,
            })],
            input,
            out,
        )?;
        UiPage::checked(&out[..count], cursor, self.operation(op)?, decode)
    }
}

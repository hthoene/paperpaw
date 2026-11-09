//! Named icons: the character an icon name stands for in the bound icon font.
use crate::native::{Error, Host, declare::records_wire as wire};
use paperpaw_native::host_glue::{PpBuffer, PpBytes};

/// The longest UTF-8 encoding of one character.
const GLYPH_MAX_BYTES: usize = 4;

/// The character of a named icon in the bound icon font (`icon.glyph`, slot 40),
/// to be drawn with `text.draw` through a `role:icons` font. It holds at most one
/// character and borrows nothing, so it outlives the callback's host binding.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct IconGlyph {
    bytes: [u8; GLYPH_MAX_BYTES],
    len: u8,
}

impl IconGlyph {
    /// Ask the host for the character it uses for the icon `name`, the SDK's icon
    /// names as in `icon.spec`: one `[a-z0-9-]` segment of 1 to 64 bytes. The name
    /// table stays in the host. Allocates nothing.
    ///
    /// # Errors
    /// Returns Invalid for a malformed name (nothing is asked), or when the host
    /// answers something that is not one character. The host's refusals pass
    /// through: NotFound for a name its set lacks or while no icon font is bound
    /// (show text instead, as menus do), and Unsupported on a host that has not
    /// wired the slot.
    pub fn named(host: &Host<'_>, name: impl AsRef<str>) -> Result<Self, Error> {
        let name = name.as_ref();
        if name.is_empty()
            || name.len() > wire::ICON_SPEC_ICON_MAX_BYTES
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(Error::Invalid);
        }
        let mut bytes = [0; GLYPH_MAX_BYTES];
        // SAFETY: the host binding is live for this call; the name and the output
        // buffer are call-local and disjoint.
        let result = unsafe {
            (host.table.icon_glyph)(
                host.ctx,
                PpBytes {
                    ptr: name.as_ptr(),
                    len: name.len(),
                },
                PpBuffer {
                    ptr: bytes.as_mut_ptr(),
                    cap: bytes.len(),
                },
            )
        };
        let Ok(len) = usize::try_from(result) else {
            return Err(Error::from_status(result));
        };
        // One character: not empty, within the buffer, valid UTF-8 with a single
        // scalar. Anything else is a host that breaks the contract.
        let one = bytes
            .get(..len)
            .and_then(|written| core::str::from_utf8(written).ok())
            .is_some_and(|text| text.chars().count() == 1);
        if !one {
            return Err(Error::Invalid);
        }
        Ok(Self {
            bytes,
            len: u8::try_from(len).map_err(|_| Error::Invalid)?,
        })
    }

    /// The character, to pass to `Font::render_text` on a `role:icons` font.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // The constructor checked these bytes as one UTF-8 character.
        core::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }
}

impl core::fmt::Debug for IconGlyph {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("IconGlyph").field(&self.as_str()).finish()
    }
}

#[cfg(test)]
#[path = "fonts_icons_tests.rs"]
mod tests;

//! Telling what a draw callback draws, for those who ask (`canvas.describe`,
//! slot 42, `screens-input.md` section 6.1): automation, reading aloud, tests.
use super::{Canvas, Error, FrameRect, status};
use paperpaw_native::host_glue::PpBytes;

/// What a described part is (`screens-input.md` 6.1). Text the plugin draws with
/// `text.draw` is told as [`PartRole::Text`] by the host itself; a part
/// around it (a row, a button) takes it in when told before it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PartRole {
    /// A title over what follows.
    Heading = 1,
    /// A row of a list; its label is the text drawn inside it, where empty.
    Row = 2,
    /// Something that does what it says when chosen.
    Button = 3,
    /// A switch, alone or in a row (the row is then on or off).
    Switch = 4,
    /// A number set along a track; its label is the value.
    Slider = 5,
    /// Text the host does not draw itself (a picture of text).
    Text = 6,
    /// A box ticked or not.
    Check = 7,
    /// One of several, of which one is chosen.
    Radio = 8,
    /// Where text is typed; its label is the text so far.
    Field = 9,
    /// A picture; its label says what it shows.
    Image = 10,
    /// Leads elsewhere when chosen.
    Link = 11,
    /// A question over the screen; its label is the question.
    Dialog = 12,
    /// How far something has come; its label is the share done.
    Progress = 13,
}

/// What more a described part is: combined with `|`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PartFlags(u32);

impl PartFlags {
    /// Nothing more.
    pub const NONE: Self = Self(0);
    /// It has the focus.
    pub const FOCUSED: Self = Self(1);
    /// Switched off (a switch, a check, a radio).
    pub const OFF: Self = Self(1 << 1);
    /// Switched on.
    pub const ON: Self = Self((1 << 1) | (1 << 2));
    /// It does nothing now (drawn faint).
    pub const DISABLED: Self = Self(1 << 3);

    /// The bits of the slot.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }
}

impl core::ops::BitOr for PartFlags {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl Canvas<'_> {
    /// Tells the part `role` with `label` at `area` (screen pixels, as drawn),
    /// with `flags`. `Ok(true)` where it was told, `Ok(false)` where nobody
    /// asks: the rest of this draw's parts may be left out. A part to choose
    /// (a row, a button, a switch, a field, a check, a radio, a link) needs
    /// an area that is not empty. Nothing is kept or allocated.
    ///
    /// # Errors
    /// The host's refusal: Invalid for an empty area of a part to choose or
    /// a label over 256 bytes, State outside a draw callback.
    pub fn describe_part(
        &self,
        role: PartRole,
        label: &str,
        area: FrameRect,
        flags: PartFlags,
    ) -> Result<bool, Error> {
        let host = self.host();
        let label = PpBytes {
            ptr: label.as_ptr(),
            len: label.len(),
        };
        // SAFETY: the canvas keeps its callback host and drawing lease; the
        // label is borrowed for the call only and the geometry is scalar.
        let told = unsafe {
            (host.table.canvas_describe)(
                host.ctx,
                role as u32,
                flags.bits(),
                label,
                area.x,
                area.y,
                area.width,
                area.height,
            )
        };
        if told >= 0 {
            Ok(told > 0)
        } else {
            status(told).map(|()| false)
        }
    }
}

#[cfg(test)]
#[path = "canvas_describe_tests.rs"]
mod tests;

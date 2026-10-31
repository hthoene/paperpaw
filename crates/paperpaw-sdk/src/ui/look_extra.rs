//! What a look can paint besides the fixed parts of [`look`](super::look): the text of
//! the interface, and parts of kinds this release does not name yet.
//!
//! Both are built by the SDK code of the plugin that draws the screen and
//! read by the painter of the plugin that provides the look: two programs,
//! compiled at different times. So the types have private spare fields,
//! zero now and read through methods later, and the kinds are constants of
//! open types.
//!
//! A painter changes how a thing *looks*, never where it goes or how wide
//! it is: layout is measured with the stock metrics, and a look that
//! changed advances would break wrapping, hit areas and screen readers.

use core::fmt;

use paperpaw_platform::{Color, Rect};

use super::TextRole;
use super::look::PaintCx;
use crate::fonts::FontHandle;

/// A run of interface text about to be drawn. The painter draws it at
/// `x` and `baseline`, and its width must be the stock width (see the
/// [module](super::look)). Paint with `font.draw(..)` for the stock look.
#[non_exhaustive]
pub struct TextPart<'t> {
    /// The text.
    pub text: &'t str,
    /// Where it starts, in pixels.
    pub x: f32,
    /// The line it stands on.
    pub baseline: i32,
    /// Its size, in pixels.
    pub size: f32,
    /// Its colour.
    pub color: Color,
    /// The area the text was given. It is NOT the text's own box: do not
    /// centre or wrap inside it, and letters may reach a little past it.
    pub area: Rect,
    /// The stock font handle (the text's ink included): `font.draw(..)`
    /// is the stock look.
    pub font: &'t FontHandle<'t>,
    /// What the text is.
    pub role: TextRole,
    /// On a black row or bar. A heuristic: it is whether the colour is
    /// white, which the SDK cannot know better here (text white on a
    /// dark image is "inverted" too; black on a light gray row is not).
    /// Use `color` when what matters is the ink.
    pub inverted: bool,
    /// Zero now; read through methods in a later release.
    spare: [u32; 2],
}

impl<'t> TextPart<'t> {
    /// A run of text, as the SDK asks a look about it.
    #[allow(clippy::too_many_arguments, reason = "a part is its fields")]
    pub fn new(
        text: &'t str,
        x: f32,
        baseline: i32,
        size: f32,
        color: Color,
        area: Rect,
        font: &'t FontHandle<'t>,
        role: TextRole,
    ) -> Self {
        Self {
            text,
            x,
            baseline,
            size,
            color,
            area,
            font,
            role,
            inverted: color == Color::WHITE,
            spare: [0; 2],
        }
    }
}

impl fmt::Debug for TextPart<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextPart")
            .field("text", &self.text)
            .field("role", &self.role)
            .field("spare", &self.spare)
            .finish_non_exhaustive()
    }
}

/// Paints a run of text; `true` when it did (else the SDK draws it as
/// usual).
pub type TextPainter = for<'t> fn(&mut PaintCx<'_, '_>, &TextPart<'t>) -> bool;

/// What a part is, of the kinds beyond the named parts of a look: a
/// constant of one open type, so more can come without a new plugin
/// interface. The SDK asks a look for a kind only from the release that
/// says so here; none is asked yet (see
/// [`Capability::LOOK_EXTRAS`](crate::Capability::LOOK_EXTRAS)), so a look
/// made now for a kind works when the SDK learns to ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PartKind(u16);

impl PartKind {
    /// A scrollbar. `values`: position, length. Asked since: not yet.
    pub const SCROLLBAR: Self = Self(1);
    /// A tab in a row of tabs. `values`: tab count, this tab. Asked
    /// since: not yet.
    pub const TAB: Self = Self(2);
    /// A badge, e.g. a count on an icon. Asked since: not yet.
    pub const BADGE: Self = Self(3);
    /// A toast: a short message over the screen. Asked since: not yet.
    pub const TOAST: Self = Self(4);
    /// A dialog's frame. Asked since: not yet.
    pub const DIALOG: Self = Self(5);

    /// The kind's number; 0 is no kind, 1 to 5 are named above.
    pub const fn value(self) -> u16 {
        self.0
    }

    /// The kind of a number.
    pub const fn from_value(value: u16) -> Self {
        Self(value)
    }
}

/// A part of a kind the look's parts do not name.
#[non_exhaustive]
pub struct ExtraPart<'t> {
    /// Which kind of part.
    pub kind: PartKind,
    /// Where it goes.
    pub rect: Rect,
    /// Bit 0 on, 1 focused, 2 inverted, 3 disabled, 4 pressed; the others
    /// are reserved (zero).
    pub state: u32,
    /// Numbers whose meaning depends on the kind (see [`PartKind`]).
    pub values: [i32; 4],
    /// Its label, if it has one.
    pub label: &'t str,
    /// Zero now; read through methods in a later release.
    spare: [u32; 2],
}

impl<'t> ExtraPart<'t> {
    /// A part of `kind` in `rect`, off, unfocused, with no values or label.
    pub const fn new(kind: PartKind, rect: Rect) -> Self {
        Self {
            kind,
            rect,
            state: 0,
            values: [0; 4],
            label: "",
            spare: [0; 2],
        }
    }

    /// With `state` (see [`ExtraPart::state`]).
    #[must_use]
    pub const fn with_state(mut self, state: u32) -> Self {
        self.state = state;
        self
    }

    /// With `values` (see [`ExtraPart::values`]).
    #[must_use]
    pub const fn with_values(mut self, values: [i32; 4]) -> Self {
        self.values = values;
        self
    }

    /// With `label`.
    #[must_use]
    pub const fn with_label(mut self, label: &'t str) -> Self {
        self.label = label;
        self
    }
}

impl fmt::Debug for ExtraPart<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExtraPart")
            .field("kind", &self.kind)
            .field("rect", &self.rect)
            .field("state", &self.state)
            .field("values", &self.values)
            .field("label", &self.label)
            .field("spare", &self.spare)
            .finish()
    }
}

/// Paints a part of a kind beyond the named ones; `true` when it did.
pub type ExtraPainter = for<'t> fn(&mut PaintCx<'_, '_>, &ExtraPart<'t>) -> bool;

/// One entry of a look's list of extra painters
/// ([`Painters::extras`](super::look::Painters::extras)).
#[derive(Clone, Copy)]
pub struct ExtraPaint {
    kind: PartKind,
    painter: ExtraPainter,
    /// Zero now.
    spare: u32,
}

impl ExtraPaint {
    /// `painter` for parts of `kind`.
    pub const fn new(kind: PartKind, painter: ExtraPainter) -> Self {
        Self {
            kind,
            painter,
            spare: 0,
        }
    }

    /// The kind it paints.
    pub const fn kind(&self) -> PartKind {
        self.kind
    }

    pub(super) fn painter(&self) -> ExtraPainter {
        self.painter
    }
}

impl fmt::Debug for ExtraPaint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExtraPaint")
            .field("kind", &self.kind)
            .field("spare", &self.spare)
            .finish_non_exhaustive()
    }
}

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
pub(crate) type TextPainter = for<'t> fn(&mut PaintCx<'_, '_>, &TextPart<'t>) -> bool;

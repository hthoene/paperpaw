//! A laid-out page drawn: the words of each line set one after the other with
//! the gaps the layout gave them. What the pen draws with is the [`Ink`]; the
//! host's is [`HostInk`], and the one place that decides how a run of text
//! reaches the host is [`draw_run`], where the font weight and the tone of
//! the letters' edges will come in with the host's new text slot (until then
//! it is `Font::render_text`). Lines of mixed direction are set in logical
//! order, underlines, strike-throughs, pictures and rules are not drawn yet
//! (`PORTING-7.md`).
use crate::native_typeset::{Faces, HostFonts, Measure, Role};
use paperpaw_sdk::{
    Color,
    layout::{Page, Typesetter},
    native::{
        Error,
        canvas::Canvas,
        fonts::{Font, TextOrigin},
    },
};

/// What a run of text is drawn with.
pub trait Ink {
    /// Draws `text` in `role` at `size` pixels, its pen starting at `x` on the
    /// row `baseline`; the advance it took, in pixels.
    ///
    /// # Errors
    /// The host's refusal.
    fn run(
        &mut self,
        role: Role,
        size: f32,
        x: f32,
        baseline: i32,
        text: &str,
    ) -> Result<f32, Error>;
}

/// The page's lines drawn with their content box's top left at `(left, top)`.
/// Stops at the first refusal of the host.
///
/// # Errors
/// What `ink` refuses.
pub fn draw_lines<M: Measure>(
    ink: &mut impl Ink,
    faces: &Faces<M>,
    page: &Page,
    (left, top): (f32, f32),
) -> Result<(), Error> {
    for line in &page.lines {
        let baseline = (top + line.baseline).round() as i32;
        let mut x = left + line.x;
        for run in &line.runs {
            let Some(text) = line.text.get(run.start..run.end) else {
                continue;
            };
            let (role, size) = (Role::of(run.face), faces.size(run.face));
            let space = faces.advance(run.face, ' ') + line.word_spacing;
            for (n, word) in text.split(' ').enumerate() {
                if n > 0 {
                    x += space;
                }
                if !word.is_empty() {
                    x += ink.run(role, size, x, baseline, word)?;
                }
            }
        }
    }
    Ok(())
}

/// How a run of text reaches the host: the one place to change when the host
/// takes a font weight and a tone with the text.
///
/// # Errors
/// The host's refusal.
pub fn draw_run(
    font: &Font<'_>,
    canvas: &Canvas<'_>,
    size: f32,
    origin: TextOrigin,
    color: Color,
    text: &str,
) -> Result<f32, Error> {
    font.render_text(canvas, size, origin, color, text)
}

/// The host's reading fonts drawing on a canvas in one colour.
#[derive(Debug)]
pub struct HostInk<'a, 'h> {
    fonts: &'a HostFonts<'h>,
    canvas: &'a Canvas<'a>,
    color: Color,
}

impl<'a, 'h> HostInk<'a, 'h> {
    /// Draws with `fonts` on `canvas` in `color`.
    #[must_use]
    pub const fn new(fonts: &'a HostFonts<'h>, canvas: &'a Canvas<'a>, color: Color) -> Self {
        Self {
            fonts,
            canvas,
            color,
        }
    }
}

impl Ink for HostInk<'_, '_> {
    fn run(
        &mut self,
        role: Role,
        size: f32,
        x: f32,
        baseline: i32,
        text: &str,
    ) -> Result<f32, Error> {
        draw_run(
            self.fonts.font_of(role),
            self.canvas,
            size,
            TextOrigin::new(x, baseline),
            self.color,
            text,
        )
    }
}

#[cfg(test)]
mod tests;

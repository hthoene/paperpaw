//! A laid-out page drawn: the words of each line set one after the other with
//! the gaps the layout gave them. What the pen draws with is the [`Ink`]; the
//! host's is [`HostInk`], and the one place that decides how a run of text
//! reaches the host is [`draw_run`], which draws with the weight and the tone
//! of the letters' edges the reading settings give (`native_look`). Lines of mixed direction are set in logical
//! order; the lines under, over and through runs and a table's rules are drawn
//! (`native_decor`), pictures are not yet (`PORTING-7.md`).
use crate::native_decor::{decorate, draw_rules};
use crate::native_look::Look;
use crate::native_typeset::{Faces, HostFonts, Measure, Role};
use paperpaw_sdk::{
    Color,
    layout::{Page, Typesetter},
    native::{
        Error,
        callbacks::FrameRect,
        canvas::Canvas,
        fonts::{Font, Ink as Inking, TextOrigin},
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

    /// Fills the rectangle at `(x, y)`, `width` by `height` pixels, in the
    /// pen's colour (a line under a run, a table's rule).
    ///
    /// # Errors
    /// The host's refusal.
    fn fill(&mut self, x: i32, y: i32, width: u32, height: u32) -> Result<(), Error>;
}

/// The page's lines (and rules) drawn with their content box's top left at `(left, top)`.
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
        // Rounded to the row (the page is below the top of the screen: never negative).
        let baseline = (top + line.baseline + 0.5) as i32;
        let mut x = left + line.x;
        for run in &line.runs {
            let from = x;
            let Some(text) = line.text.get(run.start..run.end) else {
                continue;
            };
            let (role, size) = (Role::of(run.face), faces.size(run.face));
            let space = faces.advance(run.face, ' ') + line.word_spacing;
            for (n, word) in text.split(' ').enumerate() {
                if n > 0 {
                    x += space;
                }
                x += draw_word(ink, (role, size), x, baseline, word)?;
            }
            decorate(ink, run.face.style, size, (from, x - from), baseline)?;
        }
    }
    draw_rules(ink, &page.rules, (left, top))
}

/// One word drawn without the characters that are not drawn (zero-width and
/// control ones): copied only where one is present, fallibly. The advance.
fn draw_word(
    ink: &mut impl Ink,
    (role, size): (Role, f32),
    x: f32,
    baseline: i32,
    word: &str,
) -> Result<f32, Error> {
    if !word.chars().any(paperpaw_sdk::text::is_invisible) {
        return if word.is_empty() {
            Ok(0.0)
        } else {
            ink.run(role, size, x, baseline, word)
        };
    }
    let mut visible = alloc::string::String::new();
    visible
        .try_reserve(word.len())
        .map_err(|_| Error::NoMemory)?;
    visible.extend(
        word.chars()
            .filter(|&c| !paperpaw_sdk::text::is_invisible(c)),
    );
    if visible.is_empty() {
        return Ok(0.0);
    }
    ink.run(role, size, x, baseline, &visible)
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
    inking: Inking,
) -> Result<f32, Error> {
    font.draw_inked(canvas, size, origin, color, text, inking)
}

/// The host's reading fonts drawing on a canvas in one colour.
#[derive(Debug)]
pub struct HostInk<'a, 'h> {
    fonts: &'a HostFonts<'h>,
    canvas: &'a Canvas<'a>,
    color: Color,
    look: Look,
}

impl<'a, 'h> HostInk<'a, 'h> {
    /// Draws with `fonts` on `canvas` in `color`, inked as `look` says.
    #[must_use]
    pub const fn new(
        fonts: &'a HostFonts<'h>,
        canvas: &'a Canvas<'a>,
        color: Color,
        look: Look,
    ) -> Self {
        Self {
            fonts,
            canvas,
            color,
            look,
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
            self.look.ink(size),
        )
    }

    fn fill(&mut self, x: i32, y: i32, width: u32, height: u32) -> Result<(), Error> {
        self.canvas
            .fill_rectangle(FrameRect::new(x, y, width, height), self.color)
    }
}

#[cfg(test)]
mod tests;

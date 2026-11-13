//! A laid-out page drawn: the words of each line set one after the other with
//! the gaps the layout gave them. What the pen draws with is the [`Ink`]; the
//! host's is [`HostInk`], and the one place that decides how a run of text
//! reaches the host is [`draw_run`], which draws with the weight and the tone
//! of the letters' edges the reading settings give (`native_look`). Lines of mixed direction are set in logical
//! order; the lines under, over and through runs and a table's rules are drawn
//! (`native_decor`), and so are pictures (`canvas.draw_image`).
use crate::native_decor::{decorate, draw_rules};
use crate::native_look::Look;
use crate::native_typeset::{Faces, HostFonts, Measure, Role};
use core::ops::Range;
use paperpaw_sdk::{
    Color,
    layout::{Page, Typesetter},
    native::{
        Error,
        callbacks::FrameRect,
        canvas::Canvas,
        fonts::{Font, Ink as Inking, TextOrigin},
        image::{ImageId, ImageStyle},
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

    /// Draws the picture the host knows as `image`, scaled into the rectangle at
    /// `(x, y)`, `width` by `height` pixels (`canvas.draw_image`).
    ///
    /// # Errors
    /// The host's refusal.
    fn image(&mut self, image: u32, x: i32, y: i32, width: u32, height: u32) -> Result<(), Error>;
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
    origin: (f32, f32),
) -> Result<(), Error> {
    draw_lines_marked(ink, faces, page, origin, None)
}

/// [`draw_lines`] with the words `mark` (their numbers on the page, as
/// `native_speech::words` counts them: lines a filter inserted have none) underlined
/// where the sentence read aloud is, in one line under a run of marked words.
///
/// # Errors
/// What `ink` refuses.
pub fn draw_lines_marked<M: Measure>(
    ink: &mut impl Ink,
    faces: &Faces<M>,
    page: &Page,
    origin: (f32, f32),
    mark: Option<Range<usize>>,
) -> Result<(), Error> {
    draw_page(ink, faces, page, origin, (mark, Pictures::Drawn))
}

/// What a page's pictures are in a frame (#252): drawn, or their places kept
/// empty, each exactly the size of its picture, so that nothing moves when the
/// pictures follow in the next frame (the text first: a page opens fast).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pictures {
    /// The pictures are drawn.
    Drawn,
    /// Their places stay empty (the first of two passes).
    Places,
}

/// [`draw_lines_marked`] with the pictures drawn or their places kept empty.
/// The text is drawn the same either way.
///
/// # Errors
/// What `ink` refuses.
pub fn draw_page<M: Measure>(
    ink: &mut impl Ink,
    faces: &Faces<M>,
    page: &Page,
    (left, top): (f32, f32),
    (mark, pictures): (Option<Range<usize>>, Pictures),
) -> Result<(), Error> {
    let mut number: usize = 0;
    for line in &page.lines {
        // Rounded to the row (the page is below the top of the screen: never negative).
        let baseline = (top + line.baseline + 0.5) as i32;
        let mut x = left + line.x;
        let mut under = Under::default();
        for run in &line.runs {
            let from = x;
            let Some(text) = line.text.get(run.start..run.end) else {
                continue;
            };
            let (role, size) = (Role::of(run.face), faces.size(run.face));
            let space = faces.advance(run.face, ' ') + line.word_spacing;
            let mut at = run.start;
            for (n, word) in text.split(' ').enumerate() {
                if n > 0 {
                    x += space;
                    at += 1;
                }
                let start = x;
                x += if run.face.style.small_caps() {
                    draw_small_caps(ink, (role, size), x, baseline, word)?
                } else {
                    draw_word(ink, (role, size), x, baseline, word)?
                };
                if !word.is_empty() && !line.inserted {
                    // A word goes on from the run before where no space is between.
                    if at == 0 || line.text.as_bytes().get(at - 1) == Some(&b' ') {
                        number += 1;
                    }
                    let marked = mark
                        .as_ref()
                        .is_some_and(|m| number.checked_sub(1).is_some_and(|i| m.contains(&i)));
                    under.word(ink, marked, (start, x), (baseline, size))?;
                }
                at += word.len();
            }
            decorate(ink, run.face.style, size, (from, x - from), baseline)?;
        }
        under.end(ink)?;
    }
    draw_rules(ink, &page.rules, (left, top))?;
    match pictures {
        Pictures::Drawn => draw_images(ink, page, (left, top)),
        Pictures::Places => Ok(()),
    }
}

/// The boxes the page's pictures take on the screen, with the content box's
/// top left at `(left, top)`: `(x, y, width, height)` in pixels, rounded as
/// they are drawn.
pub fn picture_boxes(
    page: &Page,
    (left, top): (f32, f32),
) -> impl Iterator<Item = (i32, i32, u32, u32)> + '_ {
    page.images
        .iter()
        .map(move |image| place(image, (left, top)))
}

/// The image id and the format's index of a picture's path (`handle:index`,
/// `native_block::picture_path`; a path of a handle only has no index).
/// None for a path that is neither.
#[must_use]
pub fn picture_of(path: &str) -> Option<(u32, Option<u32>)> {
    match path.split_once(':') {
        Some((handle, index)) => Some((handle.parse().ok()?, Some(index.parse().ok()?))),
        None => Some((path.parse().ok()?, None)),
    }
}

/// Where a picture is drawn: its box rounded to pixels.
fn place(image: &paperpaw_sdk::layout::PageImage, (left, top): (f32, f32)) -> (i32, i32, u32, u32) {
    (
        (left + image.x + 0.5) as i32,
        (top + image.y + 0.5) as i32,
        (image.width + 0.5) as u32,
        (image.height + 0.5) as u32,
    )
}

/// The page's pictures, each scaled into the box the layout gave it. A picture
/// whose handle the host no longer knows (it ended; the document is asked again
/// at the next layout) or a host that draws no pictures leaves its place empty.
fn draw_images(ink: &mut impl Ink, page: &Page, (left, top): (f32, f32)) -> Result<(), Error> {
    for image in &page.images {
        // Handle 0: the host had no id left to give (eight are out and none
        // drawn yet); the place stays empty, the page still draws.
        let Some((handle @ 1.., _)) = picture_of(&image.image.path) else {
            continue;
        };
        let (x, y, width, height) = place(image, (left, top));
        match ink.image(handle, x, y, width, height) {
            Err(Error::NotFound | Error::Unsupported) => {}
            other => other?,
        }
    }
    Ok(())
}

/// The line under a stretch of marked words on one line of the page: it
/// goes on over the space between two marked words.
#[derive(Default)]
struct Under {
    stretch: Option<(f32, f32, i32, f32)>,
}

impl Under {
    /// A word from `x.0` to `x.1` was drawn on `baseline` in `size` pixels.
    fn word(
        &mut self,
        ink: &mut impl Ink,
        marked: bool,
        x: (f32, f32),
        (baseline, size): (i32, f32),
    ) -> Result<(), Error> {
        match (&mut self.stretch, marked) {
            (Some(stretch), true) => stretch.1 = x.1,
            (_, true) => self.stretch = Some((x.0, x.1, baseline, size)),
            (_, false) => self.end(ink)?,
        }
        Ok(())
    }

    /// The stretch ends: its line is drawn, as an underline of the same
    /// geometry as the book's own (`native_decor`).
    fn end(&mut self, ink: &mut impl Ink) -> Result<(), Error> {
        let Some((from, to, baseline, size)) = self.stretch.take() else {
            return Ok(());
        };
        let thick = ((size / 16.0) as u32).max(1);
        let (left, right) = ((from + 0.5) as i32, (to + 0.5) as i32);
        ink.fill(
            left,
            baseline + ((size / 10.0) as i32).max(1),
            (right - left).max(0) as u32,
            thick,
        )
    }
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

/// One word in small capitals: its lower-case letters as capitals at
/// [`SMALL_CAPS`](crate::native_typeset::SMALL_CAPS) of `size`, the rest as
/// they are, each part drawn after the other (as the layout measured them).
fn draw_small_caps(
    ink: &mut impl Ink,
    (role, size): (Role, f32),
    x: f32,
    baseline: i32,
    word: &str,
) -> Result<f32, Error> {
    let mut width = 0.0;
    let mut part = alloc::string::String::new();
    part.try_reserve(word.len().saturating_mul(2))
        .map_err(|_| Error::NoMemory)?;
    let mut part_size = size;
    for c in word.chars() {
        if paperpaw_sdk::text::is_invisible(c) {
            continue;
        }
        let (shown, at) = crate::native_typeset::small_capital(c, true, size);
        if at.to_bits() != part_size.to_bits() && !part.is_empty() {
            width += ink.run(role, part_size, x + width, baseline, &part)?;
            part.clear();
        }
        part_size = at;
        part.try_reserve(shown.len_utf8())
            .map_err(|_| Error::NoMemory)?;
        part.push(shown);
    }
    if !part.is_empty() {
        width += ink.run(role, part_size, x + width, baseline, &part)?;
    }
    Ok(width)
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

    fn image(&mut self, image: u32, x: i32, y: i32, width: u32, height: u32) -> Result<(), Error> {
        self.canvas.draw_image(
            ImageId::new(image)?,
            FrameRect::new(x, y, width, height),
            ImageStyle::new(),
        )
    }
}

#[cfg(test)]
mod tests;

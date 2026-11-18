//! The *Add* screen's gallery: the widgets of one kind drawn as they would show
//! (as interface 6 did, as phones offer widgets), a page of them at a time. Each
//! has its name above it and its live instance below, composed in by the host
//! (`ui.compose`, `docs/abi/ui.md` 5.1). Without the host: where each preview
//! goes (`lay_out`), which page has the focus, and the made instances of the
//! page shown ([`Gallery`], over a [`Stage`] the screen implements with the host
//! and the tests with a script).
//!
//! Where a widget cannot be made, or the host will not compose it, its cell is a
//! text row (icon, name, cells); where the host does not compose at all (the X4
//! until its stack gate, `ERR_UNSUPPORTED`) the whole gallery is the list.
use alloc::vec::Vec;
use paperpaw_sdk::{
    Rect,
    native::Error,
    widgets::grid::{Cells, Geometry},
};
use paperpaw_ui_kit::mem::push;

/// Rows a widget's preview takes at most.
pub const PREVIEW_ROWS: u8 = 3;

/// A widget in the gallery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preview {
    /// Its row on the screen.
    pub row: usize,
    /// The page of the gallery it is on.
    pub page: usize,
    /// Where its name goes.
    pub title: Rect,
    /// Where the widget is drawn: as large as it would be on the page.
    pub area: Rect,
    /// The cells it would take (columns, rows).
    pub cells: (u8, u8),
}

impl Preview {
    /// Whether `point` is on the preview or its name.
    #[must_use]
    pub fn hit(&self, point: paperpaw_sdk::Point) -> bool {
        self.title.contains(point) || self.area.contains(point)
    }
}

/// The previews of `widgets` (each its row and the cells it takes) in `area`,
/// page by page: as large as each would be on the page, up to [`PREVIEW_ROWS`]
/// rows, as many to a page as fit, each with a line of `title_h` above it and a
/// `gap` round it.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn lay_out(
    widgets: &[(usize, (u8, u8))],
    geometry: &Geometry,
    area: Rect,
    title_h: u16,
    gap: u16,
) -> Result<Vec<Preview>, Error> {
    let bottom = u32::from(area.y) + u32::from(area.height);
    let mut previews = Vec::new();
    previews
        .try_reserve_exact(widgets.len())
        .map_err(|_| Error::NoMemory)?;
    let (mut page, mut y) = (0, u32::from(area.y));
    for &(row, (w, h)) in widgets {
        let cells = (w.max(1), h.clamp(1, PREVIEW_ROWS));
        let at = geometry.rect(Cells::new(0, 0, cells.0, cells.1));
        let height = u32::from(title_h) + u32::from(at.height) + 2 * u32::from(gap);
        if y + height > bottom && y > u32::from(area.y) {
            page += 1;
            y = u32::from(area.y);
        }
        let top = u16::try_from(y).unwrap_or(u16::MAX);
        let below = top.saturating_add(gap);
        previews.push(Preview {
            row,
            page,
            title: Rect::new(at.x, below, at.width, title_h),
            area: Rect::new(at.x, below.saturating_add(title_h), at.width, at.height),
            cells,
        });
        y += height;
    }
    Ok(previews)
}

/// What the host does for a gallery: the instances and their drawing.
pub trait Stage {
    /// An instance made.
    type Handle;
    /// Makes the widget `id` and tells it its place.
    ///
    /// # Errors
    /// What the host refuses.
    fn make(&mut self, id: &str, area: Rect) -> Result<Self::Handle, Error>;
    /// Lets an instance go; a refusal is of no matter (the host drops them when
    /// the screen closes).
    fn release(&mut self, widget: Self::Handle);
    /// Composes an instance into `area`.
    ///
    /// # Errors
    /// What the host refuses.
    fn compose(&mut self, widget: &Self::Handle, area: Rect) -> Result<(), Error>;
}

/// How the cells of a page came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drawn {
    /// The previews (by number) that are a text row: no instance, or one the
    /// host would not compose.
    Rows(Vec<usize>),
    /// The host composes nothing: the whole gallery is the list.
    List,
}

/// The gallery of a screen: the previews, and the instances of the page shown.
#[derive(Debug)]
pub struct Gallery<H> {
    previews: Vec<Preview>,
    /// The instances of the page `made` names, by preview number.
    made: Vec<(usize, H)>,
    /// The page the instances are of.
    page: Option<usize>,
}

impl<H> Gallery<H> {
    /// A gallery of `previews`, none made.
    #[must_use]
    pub fn new(previews: Vec<Preview>) -> Self {
        Self {
            previews,
            made: Vec::new(),
            page: None,
        }
    }

    /// The previews.
    #[must_use]
    pub fn previews(&self) -> &[Preview] {
        &self.previews
    }

    /// How many pages the gallery has.
    #[must_use]
    pub fn pages(&self) -> usize {
        self.previews.last().map_or(1, |last| last.page + 1)
    }

    /// The page the row `row` is on (the first where it is in none).
    #[must_use]
    pub fn page_of(&self, row: usize) -> usize {
        self.previews
            .iter()
            .find(|preview| preview.row == row)
            .map_or(0, |preview| preview.page)
    }

    /// The previews of `page`, with their numbers.
    pub fn on_page(&self, page: usize) -> impl Iterator<Item = (usize, &Preview)> {
        self.previews
            .iter()
            .enumerate()
            .filter(move |(_, preview)| preview.page == page)
    }

    /// The row of the first preview of `page`.
    #[must_use]
    pub fn first_row(&self, page: usize) -> Option<usize> {
        self.on_page(page).next().map(|(_, preview)| preview.row)
    }

    /// The row of the preview at `point` on `page`.
    #[must_use]
    pub fn row_at(&self, page: usize, point: paperpaw_sdk::Point) -> Option<usize> {
        self.on_page(page)
            .find(|(_, preview)| preview.hit(point))
            .map(|(_, preview)| preview.row)
    }

    /// The rows of the previews on `page`, in order.
    pub fn rows_on(&self, page: usize) -> impl Iterator<Item = usize> {
        self.on_page(page).map(|(_, preview)| preview.row)
    }

    /// Makes the instances of `page` where they are not made yet and lets those
    /// of any other page go. `id_of` names the widget of a row. A widget the
    /// host would not make is left out (its cell is a text row); `Unsupported`
    /// means the host composes nothing: the caller shows the list.
    ///
    /// # Errors
    /// [`Error::NoMemory`] where the bookkeeping cannot grow, and
    /// [`Error::Unsupported`] from the host.
    pub fn show<'a, S: Stage<Handle = H>>(
        &mut self,
        stage: &mut S,
        page: usize,
        id_of: impl Fn(usize) -> Option<&'a str>,
    ) -> Result<(), Error> {
        if self.page != Some(page) {
            self.release(stage);
            self.page = Some(page);
        }
        for (number, preview) in self.previews.iter().enumerate() {
            if preview.page != page || self.made.iter().any(|(made, _)| *made == number) {
                continue;
            }
            let Some(id) = id_of(preview.row) else {
                continue;
            };
            match stage.make(id, preview.area) {
                Ok(handle) => {
                    if self.made.try_reserve(1).is_err() {
                        stage.release(handle);
                        return Err(Error::NoMemory);
                    }
                    self.made.push((number, handle));
                }
                Err(Error::Unsupported) => return Err(Error::Unsupported),
                // Full, no memory, a trap, gone: a text row in its place.
                Err(_) => {}
            }
        }
        Ok(())
    }

    /// Composes the instances of the page made. What is not composed comes back
    /// as a text row.
    pub fn compose<S: Stage<Handle = H>>(&self, stage: &mut S, page: usize) -> Drawn {
        let mut rows = Vec::new();
        for (number, preview) in self.on_page(page) {
            let composed = match self.made.iter().find(|(made, _)| *made == number) {
                Some((_, handle)) => stage.compose(handle, preview.area),
                None => Err(Error::NotFound),
            };
            match composed {
                Ok(()) => {}
                Err(Error::Unsupported) => return Drawn::List,
                Err(_) => {
                    // Over its own memory the list of failures is the cells that
                    // were not drawn: the rest still show.
                    let _ = push(&mut rows, number);
                }
            }
        }
        Drawn::Rows(rows)
    }

    /// Lets every instance go (a page left, the screen closed).
    pub fn release<S: Stage<Handle = H>>(&mut self, stage: &mut S) {
        for (_, handle) in self.made.drain(..) {
            stage.release(handle);
        }
        self.page = None;
    }
}

#[cfg(test)]
mod tests;

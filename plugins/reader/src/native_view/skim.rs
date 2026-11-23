//! The skim screen's moves on the book on show (`native_skim::Move`), and where
//! the book is as that screen shows it, as Beta 3's skim did: ten pages on or
//! back, a chapter, a place on the bar, back to where skimming began.
use super::{Body, Documents, Error, Host, View};
use crate::native_goto::Jump;
use crate::native_skim::Move;
use alloc::string::String;
use paperpaw_sdk::native::library::Library;
use paperpaw_ui_kit::text;

/// Where the book is, as the skim screen shows it.
#[derive(Debug, Default)]
pub struct Place {
    /// The page ("12 / 300"), else the percentage, as the menu bar names it.
    pub label: String,
    /// The title of the chapter the page is in (empty for none).
    pub chapter: String,
    /// How far into the book, in thousandths.
    pub permille: String,
}

impl View {
    /// Where the open book is for the skim screen; none where no book is open.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn skim_place(&self, host: &Host<'_>) -> Result<Option<Place>, Error> {
        let Body::Open(open) = &self.body else {
            return Ok(None);
        };
        let mut digits = [0; 20];
        let chapter = open.chapter.as_ref().map_or("", |c| c.title());
        Ok(Some(Place {
            label: open.place_label(host)?,
            chapter: text::copy(chapter)?,
            permille: text::copy(text::decimal_u64(u64::from(open.ppm() / 1000), &mut digits))?,
        }))
    }

    /// The start of the page on show, where a book is open.
    #[must_use]
    pub fn place(&self) -> Option<(u32, u32)> {
        match &self.body {
            Body::Open(open) => Some(open.at),
            _ => None,
        }
    }

    /// Carries out the skim screen's `go` on the book: `Back` goes to `began`
    /// (where skimming began). The page moves as a turn or a jump does: it is
    /// reported, saved and drawn again. `Number` is the plugin's (the
    /// go-to-page dialog).
    ///
    /// # Errors
    /// `NoMemory`, and the host's refusal to redraw.
    pub fn skim(
        &mut self,
        host: &Host<'_>,
        (docs, library): (&Documents, Option<&Library>),
        go: Move,
        began: Option<(u32, u32)>,
    ) -> Result<(), Error> {
        match go {
            Move::Pages(pages) => self.skim_pages(host, (docs, library), pages),
            Move::Chapters(chapters) => {
                let Body::Open(open) = &mut self.body else {
                    return Ok(());
                };
                for _ in 0..chapters.unsigned_abs() {
                    open.step_chapter(host, (docs, library), chapters > 0)?;
                }
                Ok(())
            }
            Move::Permille(permille) => self
                .jump(host, docs, Jump::Permille(permille.min(999)))
                .map(drop),
            Move::Back => match began {
                Some(at) => self.go_to(host, docs, at),
                None => Ok(()),
            },
            _ => Ok(()),
        }
    }

    /// `pages` pages on (or back): to that page of those counted, else turned
    /// one by one (the first or the last page stops it).
    fn skim_pages(
        &mut self,
        host: &Host<'_>,
        (docs, library): (&Documents, Option<&Library>),
        pages: i32,
    ) -> Result<(), Error> {
        let Body::Open(open) = &mut self.body else {
            return Ok(());
        };
        if let Some(numbers) = open.page_numbers(host) {
            let to = page_after(numbers, pages);
            return self.jump(host, docs, Jump::Page(to)).map(drop);
        }
        let mut moved = false;
        for _ in 0..pages.unsigned_abs() {
            let went = if pages > 0 {
                open.forward(host, docs)?
            } else {
                open.previous(host, docs)?
            };
            if !went {
                break;
            }
            moved = true;
        }
        if moved {
            open.after_turn(host, (docs, library))?;
        }
        Ok(())
    }
}

/// The page (from 0) `pages` on from page `page` of `counted` (from 1), kept
/// to those counted.
fn page_after((page, counted): (u32, u32), pages: i32) -> u32 {
    let last = i64::from(counted.max(1)) - 1;
    let to = (i64::from(page) - 1 + i64::from(pages)).clamp(0, last);
    u32::try_from(to).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::page_after;

    #[test]
    fn ten_pages_on_or_back_stop_at_the_ends() {
        // Page 12 of 300 (from 1) is page 11 in a jump (from 0).
        assert_eq!(page_after((12, 300), 10), 21);
        assert_eq!(page_after((12, 300), -10), 1);
        assert_eq!(page_after((5, 300), -10), 0);
        assert_eq!(page_after((295, 300), 10), 299);
        assert_eq!(page_after((1, 0), 10), 0);
    }
}

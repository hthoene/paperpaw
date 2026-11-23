//! The page shown after the screen turned.
use super::{Dashboard, Error, HostSource, Nav, native_pages};

/// The page of `pages` to lay out and how many there are, `nav` set to it: the
/// one `wanted` names by its slot (opened to edit a page's setting; taken),
/// else the one the screen's turn leads to (`turned`), else the one shown.
pub(super) fn page_to_show(
    (nav, wanted): (&mut Option<Nav>, &mut Option<usize>),
    pages: &[native_pages::Page],
    turned: Option<usize>,
) -> (usize, usize) {
    let wanted = wanted
        .take()
        .and_then(|slot| pages.iter().position(|p| p.slot == slot));
    let nav = nav.get_or_insert(Nav::new(pages.len()));
    nav.resize(pages.len());
    if let Some(at) = wanted.or(turned) {
        nav.page = at;
    }
    (nav.page, nav.pages)
}

impl Dashboard {
    /// The screen turned: the page that shows the first item of the one before,
    /// as in Beta 3 (the pages are others on the side); none where it did not.
    #[inline(never)]
    pub(super) fn page_after_turn(
        &self,
        src: &HostSource,
        pages: &[native_pages::Page],
        wide: bool,
    ) -> Result<Option<usize>, Error> {
        let first = self
            .laid
            .as_ref()
            .filter(|before| before.wide != wide)
            .and_then(|before| before.items.first());
        let Some(first) = first else {
            return Ok(None);
        };
        for (at, page) in pages.iter().enumerate() {
            let items = native_pages::expand(src, &page.items)?;
            if items.iter().any(|item| item.what == first.what) {
                return Ok(Some(at));
            }
        }
        Ok(None)
    }
}

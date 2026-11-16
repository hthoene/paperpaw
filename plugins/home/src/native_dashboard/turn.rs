//! The page shown after the screen turned.
use super::{Dashboard, Error, HostSource, native_pages};

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

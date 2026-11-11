//! The page count of the open book (view slice R2b): the background steps
//! that lay pages out for `native_index`, and the page numbers the host's
//! report and the status lines carry once the count is complete.

use paperpaw_sdk::document::Locator;
use paperpaw_sdk::native::device::{Device, DeviceClass};
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_ui_kit::mem::buffer;

use super::{Body, Documents, Host, Open, Setup, View};
use crate::native_chapter::{self, Chapter};
use crate::native_index::{self, Counter, Laid, Step};

impl Open {
    /// The count of the layout now: the one open is kept if it is this
    /// layout's, else its file is removed and the layout's own is opened.
    pub(super) fn ensure_counter(&mut self, host: &Host<'_>, setup: &Setup) {
        let key = setup.key();
        let display = (key.display.0, key.display.1, key.display.2);
        let Ok(layout) = native_index::layout_text(&setup.reading, display, self.whole_pages)
        else {
            return;
        };
        let Ok(name) = native_index::file_name(&self.path, self.conversion, &layout) else {
            return;
        };
        if self.counter.as_ref().is_some_and(|c| c.name() == name) {
            return;
        }
        let storage = DataStorage::new(host);
        // The file of the layout left stays among the few kept.
        self.counter = None;
        native_index::keep(&storage, &name);
        self.counter = Counter::open(&storage, name).ok();
    }

    /// A book just opened: its pages are to be counted, and (a book of the
    /// library) it is the one read last.
    pub(super) fn begin(&mut self, host: &Host<'_>, library_book: bool) {
        if let Ok(setup) = Setup::read(host) {
            self.ensure_counter(host, &setup);
        }
        if library_book {
            crate::native_start::remember(host, &self.path);
        }
    }

    /// One step of the count; the wait before the next one while pages remain.
    pub(super) fn count_step(&mut self, host: &Host<'_>, docs: &Documents) -> Option<u32> {
        let setup = Setup::read(host).ok()?;
        self.ensure_counter(host, &setup);
        let mut counter = self.counter.take()?;
        let (per_step, delay) = native_index::pace(self.device_class(host));
        let storage = DataStorage::new(host);
        let step = counter.step(&storage, per_step, &mut |at: Locator| {
            let (page, unreliable) =
                self.lay_flagged(host, docs, &setup, (at.section, at.offset))?;
            Ok(Laid {
                start: page.start,
                end: page.end,
                past_end: page.is_past_end(),
                unreliable,
            })
        });
        self.counter = Some(counter);
        match step {
            Step::More => Some(delay),
            Step::Done => {
                // The counters appear: the host is told the page and the pages.
                self.announce(host, docs);
                None
            }
            Step::Stopped => None,
        }
    }

    /// The device's memory class, asked once (none where the host does not
    /// tell: the pace of a small device).
    fn device_class(&mut self, host: &Host<'_>) -> Option<DeviceClass> {
        if !self.class_asked {
            self.class_asked = true;
            self.class = read_class(host);
        }
        self.class
    }

    /// The number of the page on show and of the pages of the book, once the
    /// count is complete and the page starts where one of its pages does.
    pub(super) fn page_numbers(&self, host: &Host<'_>) -> Option<(u32, u32)> {
        let counter = self.counter.as_ref()?;
        counter.page_at(&DataStorage::new(host), self.at)
    }

    /// The pages after page `page` (of `pages`, from 1) in `chapter`, once the
    /// count is complete; `None` for a chapter without a title (the host takes
    /// no count without a chapter).
    pub(super) fn chapter_left(
        &self,
        host: &Host<'_>,
        chapter: &Chapter,
        (page, pages): (u32, u32),
    ) -> Option<u32> {
        let counter = self.counter.as_ref()?;
        if chapter.title().is_empty() {
            return None;
        }
        let next = match chapter.next() {
            Some(at) => Some(counter.locate(&DataStorage::new(host), at)?),
            None => None,
        };
        native_chapter::pages_left(page, pages, next)
    }

    /// Whether pages remain to be counted.
    pub(super) fn counting(&self) -> bool {
        self.counter.as_ref().is_some_and(Counter::counting)
    }
}

impl View {
    /// One step of the count of the book on show: the wait before the next
    /// step, if pages remain.
    pub fn count_step(&mut self, host: &Host<'_>, docs: &Documents) -> Option<u32> {
        match &mut self.body {
            Body::Open(open) => open.count_step(host, docs),
            Body::Message(_) => None,
        }
    }

    /// Whether this screen's pages still need counting.
    #[must_use]
    pub fn counting(&self) -> bool {
        matches!(&self.body, Body::Open(open) if open.counting())
    }

    /// The pages are to be counted for the layout now (a new book, a changed
    /// layout).
    pub fn prepare_count(&mut self, host: &Host<'_>) {
        if let Body::Open(open) = &mut self.body
            && let Ok(setup) = Setup::read(host)
        {
            open.ensure_counter(host, &setup);
        }
    }
}

/// The memory class `core.device` tells.
fn read_class(host: &Host<'_>) -> Option<DeviceClass> {
    let device = Device::open(host).ok()?;
    let (mut input, mut out) = (buffer(256).ok()?, buffer(1024).ok()?);
    let identity = device.device(host, &mut input, &mut out).ok()?;
    Some(identity.class())
}

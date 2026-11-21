//! The page count of the open book (view slice R2b): the background steps
//! that lay pages out for `native_index`, and the page numbers the host's
//! report and the status lines carry once the count is complete.

use paperpaw_sdk::digest::fnv1a64;
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::native::device::{Device, DeviceClass};
use paperpaw_sdk::native::files::Files;
use paperpaw_sdk::native::registry::{PageCursor, Registry, RegistryPluginState};
use paperpaw_sdk::native::storage::{DataStorage, Root};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};

use super::{Body, Documents, Host, Open, Setup, View};
use crate::native_chapter::{self, Chapter};
use crate::native_index::{self, Counter, Edition, Laid, Step};

/// What the count of an open book asks the host only once.
#[derive(Debug, Default)]
pub(super) struct Asked {
    /// The device's memory class (none where the host does not tell).
    class: Option<Option<DeviceClass>>,
    /// The book's file and the packs running, for the index's name
    /// ([`Edition`]); asked again once a pack came (`packs_changed`).
    file: Option<(u64, Option<u64>)>,
    packs: Option<u64>,
    /// The index was thrown away while the book is open (`distrust_index`).
    mended: bool,
    /// And its count is to be started again (`take_recount`).
    recount: bool,
}

/// The most pages of plugins the registry is asked for.
const MOST_PLUGIN_PAGES: usize = 16;
/// The input of a registry call.
const REGISTRY_INPUT: usize = 256;

impl Open {
    /// The count of the layout now: the one open is kept if it is this
    /// layout's, else its file is removed and the layout's own is opened.
    pub(super) fn ensure_counter(&mut self, host: &Host<'_>, setup: &Setup) {
        let key = self.key(setup);
        let display = (key.display.0, key.display.1, key.display.2);
        let Ok(layout) =
            native_index::layout_text(&setup.reading, display, self.whole_pages, key.filtered)
        else {
            return;
        };
        let edition = self.edition(host);
        let Ok(name) = native_index::file_name(&self.path, edition, &layout) else {
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
        if let Ok(setup) = self.settings.setup(host) {
            self.ensure_counter(host, &setup);
        }
        if library_book {
            crate::native_start::remember(host, &self.path);
        }
    }

    /// One step of the count; the wait before the next one while pages remain.
    pub(super) fn count_step(&mut self, host: &Host<'_>, docs: &Documents) -> Option<u32> {
        let setup = self.settings.setup(host).ok()?;
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
        *self.count.class.get_or_insert_with(|| read_class(host))
    }

    /// The book's edition for the name of its index, its file and the packs
    /// asked once (a file the card cannot tell of is size 0, as in Beta 3).
    fn edition(&mut self, host: &Host<'_>) -> Edition {
        let path = &self.path;
        let (size, modified) = *self
            .count
            .file
            .get_or_insert_with(|| book_file(host, path).unwrap_or((0, None)));
        let packs = *self.count.packs.get_or_insert_with(|| packs_running(host));
        Edition {
            conversion: self.conversion,
            size,
            modified,
            packs,
        }
    }

    /// A forward turn found no page after the one on show though the count
    /// says there are more: the index counted pages this layout does not make
    /// (counted while a letter measured otherwise). As Beta 3's
    /// `distrust_index`, it is thrown away and the pages are counted again,
    /// once while the book is open, so that the count is right and no page is
    /// promised that does not come.
    pub(super) fn distrust_index(&mut self, host: &Host<'_>) {
        let storage = DataStorage::new(host);
        let promised = self
            .counter
            .as_ref()
            .is_some_and(|counter| counter.promises_more(&storage, self.at));
        if self.count.mended || !promised {
            return;
        }
        self.count.mended = true;
        if let Some(counter) = self.counter.take() {
            counter.forget(&storage);
        }
        if let Ok(setup) = self.settings.setup(host) {
            self.ensure_counter(host, &setup);
            self.count.recount = true;
        }
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
            Body::Message(_) | Body::Preparing(_) => None,
        }
    }

    /// The page after the one on show is laid out for the next turn.
    pub fn lay_ahead(&mut self, host: &Host<'_>, docs: &Documents) {
        if let Body::Open(open) = &mut self.body {
            open.lay_ahead(host, docs);
        }
    }

    /// A pack of letters came: the packs are asked again for the name of the
    /// index, so that the pages are counted with the letters there are now.
    pub fn packs_changed(&mut self) {
        if let Body::Open(open) = &mut self.body {
            open.count.packs = None;
        }
    }

    /// Whether the count was started again by the last input (its index
    /// thrown away, `distrust_index`): its steps are to be asked for. Asked
    /// once.
    pub fn take_recount(&mut self) -> bool {
        match &mut self.body {
            Body::Open(open) => core::mem::take(&mut open.count.recount),
            Body::Message(_) | Body::Preparing(_) => false,
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
            && let Ok(setup) = open.settings.setup(host)
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

/// The size and time of the book's file on the card.
fn book_file(host: &Host<'_>, path: &str) -> Option<(u64, Option<u64>)> {
    let files = Files::open(host).ok()?;
    // The stat answers up to 4,096 bytes and the host asks for room for it.
    let found = match (buffer(1024), buffer(4096)) {
        (Ok(mut input), Ok(mut out)) => files
            .stat(host, Root::Files, path, &mut input, &mut out)
            .ok()
            .map(|entry| (entry.size(), entry.modified())),
        _ => None,
    };
    files.release(host).ok();
    found
}

/// The packs of letters, fonts and hyphenation running, as one hash of their
/// ids and versions in no order: another pack, other widths or breaks. 0
/// where the registry cannot tell.
fn packs_running(host: &Host<'_>) -> u64 {
    let Ok(registry) = Registry::open(host) else {
        return 0;
    };
    let packs = (|| {
        let (mut input, mut out) = (buffer(REGISTRY_INPUT).ok()?, buffer(SERVICE_BUFFER).ok()?);
        let mut sum = 0u64;
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PLUGIN_PAGES {
            let page = registry.plugins(host, cursor, &mut input, &mut out).ok()?;
            for plugin in page.rows() {
                let id = plugin.id();
                let letters = id.starts_with("font-")
                    || id.starts_with("script-")
                    || crate::native_script_hint::is_pack(id);
                if letters && plugin.state() == RegistryPluginState::Running {
                    sum = sum.wrapping_add(fnv1a64(id) ^ fnv1a64(plugin.version()).rotate_left(1));
                }
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Some(sum)
    })()
    .unwrap_or(0);
    registry.release(host).ok();
    packs
}

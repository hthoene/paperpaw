//! *Look for new books* (#277): the library's menu offers it, and running it
//! has the kernel make the catalog anew, so a book copied onto the card while
//! PaperPaw runs is in the library without a restart.
#![allow(clippy::unwrap_used, reason = "test assertions")]

mod native_activation;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::{Id, MemoryStorage, Params, Storage};

const CATALOG: &str = ".paperpaw/library/catalog.txt";

struct App {
    kernel: Kernel,
    card: std::rc::Rc<dyn Storage>,
    now: u64,
}

impl App {
    fn new() -> Self {
        let device = DeviceInfo::new(
            "test-x4",
            "Test X4",
            DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg90, None),
            InputCaps::new(true, false, false, &[]),
            MemoryClass::Small,
        );
        let card = native_activation::SharedCard::new(
            MemoryStorage::new()
                .with(".paperpaw/plugins/setup/done", "")
                .with("books/first.txt", "one"),
        );
        let builder = Kernel::builder(device)
            .storage(card.clone())
            .user_config("[help]\nshow=false\n[core]\nlanguage=en\n[reader]\nopen-last=false\n");
        let builder = paperpaw_app::install(builder, paperpaw_app::Fonts::Packs);
        let handle = card.handle();
        let mut kernel = native_activation::build_on(builder, card.handle());
        let effects = kernel.start(Instant(0));
        let mut app = Self {
            kernel,
            card: handle,
            now: 0,
        };
        app.settle(effects);
        app
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else { return };
            if at.0 > self.now + 61_000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    fn catalog(&self) -> String {
        let mut bytes = vec![0; 64 * 1024];
        let n = self.card.read_at(CATALOG, 0, &mut bytes).unwrap_or(0);
        String::from_utf8_lossy(&bytes[..n]).into_owned()
    }
}

#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the native files plugin and a reader's formats"
)]
fn look_for_new_books_is_in_the_library_menu_and_finds_a_book_copied_in() {
    let mut app = App::new();
    app.now += 100;
    let menu = Params::new().with(params::MENU, "files.menu");
    let effects = app.kernel.open_slot(Instant(app.now), slots::MENU, menu);
    app.settle(effects);
    app.kernel.render(&mut |_, _| {});
    let rows: Vec<String> = app.kernel.describe().into_iter().map(|p| p.label).collect();
    assert!(
        rows.iter().any(|row| row.contains("Look for new books")),
        "{rows:?}"
    );
    // A book copied onto the card with no word to anyone; the entry runs the
    // kernel's action and the library has it.
    app.card.write("books/second.txt", b"two").unwrap();
    assert!(!app.catalog().contains("second.txt"));
    app.now += 100;
    let effects = app
        .kernel
        .run_action(Instant(app.now), Id::new_static("core.library.look-again"));
    app.settle(effects);
    assert!(app.catalog().contains("second.txt"), "{}", app.catalog());
}

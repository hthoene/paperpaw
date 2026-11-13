//! The reading aids in the native reader, end to end (#257 item 14): aids
//! answers the reader's `reader.filter` stage with the rules of its settings,
//! and the reader applies them to the book's blocks before it lays them out
//! (`plugins/reader/src/native_filter.rs`). The page a rule changes is drawn
//! otherwise than the plain one; with no rule it is drawn the same.
//!
//! The app leaves out the store's interface-6 plugins (`StorePlugins::Left`):
//! no interface-6 aids fills the kernel's text filters, so only the native
//! aids (`native-linked-store`) can change the page, through the native
//! reader.
#![cfg(all(feature = "native-linked-reading", feature = "native-linked-store"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod native_activation;

use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::{MemoryStorage, Params};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &["back", "confirm", "left", "right"]),
    MemoryClass::Small,
);

const BOOK: &str = "books/aids.txt";
/// A page of words to set in bold and to replace.
const TEXT: &str = "Alice was beginning to get very tired of sitting by her sister on the \
bank, and of having nothing to do. Alice looked at the book her sister was reading, \
but it had no pictures or conversations in it.\n";

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

/// The book's first page as the native reader draws it, with the aids'
/// settings `aids` (lines of the `[aids]` section).
fn first_page(aids: &str) -> Vec<u8> {
    let card = || MemoryStorage::new().with(BOOK, TEXT);
    let storage = card().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE).storage(storage).user_config(format!(
        "[core]\nlanguage = en\n[help]\nshow = false\n[reader]\nopen-last = false\n[aids]\n{aids}"
    ));
    // The linked plugins read the book from a card of their own with it.
    let mut kernel = native_activation::build_on(
        paperpaw_app::install_with(builder, fonts(), paperpaw_app::StorePlugins::Left),
        Rc::new(card()),
    );
    let mut now = 0;
    let mut effects = kernel.start(Instant(now));
    let settle = |kernel: &mut Kernel, mut effects: Effects, now: &mut u64| {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else { return };
            if at.0 > *now + 1000 {
                return;
            }
            *now = (*now).max(at.0);
            effects = kernel.tick(Instant(*now));
        }
    };
    settle(&mut kernel, effects, &mut now);
    now += 100;
    effects = kernel.open_slot(
        Instant(now),
        slots::READER,
        Params::new().with(params::PATH, BOOK),
    );
    settle(&mut kernel, effects, &mut now);
    assert_eq!(
        kernel.active_screen().map(|id| id.as_str().to_string()),
        Some("reader.screen".to_string()),
        "the book opens in the native reader"
    );
    let display = DEVICE.display;
    let width = usize::from(display.width);
    let mut image = vec![255u8; width * usize::from(display.height)];
    kernel.render(&mut |y, view| {
        for row in 0..view.height {
            for x in 0..view.width {
                let at = usize::from(y + row) * width + usize::from(x);
                if let Some(pixel) = image.get_mut(at) {
                    *pixel = view.pixel(x, row).luma();
                }
            }
        }
    });
    image
}

#[test]
#[ignore = "needs native activation (#243; the native reader runs reader.filter and applies native_filter: Plugin wires it after #1284, #257)"]
fn bold_word_beginnings_change_the_page() {
    let plain = first_page("bold-beginnings = false\n");
    assert_eq!(
        plain,
        first_page("bold-beginnings = false\n"),
        "drawn alike"
    );
    assert_ne!(plain, first_page("bold-beginnings = true\n"));
}

#[test]
#[ignore = "needs native activation (#243; the native reader runs reader.filter and applies native_filter: Plugin wires it after #1284, #257)"]
fn replaced_words_change_the_page() {
    let plain = first_page("");
    assert_ne!(plain, first_page("replace = \"Alice = Bob\"\n"));
    // A word the book does not have changes nothing.
    assert_eq!(plain, first_page("replace = \"Zebra = Bob\"\n"));
}

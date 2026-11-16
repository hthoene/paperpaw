//! The first screen opens on a host that has no backend for the device's
//! firmware, built-in plugins or native facts (the desktop shell): the host
//! answers `Unsupported`, and a fact that is missing must never stop the page
//! from opening (Beta 3 showed the page without those rows).
#![cfg(feature = "linked")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test"
)]
#[path = "../../test-support/hosted.rs"]
mod hosted;

use paperpaw_sdk::{Id, Instant, Params};

/// The plugin's two catalogs as the kernel reads them: the production host does not load a
/// native plugin's `i18n/*.ini` yet, so a test-only plugin registers them (as in the battery's
/// hosted test).
fn catalogs(builder: paperpaw_core::KernelBuilder) -> paperpaw_core::KernelBuilder {
    let manifest = paperpaw_core::sdk::Manifest::new(
        Id::new_static("about"),
        "About catalogs",
        paperpaw_core::sdk::Version::new(1, 0, 3),
    );
    builder.items(manifest, |items| {
        for text in [
            include_str!("../i18n/en.ini"),
            include_str!("../i18n/de.ini"),
        ] {
            items.add_translations(std::borrow::Cow::Borrowed(text))?;
        }
        Ok(())
    })
}

#[test]
fn the_page_opens_where_the_device_answers_unsupported() {
    let mut installed = hosted::install_in(
        &hosted::Setup::x4().with_kernel(catalogs),
        paperpaw_plugin_about::NATIVE_ENTRY.entry(),
        "about",
        &["core.device"],
    )
    .expect("installed");
    let kernel = installed.kernel();
    kernel.start(Instant(100));
    kernel.open_screen(
        Instant(200),
        Id::try_parse("about.screen").unwrap(),
        Params::default(),
    );
    let top = kernel.active_screen().map(|id| id.as_str().to_string());
    assert_eq!(top.as_deref(), Some("about.screen"));
    assert_eq!(installed.dropped(), Vec::<(u32, i32)>::new());
}

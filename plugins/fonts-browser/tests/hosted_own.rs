//! *Your own fonts* through the kernel's native host: the action writes the
//! part of *How it works* after its heading to `own.txt` in the plugin's data
//! folder, in the language shown, and hands it to the reader; nothing dropped.
#![cfg(feature = "linked")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test"
)]
#[path = "../../test-support/hosted.rs"]
mod hosted;

use paperpaw_sdk::{Env, Id, Instant, MemoryStorage};

/// The plugin's catalogs as the kernel reads them: the production host does not load a
/// native plugin's `i18n/*.ini` yet, so a test-only plugin registers them (as in About's
/// hosted test).
struct Catalogs;
impl paperpaw_sdk::Plugin for Catalogs {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            Id::new_static("fonts-browser"),
            "Fonts catalogs",
            paperpaw_sdk::Version::new(1, 0, 8),
        )
    }
    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        for text in [
            include_str!("../i18n/en.ini"),
            include_str!("../i18n/de.ini"),
        ] {
            r.add_translations(std::borrow::Cow::Borrowed(text))?;
        }
        Ok(())
    }
}

/// What *Your own fonts* writes, with the user's configuration `config`.
fn own_page(config: &str) -> String {
    let setup = hosted::Setup::x4()
        .config(config)
        .with_kernel(|b| b.storage(MemoryStorage::default()).plugin(Catalogs));
    let mut installed = hosted::install_in(
        &setup,
        paperpaw_plugin_fonts_browser::NATIVE_ENTRY.entry(),
        "fonts-browser",
        &["core.device", "core.net", "core.registry", "core.ui"],
    )
    .expect("installed");
    let kernel = installed.kernel();
    kernel.start(Instant(100));
    kernel.run_action(Instant(200), Id::try_parse("fonts-browser.own").unwrap());
    let path = format!("{}/own.txt", Env::plugin_dir("fonts-browser"));
    let mut buf = vec![0u8; 8192];
    let n = installed
        .kernel()
        .storage()
        .read_at(&path, 0, &mut buf)
        .expect("own.txt written");
    assert_eq!(installed.dropped(), Vec::<(u32, i32)>::new());
    String::from_utf8(buf[..n].to_vec()).unwrap()
}

/// The part of `how` after `heading`, as Beta 3 showed it under `title`.
fn beta3(how: &str, heading: &str, title: &str) -> String {
    let at = how.find(heading).unwrap() + heading.len();
    format!("{title}\n\n{}\n", how[at..].trim_start())
}

#[test]
fn your_own_fonts_writes_the_english_part() {
    let page = own_page("");
    let how = include_str!("../how/en.txt");
    assert_eq!(page, beta3(how, "YOUR OWN FONTS", "Your own fonts"));
}

#[test]
fn your_own_fonts_writes_the_german_part() {
    let page = own_page("[core]\nlanguage = de\n");
    let how = include_str!("../how/de.txt");
    assert_eq!(page, beta3(how, "EIGENE SCHRIFTEN", "Eigene Schriften"));
}

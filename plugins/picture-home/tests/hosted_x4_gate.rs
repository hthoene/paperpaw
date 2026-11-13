//! The page on the X4 while its nested callbacks are closed (#250): the host
//! lays no other plugin's widget out there (`ERR_UNSUPPORTED` for the widget
//! ops that call into the widget's owner), and every widget of the default
//! page is home's. The page still shows: the picture behind it and the
//! cells, left empty; no callback of the page fails. Full parity on the X4
//! comes when the gate opens after the device's stack check.
#![cfg(feature = "linked")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test"
)]
#[path = "../../test-support/hosted.rs"]
mod hosted;

use paperpaw_core::native_host::FixedTables;
use paperpaw_platform::FrameView;
use paperpaw_sdk::{Id, Instant, MemoryStorage, Params};

/// The two plugins' catalogs as the kernel reads them: the production host does
/// not load a native plugin's `i18n/*.ini` yet, so a test-only plugin registers
/// them (as in About's hosted test).
struct Catalogs(&'static str, [&'static str; 2]);
impl paperpaw_sdk::Plugin for Catalogs {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            Id::new_static(self.0),
            "Catalogs",
            paperpaw_sdk::Version::new(1, 0, 2),
        )
    }
    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        for text in self.1 {
            r.add_translations(std::borrow::Cow::Borrowed(text))?;
        }
        Ok(())
    }
}

const WIDE: &[u8] = include_bytes!("data/wide.png");

/// The picture fills the screen; the default page (home's clock, the book to
/// continue and the recent ones) stays as it is.
const CONFIG: &str = "[plugins]\npicture-home = on\n\
    [picture-home]\nfolder = pics\npicture = wide.png\nfit = picture-home.fit.fill\n";

/// The X4's screen in two bits a pixel, kept up to date from what the kernel
/// draws anew.
struct Screen(Vec<u8>);

impl Screen {
    const STRIDE: usize = 480 / 4;

    fn new() -> Self {
        Self(vec![0; Self::STRIDE * 800])
    }

    fn render(&mut self, kernel: &mut paperpaw_core::Kernel) {
        kernel.render(&mut |top, view: FrameView<'_>| {
            for (n, row) in view.data.chunks(view.stride).enumerate() {
                let at = (usize::from(top) + n) * Self::STRIDE;
                if let Some(to) = self.0.get_mut(at..at + Self::STRIDE) {
                    to.copy_from_slice(&row[..Self::STRIDE]);
                }
            }
        });
    }

    /// The pixels that are not the paper (the most common value).
    fn inked(&self) -> usize {
        let mut counts = [0usize; 4];
        for byte in &self.0 {
            for shift in [0, 2, 4, 6] {
                counts[usize::from((byte >> shift) & 3)] += 1;
            }
        }
        let paper = counts.iter().copied().max().unwrap_or(0);
        counts.iter().sum::<usize>() - paper
    }
}

#[test]
fn the_page_shows_with_the_picture_and_empty_cells_while_the_gate_is_closed() {
    let setup = hosted::Setup::x4()
        .config(CONFIG)
        // The screen stack and the widgets, sized for the X4 as its shell wires them.
        .wired(|w| w.ui(paperpaw_sdk::MemoryClass::Small))
        .with_kernel(|b| {
            b.storage(MemoryStorage::new().with("pics/wide.png", WIDE))
                .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
                .plugin(Catalogs(
                    "picture-home",
                    [
                        include_str!("../i18n/en.ini"),
                        include_str!("../i18n/de.ini"),
                    ],
                ))
                .plugin(Catalogs(
                    "home",
                    [
                        include_str!("../../home/i18n/en.ini"),
                        include_str!("../../home/i18n/de.ini"),
                    ],
                ))
        });
    let mut installed = hosted::install_in(
        &setup,
        paperpaw_plugin_picture_home::NATIVE_ENTRY.entry(),
        "picture-home",
        &["core.files", "core.registry", "core.ui"],
    )
    .expect("picture-home installed");
    hosted::also(
        &mut installed,
        paperpaw_plugin_home::NATIVE_ENTRY.entry(),
        "home",
        &["core.library", "core.registry", "core.ui"],
        FixedTables::new(
            paperpaw_plugin_home::NATIVE_SETTINGS,
            paperpaw_plugin_home::NATIVE_GROUPS,
        )
        .with_ids(paperpaw_plugin_home::NATIVE_IDS)
        .with_titles(paperpaw_plugin_home::NATIVE_TITLES),
    )
    .expect("home installed");
    let kernel = installed.kernel();
    kernel.start(Instant(100));
    let mut effects = kernel.open_screen(
        Instant(200),
        Id::try_parse("picture-home.dashboard").unwrap(),
        Params::default(),
    );
    // The picture is made in the background: a few turns of the host.
    let mut screen = Screen::new();
    let mut now = 200;
    for _ in 0..200 {
        screen.render(kernel);
        now = match effects.wake_at() {
            Some(at) if at.0 <= now + 10_000 => now.max(at.0),
            _ => now + 100,
        };
        effects = kernel.tick(Instant(now));
    }
    let top = kernel.active_screen().map(|id| id.as_str().to_string());
    assert_eq!(top.as_deref(), Some("picture-home.dashboard"));
    // Every callback of the page ended well: none was dropped, also not
    // the draw (`ensure` used to fail on the first widget the host would
    // not place, and the page stayed blank).
    assert_eq!(installed.dropped(), Vec::<(u32, i32)>::new());
    // The picture shows behind the page: far more than the plates' edges
    // and the bar are inked.
    let ink = screen.inked();
    assert!(ink > 480 * 800 / 4, "only {ink} pixels inked");
}

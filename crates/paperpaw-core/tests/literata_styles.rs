//! Literata from the card: the X4's firmware has no room for it
//! (`PAPERPAW_FONT_LEAN`), so book text is in the UI font until the reader
//! chooses another, and the data plugin `literata-styles` brings Literata:
//! its regular face under the name the firmware had it by
//! (`fonts-default.literata`), and its bold and italic. The sections of
//! its `plugin.ini` are the files the release builds it from
//! (`tools/plugin-files`), so what is tested is what is shipped.
//!
//! - Without the plugin, and with it until Literata is chosen, book text
//!   is in the UI font; bold and italic are unbound, and readers set that
//!   text in the regular face. No file of the plugin is opened.
//! - With Literata chosen its three faces come from the card.
//! - The plugin of before (bold and italic alone) does not offer Literata,
//!   and its faces are not mixed into the UI font's text.
//! - The larger sizes (`literata-large`) extend Literata's faces, and are
//!   harmless without them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::DEVICE;
use paperpaw_core::Kernel;
use paperpaw_core::sdk::builtin::fonts;
use paperpaw_core::sdk::{
    Font, FontFace, FontSpec, Id, Manifest, MemoryStorage, Plugin, Priority, RegisterError,
    Registrar, SettingKind, Version,
};
use paperpaw_platform::Instant;
use paperpaw_text::{Glyph, LineMetrics, PackBuilder};

const STYLES: &str = include_str!("../../../tools/plugin-files/literata-styles.ini");
const LARGE: &str = include_str!("../../../tools/plugin-files/literata-large.ini");

const SANS: &str = "fonts-default.source-sans";
const LITERATA: &str = "fonts-default.literata";
const CHOSEN: &str = "[core]\nreading-font = Literata\n";

/// A pack of `sizes`, its `a` `advance` wide: the advance tells the
/// faces apart.
fn pack(sizes: &[f32], advance: f32) -> Vec<u8> {
    let mut builder = PackBuilder::new();
    for &size in sizes {
        let metrics = LineMetrics::new(size * 0.8, size * 0.2, size * 1.2);
        builder.add_strike(size, metrics, vec![('a', Glyph::empty(advance))]);
    }
    builder.build()
}

/// What the lean firmware has (`plugins/fonts-default`, `LeanFontPacks`):
/// the UI font up to 32 px, for the menus and for book text, and the
/// name of Literata's regular face in the plugin's folder on the card.
struct Lean;

impl Plugin for Lean {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("fonts-default"),
            "Fonts",
            Version::new(1, 0, 0),
        )
        .uses(paperpaw_core::sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let font = Font::from_data(pack(&[20.0, 32.0], 6.0), &[]).unwrap();
        let sans = Id::new_static(SANS);
        r.add_font(FontSpec::new(sans.clone(), font).family("PaperPaw Sans", false, false))?;
        r.bind_font(fonts::UI, sans.clone(), Priority::DEFAULT)?;
        r.bind_font(fonts::READING, sans, Priority::DEFAULT)?;
        r.add_font_face(FontFace::new(
            Id::new_static(LITERATA),
            "plugins/literata-styles/fonts/literata.ppfp",
            "Literata",
            false,
            false,
        ))
    }
}

/// As `tools/plugin-files/data.py` and `tools/literata-packs.py` write it.
fn plugin_ini(id: &str, name: &str, sections: &str) -> String {
    format!("[plugin]\nid = {id}\nname = {name}\nversion = 1.0.0\nformats = 3\n\n{sections}")
}

/// What of `literata-styles` the card has.
#[derive(Clone, Copy, PartialEq)]
enum Styles {
    None,
    /// The plugin of before: bold and italic.
    Before,
    /// With the regular face too.
    All,
}

/// The card with the plugins named.
fn card(styles: Styles, large: bool) -> MemoryStorage {
    let mut card = MemoryStorage::new();
    if styles != Styles::None {
        card = card
            .with(
                "plugins/literata-styles/plugin.ini",
                plugin_ini("literata-styles", "Literata", STYLES),
            )
            .with(
                "plugins/literata-styles/fonts/literata-bold.ppfp",
                pack(&[20.0, 32.0], 12.0),
            )
            .with(
                "plugins/literata-styles/fonts/literata-italic.ppfp",
                pack(&[20.0, 32.0], 8.0),
            );
    }
    if styles == Styles::All {
        card = card.with(
            "plugins/literata-styles/fonts/literata.ppfp",
            pack(&[20.0, 32.0], 10.0),
        );
    }
    if large {
        card = card
            .with(
                "plugins/literata-large/plugin.ini",
                plugin_ini("literata-large", "Literata, larger sizes", LARGE),
            )
            .with(
                "plugins/literata-large/fonts/literata-large.ppfp",
                pack(&[48.0], 10.0),
            )
            .with(
                "plugins/literata-large/fonts/literata-bold-large.ppfp",
                pack(&[48.0], 12.0),
            )
            .with(
                "plugins/literata-large/fonts/literata-italic-large.ppfp",
                pack(&[48.0], 8.0),
            );
    }
    card
}

fn started(card: MemoryStorage) -> Kernel {
    started_with(card, "")
}

fn started_with(card: MemoryStorage, config: &str) -> Kernel {
    let (kernel, warnings) = started_warned(card, config);
    assert!(warnings.is_empty(), "{warnings:?}");
    kernel
}

/// The kernel started and what it warned of, as text.
fn started_warned(card: MemoryStorage, config: &str) -> (Kernel, Vec<String>) {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Lean)
        .storage(card)
        .user_config(config)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let warnings = kernel.take_warnings();
    let warnings = warnings.iter().map(|w| w.message.to_string()).collect();
    (kernel, warnings)
}

/// Literata chosen where the card lacks its regular face is as any font
/// chosen that is not there: said once as a setting that does not hold,
/// and the default stands (the choice is kept for when the font comes).
fn assert_choice_gives_way(card: MemoryStorage) {
    let (kernel, warnings) = started_warned(card, CHOSEN);
    assert_ui_font_sets_the_book(&kernel);
    assert_eq!(
        warnings,
        ["`core.reading-font`: invalid value for this setting"]
    );
}

/// Whether Literata is among the fonts the reading font setting offers.
fn offered(kernel: &Kernel) -> bool {
    let spec = kernel.settings().spec("core.reading-font").unwrap();
    let SettingKind::Choice(choices) = &spec.kind else {
        panic!("the reading font is a choice");
    };
    choices.iter().any(|choice| choice == "Literata")
}

/// Book text is in the UI font, bold and italic words too, and none of
/// the plugin's files is open.
fn assert_ui_font_sets_the_book(kernel: &Kernel) {
    let fonts = kernel.fonts();
    let regular = fonts.role(fonts::READING.as_str()).unwrap();
    assert_eq!(regular.id().as_str(), SANS);
    assert_eq!(regular.measure(20.0, "a"), 6.0);
    assert!(fonts.bound(fonts::READING_BOLD.as_str()).is_none());
    assert!(fonts.bound(fonts::READING_ITALIC.as_str()).is_none());
    assert!(fonts.bound(fonts::READING_BOLD_ITALIC.as_str()).is_none());
    for face in [LITERATA, "literata-styles.bold", "literata-styles.italic"] {
        assert!(fonts.get(face).is_none(), "{face} is open");
    }
}

#[test]
fn without_the_plugin_book_text_is_in_the_ui_font_and_literata_is_not_offered() {
    let kernel = started(card(Styles::None, false));
    assert_ui_font_sets_the_book(&kernel);
    assert!(!offered(&kernel));
    // Chosen before, on a firmware with Literata.
    assert_choice_gives_way(card(Styles::None, false));
}

#[test]
fn with_the_plugin_literata_is_offered_and_book_text_stays_in_the_ui_font() {
    let kernel = started(card(Styles::All, false));
    assert_ui_font_sets_the_book(&kernel);
    assert!(offered(&kernel));
}

#[test]
fn with_literata_chosen_regular_bold_and_italic_come_from_the_card() {
    let kernel = started_with(card(Styles::All, false), CHOSEN);
    let fonts = kernel.fonts();
    let regular = fonts.role(fonts::READING.as_str()).unwrap();
    let bold = fonts.bound(fonts::READING_BOLD.as_str()).unwrap();
    let italic = fonts.bound(fonts::READING_ITALIC.as_str()).unwrap();
    assert_eq!(regular.id().as_str(), LITERATA);
    assert_eq!(bold.id().as_str(), "literata-styles.bold");
    assert_eq!(italic.id().as_str(), "literata-styles.italic");
    assert_eq!(regular.measure(20.0, "a"), 10.0);
    assert_eq!(bold.measure(20.0, "a"), 12.0);
    assert_eq!(italic.measure(20.0, "a"), 8.0);
    // Bold italic is the other plugin's; the menus stay as they are.
    assert!(fonts.bound(fonts::READING_BOLD_ITALIC.as_str()).is_none());
    assert_eq!(fonts.role(fonts::UI.as_str()).unwrap().id().as_str(), SANS);
}

/// The plugin as it was before it had the regular face (a card set up
/// for an earlier firmware): Literata is not offered, a choice of it made
/// then gives way, and its bold and italic are not set beside the UI
/// font's regular text.
#[test]
fn the_plugin_of_before_does_not_mix_its_styles_into_the_ui_font() {
    let kernel = started(card(Styles::Before, false));
    assert_ui_font_sets_the_book(&kernel);
    assert!(!offered(&kernel));
    assert_choice_gives_way(card(Styles::Before, false));
}

#[test]
fn the_larger_sizes_extend_regular_bold_and_italic() {
    let kernel = started_with(card(Styles::All, true), CHOSEN);
    let fonts = kernel.fonts();
    for (role, advance) in [
        (fonts::READING, 10.0),
        (fonts::READING_BOLD, 12.0),
        (fonts::READING_ITALIC, 8.0),
    ] {
        let face = fonts.bound(role.as_str()).unwrap();
        assert_eq!(face.largest_size(), Some(48.0), "{role}");
        assert_eq!(face.measure(48.0, "a"), advance, "{role} at 48 px");
    }
}

/// Literata's larger sizes are not the UI font's: book text in it keeps
/// its own sizes.
#[test]
fn the_larger_sizes_without_literata_chosen_do_no_harm() {
    for styles in [Styles::None, Styles::Before, Styles::All] {
        let kernel = started(card(styles, true));
        assert_ui_font_sets_the_book(&kernel);
        let regular = kernel.fonts().role(fonts::READING.as_str()).unwrap();
        assert_eq!(regular.largest_size(), Some(32.0));
    }
}

/// A family of the reader's own takes the reading roles: the pack's faces
/// are not opened for nothing.
#[test]
fn with_another_family_chosen_the_packs_faces_stay_closed() {
    let card = card(Styles::All, false)
        .with(
            "plugins/other/plugin.ini",
            plugin_ini(
                "other",
                "Other",
                "[family]\nname = Other\nregular = other.ppfp\n",
            ),
        )
        .with("plugins/other/fonts/other.ppfp", pack(&[20.0, 32.0], 9.0));
    let kernel = started_with(card, "[core]\nreading-font = Other\n");
    let fonts = kernel.fonts();
    assert!(fonts.get(LITERATA).is_none());
    assert!(fonts.get("literata-styles.bold").is_none());
    assert!(fonts.get("literata-styles.italic").is_none());
    let regular = fonts.role(fonts::READING.as_str()).unwrap();
    assert_eq!(regular.id().as_str(), "other.regular");
    assert!(fonts.bound(fonts::READING_BOLD.as_str()).is_none());
}

/// A face on the card the reader binds in their own `[fonts]` is loaded.
#[test]
fn a_face_the_reader_binds_is_loaded() {
    let kernel = started_with(
        card(Styles::All, false),
        "[fonts]\ncore.font-reading-bold = literata-styles.italic\n",
    );
    let bold = kernel.fonts().bound(fonts::READING_BOLD.as_str()).unwrap();
    assert_eq!(bold.id().as_str(), "literata-styles.italic");
    // The proposal it replaces is not opened for nothing.
    assert!(kernel.fonts().get("literata-styles.bold").is_none());
}

/// A proposal naming a font nobody added does not hide the next one.
/// Regular book text is Literata's by the reader's own binding here, so
/// that the proposals for bold count (no family is chosen).
#[test]
fn a_proposal_without_a_font_gives_way_to_the_next() {
    let card = card(Styles::All, false).with(
        "plugins/zzz-ghost/plugin.ini",
        plugin_ini(
            "zzz-ghost",
            "Ghost",
            "[fonts]\ncore.font-reading-bold = zzz-ghost.bold\n",
        ),
    );
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(Lean)
        .storage(card)
        .user_config("[fonts]\ncore.font-reading = fonts-default.literata\n")
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let regular = kernel.fonts().role(fonts::READING.as_str()).unwrap();
    assert_eq!(regular.id().as_str(), LITERATA);
    let bold = kernel.fonts().bound(fonts::READING_BOLD.as_str()).unwrap();
    assert_eq!(bold.id().as_str(), "literata-styles.bold");
}

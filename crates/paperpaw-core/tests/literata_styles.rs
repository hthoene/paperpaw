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
//! - While running: Literata chosen before stays chosen in the file as
//!   plugins come and go, the plugin installed over the one of before
//!   brings it, and a card that cannot say whether its file is there
//!   takes no choice away.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{DEVICE, Harness};
use paperpaw_core::Kernel;
use paperpaw_core::sdk::builtin::fonts;
use paperpaw_core::sdk::{
    Command, Font, FontFace, FontSpec, Id, Manifest, MemoryStorage, Plugin, Priority,
    RegisterError, Registrar, SettingKind, SettingValue, Version,
};
use paperpaw_platform::{DirEntry, Instant, Storage, StorageError};
use paperpaw_text::{Glyph, LineMetrics, PackBuilder};

const STYLES: &str = include_str!("../../../tools/plugin-files/literata-styles.ini");
const LARGE: &str = include_str!("../../../tools/plugin-files/literata-large.ini");

const SANS: &str = "fonts-default.source-sans";
const LITERATA: &str = "fonts-default.literata";
const CHOSEN: &str = "[core]\nreading-font = Literata\n";
/// What is said of Literata chosen where the card lacks it.
const MISSING: &str = "`core.reading-font`: the font Literata is not on the card (install the \
                       plugin that brings it); the default is used until then";

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

/// Literata chosen where the card lacks its regular face: said once, by
/// the font's name and that its plugin is missing, and the default
/// stands (the file keeps the choice for the start that finds the font).
fn assert_choice_gives_way(card: MemoryStorage) {
    let (kernel, warnings) = started_warned(card, CHOSEN);
    assert_ui_font_sets_the_book(&kernel);
    assert_eq!(warnings, [MISSING]);
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

/// The card, shared with the test (which writes to it and starts anew
/// on it). While `busy` it cannot say whether Literata's file is there.
struct Shared {
    card: Rc<MemoryStorage>,
    busy: Rc<Cell<bool>>,
}

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.card.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        if self.busy.get() && path.ends_with("/literata.ppfp") {
            return Err(StorageError::Io);
        }
        self.card.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.card.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.card.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.card.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.card.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.card.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.card.create_dir(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.card.remove_dir_all(path)
    }
}

/// A device running on a card the test keeps: commands go through the
/// harness, as a plugin's would.
struct Running {
    kernel: Kernel,
    harness: Harness,
    card: Rc<MemoryStorage>,
    busy: Rc<Cell<bool>>,
    at: u64,
}

impl Running {
    /// Started on `card` with the configuration file `config`; what the
    /// start warned of.
    fn on(card: MemoryStorage, config: &str) -> (Self, Vec<String>) {
        let card = Rc::new(card.with("paperpaw.ini", config.as_bytes().to_vec()));
        Self::again(card)
    }

    /// A start on `card` as it is now.
    fn again(card: Rc<MemoryStorage>) -> (Self, Vec<String>) {
        let harness = Harness::new();
        let busy = Rc::new(Cell::new(false));
        let shared = Shared {
            card: card.clone(),
            busy: busy.clone(),
        };
        let mut kernel = Kernel::builder(DEVICE)
            .plugin(Lean)
            .plugin(harness.plugin())
            .storage(shared)
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let warnings = kernel.take_warnings();
        let warnings = warnings.iter().map(|w| w.message.to_string()).collect();
        let running = Self {
            kernel,
            harness,
            card,
            busy,
            at: 0,
        };
        (running, warnings)
    }

    /// Carries `command` out; nothing may be warned of.
    fn run(&mut self, command: Command) {
        self.at += 10;
        self.harness.run(&mut self.kernel, self.at, [command]);
        let warnings = self.kernel.take_warnings();
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    /// Another data plugin comes to the card and is loaded.
    fn another_comes(&mut self) {
        let ini = plugin_ini("other", "Other", "");
        self.card
            .write("plugins/other/plugin.ini", ini.as_bytes())
            .unwrap();
        self.run(Command::LoadPlugin("plugins/other".into()));
        assert!(self.kernel.plugins().get("other").is_some());
    }

    /// The reading font chosen, and the face book text is in.
    fn reads_in(&self) -> (&str, String) {
        let fonts = self.kernel.fonts();
        let face = fonts.role(fonts::READING.as_str()).unwrap().id();
        let chosen = self.kernel.settings().text("core.reading-font");
        (chosen, face.to_string())
    }

    /// Whether the configuration file names Literata as the reading font.
    fn kept_in_the_file(&self) -> bool {
        let file = paperpaw_core::sdk::read_file(&*self.card, "paperpaw.ini", 4096).unwrap();
        let file = String::from_utf8(file).unwrap();
        file.contains("reading-font = Literata")
    }
}

/// The plugin with the regular face, version 1.1.0, in `folder`.
fn write_literata(card: &MemoryStorage, folder: &str) {
    let ini = plugin_ini("literata-styles", "Literata", STYLES).replace("1.0.0", "1.1.0");
    let files = [
        ("plugin.ini", ini.into_bytes()),
        ("fonts/literata.ppfp", pack(&[20.0, 32.0], 10.0)),
        ("fonts/literata-bold.ppfp", pack(&[20.0, 32.0], 12.0)),
        ("fonts/literata-italic.ppfp", pack(&[20.0, 32.0], 8.0)),
    ];
    for (file, data) in files {
        card.write(&format!("{folder}/{file}"), &data).unwrap();
    }
}

/// Literata chosen on a firmware that had it (Beta 3), on a card without
/// the plugin: the UI font sets the book and the file keeps the choice,
/// whatever other plugins come and go, so the start after the plugin
/// came reads in Literata again.
#[test]
fn literata_chosen_before_stays_in_the_file_until_its_plugin_comes() {
    let (mut device, warnings) = Running::on(card(Styles::None, false), CHOSEN);
    assert_eq!(warnings, [MISSING]);
    assert_ui_font_sets_the_book(&device.kernel);
    device.another_comes();
    assert!(device.kept_in_the_file());
    device.run(Command::RemovePlugin(Id::new_static("other")));
    assert!(device.kernel.plugins().get("other").is_none());
    assert!(device.kept_in_the_file());
    assert_ui_font_sets_the_book(&device.kernel);

    // The plugin comes: Literata is offered, and the next start takes it.
    write_literata(&device.card, "plugins/.literata-styles.part");
    device.run(Command::InstallPlugin(
        Id::new_static("literata-styles"),
        "plugins/.literata-styles.part".into(),
    ));
    assert!(offered(&device.kernel));
    assert!(device.kept_in_the_file());
    let (device, warnings) = Running::again(device.card);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(device.reads_in(), ("Literata", LITERATA.to_string()));
}

/// The plugin with the regular face installed over the one of before
/// (bold and italic alone) while the device runs: Literata is offered at
/// once, and chosen its three faces come from the card.
#[test]
fn the_plugin_installed_over_the_one_of_before_brings_literata() {
    let (mut device, warnings) = Running::on(card(Styles::Before, false), "");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(!offered(&device.kernel));
    write_literata(&device.card, "plugins/.literata-styles.part");
    device.run(Command::InstallPlugin(
        Id::new_static("literata-styles"),
        "plugins/.literata-styles.part".into(),
    ));
    let plugin = device.kernel.plugins().get("literata-styles").unwrap();
    assert_eq!(plugin.manifest.version.minor, 1);
    assert!(offered(&device.kernel));
    assert_ui_font_sets_the_book(&device.kernel);
    device.run(Command::SetSetting(
        Id::new_static("core.reading-font"),
        SettingValue::Text("Literata".into()),
    ));
    assert_eq!(device.reads_in(), ("Literata", LITERATA.to_string()));
    let fonts = device.kernel.fonts();
    let bold = fonts.bound(fonts::READING_BOLD.as_str()).unwrap();
    let italic = fonts.bound(fonts::READING_ITALIC.as_str()).unwrap();
    assert_eq!(bold.measure(20.0, "a"), 12.0);
    assert_eq!(italic.measure(20.0, "a"), 8.0);
    assert!(device.kept_in_the_file());
}

/// A card that cannot say whether Literata's file is there (busy, a read
/// fault) while the plugins change has not said that it is gone: the
/// choice stays, in the settings and in the file, and so does the font.
#[test]
fn a_card_that_cannot_say_takes_no_choice_of_literata_away() {
    let (mut device, warnings) = Running::on(card(Styles::All, false), CHOSEN);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(device.reads_in(), ("Literata", LITERATA.to_string()));
    device.busy.set(true);
    device.another_comes();
    assert_eq!(device.reads_in(), ("Literata", LITERATA.to_string()));
    assert!(offered(&device.kernel));
    assert!(device.kept_in_the_file());
    // Gone for good, the choice gives way as ever.
    device.busy.set(false);
    device.run(Command::RemovePlugin(Id::new_static("literata-styles")));
    assert_eq!(device.reads_in().0, "core.font.default");
    assert!(!device.kept_in_the_file());
    assert_ui_font_sets_the_book(&device.kernel);
}

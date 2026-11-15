//! PaperPaw's default fonts, contributed like any other plugin:
//!
//! - **Literata** for reading (`fonts-default.literata`), designed for long
//!   texts on screens, with bold, italic and bold italic faces.
//! - **Source Sans 3** for the UI (`fonts-default.source-sans`), a
//!   humanist sans serif: calm and clear, with a warmer, livelier hand
//!   than a technical face. Its font file is shipped unchanged (its
//!   licence reserves the name "Source" for the original) and used at
//!   weight 430 ([`UI_WEIGHT`]); the bitmap packs are drawn from it at
//!   build time, at that weight and for Latin text only. Changed so, they
//!   are a Modified Version under the licence and may not carry the name:
//!   their family is **PaperPaw Sans** ([`UI_FAMILY`]). A reading or
//!   status font chosen as "Source Sans 3" on a device of packs is read
//!   as PaperPaw Sans (the kernel's `config::RENAMED`), so the reader keeps
//!   their font.
//! - **PaperPaw Icons** (`fonts-default.icons`): the icons of the menus,
//!   taken from Tabler Icons (MIT licence; see `tools/icons/subset.py`).
//!
//! They are no plugin of code: [`DEFAULT_FONTS`] is a table in the program
//! (`paperpaw_sdk::BuiltInFonts`), which the kernel registers as it is
//! built (`KernelBuilder::built_in_fonts`). It holds scalable fonts when
//! built with the `outline` feature, else bitmap packs.
//! `DEFAULT_FONT_PACKS` always holds bitmap packs (feature `packs`).
//!
//! A lean build (`PAPERPAW_FONT_LEAN=1`, the X4's firmware, whose flash is
//! short) carries no Literata: the UI font sets book text too, until the
//! reader chooses another font. Literata is a font of the card there:
//! `fonts-default.literata` names the pack [`LITERATA_ON_CARD`] of the
//! data plugin `literata-styles`, read while the reader has chosen
//! Literata. `LEAN_FONT_PACKS` is that in any build, to emulate
//! such a device.
//!
//! The text fonts are under the SIL Open Font License 1.1. Users can bind
//! other fonts to the roles:
//!
//! ```ini
//! [fonts]
//! core.font-reading = my-fonts.garamond
//! ```

#![no_std]

use paperpaw_sdk::builtin::fonts;
use paperpaw_sdk::{BuiltInFont, BuiltInFonts, Id, Manifest, Plugin, RegisterError, Registrar};

pub const PLUGIN: Id = Id::new_static("fonts-default");
pub const LITERATA: Id = Id::new_static("fonts-default.literata");
pub const LITERATA_BOLD: Id = Id::new_static("fonts-default.literata-bold");
pub const LITERATA_ITALIC: Id = Id::new_static("fonts-default.literata-italic");
pub const LITERATA_BOLD_ITALIC: Id = Id::new_static("fonts-default.literata-bold-italic");
pub const SOURCE_SANS: Id = Id::new_static("fonts-default.source-sans");
pub const ICONS: Id = Id::new_static("fonts-default.icons");

/// The weight of the UI font: a little above regular, so thin strokes
/// hold on e-ink panels with one bit a pixel. Also in `build.rs`.
pub const UI_WEIGHT: f32 = 430.0;

/// The UI font's family as the font file names it, unchanged.
pub const SOURCE_SANS_FAMILY: &str = "Source Sans 3";
/// The UI font's family where it is drawn from bitmap packs: not the
/// Reserved Font Name.
pub const PAPERPAW_SANS_FAMILY: &str = "PaperPaw Sans";
/// The UI font's family in this build.
pub const UI_FAMILY: &str = preferred::UI_FAMILY;

/// Where a lean build finds Literata's regular face: a pack of the data
/// plugin `literata-styles` on the card (`tools/literata-packs.py`).
pub const LITERATA_ON_CARD: &str = "plugins/literata-styles/fonts/literata.ppfp";

/// A text face of `family`, drawn at `weight`.
const fn text(
    id: Id,
    role: Id,
    data: &'static [u8],
    weight: f32,
    family: &'static str,
    italic: bool,
) -> BuiltInFont {
    BuiltInFont::Data {
        id,
        role,
        data,
        weight: Some(weight),
        family: Some(family),
        bold: weight >= 600.0,
        italic,
    }
}

/// A face of Literata for `role`.
#[cfg(not(all(paperpaw_lean_fonts, not(feature = "outline"))))]
const fn literata(id: Id, role: Id, data: &'static [u8], weight: f32, italic: bool) -> BuiltInFont {
    text(id, role, data, weight, "Literata", italic)
}

/// The UI face, of the family this kind of data may carry.
const fn ui(data: &'static [u8], family: &'static str) -> BuiltInFont {
    text(SOURCE_SANS, fonts::UI, data, UI_WEIGHT, family, false)
}

/// The icons: outside any family, so never offered for text.
const fn icons(data: &'static [u8]) -> BuiltInFont {
    BuiltInFont::Data {
        id: ICONS,
        role: fonts::ICONS,
        data,
        weight: None,
        family: None,
        bold: false,
        italic: false,
    }
}

#[cfg(feature = "outline")]
mod outlines {
    use super::*;

    static LITERATA_FILE: &[u8] = include_bytes!("../fonts/Literata.ttf");
    static LITERATA_ITALIC_FILE: &[u8] = include_bytes!("../fonts/Literata-Italic.ttf");
    static SOURCE_SANS_FILE: &[u8] = include_bytes!("../fonts/SourceSans3.ttf");
    static ICONS_FILE: &[u8] = include_bytes!("../fonts/PaperPawIcons.ttf");
    pub const UI_FAMILY: &str = super::SOURCE_SANS_FAMILY;

    /// Bold faces are instances of the variable fonts.
    pub static FONTS: &[BuiltInFont] = &[
        literata(LITERATA, fonts::READING, LITERATA_FILE, 400.0, false),
        literata(
            LITERATA_BOLD,
            fonts::READING_BOLD,
            LITERATA_FILE,
            700.0,
            false,
        ),
        literata(
            LITERATA_ITALIC,
            fonts::READING_ITALIC,
            LITERATA_ITALIC_FILE,
            400.0,
            true,
        ),
        literata(
            LITERATA_BOLD_ITALIC,
            fonts::READING_BOLD_ITALIC,
            LITERATA_ITALIC_FILE,
            700.0,
            true,
        ),
        ui(SOURCE_SANS_FILE, UI_FAMILY),
        icons(ICONS_FILE),
    ];
}

#[cfg(any(feature = "packs", not(feature = "outline")))]
mod packs {
    use super::*;

    macro_rules! pack {
        ($name:literal) => {
            include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".ppfp"))
        };
    }

    static ICONS_PACK: &[u8] = pack!("icons");
    pub const UI_FAMILY: &str = super::PAPERPAW_SANS_FAMILY;
    /// One copy of the UI font's pack, whichever table names it.
    static SANS: &[u8] = pack!("paperpaw-sans");

    /// The UI font alone: what a lean build (the firmware) carries, where
    /// it sets book text too, until the reader chooses another. Literata
    /// comes from the card there, with the data plugin `literata-styles`,
    /// for whoever chooses it: it keeps its name as a face of the card, so
    /// that what extends it still does.
    pub static LEAN: &[BuiltInFont] = &[
        ui(SANS, UI_FAMILY),
        BuiltInFont::Role {
            role: fonts::READING,
            font: SOURCE_SANS,
        },
        BuiltInFont::File {
            id: LITERATA,
            path: LITERATA_ON_CARD,
            family: "Literata",
            bold: false,
            italic: false,
        },
        icons(ICONS_PACK),
    ];

    #[cfg(paperpaw_lean_fonts)]
    pub static FONTS: &[BuiltInFont] = LEAN;

    #[cfg(not(paperpaw_lean_fonts))]
    pub static FONTS: &[BuiltInFont] = &[
        literata(LITERATA, fonts::READING, pack!("literata"), 400.0, false),
        literata(
            LITERATA_BOLD,
            fonts::READING_BOLD,
            pack!("literata-bold"),
            700.0,
            false,
        ),
        literata(
            LITERATA_ITALIC,
            fonts::READING_ITALIC,
            pack!("literata-italic"),
            400.0,
            true,
        ),
        literata(
            LITERATA_BOLD_ITALIC,
            fonts::READING_BOLD_ITALIC,
            pack!("literata-bold-italic"),
            700.0,
            true,
        ),
        ui(SANS, UI_FAMILY),
        icons(ICONS_PACK),
    ];
}

#[cfg(feature = "outline")]
use outlines as preferred;
#[cfg(not(feature = "outline"))]
use packs as preferred;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

/// The translations of its name and description.
static TRANSLATIONS: [&str; 2] = [
    include_str!("../i18n/en.ini"),
    include_str!("../i18n/de.ini"),
];

/// `fonts` with who brings them, as the list of plugins shows it.
const fn built_in(fonts: &'static [BuiltInFont]) -> BuiltInFonts {
    BuiltInFonts {
        manifest: Manifest::fixed(
            PLUGIN,
            "Default fonts",
            "The fonts and icons PaperPaw comes with.",
            paperpaw_sdk::package_version!(),
        )
        .how(&HOW),
        translations: &TRANSLATIONS,
        fonts,
    }
}

/// The default fonts: scalable fonts if available, else bitmap packs.
pub static DEFAULT_FONTS: BuiltInFonts = built_in(preferred::FONTS);

/// The default fonts as bitmap packs, read in place without a glyph cache.
#[cfg(any(feature = "packs", not(feature = "outline")))]
pub static DEFAULT_FONT_PACKS: BuiltInFonts = built_in(packs::FONTS);

/// The fonts of a lean build, whatever this build is: the UI font, which
/// sets book text too, and the icons. What the X4's firmware comes with,
/// so an emulator shows that device with it.
#[cfg(any(feature = "packs", not(feature = "outline")))]
pub static LEAN_FONT_PACKS: BuiltInFonts = built_in(packs::LEAN);

/// The default fonts as an interface-6 plugin: [`DEFAULT_FONTS`], registered by a `Plugin`.
#[derive(Debug, Default)]
pub struct DefaultFonts;

/// [`DEFAULT_FONT_PACKS`], registered by a `Plugin`.
#[cfg(any(feature = "packs", not(feature = "outline")))]
#[derive(Debug, Default)]
pub struct DefaultFontPacks;

/// [`LEAN_FONT_PACKS`], registered by a `Plugin`.
#[cfg(any(feature = "packs", not(feature = "outline")))]
#[derive(Debug, Default)]
pub struct LeanFontPacks;

impl Plugin for DefaultFonts {
    fn manifest(&self) -> Manifest {
        DEFAULT_FONTS.manifest.clone()
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        DEFAULT_FONTS.register(r)
    }
}

#[cfg(any(feature = "packs", not(feature = "outline")))]
impl Plugin for DefaultFontPacks {
    fn manifest(&self) -> Manifest {
        DEFAULT_FONT_PACKS.manifest.clone()
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        DEFAULT_FONT_PACKS.register(r)
    }
}

#[cfg(any(feature = "packs", not(feature = "outline")))]
impl Plugin for LeanFontPacks {
    fn manifest(&self) -> Manifest {
        LEAN_FONT_PACKS.manifest.clone()
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        LEAN_FONT_PACKS.register(r)
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}

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
//! [`DefaultFonts`] ships them as scalable fonts when built with the
//! `outline` feature, else as bitmap packs. `DefaultFontPacks` always uses
//! bitmap packs (feature `packs`).
//!
//! A lean build (`PAPERPAW_FONT_LEAN=1`, the X4's firmware, whose flash is
//! short) carries no Literata: the UI font sets book text too, until the
//! reader chooses another font. Literata is a font of the card there:
//! `fonts-default.literata` names the pack [`LITERATA_ON_CARD`] of the
//! data plugin `literata-styles`, read while the reader has chosen
//! Literata. `LeanFontPacks` registers that in any build, to emulate
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

extern crate alloc;

use alloc::borrow::Cow;

use paperpaw_sdk::builtin::fonts;
use paperpaw_sdk::{
    Font, FontFace, FontSpec, Id, Manifest, Plugin, Priority, RegisterError, Registrar,
};

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

/// The faces: id, role, font data and weight (for variable fonts).
type Faces = &'static [(Id, Id, &'static [u8], f32)];

#[cfg(feature = "outline")]
mod outlines {
    const LITERATA: &[u8] = include_bytes!("../fonts/Literata.ttf");
    const LITERATA_ITALIC: &[u8] = include_bytes!("../fonts/Literata-Italic.ttf");
    const SOURCE_SANS: &[u8] = include_bytes!("../fonts/SourceSans3.ttf");
    pub const ICONS: &[u8] = include_bytes!("../fonts/PaperPawIcons.ttf");
    pub const UI_FAMILY: &str = super::SOURCE_SANS_FAMILY;

    /// Bold faces are instances of the variable fonts.
    pub fn faces() -> super::Faces {
        use super::*;
        static FACES: super::Faces = &[
            (LITERATA, fonts::READING, self::LITERATA, 400.0),
            (LITERATA_BOLD, fonts::READING_BOLD, self::LITERATA, 700.0),
            (
                LITERATA_ITALIC,
                fonts::READING_ITALIC,
                self::LITERATA_ITALIC,
                400.0,
            ),
            (
                LITERATA_BOLD_ITALIC,
                fonts::READING_BOLD_ITALIC,
                self::LITERATA_ITALIC,
                700.0,
            ),
            (SOURCE_SANS, fonts::UI, self::SOURCE_SANS, UI_WEIGHT),
        ];
        FACES
    }
}

#[cfg(any(feature = "packs", not(feature = "outline")))]
mod packs {
    macro_rules! pack {
        ($name:literal) => {
            include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".ppfp"))
        };
    }

    pub const ICONS: &[u8] = pack!("icons");
    pub const UI_FAMILY: &str = super::PAPERPAW_SANS_FAMILY;
    /// One copy of the UI font's pack, whichever faces name it.
    static SANS: &[u8] = pack!("paperpaw-sans");

    /// The UI font alone: what a lean build (the firmware) carries, where
    /// it sets book text too. Literata comes from the card there, with
    /// the data plugin `literata-styles`, for whoever chooses it.
    pub fn lean_faces() -> super::Faces {
        use super::*;
        static FACES: super::Faces = &[(SOURCE_SANS, fonts::UI, SANS, UI_WEIGHT)];
        FACES
    }

    #[cfg(paperpaw_lean_fonts)]
    pub fn faces() -> super::Faces {
        lean_faces()
    }

    #[cfg(not(paperpaw_lean_fonts))]
    pub fn faces() -> super::Faces {
        use super::*;
        static FACES: super::Faces = &[
            (LITERATA, fonts::READING, pack!("literata"), 400.0),
            (
                LITERATA_BOLD,
                fonts::READING_BOLD,
                pack!("literata-bold"),
                700.0,
            ),
            (
                LITERATA_ITALIC,
                fonts::READING_ITALIC,
                pack!("literata-italic"),
                400.0,
            ),
            (
                LITERATA_BOLD_ITALIC,
                fonts::READING_BOLD_ITALIC,
                pack!("literata-bold-italic"),
                700.0,
            ),
            (SOURCE_SANS, fonts::UI, SANS, UI_WEIGHT),
        ];
        FACES
    }
}

#[cfg(feature = "outline")]
use outlines as preferred;
#[cfg(not(feature = "outline"))]
use packs as preferred;

/// The default fonts plugin: scalable fonts if available, else bitmap packs.
#[derive(Debug, Default)]
pub struct DefaultFonts;

/// The default fonts as bitmap packs, read in place without a glyph cache.
#[cfg(any(feature = "packs", not(feature = "outline")))]
#[derive(Debug, Default)]
pub struct DefaultFontPacks;

/// The fonts of a lean build, whatever this build is: the UI font, which
/// sets book text too, and the icons. What the X4's firmware comes with,
/// so an emulator shows that device with it.
#[cfg(any(feature = "packs", not(feature = "outline")))]
#[derive(Debug, Default)]
pub struct LeanFontPacks;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

fn manifest() -> Manifest {
    Manifest::new(PLUGIN, "Default fonts", paperpaw_sdk::package_version!())
        .description("The fonts and icons PaperPaw comes with.")
        .how(&HOW)
        .uses(paperpaw_sdk::Uses::NOTHING)
}

fn register(
    r: &mut Registrar,
    faces: Faces,
    icons: &'static [u8],
    ui_family: &'static str,
) -> Result<(), RegisterError> {
    r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
    r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
    for (id, role, data, weight) in faces.iter().cloned() {
        let font =
            Font::from_data(data, &[("wght", weight)]).map_err(|e| RegisterError::Invalid {
                message: Cow::Owned(alloc::format!("{id}: {e}")),
            })?;
        let family = if role == fonts::UI {
            ui_family
        } else {
            "Literata"
        };
        let italic = role == fonts::READING_ITALIC || role == fonts::READING_BOLD_ITALIC;
        r.add_font(FontSpec::new(id.clone(), font).family(family, weight >= 600.0, italic))?;
        r.bind_font(role, id, Priority::DEFAULT)?;
    }
    // Without a reading font of its own (a lean build) book text is in
    // the UI font until the reader chooses another. Literata keeps its
    // name as a face of the card, so that what extends it still does.
    if !faces.iter().any(|(_, role, _, _)| *role == fonts::READING) {
        r.bind_font(fonts::READING, SOURCE_SANS, Priority::DEFAULT)?;
        r.add_font_face(FontFace::new(
            LITERATA,
            LITERATA_ON_CARD,
            "Literata",
            false,
            false,
        ))?;
    }
    // Outside any family, so it is never offered for text.
    let font = Font::from_data(icons, &[]).map_err(|e| RegisterError::Invalid {
        message: Cow::Owned(alloc::format!("{ICONS}: {e}")),
    })?;
    r.add_font(FontSpec::new(ICONS, font))?;
    r.bind_font(fonts::ICONS, ICONS, Priority::DEFAULT)?;
    Ok(())
}

impl Plugin for DefaultFonts {
    fn manifest(&self) -> Manifest {
        manifest()
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        register(
            r,
            preferred::faces(),
            preferred::ICONS,
            preferred::UI_FAMILY,
        )
    }
}

#[cfg(any(feature = "packs", not(feature = "outline")))]
impl Plugin for DefaultFontPacks {
    fn manifest(&self) -> Manifest {
        manifest()
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        register(r, packs::faces(), packs::ICONS, packs::UI_FAMILY)
    }
}

#[cfg(any(feature = "packs", not(feature = "outline")))]
impl Plugin for LeanFontPacks {
    fn manifest(&self) -> Manifest {
        manifest()
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        register(r, packs::lean_faces(), packs::ICONS, packs::UI_FAMILY)
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

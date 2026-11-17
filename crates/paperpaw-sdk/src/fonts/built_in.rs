//! The fonts a program is built with, as a table in the program: what
//! the kernel registers for it without a plugin's code
//! (`KernelBuilder::built_in_fonts` of `paperpaw-core`).

use alloc::borrow::Cow;

use paperpaw_text::Font;

use super::FontSpec;
use crate::id::Id;
use crate::plugin::{Manifest, RegisterError};

/// One entry of [`BuiltInFonts`], registered in the table's order.
#[derive(Debug, Clone)]
pub enum BuiltInFont {
    /// A font whose data the program holds (a pack or, where scalable
    /// fonts are, an OpenType/TrueType file), proposed for `role` at the
    /// default priority.
    Data {
        id: Id,
        role: Id,
        data: &'static [u8],
        /// The weight a variable font is drawn at; `None` as its file has it.
        weight: Option<f32>,
        /// The family users choose it by; `None` keeps it out of the choice.
        family: Option<&'static str>,
        bold: bool,
        italic: bool,
    },
    /// `font` proposed for one more `role` at the default priority.
    Role { role: Id, font: Id },
    /// A face of `family` in a file on the storage, read only while its
    /// family is chosen (`FontFace`).
    File {
        id: Id,
        path: &'static str,
        family: &'static str,
        bold: bool,
        italic: bool,
    },
}

/// The fonts a program is built with: who brings them (shown in the list
/// of plugins as a plugin of data is), their translations and the fonts.
#[derive(Debug, Clone)]
pub struct BuiltInFonts {
    pub manifest: Manifest,
    /// Translation catalogs (`.ini`), e.g. of the name and description.
    pub translations: &'static [&'static str],
    pub fonts: &'static [BuiltInFont],
}

impl BuiltInFonts {
    /// The font `id` made of its data, e.g. for a screen drawn before the
    /// kernel is built; `None` where the table holds no data of that id.
    pub fn font(&self, id: &Id) -> Option<Result<FontSpec, RegisterError>> {
        self.fonts
            .iter()
            .filter(|font| matches!(font, BuiltInFont::Data { id: its, .. } if its == id))
            .find_map(BuiltInFont::spec)
    }
}

impl BuiltInFont {
    /// The font made of its data; `None` for an entry without data.
    fn spec(&self) -> Option<Result<FontSpec, RegisterError>> {
        let BuiltInFont::Data {
            id,
            data,
            weight,
            family,
            bold,
            italic,
            ..
        } = self
        else {
            return None;
        };
        let made = match weight {
            Some(weight) => Font::from_data(*data, &[("wght", *weight)]),
            None => Font::from_data(*data, &[]),
        };
        Some(match made {
            Ok(font) => {
                let spec = FontSpec::new(id.clone(), font);
                Ok(match family {
                    Some(family) => spec.family(*family, *bold, *italic),
                    None => spec,
                })
            }
            Err(e) => Err(RegisterError::Invalid {
                message: Cow::Owned(alloc::format!("{id}: {e}")),
            }),
        })
    }
}

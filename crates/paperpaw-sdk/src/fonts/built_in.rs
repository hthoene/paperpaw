//! The fonts a program is built with, as a table in the program: what
//! the kernel registers for it without a plugin's code
//! (`KernelBuilder::built_in_fonts` of `paperpaw-core`).

use alloc::borrow::Cow;
use alloc::boxed::Box;

use paperpaw_text::Font;

use super::{FontFace, FontSpec};
use crate::contribution::item::Contribution;
use crate::id::Id;
use crate::plugin::{Manifest, Priority, RegisterError};

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
    /// family is chosen ([`FontFace`]).
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
    /// Registers the translations and the fonts, in the table's order,
    /// through `add` (the kernel's `Items::add`).
    pub fn register(
        &self,
        add: &mut dyn FnMut(Contribution) -> Result<(), RegisterError>,
    ) -> Result<(), RegisterError> {
        let binding = |role: &Id, font: &Id| Contribution::FontBinding {
            role: role.clone(),
            font: font.clone(),
            priority: Priority::DEFAULT,
        };
        for source in self.translations {
            add(Contribution::Translations(Cow::Borrowed(*source)))?;
        }
        for font in self.fonts {
            match font {
                BuiltInFont::Data { id, role, .. } => {
                    if let Some(spec) = font.spec() {
                        add(Contribution::Font(Box::new(spec?)))?;
                    }
                    add(binding(role, id))?;
                }
                BuiltInFont::Role { role, font } => add(binding(role, font))?,
                BuiltInFont::File {
                    id,
                    path,
                    family,
                    bold,
                    italic,
                } => add(Contribution::FontFace(Box::new(FontFace::new(
                    id.clone(),
                    *path,
                    *family,
                    *bold,
                    *italic,
                ))))?,
            }
        }
        Ok(())
    }

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

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use alloc::vec::Vec;

    use super::*;
    use crate::version::Version;

    const ID: Id = Id::new_static("demo.face");
    const ROLE: Id = Id::new_static("core.font-ui");

    fn table(fonts: &'static [BuiltInFont]) -> BuiltInFonts {
        let manifest = Manifest::fixed(Id::new_static("demo"), "Demo", "", Version::new(1, 0, 0));
        BuiltInFonts {
            manifest,
            translations: &[],
            fonts,
        }
    }

    /// Data that is no font is refused by the font's name, as a plugin's
    /// was: nothing of it is registered, and nothing panics.
    #[test]
    fn damaged_data_is_refused_by_name() {
        static DAMAGED: [BuiltInFont; 1] = [BuiltInFont::Data {
            id: ID,
            role: ROLE,
            data: b"no font",
            weight: Some(400.0),
            family: Some("Demo"),
            bold: false,
            italic: false,
        }];
        let fonts = table(&DAMAGED);
        let mut all = Vec::new();
        let mut add = |c| {
            all.push(c);
            Ok(())
        };
        let Err(RegisterError::Invalid { message }) = fonts.register(&mut add) else {
            panic!("refused");
        };
        assert!(message.starts_with("demo.face: "), "{message}");
        assert!(all.is_empty());
        let made = fonts.font(&ID).map(|made| made.map_err(|e| e.to_string()));
        assert!(matches!(made, Some(Err(_))), "{made:?}");
    }

    /// Roles and faces in files are registered in the table's order; only
    /// an entry with data is a font that can be made beforehand.
    #[test]
    fn roles_and_files_register_without_data() {
        static FONTS: [BuiltInFont; 2] = [
            BuiltInFont::Role {
                role: ROLE,
                font: ID,
            },
            BuiltInFont::File {
                id: ID,
                path: "plugins/demo/fonts/face.ppfp",
                family: "Demo",
                bold: true,
                italic: false,
            },
        ];
        let fonts = table(&FONTS);
        let mut all = Vec::new();
        let mut add = |c| {
            all.push(c);
            Ok(())
        };
        fonts.register(&mut add).unwrap();
        assert!(
            matches!(&all[0], Contribution::FontBinding { role, font, .. }
            if *role == ROLE && *font == ID)
        );
        assert!(matches!(&all[1], Contribution::FontFace(face)
            if face.path == "plugins/demo/fonts/face.ppfp" && face.bold && !face.italic));
        assert_eq!(all.len(), 2);
        assert!(fonts.font(&ID).is_none());
    }
}

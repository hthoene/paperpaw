//! Font and hyphenation records of a native plugin (`resource.font_face`,
//! `resource.font_extension`, `resource.hyphenation`): mapped to owned values
//! with fallible allocation, then published with the rest of the plugin's
//! contributions (all or nothing) into the font and hyphenation registries.
//!
//! The files are named relative to the plugin's folder, which the loader of
//! the `.ppn` passes to [`crate::Kernel::register_native_in`]; a plugin with
//! such records and no folder (a linked built-in) is refused whole.
use super::{
    native_values::{checked_id, copy, qualified},
    registrations as wire,
};
use alloc::{borrow::Cow, string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY};
use paperpaw_sdk::{FontFace, Id, Priority, RegisterError};

/// Style bit 0 of a font face: bold. Bit 1: italic.
const BOLD: u32 = 1;
const ITALIC: u32 = 1 << 1;

/// One resource record, owned.
pub(super) enum Resource {
    Face(Face),
    Extension(Extension),
    Hyphenation(Hyphenation),
}

/// A face of a family in a file of the plugin's folder.
pub(super) struct Face {
    id: Id,
    path: String,
    /// Empty: outside any family (bound to a role, never offered for text).
    family: String,
    bold: bool,
    italic: bool,
    /// The role it is proposed for, with its priority.
    role: Option<(Id, Priority)>,
    /// The weight given does not say what the style bits do.
    weight_differs: bool,
}

/// A pack that adds glyphs to another font.
pub(super) struct Extension {
    id: Id,
    path: String,
    extends: Id,
}

/// The table of a language.
pub(super) struct Hyphenation {
    language: String,
    path: String,
}

/// The owned mapping of the resource record `record` of plugin `owner`, or
/// `None` for another kind of record.
pub(super) fn map(owner: &str, record: wire::Record<'_>) -> Result<Option<Resource>, i32> {
    match record {
        wire::Record::ResourceFontFace(face) => face_of(owner, &face).map(Resource::Face),
        wire::Record::ResourceFontExtension(extension) => Ok(Resource::Extension(Extension {
            id: qualified(owner, extension.name, &mut checked_id)?,
            path: copy(extension.path)?,
            extends: checked_id(extension.extends)?,
        })),
        wire::Record::ResourceHyphenation(table) => Ok(Resource::Hyphenation(Hyphenation {
            language: copy(table.language)?,
            path: copy(table.path)?,
        })),
        _ => return Ok(None),
    }
    .map(Some)
}

fn face_of(owner: &str, face: &wire::ResourceFontFace<'_>) -> Result<Face, i32> {
    let bold = face.style & BOLD != 0;
    let role = if face.role.is_empty() {
        None
    } else {
        let priority = i16::try_from(face.priority).map_err(|_| ERR_INVALID)?;
        Some((checked_id(face.role)?, Priority(priority)))
    };
    Ok(Face {
        id: qualified(owner, face.name, &mut checked_id)?,
        path: copy(face.path)?,
        family: if face.family.is_empty() {
            String::new()
        } else {
            copy(face.family)?
        },
        bold,
        italic: face.style & ITALIC != 0,
        role,
        weight_differs: face.weight != 0 && face.weight != if bold { 700 } else { 400 },
    })
}

/// What a plugin's resource records came to, ready to publish.
#[derive(Default)]
pub(super) struct Resources {
    faces: Vec<FontFace>,
    extensions: Vec<(Id, String, Id)>,
    roles: Vec<(Id, Id, Priority)>,
    tables: Vec<(String, String)>,
    /// The faces whose weight does not match their style bits.
    differing: Vec<Id>,
}

/// `folder/path`, with fallible allocation.
fn joined(folder: &str, path: &str) -> Result<String, i32> {
    let mut full = String::new();
    full.try_reserve_exact(folder.len() + 1 + path.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    full.push_str(folder);
    full.push('/');
    full.push_str(path);
    Ok(full)
}

fn reserve<T>(list: &mut Vec<T>) -> Result<(), i32> {
    list.try_reserve_exact(1).map_err(|_| ERR_NO_MEMORY)
}

impl Resources {
    /// Takes `resource` in, its files placed in `folder`.
    pub(super) fn add(&mut self, resource: Resource, folder: &str) -> Result<(), i32> {
        match resource {
            Resource::Face(face) => {
                reserve(&mut self.faces)?;
                if let Some((role, priority)) = face.role {
                    reserve(&mut self.roles)?;
                    let font = face.id.try_clone().ok_or(ERR_NO_MEMORY)?;
                    self.roles.push((role, font, priority));
                }
                if face.weight_differs {
                    reserve(&mut self.differing)?;
                    self.differing
                        .push(face.id.try_clone().ok_or(ERR_NO_MEMORY)?);
                }
                self.faces.push(FontFace::new(
                    face.id,
                    joined(folder, &face.path)?,
                    // An empty family costs no allocation.
                    Cow::Owned(face.family),
                    face.bold,
                    face.italic,
                ));
            }
            Resource::Extension(extension) => {
                reserve(&mut self.extensions)?;
                self.extensions.push((
                    extension.id,
                    joined(folder, &extension.path)?,
                    extension.extends,
                ));
            }
            Resource::Hyphenation(table) => {
                reserve(&mut self.tables)?;
                // A language is named once (the tag is compared without case).
                if self
                    .tables
                    .iter()
                    .any(|(language, _)| language.eq_ignore_ascii_case(&table.language))
                {
                    return Err(ERR_INVALID);
                }
                self.tables.push((table.language, table.path));
            }
        }
        Ok(())
    }

    fn entries(&self) -> usize {
        self.faces.len() + self.extensions.len()
    }
}

impl crate::Kernel {
    /// The refusals of the resources of `owner`, all before anything is
    /// published: a font id already there or named twice.
    pub(super) fn check_resources(&self, resources: &Resources) -> Result<(), RegisterError> {
        let ids = || {
            resources
                .faces
                .iter()
                .map(|face| &face.id)
                .chain(resources.extensions.iter().map(|(id, ..)| id))
        };
        for (at, id) in ids().enumerate() {
            if self.shared.fonts.contains(id.as_str()) || ids().take(at).any(|other| other == id) {
                let id = id.try_clone().ok_or(RegisterError::Invalid {
                    message: crate::registry::NATIVE_CAPACITY_UNAVAILABLE.into(),
                })?;
                return Err(RegisterError::Duplicate { id });
            }
        }
        Ok(())
    }

    /// Makes the room the resources need, and adds the hyphenation tables
    /// (the one step that can still be refused); after it, [`Self::publish_fonts`]
    /// cannot fail.
    pub(super) fn reserve_resources(
        &mut self,
        owner: &Id,
        folder: &str,
        resources: &Resources,
    ) -> Result<(), RegisterError> {
        let no_room = || RegisterError::Invalid {
            message: crate::registry::NATIVE_CAPACITY_UNAVAILABLE.into(),
        };
        self.shared
            .fonts
            .try_reserve(resources.entries(), resources.roles.len())
            .map_err(|_| no_room())?;
        if !resources.tables.is_empty() {
            self.shared
                .hyphenation
                .add_tables(
                    owner.as_str(),
                    folder,
                    resources
                        .tables
                        .iter()
                        .map(|(language, path)| (language.as_str(), path.as_str())),
                )
                .map_err(|_| no_room())?;
        }
        Ok(())
    }

    /// Takes the hyphenation tables of `owner` out again: the rest of its
    /// publication was refused.
    pub(super) fn undo_resources(&mut self, owner: &Id) {
        self.shared.hyphenation.remove_tables(owner.as_str());
    }

    /// Adds the fonts (the room is made and the ids are free).
    pub(super) fn publish_fonts(
        &mut self,
        owner: &Id,
        resources: Resources,
    ) -> Result<(), RegisterError> {
        for (id, path, extends) in resources.extensions {
            self.shared.fonts.add_extension(id, path, extends)?;
        }
        for face in resources.faces {
            if !face.is_pack() && !crate::paperpaw_text_outlines() {
                // This device reads only packs: the face is left out.
                self.native_warning(
                    owner,
                    &[
                        &face.path,
                        ": this device only reads fonts made for it (.ppfp)",
                    ],
                );
                continue;
            }
            if face.family.is_empty() {
                self.shared.fonts.add_unfamilied_face(
                    face.id,
                    face.path,
                    face.bold,
                    face.italic,
                )?;
            } else {
                self.shared.fonts.add_face(face)?;
            }
        }
        for (role, font, priority) in resources.roles {
            self.shared.fonts.propose(role, font, priority);
        }
        for id in &resources.differing {
            self.native_warning(
                owner,
                &[
                    id.as_str(),
                    ": the weight does not match the style bits; the bold bit is used",
                ],
            );
        }
        self.tell_hyphenation_collisions(owner);
        Ok(())
    }

    /// Tells which languages another plugin's table keeps from `owner`'s, or
    /// the other way round (as a data plugin's tables are told).
    fn tell_hyphenation_collisions(&mut self, owner: &Id) {
        let mut told: Vec<String> = Vec::new();
        for (lost, kept) in self.shared.hyphenation.collisions(owner.as_str()) {
            let parts = [
                lost.plugin(),
                ":",
                lost.tag(),
                ": hyphenation left out, ",
                kept.plugin(),
                " has the language",
            ];
            let Some(message) = fallible_string(&parts) else {
                break;
            };
            if told.try_reserve_exact(1).is_err() {
                break;
            }
            told.push(message);
        }
        for message in told {
            self.native_warning_text(owner, message);
        }
    }

    /// A warning from `owner` made of `parts`; none if the heap refuses it.
    fn native_warning(&mut self, owner: &Id, parts: &[&str]) {
        if let Some(message) = fallible_string(parts) {
            self.native_warning_text(owner, message);
        }
    }

    fn native_warning_text(&mut self, owner: &Id, message: String) {
        let Some(plugin) = owner.try_clone() else {
            return;
        };
        if self.warnings.try_reserve(1).is_ok() {
            self.warn(crate::WarningSource::Plugin(plugin), None, message);
        }
    }
}

/// The `parts` one after another, with fallible allocation.
fn fallible_string(parts: &[&str]) -> Option<String> {
    let mut text = String::new();
    text.try_reserve_exact(parts.iter().map(|part| part.len()).sum())
        .ok()?;
    for part in parts {
        text.push_str(part);
    }
    Some(text)
}

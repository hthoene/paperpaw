use alloc::borrow::Cow;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::fmt;
use core::ops::Range;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Storage};
use paperpaw_text::sfnt::FontInfo;
use paperpaw_text::{Font, GlyphCache, LineMetrics, PackSource, StoredPack};

use crate::builtin::fonts as roles;
use crate::collections::VecMap;
use crate::id::Id;
use crate::plugin::{Priority, RegisterError};

/// A font contribution.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FontSpec {
    /// `<plugin>.<name>`, e.g. `fonts-default.literata`.
    pub id: Id,
    pub font: Font,
    /// The family it belongs to, e.g. `Literata`: families are what users
    /// choose (with their bold and italic faces). `None` keeps a font out
    /// of the choice.
    pub family: Option<Cow<'static, str>>,
    pub bold: bool,
    pub italic: bool,
}

impl FontSpec {
    /// A font outside any family.
    pub fn new(id: Id, font: Font) -> Self {
        Self {
            id,
            font,
            family: None,
            bold: false,
            italic: false,
        }
    }

    /// Puts the font into `family` as its regular, bold, italic or bold
    /// italic face.
    pub fn family(
        mut self,
        family: impl Into<Cow<'static, str>>,
        bold: bool,
        italic: bool,
    ) -> Self {
        self.family = Some(family.into());
        self.bold = bold;
        self.italic = italic;
        self
    }
}

/// A face of a family in a file on the storage, as a data plugin names
/// it (`[family]` in its `plugin.ini`): a font pack (`.ppfp`), read a
/// glyph at a time and drawn on every device, or an OpenType/TrueType
/// file, drawn where scalable fonts are, at its style's weight (400 or
/// 700) where it is a variable font. Loaded only while its family is
/// chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FontFace {
    /// `<plugin>.<name>`, e.g. `font-fredoka.bold`.
    pub id: Id,
    /// The file on the storage.
    pub path: String,
    /// The family it belongs to, e.g. `Fredoka`.
    pub family: Cow<'static, str>,
    pub bold: bool,
    pub italic: bool,
}

impl FontFace {
    pub fn new(
        id: Id,
        path: impl Into<String>,
        family: impl Into<Cow<'static, str>>,
        bold: bool,
        italic: bool,
    ) -> Self {
        Self {
            id,
            path: path.into(),
            family: family.into(),
            bold,
            italic,
        }
    }

    /// Whether its file is a font pack, by its name.
    pub fn is_pack(&self) -> bool {
        is_pack(&self.path)
    }
}

/// Whether `path` names a font pack (`.ppfp`).
fn is_pack(path: &str) -> bool {
    path.len() > 5
        && path.is_char_boundary(path.len() - 5)
        && path[path.len() - 5..].eq_ignore_ascii_case(".ppfp")
}

/// One registered font.
#[derive(Debug)]
struct Entry {
    id: Id,
    /// `None` for a font file not loaded (yet).
    font: Option<Font>,
    family: Option<Cow<'static, str>>,
    bold: bool,
    italic: bool,
    /// 400 regular, 700 bold; picks among faces of the same style.
    weight: u16,
    /// A font file on the storage, loaded only while its family is chosen.
    file: Option<String>,
    /// For a pack that adds characters to another font (e.g. Cyrillic to
    /// a font for Latin): that font.
    extends: Option<Id>,
    /// A face named by its plugin ([`FontFace`]): a pack, or a scalable
    /// font drawn at its style's weight.
    face: bool,
}

/// Fonts asked in turn for a character a font lacks: its extensions, the
/// fallback font, the fallback's extensions.
const OTHERS: usize = 4;

/// Largest font file loaded.
const MAX_FONT_FILE: u64 = 48 * 1024 * 1024;

/// The usual step between a pack's sizes, in pixels: a size asked more
/// than half of it past a pack's largest is drawn by a pack with larger
/// ones, where one extends it.
const LARGER_STEP: f32 = 4.0;

/// Letters a pack of a font's larger sizes has where the font has them.
const LARGER_LETTERS: &str = "aenoAEO0";

/// The lines of a heading or title, as many times its size apart
/// ([`FontHandle::heading_line_height`]).
const HEADING_LEADING: f32 = 1.2;

/// All registered fonts, the role bindings and the glyph cache.
///
/// Fonts are bound to *roles* such as
/// [`builtin::fonts::UI`](crate::builtin::fonts::UI), like screens to slots.
/// Plugins propose fonts for roles; a chosen reading family
/// (`Fonts::set_family`) fills the reading roles, and the user's
/// `[fonts]` configuration overrides everything.
pub struct Fonts {
    entries: Vec<Entry>,
    candidates: Vec<(Id, Id, Priority)>,
    user: VecMap<Id, Id>,
    /// The family chosen for reading.
    family: Option<String>,
    /// What the status lines are in: a value of
    /// [`STATUS_FONT`](crate::builtin::settings::STATUS_FONT).
    status: Option<String>,
    cache: RefCell<GlyphCache>,
    /// Only described, not drawn (see [`Fonts::set_describing`]): no
    /// glyph is made.
    describing: core::cell::Cell<bool>,
}

impl Fonts {
    /// While on, screens are only described, not drawn (a blind
    /// framebuffer): no glyph is made, also not to measure ink, which is
    /// taken from the line's measures. The kernel sets it around
    /// describing a screen.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_describing(&self, on: bool) {
        self.describing.set(on);
    }
}

impl fmt::Debug for Fonts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Fonts")
            .field(
                "fonts",
                &self.entries.iter().map(|e| &e.id).collect::<Vec<_>>(),
            )
            .field("family", &self.family)
            .finish_non_exhaustive()
    }
}

impl Fonts {
    /// `cache_bytes` bounds the memory for glyphs rasterized at runtime.
    pub fn new(cache_bytes: usize) -> Self {
        Self {
            entries: Vec::new(),
            candidates: Vec::new(),
            user: VecMap::new(),
            family: None,
            status: None,
            cache: RefCell::new(GlyphCache::new(cache_bytes)),
            describing: core::cell::Cell::new(false),
        }
    }

    /// Lets the glyphs drawn at runtime go, to make room; they are drawn
    /// again as they are needed.
    #[cfg(feature = "kernel")]
    pub fn clear_cache(&self) {
        self.cache.borrow_mut().clear();
    }

    fn check_new(&self, id: &Id) -> Result<(), RegisterError> {
        if self.entries.iter().any(|e| e.id == *id) {
            return Err(RegisterError::Duplicate { id: id.clone() });
        }
        Ok(())
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, spec: FontSpec) -> Result<(), RegisterError> {
        self.check_new(&spec.id)?;
        self.entries.push(Entry {
            id: spec.id,
            font: Some(spec.font),
            family: spec.family,
            weight: if spec.bold { 700 } else { 400 },
            bold: spec.bold,
            italic: spec.italic,
            file: None,
            extends: None,
            face: false,
        });
        Ok(())
    }

    /// Adds a font pack on the storage (`.ppfp`) that adds characters to
    /// font `extends`, e.g. Cyrillic and Greek to `fonts-default.literata`:
    /// read a glyph at a time when drawn, so it takes almost no memory, and
    /// asked for what that font lacks.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_extension(
        &mut self,
        id: Id,
        path: String,
        extends: Id,
    ) -> Result<(), RegisterError> {
        self.check_new(&id)?;
        self.entries.push(Entry {
            id,
            font: None,
            family: None,
            bold: false,
            italic: false,
            weight: 400,
            file: Some(path),
            extends: Some(extends),
            face: false,
        });
        Ok(())
    }

    /// Adds a font file on the storage, described by its header (see
    /// [`sfnt::read_info`](paperpaw_text::sfnt::read_info)); it is loaded
    /// only when its family is chosen.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_file(&mut self, id: Id, path: String, info: FontInfo) -> Result<(), RegisterError> {
        self.check_new(&id)?;
        self.entries.push(Entry {
            id,
            font: None,
            family: Some(Cow::Owned(info.family)),
            bold: info.bold,
            italic: info.italic,
            weight: info.weight,
            file: Some(path),
            extends: None,
            face: false,
        });
        Ok(())
    }

    /// Adds a face of a family in a file on the storage (see
    /// [`FontFace`]); it is loaded only when its family is chosen.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_face(&mut self, face: FontFace) -> Result<(), RegisterError> {
        self.check_new(&face.id)?;
        self.entries.push(Entry {
            id: face.id,
            font: None,
            family: Some(face.family),
            weight: if face.bold { 700 } else { 400 },
            bold: face.bold,
            italic: face.italic,
            file: Some(face.path),
            extends: None,
            face: true,
        });
        Ok(())
    }

    /// Forgets every font of plugin `plugin` (ids `<plugin>.…`), e.g. of a
    /// data plugin removed while running; returns how many. Glyphs drawn
    /// so far are forgotten too.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn remove_plugin(&mut self, plugin: &str) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| e.id.namespace() != plugin);
        self.candidates
            .retain(|(_, font, _)| font.namespace() != plugin);
        let removed = before - self.entries.len();
        if removed > 0 {
            // The cache knows fonts by their place in the list.
            self.cache.borrow_mut().clear();
        }
        removed
    }

    /// Proposes `font` for `role`; the highest priority wins.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn propose(&mut self, role: Id, font: Id, priority: Priority) {
        self.candidates.push((role, font, priority));
    }

    /// Releases unused capacity once registration is complete.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.entries.shrink_to_fit();
        self.candidates.shrink_to_fit();
        self.user.shrink_to_fit();
    }

    /// The user's explicit choice for a role.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn bind_user(&mut self, role: Id, font: Id) {
        self.user.insert(role, font);
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.iter().any(|e| e.id.as_str() == id)
    }

    /// Ids of all fonts, loaded or not.
    pub fn ids(&self) -> impl Iterator<Item = &Id> {
        self.entries.iter().map(|e| &e.id)
    }

    /// The families to choose from, in registration order.
    pub fn families(&self) -> Vec<&str> {
        let mut families: Vec<&str> = Vec::new();
        for family in self.entries.iter().filter_map(|e| e.family.as_deref()) {
            if !families.contains(&family) {
                families.push(family);
            }
        }
        families
    }

    /// The family chosen for reading, if any.
    pub fn family(&self) -> Option<&str> {
        self.family.as_deref()
    }

    /// Chooses the family for reading (`None`: the plugins' proposals).
    /// Call [`Fonts::load`] afterwards for font files.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_family(&mut self, family: Option<&str>) {
        self.family = family.map(String::from);
    }

    /// Chooses what the status lines are in ([`roles::STATUS`]): a value
    /// of [`STATUS_FONT`](crate::builtin::settings::STATUS_FONT), as the
    /// menus, as the book or a family's name. Call [`Fonts::load`]
    /// afterwards for font files.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_status(&mut self, choice: &str) {
        self.status = Some(String::from(choice));
    }

    /// The family of its own the status lines are in, if one is chosen.
    fn status_family(&self) -> Option<&str> {
        use crate::builtin::settings::{STATUS_FONT_BOOK, STATUS_FONT_UI};
        self.status
            .as_deref()
            .filter(|s| *s != STATUS_FONT_UI && *s != STATUS_FONT_BOOK)
    }

    /// The face for the status lines: the user's binding, else as chosen
    /// ([`Fonts::set_status`]), else the menus'.
    fn status_face(&self) -> Option<FontHandle<'_>> {
        use crate::builtin::settings::STATUS_FONT_BOOK;
        if let Some(font) = self.user.get(roles::STATUS.as_str()) {
            return self.get(font.as_str());
        }
        let own = self
            .status_family()
            .and_then(|family| self.best_face(family, false, false, true))
            .map(|index| self.handle(index));
        let book = (self.status.as_deref() == Some(STATUS_FONT_BOOK))
            .then(|| self.role(roles::READING.as_str()))
            .flatten();
        own.or(book).or_else(|| self.role(roles::UI.as_str()))
    }

    /// The entry of `family` best for a regular, bold, italic or bold
    /// italic face: that style, the weight closest to 400 or 700; among
    /// equals the first.
    fn best_face(&self, family: &str, bold: bool, italic: bool, loaded: bool) -> Option<usize> {
        let target: i32 = if bold { 700 } else { 400 };
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.family.as_deref() == Some(family)
                    && e.bold == bold
                    && e.italic == italic
                    && (!loaded || e.font.is_some())
            })
            .min_by_key(|(_, e)| (i32::from(e.weight) - target).abs())
            .map(|(index, _)| index)
    }

    /// Loads the files of the chosen family's faces (one per style) and
    /// frees all others; returns the files that could not be loaded.
    /// Packs that extend fonts are opened too (they stay where they are).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn load(&mut self, storage: &Rc<dyn Storage>) -> Vec<String> {
        let mut failed = Vec::new();
        let chosen = self.family.clone();
        let mut wanted: Vec<usize> = chosen
            .as_deref()
            .map(|family| {
                [(false, false), (true, false), (false, true), (true, true)]
                    .iter()
                    .filter_map(|&(b, i)| self.best_face(family, b, i, false))
                    .collect()
            })
            .unwrap_or_default();
        // The status lines' family of their own: its regular face.
        if let Some(index) = self
            .status_family()
            .and_then(|family| self.best_face(family, false, false, false))
        {
            wanted.push(index);
        }
        for (index, entry) in self.entries.iter_mut().enumerate() {
            let Some(path) = &entry.file else {
                continue;
            };
            if entry.extends.is_some() {
                if entry.font.is_none() {
                    let source = Rc::new(Stored {
                        storage: storage.clone(),
                        path: path.clone(),
                    });
                    match StoredPack::open(source) {
                        Ok(pack) => entry.font = Some(Font::Stored(pack)),
                        Err(_) => failed.push(path.clone()),
                    }
                }
                continue;
            }
            if !wanted.contains(&index) {
                entry.font = None;
                continue;
            }
            if entry.font.is_some() {
                continue;
            }
            let font = if entry.face && is_pack(path) {
                let source = Rc::new(Stored {
                    storage: storage.clone(),
                    path: path.clone(),
                });
                StoredPack::open(source).ok().map(Font::Stored)
            } else {
                // A variable font at its style's weight.
                let weight = entry.face.then(|| f32::from(entry.weight));
                read_font(storage.as_ref(), path, weight)
            };
            match font {
                Some(font) => entry.font = Some(font),
                None => failed.push(path.clone()),
            }
        }
        failed
    }

    /// A font by id, if loaded.
    pub fn get(&self, id: &str) -> Option<FontHandle<'_>> {
        self.index_of(id).map(|index| self.handle(index))
    }

    /// The chosen family's face for a reading role: `None` if no family
    /// is chosen or the role is not a reading role; `Some(None)` if the
    /// family lacks that face.
    fn family_face(&self, role: &str) -> Option<Option<FontHandle<'_>>> {
        let family = self.family.as_deref()?;
        let (bold, italic) = match role {
            r if r == roles::READING.as_str() => (false, false),
            r if r == roles::READING_BOLD.as_str() => (true, false),
            r if r == roles::READING_ITALIC.as_str() => (false, true),
            r if r == roles::READING_BOLD_ITALIC.as_str() => (true, true),
            _ => return None,
        };
        let index = self.best_face(family, bold, italic, true);
        Some(index.map(|index| self.handle(index)))
    }

    fn proposed(&self, role: &str) -> Option<FontHandle<'_>> {
        // The loaded proposals' entries, so the handle is made once, for
        // the one chosen.
        self.candidates
            .iter()
            .filter(|(r, _, _)| r.as_str() == role)
            .filter_map(|(_, font, priority)| Some((self.index_of(font.as_str())?, *priority)))
            .max_by_key(|(_, priority)| *priority)
            .map(|(index, _)| self.handle(index))
    }

    /// The font bound to `role`: the user's choice, else the chosen
    /// family's face, else the best proposal, else the first font.
    pub fn role(&self, role: &str) -> Option<FontHandle<'_>> {
        if role == roles::STATUS.as_str() {
            return self.status_face();
        }
        let user = self.user.get(role).and_then(|id| self.get(id.as_str()));
        let first = || {
            self.entries
                .iter()
                .position(|e| e.font.is_some())
                .map(|index| self.handle(index))
        };
        user.or_else(|| self.family_face(role).flatten())
            .or_else(|| self.proposed(role))
            .or_else(first)
    }

    /// The font bound to `role` by the user, a chosen family or a plugin;
    /// unlike [`Fonts::role`], no fallback, for optional roles such as bold
    /// faces (a chosen family without bold has none, so its regular face
    /// is used rather than another family's bold).
    pub fn bound(&self, role: &str) -> Option<FontHandle<'_>> {
        let user = self.user.get(role).and_then(|id| self.get(id.as_str()));
        user.or_else(|| match self.family_face(role) {
            Some(face) => face,
            None => self.proposed(role),
        })
    }

    /// Whether the user chose the font for `role` in the configuration.
    pub fn user_bound(&self, role: &str) -> bool {
        self.user.contains_key(role)
    }

    /// Bytes used by the glyph cache.
    pub fn cache_bytes(&self) -> usize {
        self.cache.borrow().bytes()
    }

    paperpaw_table::shared! {
    /// A handle of loaded entry `index`, with the fonts asked for the
    /// characters it lacks: its extensions, the fallback font and the
    /// fallback's extensions.
    pub(crate) fn handle(&self, index: usize) -> FontHandle<'_> {
        let entry = &self.entries[index];
        let mut others: [Option<(&Font, u16)>; OTHERS] = [None; OTHERS];
        let mut add = |i: usize| {
            let Some(font) = self.entries[i].font.as_ref() else {
                return;
            };
            if i != index
                && !others.iter().flatten().any(|(_, n)| usize::from(*n) == i)
                && let Some(slot) = others.iter_mut().find(|o| o.is_none())
            {
                *slot = Some((font, i as u16));
            }
        };
        let mut extended = false;
        for i in self.extensions_of(index) {
            extended = true;
            add(i);
        }
        // The extension with the largest sizes beyond this pack's own, if
        // one has any: one with the font's own letters (Literata's larger
        // sizes), not one with letters of another script it adds in more
        // sizes (Arabic), by a few letters the font has. Sizes are only
        // read where there are extensions.
        let largest = |f: &Font| f.largest_size().unwrap_or(0.0);
        let larger = extended
            .then(|| {
                let font = entry.font.as_ref()?;
                let own = largest(font);
                let probe = |f: &Font| {
                    LARGER_LETTERS
                        .chars()
                        .filter(|&c| font.has_char(c))
                        .all(|c| f.has_char(c))
                };
                self.extensions_of(index)
                    .filter_map(|i| self.entries[i].font.as_ref().map(|f| (f, i as u16)))
                    .filter(|(f, _)| own > 0.0 && largest(f) > own && probe(f))
                    .max_by(|a, b| largest(a.0).total_cmp(&largest(b.0)))
            })
            .flatten();
        if let Some(fallback) = self.fallback_index() {
            add(fallback);
            for i in self.extensions_of(fallback) {
                add(i);
            }
        }
        FontHandle {
            id: &entry.id,
            // Handles are made for loaded fonts only.
            font: entry.font.as_ref().unwrap_or_else(|| unreachable!()),
            cache: &self.cache,
            describing: &self.describing,
            index: index as u16,
            others,
            larger,
            display: false,
            kerning: true,
            ligatures: false,
        }
    } @ text::FONTS_HANDLE
    }

    /// The loaded packs that extend entry `index`.
    fn extensions_of(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        let id = &self.entries[index].id;
        self.entries
            .iter()
            .enumerate()
            .filter(move |(_, e)| e.extends.as_ref() == Some(id) && e.font.is_some())
            .map(|(i, _)| i)
    }

    /// The font for characters another one lacks (e.g. Cyrillic in a UI
    /// font for Latin): the one plugins propose for reading, as reading
    /// fonts cover the most scripts.
    fn fallback_index(&self) -> Option<usize> {
        let reading = roles::READING;
        let reading = reading.as_str();
        self.candidates
            .iter()
            .filter(|(r, font, _)| r.as_str() == reading && self.index_of(font.as_str()).is_some())
            .max_by_key(|(_, _, priority)| *priority)
            .and_then(|(_, font, _)| self.index_of(font.as_str()))
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| e.id.as_str() == id && e.font.is_some())
    }
}

/// A pack on the storage, read where it is.
struct Stored {
    storage: Rc<dyn Storage>,
    path: String,
}

impl PackSource for Stored {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
        let mut done = 0;
        while done < buf.len() {
            match self
                .storage
                .read_at(&self.path, at + done as u64, &mut buf[done..])
            {
                Ok(0) | Err(_) => return false,
                Ok(n) => done += n,
            }
        }
        true
    }
}

/// Reads a font file into memory.
fn read_font(storage: &dyn Storage, path: &str, weight: Option<f32>) -> Option<Font> {
    let size = storage.size(path).ok()?;
    if size > MAX_FONT_FILE {
        return None;
    }
    let mut data = alloc::vec![0u8; size as usize];
    let mut at = 0;
    while at < data.len() {
        let n = storage.read_at(path, at as u64, &mut data[at..]).ok()?;
        if n == 0 {
            return None;
        }
        at += n;
    }
    match weight {
        Some(weight) => Font::from_data(data, &[("wght", weight)]).ok(),
        None => Font::from_data(data, &[]).ok(),
    }
}

/// Fonts a plugin loads itself, e.g. the fonts embedded in a book, with
/// a glyph cache of their own; freed when the set is dropped.
pub struct FontSet {
    fonts: Vec<(Id, Font)>,
    cache: RefCell<GlyphCache>,
    /// Only described, not drawn (see [`Fonts::set_describing`]): no
    /// glyph is made.
    describing: core::cell::Cell<bool>,
}

impl fmt::Debug for FontSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontSet")
            .field(
                "fonts",
                &self.fonts.iter().map(|(id, _)| id).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl FontSet {
    /// `cache_bytes` bounds the memory for the set's glyphs.
    pub fn new(cache_bytes: usize) -> Self {
        Self {
            fonts: Vec::new(),
            cache: RefCell::new(GlyphCache::new(cache_bytes)),
            describing: core::cell::Cell::new(false),
        }
    }

    /// Adds a font; returns its index.
    pub fn add(&mut self, id: Id, font: Font) -> usize {
        self.fonts.push((id, font));
        self.fonts.len() - 1
    }

    pub fn len(&self) -> usize {
        self.fonts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fonts.is_empty()
    }

    /// Font `index`, ready to measure and draw.
    pub fn get(&self, index: usize) -> Option<FontHandle<'_>> {
        let (id, font) = self.fonts.get(index)?;
        Some(FontHandle {
            id,
            font,
            cache: &self.cache,
            describing: &self.describing,
            index: index as u16,
            others: [None; OTHERS],
            larger: None,
            display: false,
            kerning: true,
            ligatures: false,
        })
    }
}

/// A font ready to measure and draw text.
#[derive(Clone, Copy)]
pub struct FontHandle<'a> {
    id: &'a Id,
    font: &'a Font,
    /// The glyph cache of the registry or set the font is in.
    cache: &'a RefCell<GlyphCache>,
    /// Its registry only describes what is drawn: no glyph is made.
    describing: &'a core::cell::Cell<bool>,
    /// The font's number in that cache.
    index: u16,
    /// Fonts for the characters this one lacks, asked in turn, and their
    /// numbers in the same cache.
    others: [Option<(&'a Font, u16)>; OTHERS],
    /// A pack that extends this one with larger sizes (a plugin's, for
    /// sizes the firmware leaves out), and its number: it draws text
    /// asked larger than this font's largest size.
    larger: Option<(&'a Font, u16)>,
    /// Draws at a pack's large sizes made for a few characters too (see
    /// [`FontHandle::display`]).
    display: bool,
    /// Pairs of characters kerned (see [`FontHandle::unkerned`]).
    kerning: bool,
    /// Ligatures drawn (see [`FontHandle::ligatures`]).
    ligatures: bool,
}

/// The ligature of two characters, if there is one: drawn in their place
/// where the font has it, the pair set as wide as the ligature is (see
/// [`FontHandle::ligatures`]).
fn ligature(left: char, right: char) -> Option<char> {
    match (left, right) {
        ('f', 'i') => Some('\u{fb01}'),
        ('f', 'l') => Some('\u{fb02}'),
        _ => None,
    }
}

/// Ligatures of three characters and of `ff`: drawn only where they take
/// the room their characters take apart, within this many pixels.
const LIGATURE_FIT: f32 = 0.5;

impl fmt::Debug for FontHandle<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("FontHandle").field(self.id()).finish()
    }
}

impl<'a> FontHandle<'a> {
    pub fn id(&self) -> &'a Id {
        self.id
    }

    fn font(&self) -> &'a Font {
        self.font
    }

    paperpaw_table::shared! {
    /// The largest size text can be set in, in pixels: a font pack has
    /// the sizes it was made in, and those of a pack that extends it with
    /// larger ones; `None` for a scalable font, which has every size.
    pub fn largest_size(&self) -> Option<f32> {
        let own = self.font().largest_size();
        match self.larger.and_then(|(font, _)| font.largest_size()) {
            Some(larger) => Some(own.map_or(larger, |own| own.max(larger))),
            None => own,
        }
    } @ text::HANDLE_LARGEST_SIZE
    }

    /// The handle of the pack that extends this one with larger sizes, for
    /// text asked at `size`, where it is past this font's largest size by
    /// more than half of the usual step between sizes; `None` where this
    /// font draws it.
    fn larger_for(&self, size: f32) -> Option<Self> {
        let (font, index) = self.larger?;
        let own = self.font.largest_size()?;
        (size >= own + LARGER_STEP / 2.0).then_some(Self {
            font,
            index,
            larger: None,
            ..*self
        })
    }

    paperpaw_table::shared! {
    /// Ascent, descent and line height at `size` pixels: of the size text
    /// is drawn at (see [`FontHandle::size_for`]; for a display, the size
    /// asked).
    pub fn line_metrics(&self, size: f32) -> LineMetrics {
        if let Some(larger) = self.larger_for(size) {
            return larger.line_metrics(size);
        }
        self.font()
            .line_metrics(self.font().size_for(size, "", self.display))
    } @ text::HANDLE_LINE_METRICS
    }

    paperpaw_table::shared! {
    /// How far apart the lines of a heading or title set on more lines
    /// are, in pixels: closer than those of text (its line height, about
    /// 1.4 times the size with the default font), 1.2 times the size, so
    /// that a title on two lines reads as one; the highest letters of a
    /// line (capitals with accents) still stay clear of the lowest of the
    /// line above (descenders). Text keeps its line height.
    pub fn heading_line_height(&self, size: f32) -> f32 {
        if let Some(larger) = self.larger_for(size) {
            return larger.heading_line_height(size);
        }
        let drawn = self.font().size_for(size, "", self.display);
        // In whole pixels, rounded up (no float rounding in `no_std`).
        let apart = (drawn * HEADING_LEADING + 0.999) as u32 as f32;
        apart.min(self.line_metrics(size).line_height)
    } @ text::HANDLE_HEADING_LINE_HEIGHT
    }

    /// The font that draws `ch` and its number in the cache: this one,
    /// or the fallback if only that has it. Without fallbacks, this one
    /// whatever it has: nothing to look up.
    fn pick(&self, ch: char) -> (&'a Font, u16) {
        if self.others[0].is_none() || self.font.has_char(ch) {
            return (self.font, self.index);
        }
        self.others
            .iter()
            .flatten()
            .find(|(font, _)| font.has_char(ch))
            .copied()
            .unwrap_or((self.font, self.index))
    }

    paperpaw_table::shared! {
    /// Whether this font (or its fallback) draws `ch`; without it, the
    /// character would show as a question mark or not at all.
    pub fn has_char(&self, ch: char) -> bool {
        self.font.has_char(ch)
            || self
                .others
                .iter()
                .flatten()
                .any(|(font, _)| font.has_char(ch))
    } @ text::HANDLE_HAS_CHAR
    }

    paperpaw_table::shared! {
    /// Horizontal advance of one character in pixels, at the size it is
    /// drawn at alone (see [`FontHandle::size_for`]).
    pub fn advance(&self, size: f32, ch: char) -> f32 {
        self.advance_by(size, ch, None)
    } @ text::HANDLE_ADVANCE
    }

    /// [`FontHandle::advance`], `font` the one [`FontHandle::pick`] gives
    /// for `ch` where the caller has it already.
    fn advance_by(&self, size: f32, ch: char, font: Option<&Font>) -> f32 {
        if let Some(larger) = self.larger_for(size) {
            return larger.advance(size, ch);
        }
        let mut buffer = [0u8; 4];
        let size = self.size_for(size, ch.encode_utf8(&mut buffer));
        font.unwrap_or_else(|| self.pick(ch).0).advance(ch, size)
    }

    /// This font without kerning: each character set after the one
    /// before by its advance alone, as fonts were set before kerning (e.g.
    /// a reader's setting for book text). Measuring and drawing agree
    /// either way.
    pub fn unkerned(self) -> Self {
        Self {
            kerning: false,
            ..self
        }
    }

    /// This font with ligatures: `fi` and `fl` drawn as one glyph where
    /// the font has it, the pair set as wide as that glyph (so measuring,
    /// as [`FontHandle::kern`] does, agrees); `ff`, `ffi` and `ffl` drawn
    /// as one only where that takes the room of their characters apart.
    /// The text stays as it is: positions, hyphenation and search are
    /// those of its characters. For book text, not for the menus.
    pub fn ligatures(self) -> Self {
        Self {
            ligatures: true,
            ..self
        }
    }

    paperpaw_table::shared! {
    /// How much `right` moves when it follows `left` at `size`, in pixels
    /// (negative: closer), as [`FontHandle::measure`] and the drawing
    /// methods set the pair: the font's kerning (none where the two are
    /// drawn by different fonts, or [`FontHandle::unkerned`]), or with
    /// [`FontHandle::ligatures`] the room a ligature saves. What a layout
    /// adds between the characters of a word it measures one by one.
    pub fn kern(&self, size: f32, left: char, right: char) -> f32 {
        if let Some(larger) = self.larger_for(size) {
            return larger.kern(size, left, right);
        }
        let font = match self.others[0] {
            None => self.font,
            Some(_) => {
                let (font, index) = self.pick(left);
                if self.pick(right).1 != index {
                    return 0.0;
                }
                font
            }
        };
        let size = match self.display {
            true => {
                let mut pair = [0u8; 8];
                let len = left.encode_utf8(&mut pair).len();
                let len = len + right.encode_utf8(&mut pair[len..]).len();
                self.size_for(size, core::str::from_utf8(&pair[..len]).unwrap_or(""))
            }
            false => size,
        };
        self.pair_in(font, size, left, right)
    } @ text::HANDLE_KERN
    }

    /// The adjustment of a pair both drawn by `font` at `size` (see
    /// [`FontHandle::kern`]).
    fn pair_in(&self, font: &Font, size: f32, left: char, right: char) -> f32 {
        if self.ligatures
            && let Some(joined) = ligature(left, right)
            && font.has_char(joined)
        {
            let apart = font.advance(left, size) + font.advance(right, size);
            return font.advance(joined, size) - apart;
        }
        match self.kerning {
            true => font.kern(left, right, size),
            false => 0.0,
        }
    }

    /// This font for a text of a few characters drawn large alone, a
    /// clock's time or a number: where its pack keeps large sizes only for
    /// some characters (the digits and the colon, on devices that draw
    /// from packs), a text of those is drawn at them; any other text at
    /// the sizes that have every character, as by the font itself.
    pub fn display(self) -> Self {
        Self {
            display: true,
            ..self
        }
    }

    paperpaw_table::shared! {
    /// The size `text` is drawn at when asked for `size`: the nearest size
    /// of a pack that has all of its characters (see
    /// [`FontHandle::display`]), one for the whole text; `size` where the
    /// font is scalable. Measuring, inking and drawing use it.
    pub fn size_for(&self, size: f32, text: &str) -> f32 {
        if let Some(larger) = self.larger_for(size) {
            return larger.size_for(size, text);
        }
        self.font.size_for(size, text, self.display)
    } @ text::HANDLE_SIZE_FOR
    }

    paperpaw_table::shared! {
    /// Width of a single line of text in pixels, its pairs kerned (see
    /// [`FontHandle::kern`]).
    pub fn measure(&self, size: f32, text: &str) -> f32 {
        if let Some(larger) = self.larger_for(size) {
            return larger.measure(size, text);
        }
        let size = self.size_for(size, text);
        let single = self.others[0].is_none();
        let mut width = 0.0;
        let mut before: Option<(char, u16)> = None;
        for ch in text.chars() {
            let (font, index) = match single {
                true => (self.font, self.index),
                false => self.pick(ch),
            };
            if let Some((left, at)) = before
                && at == index
            {
                width += self.pair_in(font, size, left, ch);
            }
            width += match single {
                true => font.advance(ch, size),
                false => self.advance_by(size, ch, Some(font)),
            };
            before = Some((ch, index));
        }
        width
    } @ text::HANDLE_MEASURE
    }

    paperpaw_table::shared! {
    /// How far the ink of one line of `text` reaches above and below its
    /// baseline, in pixels (above negative), counting pixels at least half
    /// covered, as panels of black and white show them: what centres it
    /// by its ink rather than by its line ([`Ui::header`]). `None`
    /// for text without ink (spaces).
    ///
    /// [`Ui::header`]: crate::ui::Ui::header
    pub fn ink(&self, size: f32, text: &str) -> Option<(i32, i32)> {
        if let Some(larger) = self.larger_for(size) {
            return larger.ink(size, text);
        }
        // Only described: as a line would be, no glyph made.
        if self.describing.get() {
            let metrics = self.line_metrics(size);
            return Some((-(metrics.ascent as i32), -(metrics.descent as i32)));
        }
        let size = self.size_for(size, text);
        let mut cache = self.cache.borrow_mut();
        let mut ink: Option<(i32, i32)> = None;
        for ch in text.chars() {
            let (font, index) = self.pick(ch);
            let glyph = cache.glyph(index, font, ch, size);
            let inked = |y: u16| (0..glyph.width).any(|x| glyph.coverage(x, y) >= 128);
            let (Some(first), Some(last)) = (
                (0..glyph.height).find(|y| inked(*y)),
                (0..glyph.height).rev().find(|y| inked(*y)),
            ) else {
                continue;
            };
            let top = i32::from(glyph.top) + i32::from(first);
            let bottom = i32::from(glyph.top) + i32::from(last) + 1;
            ink = Some(ink.map_or((top, bottom), |(t, b)| (t.min(top), b.max(bottom))));
        }
        ink
    } @ text::HANDLE_INK
    }

    paperpaw_table::shared! {
    /// Where the ink of `text` at `size` begins and ends across, from the
    /// pen's start, in pixels (counting pixels at least half covered, as
    /// [`FontHandle::ink`] does): what centres it by its ink rather than by
    /// its advance (e.g. an icon in its column). `None` for text without
    /// ink.
    pub fn ink_across(&self, size: f32, text: &str) -> Option<(i32, i32)> {
        if let Some(larger) = self.larger_for(size) {
            return larger.ink_across(size, text);
        }
        // Only described: as far as its advances go, no glyph made.
        if self.describing.get() {
            return Some((0, self.measure(size, text) as i32));
        }
        let size = self.size_for(size, text);
        let mut cache = self.cache.borrow_mut();
        let mut ink: Option<(i32, i32)> = None;
        let mut pen = 0.0f32;
        let mut based = false;
        let mut before: Option<(char, u16)> = None;
        for ch in text.chars() {
            let (font, index) = self.pick(ch);
            if let Some((left, at)) = before
                && at == index
            {
                pen += self.pair_in(font, size, left, ch);
            }
            before = Some((ch, index));
            let glyph = cache.glyph(index, font, ch, size);
            let mark = paperpaw_text::is_mark(ch);
            // Where it is drawn ([`FontHandle::draw`]).
            let at = paperpaw_text::glyph_x(pen, glyph.left, mark && !based);
            based |= !mark;
            let inked = |x: u16| (0..glyph.height).any(|y| glyph.coverage(x, y) >= 128);
            if let (Some(first), Some(last)) = (
                (0..glyph.width).find(|x| inked(*x)),
                (0..glyph.width).rev().find(|x| inked(*x)),
            ) {
                let (left, right) = (at + i32::from(first), at + i32::from(last) + 1);
                ink = Some(ink.map_or((left, right), |(l, r)| (l.min(left), r.max(right))));
            }
            pen += glyph.advance;
        }
        ink
    } @ text::HANDLE_INK_ACROSS
    }

    paperpaw_table::shared! {
    /// Breaks `text` into lines that fit `max_width`, its pairs kerned
    /// within each line as drawn.
    pub fn wrap(&self, size: f32, text: &str, max_width: f32) -> Vec<Range<usize>> {
        let advance = |_: usize, ch: char| {
            let mut buffer = [0u8; 4];
            self.measure(size, ch.encode_utf8(&mut buffer))
        };
        let kern = |_: usize, left: char, right: char| self.kern(size, left, right);
        paperpaw_text::wrap_lines_kerned(text, |_| max_width, advance, kern, None)
            .into_iter()
            .map(|line| line.range)
            .collect()
    } @ text::HANDLE_WRAP
    }

    paperpaw_table::shared! {
    /// Draws one line with its baseline at `baseline`; returns its width.
    #[allow(
        clippy::too_many_arguments,
        reason = "its inputs are separate things; a struct of them would serve this call alone"
    )]
    pub fn draw(
        &self,
        canvas: &mut Canvas<'_>,
        size: f32,
        x: f32,
        baseline: i32,
        text: &str,
        color: Color,
    ) -> f32 {
        self.draw_bolder(canvas, size, x, baseline, text, color, 0)
    } @ text::HANDLE_DRAW
    }

    paperpaw_table::shared! {
    /// Like [`FontHandle::draw`], with glyphs widened by `extra` pixels
    /// (e.g. a reader's font weight setting).
    #[allow(
        clippy::too_many_arguments,
        reason = "its inputs are separate things; a struct of them would serve this call alone"
    )]
    pub fn draw_bolder(
        &self,
        canvas: &mut Canvas<'_>,
        size: f32,
        x: f32,
        baseline: i32,
        text: &str,
        color: Color,
        extra: u16,
    ) -> f32 {
        let tone = paperpaw_text::ToneCurve::SMOOTH;
        self.draw_toned(canvas, size, x, baseline, text, color, extra, tone)
    } @ text::HANDLE_DRAW_BOLDER
    }

    paperpaw_table::shared! {
    /// Like [`FontHandle::draw_bolder`], the glyphs' coverage drawn in
    /// `tone`: on a contrast curve, or without grays (e.g. a reader's
    /// settings for book text).
    #[allow(
        clippy::too_many_arguments,
        reason = "its inputs are separate things; a struct of them would serve this call alone"
    )]
    pub fn draw_toned(
        &self,
        canvas: &mut Canvas<'_>,
        size: f32,
        x: f32,
        baseline: i32,
        text: &str,
        color: Color,
        extra: u16,
        tone: paperpaw_text::ToneCurve,
    ) -> f32 {
        if let Some(larger) = self.larger_for(size) {
            return larger.draw_toned(canvas, size, x, baseline, text, color, extra, tone);
        }
        // Only described, as a screen reader asks: no glyph made.
        if canvas.is_blind() || self.describing.get() {
            return self.measure(size, text);
        }
        let size = self.size_for(size, text);
        let mut cache = self.cache.borrow_mut();
        let mut pen = x;
        // Runs of characters drawn by the same font; marks at the start of
        // one sit on the character before it, if there is one.
        let mut rest = text;
        let mut based = false;
        while let Some(first) = rest.chars().next() {
            let (font, index) = self.pick(first);
            let end = rest
                .char_indices()
                .skip(1)
                .find(|(_, ch)| self.pick(*ch).1 != index)
                .map_or(rest.len(), |(at, _)| at);
            let style = paperpaw_text::TextStyle::new(font, index, size, color)
                .bolder(extra)
                .curve(tone)
                .based(based);
            let run = &rest[..end];
            pen += self.draw_run(canvas, &mut cache, &style, pen, baseline, run);
            based |= run.chars().any(|ch| !paperpaw_text::is_mark(ch));
            rest = &rest[end..];
        }
        pen - x
    } @ text::HANDLE_DRAW_TONED
    }

    /// Width of `text` at `size` as [`FontHandle::draw_in`] sets it.
    pub fn measure_in(&self, size: f32, text: &str, around: &paperpaw_text::Around) -> f32 {
        if let Some(larger) = self.larger_for(size) {
            return larger.measure_in(size, text, around);
        }
        if plain_in(text, around) {
            return self.measure(size, text);
        }
        let size = self.size_for(size, text);
        paperpaw_text::measure_in(self.pick_text(text).0, size, text, around)
    }

    /// Draws one line of `text` as what lies `around` it asks (see
    /// [`Around`](paperpaw_text::Around)): its characters in the order
    /// they are shown, Arabic letters joined (also with `around`'s
    /// neighbours), brackets mirrored in right-to-left text; returns its
    /// width. Text left to right alone, with nothing to join at its edges,
    /// is drawn as [`FontHandle::draw_toned`] draws it. Other text is
    /// drawn in one font: the one of its first letter.
    #[allow(
        clippy::too_many_arguments,
        reason = "its inputs are separate things; a struct of them would serve this call alone"
    )]
    pub fn draw_in(
        &self,
        canvas: &mut Canvas<'_>,
        size: f32,
        x: f32,
        baseline: i32,
        text: &str,
        color: Color,
        (extra, tone): (u16, paperpaw_text::ToneCurve),
        around: &paperpaw_text::Around,
    ) -> f32 {
        if let Some(larger) = self.larger_for(size) {
            return larger.draw_in(
                canvas,
                size,
                x,
                baseline,
                text,
                color,
                (extra, tone),
                around,
            );
        }
        if plain_in(text, around) {
            return self.draw_toned(canvas, size, x, baseline, text, color, extra, tone);
        }
        if canvas.is_blind() || self.describing.get() {
            return self.measure_in(size, text, around);
        }
        let size = self.size_for(size, text);
        let (font, index) = self.pick_text(text);
        let style = paperpaw_text::TextStyle::new(font, index, size, color)
            .bolder(extra)
            .curve(tone);
        let mut cache = self.cache.borrow_mut();
        paperpaw_text::draw_text_in(canvas, &mut cache, &style, x, baseline, text, around)
    }

    /// The font `text` is drawn in by [`FontHandle::draw_in`]: the one of
    /// its first character that is not a space or a mark.
    fn pick_text(&self, text: &str) -> (&'a Font, u16) {
        text.chars()
            .find(|c| !c.is_whitespace() && !paperpaw_text::is_mark(*c))
            .map_or((self.font, self.index), |c| self.pick(c))
    }

    /// Draws `run`, all in `style`'s font, from `x`; its width. At once
    /// where the font kerns it as this handle does; else character by
    /// character, each where measuring puts it, ligatures in place of
    /// their characters.
    fn draw_run(
        &self,
        canvas: &mut Canvas<'_>,
        cache: &mut GlyphCache,
        style: &paperpaw_text::TextStyle<'_>,
        x: f32,
        baseline: i32,
        run: &str,
    ) -> f32 {
        if self.kerning && !(self.ligatures && run.contains('f')) {
            return paperpaw_text::draw_text(canvas, cache, style, x, baseline, run);
        }
        let (font, size) = (style.font, style.size);
        let mut style = *style;
        let mut pen = x;
        let mut before: Option<char> = None;
        let mut at = 0;
        let mut buffer = [0u8; 4];
        while let Some(ch) = run[at..].chars().next() {
            if let Some(left) = before {
                pen += self.pair_in(font, size, left, ch);
            }
            let joined = match self.ligatures && ch == 'f' {
                true => self.ligature_at(font, size, &run[at..]),
                false => None,
            };
            let (drawn, len, width) = joined.unwrap_or((ch, ch.len_utf8(), font.advance(ch, size)));
            paperpaw_text::draw_text(
                canvas,
                cache,
                &style,
                pen,
                baseline,
                drawn.encode_utf8(&mut buffer),
            );
            if !paperpaw_text::is_mark(ch) {
                style = style.based(true);
            }
            pen += width;
            before = run[..at + len].chars().next_back();
            at += len;
        }
        pen - x
    }

    /// The ligature drawn for the start of `text` (an `f`): its
    /// character, the bytes of `text` it stands for and the width those
    /// take as measured, with the pairs between them. `None` where the
    /// `f` is drawn alone: the font lacks the ligature, or one of three
    /// characters or `ff` would not take their room, or the next `f`
    /// makes a ligature with the character after it.
    fn ligature_at(&self, font: &Font, size: f32, text: &str) -> Option<(char, usize, f32)> {
        let mut chars = text.chars();
        let (Some('f'), Some(second)) = (chars.next(), chars.next()) else {
            return None;
        };
        let advance = |ch: char| font.advance(ch, size);
        if let Some(joined) = ligature('f', second) {
            let width = advance('f') + self.pair_in(font, size, 'f', second) + advance(second);
            return font.has_char(joined).then_some((joined, 2, width));
        }
        if second != 'f' {
            return None;
        }
        let two = advance('f') + self.pair_in(font, size, 'f', 'f') + advance('f');
        let fits = |joined: char, width: f32| {
            font.has_char(joined) && (advance(joined) - width).abs() <= LIGATURE_FIT
        };
        if let Some(third @ ('i' | 'l')) = chars.next() {
            let three = two + self.pair_in(font, size, 'f', third) + advance(third);
            let joined = if third == 'i' { '\u{fb03}' } else { '\u{fb04}' };
            if fits(joined, three) {
                return Some((joined, 3, three));
            }
            if ligature('f', third).is_some_and(|l| font.has_char(l)) {
                return None;
            }
        }
        fits('\u{fb00}', two).then_some(('\u{fb00}', 2, two))
    }
}

/// Whether `text` is set as [`FontHandle::measure`] sets it, whatever is
/// `around` it: left to right alone, no flags, no letter at its edges it
/// could join.
fn plain_in(text: &str, around: &paperpaw_text::Around) -> bool {
    let joins = |c: Option<char>| c.is_some_and(|c| matches!(c, '\u{0600}'..='\u{08FF}'));
    around.base != paperpaw_lang::Direction::Rtl
        && around.flags == 0
        && !joins(around.before)
        && !joins(around.after)
        && !paperpaw_text::bidi::needs_bidi(text)
}

#[cfg(test)]
mod tests;

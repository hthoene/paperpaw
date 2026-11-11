use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_document::{Document, DocumentError};
use paperpaw_image::Fit;
use paperpaw_platform::Storage;

use crate::id::Id;
use crate::plugin::RegisterError;

/// Opens a file of a format as a [`Document`].
pub type FormatOpener =
    Box<dyn Fn(&dyn Storage, &str, &OpenOptions) -> Result<Box<dyn Document>, DocumentError>>;

/// Whether a file's first bytes are of a format (see
/// [`FormatSpec::sniff`]).
pub type FormatSniff = Box<dyn Fn(&[u8]) -> bool>;

/// Bytes of a file a format's [`FormatSpec::sniff`] gets at most.
pub const SNIFF_BYTES: usize = 512;

/// Files told by their first bytes kept (see [`Formats::for_file`]).
pub const SNIFFED: usize = 32;

/// Whether a document opens at once as `options` say (see
/// [`FormatSpec::prepared`]).
pub type FormatPrepared = Box<dyn Fn(&dyn Storage, &str, &OpenOptions) -> bool>;

/// How a document is opened.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpenOptions {
    /// Apply the book's own styles (e.g. the CSS of an EPUB). Without
    /// them the text keeps only its markup: emphasis, headings, lists.
    pub publisher_styles: bool,
    /// Stylesheets to apply after the book's own, e.g. the plugins'
    /// ([`Env::stylesheets`](crate::Env)); with the publisher's styles
    /// only.
    pub css: String,
    /// Pictures in colour, for colour screens (ADR 0015); gray otherwise.
    pub color: bool,
    /// The size pictures are decoded to at most: as large as they show on
    /// the screen, so a small device does not decode what it cannot show.
    pub image_fit: Fit,
    /// The size covers are decoded to at most ([`Document::cover`](crate::document::Document::cover)):
    /// as large as lists and the home screen show them.
    pub cover_fit: Fit,
    /// The values of the format plugins' own settings, as configuration
    /// files write them (a text file's encoding, a comic's direction),
    /// each book's own where it has them: a format reads its own with
    /// [`OpenOptions::setting`].
    pub settings: Vec<(String, String)>,
}

/// The size of pictures where the screen does not say otherwise.
pub const IMAGE_FIT: Fit = Fit::side(1600);

/// The size of covers where the screen does not say otherwise.
pub const COVER_FIT: Fit = Fit::side(300);

/// The size of covers on a screen whose pictures fit `screen`: three
/// eighths of its longer side (about as a grid of covers shows them), at
/// least [`COVER_FIT`].
pub fn cover_fit(screen: Fit) -> Fit {
    Fit::side((screen.long * 3 / 8).max(COVER_FIT.long))
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            publisher_styles: true,
            css: String::new(),
            color: false,
            image_fit: IMAGE_FIT,
            cover_fit: COVER_FIT,
            settings: Vec::new(),
        }
    }
}

impl OpenOptions {
    pub fn publisher_styles(mut self, on: bool) -> Self {
        self.publisher_styles = on;
        self
    }

    /// With `css` applied after the book's own styles.
    pub fn css(mut self, css: String) -> Self {
        self.css = css;
        self
    }

    /// With pictures in colour, or gray.
    pub fn color(mut self, on: bool) -> Self {
        self.color = on;
        self
    }

    /// With pictures decoded no larger than `fit` allows.
    pub fn image_fit(mut self, fit: Fit) -> Self {
        self.image_fit = fit;
        self
    }

    /// With covers decoded no larger than `fit` allows.
    pub fn cover_fit(mut self, fit: Fit) -> Self {
        self.cover_fit = fit;
        self
    }

    /// With these values of format settings (`id`, value).
    pub fn settings(mut self, settings: Vec<(String, String)>) -> Self {
        self.settings = settings;
        self
    }

    /// The value of format setting `id`, as configuration files write it,
    /// if given.
    pub fn setting(&self, id: &str) -> Option<&str> {
        self.settings
            .iter()
            .find(|(setting, _)| setting == id)
            .map(|(_, value)| value.as_str())
    }
}

/// A file format contribution (TXT, EPUB, FB2, …).
#[non_exhaustive]
pub struct FormatSpec {
    /// `<plugin>.<name>`, e.g. `format-txt.txt`.
    pub id: Id,
    /// Translation key of the format's display name.
    pub title: Cow<'static, str>,
    /// Lowercase file extensions without the dot, e.g. `["txt", "text"]`;
    /// also double ones such as `fb2.zip`.
    pub extensions: Vec<Cow<'static, str>>,
    pub open: FormatOpener,
    /// Whether a document opens at once, or is prepared first (e.g.
    /// converted into a cache), which takes a while the first time: a
    /// reader says so meanwhile. `None`: always at once.
    pub prepared: Option<FormatPrepared>,
    /// Whether a file whose extension no format has is of this format,
    /// from its first bytes (at most `SNIFF_BYTES`), e.g. a book
    /// downloaded without one.
    pub sniff: Option<FormatSniff>,
}

impl FormatSpec {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(
        id: Id,
        title: impl Into<Cow<'static, str>>,
        extensions: Vec<Cow<'static, str>>,
        open: FormatOpener,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            extensions,
            open,
            prepared: None,
            sniff: None,
        }
    }

    pub fn prepared(mut self, prepared: FormatPrepared) -> Self {
        self.prepared = Some(prepared);
        self
    }

    /// Tells files of this format by their first bytes where their
    /// extension does not.
    pub fn sniff(mut self, sniff: impl Fn(&[u8]) -> bool + 'static) -> Self {
        self.sniff = Some(Box::new(sniff));
        self
    }
}

impl fmt::Debug for FormatSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FormatSpec")
            .field("id", &self.id)
            .field("extensions", &self.extensions)
            .finish_non_exhaustive()
    }
}

/// The formats the kernel knows of besides the ones added here (those of
/// native format plugins, interface 7): their files are books too, though
/// they are not opened through [`Formats::open`]. Asked by name only; a
/// check borrows what it compares and allocates nothing (a walk asks for
/// every file of the library).
pub trait OtherFormats {
    /// Whether there is any.
    fn any_other_format(&self) -> bool;
    /// Whether one claims the file `name` by its extension (ASCII case
    /// ignored, as FAT does).
    fn other_format_claims(&self, name: &str) -> bool;
}

/// The kernel's [`OtherFormats`], if set.
#[derive(Default)]
struct Others(Option<Box<dyn OtherFormats>>);

impl fmt::Debug for Others {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_some() {
            "Others(set)"
        } else {
            "Others(none)"
        })
    }
}

/// All registered file formats.
#[derive(Debug, Default)]
pub struct Formats {
    specs: Vec<FormatSpec>,
    /// The formats of native plugins, asked where `specs` do not match.
    others: Others,
    /// The screen shows colour: documents open with colour pictures.
    color: bool,
    /// The screen's size for pictures, if set.
    image_fit: Option<Fit>,
    /// The files told by their first bytes lately, with their sizes and
    /// formats (by index): asked again (a list turning pages, a book
    /// prepared then opened), they are not read again. At most
    /// [`SNIFFED`], by a hash of their path (little memory); the storage
    /// gives no time a file changed, so one replaced by another of the
    /// same size is told as before.
    sniffed: core::cell::RefCell<Vec<(u64, u64, Option<usize>)>>,
}

impl Formats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether documents open with colour pictures, for colour screens
    /// (set by the kernel from the device).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_color(&mut self, on: bool) {
        self.color = on;
    }

    /// The screen's size for pictures: they are decoded no larger than it
    /// shows them, upright or turned (set by the kernel from the device).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_image_fit(&mut self, fit: Fit) {
        self.image_fit = Some(fit);
    }

    /// How documents open here unless asked otherwise: as the book
    /// intends, with pictures in colour on colour screens, as large as the
    /// screen shows them.
    pub fn options(&self) -> OpenOptions {
        let fit = self.image_fit.map_or(IMAGE_FIT, |f| f.min(IMAGE_FIT));
        let covers = self.image_fit.map_or(COVER_FIT, cover_fit);
        OpenOptions::default()
            .color(self.color)
            .image_fit(fit)
            .cover_fit(covers)
    }

    /// The formats known besides the ones added (see [`OtherFormats`]),
    /// set by the kernel when the first native plugin is admitted.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_others(&mut self, others: Box<dyn OtherFormats>) {
        self.others = Others(Some(others));
    }

    /// Releases unused capacity once registration is complete.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.specs.shrink_to_fit();
    }

    /// Registers a format. Later registrations win for shared extensions,
    /// so a plugin can replace a built-in format.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, spec: FormatSpec) -> Result<(), RegisterError> {
        if self.specs.iter().any(|s| s.id == spec.id) {
            return Err(RegisterError::Duplicate { id: spec.id });
        }
        self.specs.push(spec);
        Ok(())
    }

    /// Whether no format was added and the kernel knows of no other
    /// ([`OtherFormats`]): nothing can be a book.
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty() && !self.others.0.as_ref().is_some_and(|o| o.any_other_format())
    }

    pub fn contains(&self, id: &str) -> bool {
        self.specs.iter().any(|s| s.id.as_str() == id)
    }

    /// The format handling `path`, by file extension (the longest that
    /// matches, so `book.fb2.zip` goes to `fb2.zip`).
    pub fn for_path(&self, path: &str) -> Option<&FormatSpec> {
        let name = crate::files::name(path);
        let mut best: Option<(&FormatSpec, usize)> = None;
        for spec in self.specs.iter().rev() {
            for e in spec
                .extensions
                .iter()
                .filter(|e| crate::files::has_extension(name, e))
            {
                if best.is_none_or(|(_, len)| e.len() > len) {
                    best = Some((spec, e.len()));
                }
            }
        }
        best.map(|(spec, _)| spec)
    }

    /// Whether any format can open `path`, by its extension: one added
    /// here or another the kernel knows of ([`OtherFormats`]).
    pub fn supports(&self, path: &str) -> bool {
        self.for_path(path).is_some()
            || self
                .others
                .0
                .as_ref()
                .is_some_and(|o| o.other_format_claims(crate::files::name(path)))
    }

    /// The format of the file at `path`: by its extension, else, for a
    /// file without one ([`Formats::may_sniff`]), by its first bytes
    /// ([`FormatSpec::sniff`]), read once for the same file and size.
    pub fn for_file(&self, storage: &dyn Storage, path: &str) -> Option<&FormatSpec> {
        if let Some(spec) = self.for_path(path) {
            return Some(spec);
        }
        if !self.may_sniff(path) {
            return None;
        }
        let size = storage.size(path).ok()?;
        let hash = crate::digest::fnv1a64(path);
        let known = self
            .sniffed
            .borrow()
            .iter()
            .find(|(known, known_size, _)| *known == hash && *known_size == size)
            .map(|(_, _, at)| *at);
        if let Some(at) = known {
            return at.and_then(|at| self.specs.get(at));
        }
        let mut head = [0u8; SNIFF_BYTES];
        let read = storage.read_at(path, 0, &mut head).ok()?;
        let head = &head[..read.min(SNIFF_BYTES)];
        let at = self
            .specs
            .iter()
            .rposition(|s| s.sniff.as_ref().is_some_and(|sniff| sniff(head)));
        let mut sniffed = self.sniffed.borrow_mut();
        sniffed.retain(|(known, _, _)| *known != hash);
        if sniffed.len() == SNIFFED {
            sniffed.remove(0);
        }
        sniffed.push((hash, size, at));
        at.and_then(|at| self.specs.get(at))
    }

    /// Whether any format tells files by their first bytes.
    pub fn sniffs(&self) -> bool {
        self.specs.iter().any(|s| s.sniff.is_some())
    }

    /// Whether the file `name` is told by its first bytes, if at all: it
    /// has no extension (a book downloaded without one) and a format
    /// tells files so. Lists read only those, and after listing.
    pub fn may_sniff(&self, name: &str) -> bool {
        let name = crate::files::name(name);
        !name.trim_start_matches('.').contains('.') && self.sniffs()
    }

    /// Whether any format can open the file at `path`: by its extension,
    /// or, without one, by its first bytes.
    pub fn supports_file(&self, storage: &dyn Storage, path: &str) -> bool {
        self.supports(path) || self.may_sniff(path) && self.for_file(storage, path).is_some()
    }

    /// Opens `path` with the matching format, as the book intends (see
    /// [`Formats::options`]).
    pub fn open(
        &self,
        storage: &dyn Storage,
        path: &str,
    ) -> Result<Box<dyn Document>, DocumentError> {
        self.open_with(storage, path, &self.options())
    }

    /// Whether `path` opens at once with `options`, or is prepared first,
    /// which takes a while (see [`FormatSpec::prepared`]).
    pub fn prepared(&self, storage: &dyn Storage, path: &str, options: &OpenOptions) -> bool {
        self.for_file(storage, path)
            .and_then(|spec| spec.prepared.as_ref())
            .is_none_or(|prepared| prepared(storage, path, options))
    }

    /// Opens `path` with the matching format (see [`Formats::for_file`])
    /// and `options`.
    pub fn open_with(
        &self,
        storage: &dyn Storage,
        path: &str,
        options: &OpenOptions,
    ) -> Result<Box<dyn Document>, DocumentError> {
        let spec = self
            .for_file(storage, path)
            .ok_or(DocumentError::UnsupportedFormat)?;
        (spec.open)(storage, path, options)
    }

    pub fn iter(&self) -> impl Iterator<Item = &FormatSpec> {
        self.specs.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn spec(id: &'static str, ext: &'static str) -> FormatSpec {
        FormatSpec {
            id: Id::new_static(id),
            title: Cow::Borrowed(""),
            extensions: vec![Cow::Borrowed(ext)],
            open: Box::new(|_, _, _| Err(DocumentError::UnsupportedFormat)),
            prepared: None,
            sniff: None,
        }
    }

    #[test]
    fn finds_formats_by_extension_and_later_wins() {
        let mut formats = Formats::new();
        formats.add(spec("a.txt", "txt")).unwrap();
        formats.add(spec("b.txt", "txt")).unwrap();
        assert_eq!(
            formats.for_path("books/Alice.TXT").map(|s| s.id.as_str()),
            Some("b.txt")
        );
        assert!(!formats.supports("books/alice"));
        assert!(!formats.supports("books.d/alice"));
        assert!(formats.add(spec("a.txt", "md")).is_err());
    }

    #[test]
    fn files_without_a_known_extension_are_told_by_their_first_bytes() {
        let mut formats = Formats::new();
        formats.add(spec("a.txt", "txt")).unwrap();
        formats
            .add(spec("b.zip", "zip").sniff(|head| head.starts_with(b"PK")))
            .unwrap();
        let storage = crate::MemoryStorage::new()
            .with("books/download", "PK and more")
            .with("books/notes", "plain text")
            .with("books/odd.bin", "PK and more");
        let found = |path| formats.for_file(&storage, path).map(|s| s.id.as_str());
        assert_eq!(found("books/download"), Some("b.zip"));
        assert_eq!(found("books/notes"), None);
        assert_eq!(found("books/missing"), None);
        // The extension first.
        assert_eq!(found("books/x.txt"), Some("a.txt"));
        assert!(formats.supports_file(&storage, "books/download"));
        assert!(!formats.supports_file(&storage, "books/notes"));
        // Only files without an extension are read.
        assert!(formats.may_sniff("books/download"));
        assert!(!formats.may_sniff("books/photo.jpg"));
        // A file with another extension is not read, whatever it holds.
        assert_eq!(found("books/odd.bin"), None);
        // Told again from memory: the same answer, also for many files.
        for n in 0..(SNIFFED + 5) {
            let _ = formats.for_file(&storage, &alloc::format!("books/x{n}"));
        }
        assert!(formats.sniffed.borrow().len() <= SNIFFED);
        assert_eq!(found("books/download"), Some("b.zip"));
    }

    #[test]
    fn a_format_reads_its_own_settings() {
        let options = OpenOptions::default().settings(vec![(
            String::from("format-txt.encoding"),
            String::from("latin1"),
        )]);
        assert_eq!(options.setting("format-txt.encoding"), Some("latin1"));
        assert_eq!(options.setting("format-txt.other"), None);
    }
}

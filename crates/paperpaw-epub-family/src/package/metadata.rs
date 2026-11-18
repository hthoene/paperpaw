//! The first pass over the package document: metadata and the spine.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::digest::fnv1a64;
use paperpaw_sdk::document::{DocumentError, Metadata, PageProgression};
use paperpaw_sdk::xml::{Attrs, Token, decode, local_name};
use paperpaw_sdk::zip::ZipArchive;

use super::{
    MAX_AUTHORS, MAX_SPINE, MAX_SUBJECTS, Opf, capture, keep, own, plain_text, tokenize, value,
};
use crate::fallible;

/// Reads the metadata and the spine's ids of the package document at
/// `opf_path`.
pub(super) fn read_metadata(
    storage: &dyn Storage,
    archive: &ZipArchive,
    opf_path: &str,
) -> Result<Opf, DocumentError> {
    let mut reader = Reader::default();
    tokenize(storage, archive, opf_path, &mut |token| match token {
        Token::Start {
            name,
            attrs,
            self_closing,
        } => reader.start(local_name(name), &attrs, self_closing),
        Token::Text(t) if reader.capture.is_some() => {
            reader.out_of_memory |= !capture(&mut reader.text, &decode(&t));
        }
        Token::CData(t) if reader.capture.is_some() => {
            reader.out_of_memory |= !capture(&mut reader.text, t);
        }
        Token::End { name } => reader.end(local_name(name)),
        _ => {}
    })?;
    if reader.out_of_memory {
        return Err(DocumentError::OutOfMemory);
    }
    let Reader {
        metadata,
        mut spine_ids,
        ncx_id,
        cover_id,
        identifier,
        ..
    } = reader;
    paperpaw_sdk::sort::sort(&mut spine_ids);
    Ok(Opf {
        metadata,
        spine_ids,
        ncx_id,
        cover_id,
        identifier,
    })
}

/// What the first pass has found so far.
#[derive(Default)]
struct Reader {
    metadata: Metadata,
    /// The spine's ids, hashed: 8 bytes each.
    spine_ids: Vec<(u64, u32)>,
    ncx_id: Option<u64>,
    /// EPUB 2 names the cover's manifest item in `<meta name="cover">`.
    cover_id: Option<u64>,
    /// The field whose text is read now, and that text.
    capture: Option<&'static str>,
    text: String,
    /// The id of the unique identifier, and the identifier. It is the key
    /// of obfuscated fonts.
    unique_id: Option<String>,
    identifier: String,
    /// EPUB 3 series: `<meta property="belongs-to-collection" id="c">`,
    /// its number in `<meta refines="#c" property="group-position">`.
    collection_id: Option<String>,
    out_of_memory: bool,
}

impl Reader {
    fn start(&mut self, name: &str, attrs: &Attrs<'_>, self_closing: bool) {
        match name {
            "package" => {
                self.unique_id = own(&mut self.out_of_memory, attrs.get("unique-identifier"));
            }
            "identifier"
                if !self_closing
                    && self.identifier.is_empty()
                    && attrs
                        .get("id")
                        .is_some_and(|id| self.unique_id.as_deref() == Some(&*id)) =>
            {
                self.capture("identifier");
            }
            "title" => self.capture_unless("title", self_closing),
            "creator" => self.capture_unless("creator", self_closing),
            "subject" => self.capture_unless("subject", self_closing),
            "description" => self.capture_unless("description", self_closing),
            "language" => self.capture_unless("language", self_closing),
            "spine" => {
                self.ncx_id = attrs.get("toc").map(|v| fnv1a64(v.as_bytes()));
                if attrs.get("page-progression-direction").as_deref() == Some("rtl") {
                    self.metadata.direction = PageProgression::RightToLeft;
                }
            }
            "meta" => self.meta(attrs, self_closing),
            "itemref" if self.spine_ids.len() < MAX_SPINE => {
                if let Some(id) = attrs.get("idref") {
                    let position = self.spine_ids.len() as u32;
                    match self.spine_ids.try_reserve(1) {
                        Ok(()) => self.spine_ids.push((fnv1a64(id.as_bytes()), position)),
                        Err(_) => self.out_of_memory = true,
                    }
                }
            }
            _ => {}
        }
    }

    /// Reads the text of `field` from here on.
    fn capture(&mut self, field: &'static str) {
        self.capture = Some(field);
        self.text.clear();
    }

    /// Reads the text of `field`, unless its element is empty.
    fn capture_unless(&mut self, field: &'static str, self_closing: bool) {
        if !self_closing {
            self.capture(field);
        }
    }

    /// A `<meta>` element: the cover, or a series in Calibre's or in
    /// EPUB 3's way.
    fn meta(&mut self, attrs: &Attrs<'_>, self_closing: bool) {
        let name = attrs.get("name");
        let oom = &mut self.out_of_memory;
        match name.as_deref() {
            Some("cover") => {
                self.cover_id = attrs.get("content").map(|v| fnv1a64(v.as_bytes()));
            }
            // Calibre's series.
            Some("calibre:series") => {
                if let Some(series) = attrs.get("content").and_then(|v| keep(oom, value(&v))) {
                    self.metadata.series.get_or_insert(series);
                }
            }
            Some("calibre:series_index") => {
                if let Some(index) = attrs.get("content").and_then(|v| keep(oom, value(&v))) {
                    self.metadata.series_index.get_or_insert(index);
                }
            }
            _ if !self_closing => {
                let property = attrs.get("property");
                if property.as_deref() == Some("belongs-to-collection") {
                    self.collection_id = own(oom, attrs.get("id"));
                    self.capture("collection");
                } else if property.as_deref() == Some("group-position")
                    && attrs.get("refines").is_some_and(|r| {
                        self.collection_id
                            .as_deref()
                            .is_some_and(|id| r.trim_start_matches('#') == id)
                    })
                {
                    self.capture("position");
                }
            }
            _ => {}
        }
    }

    /// The end of element `name`: the end of the field read, if it is its.
    fn end(&mut self, name: &str) {
        // Series are captured from `<meta>` elements.
        let element = |field: &'static str| -> &'static str {
            match field {
                "collection" | "position" => "meta",
                field => field,
            }
        };
        let Some(field) = self.capture.filter(|&field| name == element(field)) else {
            return;
        };
        self.capture = None;
        if let Some(value) = keep(&mut self.out_of_memory, value(&self.text)) {
            self.field(field, value);
        }
    }

    /// Keeps `value`, the text of `field`, where the field takes it.
    fn field(&mut self, field: &str, value: String) {
        let (metadata, oom) = (&mut self.metadata, &mut self.out_of_memory);
        match field {
            "title" if metadata.title.is_none() && !value.is_empty() => {
                metadata.title = Some(value);
            }
            "creator" if !value.is_empty() && metadata.authors.len() < MAX_AUTHORS => {
                *oom |= fallible::push(&mut metadata.authors, value).is_err();
            }
            "subject"
                if !value.is_empty()
                    && metadata.subjects.len() < MAX_SUBJECTS
                    && !metadata.subjects.contains(&value) =>
            {
                *oom |= fallible::push(&mut metadata.subjects, value).is_err();
            }
            "description" if metadata.description.is_none() => {
                if let Some(plain) = keep(oom, plain_text(&self.text)) {
                    metadata.description = (!plain.is_empty()).then_some(plain);
                }
            }
            "language" if metadata.language.is_none() && !value.is_empty() => {
                metadata.language = Some(value);
            }
            "identifier" => self.identifier = value,
            "collection" if !value.is_empty() => {
                metadata.series.get_or_insert(value);
            }
            "position" if !value.is_empty() => {
                metadata.series_index.get_or_insert(value);
            }
            _ => {}
        }
    }
}

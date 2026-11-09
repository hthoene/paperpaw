use super::{Error, FormatCall, FormatOp, schema};
use crate::document::{Metadata, PageProgression};
use paperpaw_lang::WritingMode;

impl FormatCall<'_> {
    /// Encode metadata by borrowing the format's document model.
    /// Title and series previews use UTF-8 boundaries and advertise any cut;
    /// `reply_metadata_text` can return their full contents in parts. A language
    /// over 35 bytes or a series index over 32 bytes is left out (sent empty), not
    /// cut and not a refusal.
    ///
    /// # Errors
    /// State outside meta; Unsupported for a writing mode not carried on the wire;
    /// NoSpace for a count above its wire cap or output too short for the complete
    /// reply.
    pub fn reply_metadata(
        &self,
        metadata: &Metadata,
        sections: u32,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        self.require(&[FormatOp::Meta])?;
        let title = metadata.title.as_deref().unwrap_or("");
        let series = metadata.series.as_deref().unwrap_or("");
        let language = metadata.language.as_deref().unwrap_or("");
        let series_index = metadata.series_index.as_deref().unwrap_or("");
        // A language tag or series index over its wire cap comes from the book, and a
        // cut tag or number would be wrong, so it is left out: a book with an odd tag
        // still opens.
        let language = if language.len() > schema::FMT_META_LANGUAGE_MAX_BYTES {
            ""
        } else {
            language
        };
        let series_index = if series_index.len() > schema::FMT_META_SERIES_INDEX_MAX_BYTES {
            ""
        } else {
            series_index
        };
        let short_title = prefix(title, schema::FMT_META_TITLE_MAX_BYTES);
        let short_series = prefix(series, schema::FMT_META_SERIES_MAX_BYTES);
        let flags = u32::from(short_title.len() < title.len())
            | (u32::from(short_series.len() < series.len()) << 1);
        schema::FmtMeta::new(
            short_title,
            language,
            short_series,
            series_index,
            u32::from(metadata.direction == PageProgression::RightToLeft),
            match metadata.writing_mode {
                WritingMode::HorizontalTb => 0,
                WritingMode::VerticalRl => 1,
                WritingMode::VerticalLr => 2,
                _ => return Err(Error::Unsupported),
            },
            sections,
            count(metadata.authors.len())?,
            count(metadata.subjects.len())?,
            count(metadata.description.as_deref().unwrap_or("").len())?,
            flags,
        )
        .encode(out)
        .map_err(Error::from_status)
    }
    /// Encode the requested complete metadata value or its next UTF-8 part.
    /// Offsets and total lengths are bytes; text is never cut permanently.
    ///
    /// # Errors
    /// State outside meta_text; NotFound for an absent list index; Invalid
    /// for an offset outside the text or inside a UTF-8 character; NoSpace
    /// for an unrepresentable byte count or a short output buffer.
    pub fn reply_metadata_text(&self, metadata: &Metadata, out: &mut [u8]) -> Result<usize, Error> {
        self.require(&[FormatOp::MetaText])?;
        let index = usize::try_from(self.index()).map_err(|_| Error::NotFound)?;
        let text = match self.flags() {
            0 if index == 0 => metadata.title.as_deref().unwrap_or(""),
            1 => metadata.authors.get(index).ok_or(Error::NotFound)?.as_str(),
            2 => metadata
                .subjects
                .get(index)
                .ok_or(Error::NotFound)?
                .as_str(),
            3 if index == 0 => metadata.description.as_deref().unwrap_or(""),
            4 if index == 0 => metadata.series.as_deref().unwrap_or(""),
            _ => return Err(Error::NotFound),
        };
        let offset = usize::try_from(self.offset()).map_err(|_| Error::Invalid)?;
        let rest = text.get(offset..).ok_or(Error::Invalid)?;
        schema::FmtText::new(
            self.flags(),
            self.index(),
            count(text.len())?,
            prefix(rest, schema::FMT_TEXT_TEXT_MAX_BYTES),
        )
        .encode(out)
        .map_err(Error::from_status)
    }
}
fn count(value: usize) -> Result<u32, Error> {
    u32::try_from(value).map_err(|_| Error::NoSpace)
}
fn prefix(text: &str, cap: usize) -> &str {
    let mut end = text.len().min(cap);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

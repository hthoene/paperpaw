use super::{Error, FormatCall, FormatOp, schema};

/// Preparation outcome for an open or prepare reply.
#[derive(Clone, Copy, Debug)]
pub struct FormatState {
    state: u32,
    progress: u32,
    status: i32,
}
impl FormatState {
    /// The document can be read now.
    #[must_use]
    pub const fn ready() -> Self {
        Self {
            state: 0,
            progress: 1_000_000,
            status: 0,
        }
    }
    /// More incremental work is needed; progress is in millionths.
    ///
    /// # Errors
    /// Invalid for progress above one million.
    pub const fn preparing(progress_ppm: u32) -> Result<Self, Error> {
        if progress_ppm > 1_000_000 {
            return Err(Error::Invalid);
        }
        Ok(Self {
            state: 1,
            progress: progress_ppm,
            status: 0,
        })
    }
    /// Preparation failed with a negative native status.
    ///
    /// # Errors
    /// Invalid if an unknown error holds a nonnegative status.
    pub const fn failed(error: Error) -> Result<Self, Error> {
        if matches!(error, Error::Other(status) if status >= 0) {
            return Err(Error::Invalid);
        }
        let status = error.status();
        Ok(Self {
            state: 2,
            progress: 0,
            status,
        })
    }
}

/// Capabilities and cache state reported by a format document.
#[derive(Clone, Copy, Debug, Default)]
pub struct FormatFeatures(u32);
impl FormatFeatures {
    /// No optional capabilities and no newly made cache.
    #[must_use]
    pub const fn new() -> Self {
        Self(0)
    }
    /// The document consists of whole-page pictures.
    #[must_use]
    pub const fn whole_pages(self) -> Self {
        Self(self.0 | 1)
    }
    /// The document has a table of contents.
    #[must_use]
    pub const fn table_of_contents(self) -> Self {
        Self(self.0 | 2)
    }
    /// The format supports XPointers.
    #[must_use]
    pub const fn xpointers(self) -> Self {
        Self(self.0 | 4)
    }
    /// The document has embedded fonts.
    #[must_use]
    pub const fn embedded_fonts(self) -> Self {
        Self(self.0 | 8)
    }
    /// The format supplies printed page labels.
    #[must_use]
    pub const fn page_labels(self) -> Self {
        Self(self.0 | 16)
    }
    /// The pages progress from right to left.
    #[must_use]
    pub const fn right_to_left(self) -> Self {
        Self(self.0 | 32)
    }
    /// The cache was made by this preparation.
    #[must_use]
    pub const fn cache_made(self) -> Self {
        Self(self.0 | 64)
    }
}
impl FormatCall<'_> {
    pub(super) fn require(&self, ops: &[FormatOp]) -> Result<(), Error> {
        if ops.contains(&self.operation()) {
            Ok(())
        } else {
            Err(Error::State)
        }
    }
    /// Accept a sniffed file with the generated yes record.
    ///
    /// # Errors
    /// State outside sniff; NoSpace when output cannot hold the complete reply.
    pub fn reply_accept(&self, out: &mut [u8]) -> Result<usize, Error> {
        self.require(&[FormatOp::Sniff])?;
        schema::FmtYes::new(0)
            .encode(out)
            .map_err(Error::from_status)
    }
    /// Decline a sniffed file with the generated empty reply.
    ///
    /// # Errors
    /// State outside sniff; NoSpace for a short output buffer.
    pub fn reply_decline(&self, out: &mut [u8]) -> Result<usize, Error> {
        self.require(&[FormatOp::Sniff])?;
        schema::reply_encode(&[], out).map_err(Error::from_status)
    }
    /// Acknowledge that the document's resources have been released.
    ///
    /// # Errors
    /// State outside close; NoSpace for a short output buffer.
    pub fn reply_closed(&self, out: &mut [u8]) -> Result<usize, Error> {
        self.require(&[FormatOp::Close])?;
        schema::reply_encode(&[], out).map_err(Error::from_status)
    }
    /// Reply to open or prepare, retaining the request's document and conversion.
    ///
    /// # Errors
    /// State outside open/prepare; Invalid for a malformed format id;
    /// NoSpace for an overlong id or short output buffer.
    pub fn reply_state(
        &self,
        format: &str,
        state: FormatState,
        features: FormatFeatures,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        self.require(&[FormatOp::Open, FormatOp::Prepare])?;
        validate_id(format)?;
        schema::FmtState::new(
            self.document(),
            state.state,
            state.progress,
            state.status,
            features.0,
            self.conversion(),
            format,
        )
        .encode(out)
        .map_err(Error::from_status)
    }
    /// Reply with progress in millionths at the requested source position.
    ///
    /// # Errors
    /// State outside progress; Invalid above one million; NoSpace for short output.
    pub fn reply_progress(&self, progress_ppm: u32, out: &mut [u8]) -> Result<usize, Error> {
        self.require(&[FormatOp::Progress])?;
        if progress_ppm > 1_000_000 {
            return Err(Error::Invalid);
        }
        schema::FmtProgress::new(progress_ppm)
            .encode(out)
            .map_err(Error::from_status)
    }
    /// Reply with the exact source position of a TOC entry, XPointer or link.
    ///
    /// # Errors
    /// State outside a position query; NoSpace for short output.
    pub fn reply_position(
        &self,
        section: u32,
        offset: u32,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        self.require(&[FormatOp::TocTarget, FormatOp::Locate, FormatOp::LinkTarget])?;
        schema::FmtAt::new(self.document(), section, offset, 0, 0, 0)
            .encode(out)
            .map_err(Error::from_status)
    }
    /// Reply with a complete printed page label, never silently truncated.
    ///
    /// # Errors
    /// State outside page_label; NoSpace for overlong text or short output.
    pub fn reply_label(&self, text: &str, out: &mut [u8]) -> Result<usize, Error> {
        self.require(&[FormatOp::PageLabel])?;
        if text.len() > schema::FMT_LABEL_TEXT_MAX_BYTES {
            return Err(Error::NoSpace);
        }
        schema::FmtLabel::new(text)
            .encode(out)
            .map_err(Error::from_status)
    }
    /// Reply with the complete XPointer and the request's document identity.
    ///
    /// # Errors
    /// State outside xpointer; NoSpace for overlong text or short output.
    pub fn reply_xpointer(&self, text: &str, out: &mut [u8]) -> Result<usize, Error> {
        self.require(&[FormatOp::Xpointer])?;
        if text.len() > schema::FMT_XPOINTER_TEXT_MAX_BYTES {
            return Err(Error::NoSpace);
        }
        schema::FmtXpointer::new(self.document(), text)
            .encode(out)
            .map_err(Error::from_status)
    }
}

pub(super) fn validate_id(id: &str) -> Result<(), Error> {
    if id.len() > 128 {
        return Err(Error::NoSpace);
    }
    if !id.contains('.')
        || id.split('.').any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    {
        return Err(Error::Invalid);
    }
    Ok(())
}

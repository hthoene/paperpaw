//! The format plugin's call as the document service makes it: `fmt.call`'s
//! fields (`document.md` 3) and an open's `doc_option` rows.
use super::{FormatRef, wire};

/// The `doc_option` rows of an `open`, still in the caller's batch.
#[derive(Clone, Copy)]
pub(crate) struct Options<'a> {
    pub(super) batch: &'a [u8],
}
impl<'a> Options<'a> {
    pub(crate) const NONE: Options<'static> = Options { batch: &[0] };
    /// The options of a counted batch with `doc_option` rows (an open's kept
    /// batch, `pending`; a test's rows).
    pub(crate) const fn from_batch(batch: &'a [u8]) -> Self {
        Options { batch }
    }
    /// Each option in order, `key` a full setting id. The host forwards the
    /// ones of the format's own plugin whole; none is cut, none is dropped.
    pub(crate) fn each(&self, visit: impl FnMut(&wire::ServiceDocOption<'a>)) {
        super::args::each_option(self.batch, visit);
    }
}

/// One call of the format plugin: the typed `fmt.call` of `document.md` 3.
/// The reply is the op's records in the **service** tags, a bare stream; the
/// adapter translates the format's stage-local tags to them. For a `siblings`
/// format the adapter adds the opened file's name as `text`.
pub(crate) struct FormatCall<'a> {
    pub(crate) format: FormatRef,
    pub(crate) op: u32,
    pub(crate) document: u32,
    pub(crate) section: u32,
    pub(crate) offset: u32,
    pub(crate) index: u32,
    pub(crate) cursor: u32,
    pub(crate) generation: u32,
    pub(crate) flags: u32,
    pub(crate) image_side: u32,
    pub(crate) cover_side: u32,
    pub(crate) conversion: u64,
    /// Op 13 and 15: the XPointer or href.
    pub(crate) text: &'a str,
    /// Op 2 only.
    pub(crate) options: Options<'a>,
    /// Op 2 only: the user's CSS file (`doc_open.css_root`, `css_path`;
    /// `""` for none), which the format reads as root 4.
    pub(crate) css_root: u32,
    pub(crate) css_path: &'a str,
}
impl FormatCall<'_> {
    pub(crate) fn new(format: FormatRef, op: u32, document: u32) -> Self {
        Self {
            format,
            op,
            document,
            section: 0,
            offset: 0,
            index: 0,
            cursor: 0,
            generation: 0,
            flags: 0,
            image_side: 0,
            cover_side: 0,
            conversion: 0,
            text: "",
            options: Options::NONE,
            css_root: 0,
            css_path: "",
        }
    }
}

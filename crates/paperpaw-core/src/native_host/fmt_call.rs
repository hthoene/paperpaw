//! The stage adapter of `core.format` (`docs/abi/document.md` section 3): one
//! typed `fmt.call` for the document service's `DocHost::call_format`.
//!
//! The adapter encodes the call (`fmt.call`, and for `open` the forwarded
//! `fmt.option` rows) with the generated `core.format@1` codec, makes the one
//! directed call through [`format_call::call`] (retained instance registry,
//! lease, depth budget, same-instance `ERR_BUSY`, the 3,840-byte caps and
//! cancellation) and translates the format's stage-local reply to the
//! **service** tags the document service checks: mirrored records change only
//! their tag ([`core_format_v1::SERVICE_TAGS`]); a `fmt.image` becomes a
//! `doc_image` through [`ImageMap`] (or is left out); a tag the op does not
//! answer with is `ERR_INVALID`. The answer is rewritten inside `out`, no
//! second reply buffer, and never leaves a half stream: on an error `out` is
//! unspecified and its length is not used.
//!
//! Memory: the encoded call (at most 1,600 bytes, 3,840 for `open` with
//! options) and what [`format_call::call`] takes (the framed call and the
//! reply, 3,840 bytes), all fallible, freed before return; nothing is kept.
//! The 7-9 KiB of an ordinary call and 12 KiB of `open` are above the 6 KiB
//! X4 gate of the DOC host side and are measured at activation.
//!
//! HOOK: the generated `core.format` input is one `fmt.call` record
//! (`input_encode`/`input_decode` take exactly one); `open` carries the
//! options after it, so the call is encoded with the side-agnostic batch
//! encoder until the schema's `input_rows` lands (#385). The batch bytes do
//! not change then.
use super::{
    doc_service::{FormatCall, fmt},
    format_call::{self, PAYLOAD_MAX, Target},
    native_instances::NativeInstances,
    registrations::{self as wire, Record},
    stages_wire::core_format_v1 as local,
};
use crate::native_host::codec;
use alloc::vec::Vec;
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_UNSUPPORTED,
};
use paperpaw_sdk::Env;

/// Most `fmt.option` rows a call forwards (`document.md` 3).
const OPTIONS_MAX: usize = 12;
/// An encoded call without options: one `fmt.call` record, framed.
const CALL_MAX: usize = 1_600;
/// The framed `fmt.image` the adapter decodes on its own (path 256).
const IMAGE_FRAME_MAX: usize = 320;
/// Sniff, close: ops of the stage the document service does not carry.
const SNIFF: u32 = 1;
const PREPARE: u32 = 3;
const CLOSE: u32 = 4;

/// Turns a format's picture into the opener's `doc_image` (`document.md` 5):
/// the cache-path checks, the image id. `None`: the picture is left out.
pub(crate) trait ImageMap {
    /// `document` of `owner`; `in_block`: a block stream, where all ids being
    /// out is `image` 0 rather than `ERR_BUSY`.
    fn map(
        &self,
        owner: u64,
        document: u32,
        image: &local::FmtImage<'_>,
        in_block: bool,
    ) -> Result<Option<wire::ServiceDocImage>, i32>;
}

/// How one framed payload reaches the format at a [`Target`] and its checked
/// reply comes back (the front of `out`, its length returned).
pub(crate) trait Route {
    /// # Errors
    /// What [`format_call::call`] says.
    fn call(&self, target: Target, payload: &[u8], out: &mut [u8]) -> Result<usize, i32>;
}

/// Through the retained instance registry (the shell's own loop).
pub(crate) struct Registry<'a> {
    pub(crate) registry: &'a NativeInstances,
    pub(crate) env: &'a Env<'a>,
}
impl Route for Registry<'_> {
    fn call(&self, target: Target, payload: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        format_call::call(self.registry, target, payload, out, self.env)
    }
}

/// Through the calling plugin's frame (`Frame::call_native`): the format
/// runs in its own instance at depth 2, the reply is written where the
/// caller lent room and checked before it counts.
pub(crate) struct FrameRoute<'a>(pub(crate) &'a dyn crate::abi7::context::Frame);
impl Route for FrameRoute<'_> {
    fn call(&self, target: Target, payload: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        let framed = format_call::framed(payload)?;
        let room = out.len().min(PAYLOAD_MAX);
        self.0.call_native(
            target.plugin,
            target.token,
            &framed,
            &mut out[..room],
            &mut |reply| format_call::accept_reply(reply),
        )
    }
}

/// Who a call is for and what it needs from the host.
pub(crate) struct Stage<'a> {
    pub(crate) route: &'a dyn Route,
    pub(crate) images: &'a dyn ImageMap,
}

/// The opener of a document and what is its own for the call.
#[derive(Clone, Copy)]
pub(crate) struct Opener<'a> {
    pub(crate) owner: u64,
    /// For a `siblings` format: the opened file's name within the book's
    /// folder, carried in `text` where the op uses no other text.
    pub(crate) sibling: &'a str,
}

fn buffer(capacity: usize) -> Result<Vec<u8>, i32> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| ERR_NO_MEMORY)?;
    bytes.resize(capacity, 0);
    Ok(bytes)
}

/// The local tags an op may answer with.
fn answers(op: u32) -> &'static [u32] {
    use local::{
        FMT_AT_TAG as AT, FMT_BLOCK_TAG as BLOCK, FMT_CELL_TAG as CELL, FMT_COLUMNS_TAG as COLUMNS,
        FMT_END_TAG as END, FMT_FONT_TAG as FONT, FMT_IMAGE_TAG as IMAGE,
        FMT_INLINE_LANG_TAG as LANG, FMT_LABEL_TAG as LABEL, FMT_LINK_TAG as LINK,
        FMT_MAP_TAG as MAP, FMT_META_TAG as META, FMT_PROGRESS_TAG as PROGRESS,
        FMT_RUBY_TAG as RUBY, FMT_RUN_TAG as RUN, FMT_SECTION_TAG as SECTION,
        FMT_STATE_TAG as STATE, FMT_TEXT_PART_TAG as PART, FMT_TEXT_TAG as TEXT,
        FMT_TOC_TAG as TOC, FMT_XPOINTER_TAG as XPOINTER, FMT_YES_TAG as YES,
    };
    match op {
        SNIFF => &[YES],
        fmt::OPEN | PREPARE => &[STATE],
        fmt::META => &[META],
        fmt::META_TEXT => &[TEXT],
        fmt::SECTIONS => &[SECTION, END],
        fmt::BLOCK => &[BLOCK, PART, RUN, MAP, COLUMNS, CELL, RUBY, LANG, IMAGE, END],
        fmt::TOC => &[TOC, END],
        fmt::TOC_TARGET | fmt::LOCATE | fmt::LINK_TARGET => &[AT],
        fmt::PAGE_LABEL => &[LABEL],
        fmt::XPOINTER => &[XPOINTER],
        fmt::LINKS => &[LINK, END],
        fmt::FONTS => &[FONT, END],
        fmt::COVER | fmt::IMAGE => &[IMAGE],
        fmt::PROGRESS => &[PROGRESS],
        _ => &[],
    }
}

/// The service tag of a mirrored local one.
fn service_tag(local: u32) -> Option<u32> {
    local::SERVICE_TAGS
        .iter()
        .find(|(from, _)| *from == local)
        .map(|(_, to)| *to)
}

impl<'a> Stage<'a> {
    /// One `fmt.call` of `call` (op 2 to 19) to the format at `target`;
    /// the answer in service tags, a bare stream, at the front of `out`
    /// (4,096 bytes lent by the dispatcher); returns its length.
    ///
    /// # Errors
    /// `ERR_UNSUPPORTED` for an op the service does not carry (1, 4: see
    /// [`Stage::sniff`], [`Stage::close`]), `ERR_NO_SPACE` for a call over
    /// 3,840 bytes or an answer that does not fit `out` once translated,
    /// `ERR_NO_MEMORY` (nothing was entered), `ERR_INVALID` for an answer
    /// with a record the op does not answer with, and what
    /// [`format_call::call`] says: `ERR_BUSY` (format not started, or the
    /// caller itself), `ERR_CANCELLED`, the format's own status.
    pub(crate) fn call(
        &self,
        who: Opener<'_>,
        target: Target,
        call: &FormatCall<'_>,
        out: &mut [u8],
    ) -> Result<usize, i32> {
        if !(fmt::OPEN..=fmt::PROGRESS).contains(&call.op) || call.op == CLOSE {
            return Err(ERR_UNSUPPORTED);
        }
        let text = if call.text.is_empty() {
            who.sibling
        } else {
            call.text
        };
        let record = local::FmtCall::new(
            call.op,
            call.document,
            call.section,
            call.offset,
            call.index,
            call.cursor,
            call.generation,
            call.flags,
            call.image_side,
            call.cover_side,
            call.conversion,
            text,
            &[],
        );
        // Options ride with op 2 `open` only (document.md section 3).
        let options = if call.op == fmt::OPEN {
            &call.options
        } else {
            &super::doc_service::Options::NONE
        };
        // A call for the host's own background job (a format preparing or
        // making a cover on its own time) is told to the shell as such.
        let _job = (who.owner == crate::native_host::doc_service::HOST_JOB)
            .then(crate::native_host::job_step::enter);
        let length = self.run(target, &record, options, out)?;
        self.translate(who, call, out, length)
    }

    /// Op 1: whether the format reads a file whose first bytes are `first`
    /// (at most 512). No answer is "not mine".
    ///
    /// # Errors
    /// As [`Stage::call`].
    pub(crate) fn sniff(&self, target: Target, first: &[u8], out: &mut [u8]) -> Result<bool, i32> {
        let first = first.get(..first.len().min(local::FMT_CALL_ARG_MAX_BYTES));
        let record = local::FmtCall::new(
            SNIFF,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            "",
            first.unwrap_or(&[]),
        );
        let length = self.run(target, &record, &super::doc_service::Options::NONE, out)?;
        let reply = local::reply_decode(out.get(..length).ok_or(ERR_INVALID)?)?;
        let mut yes = false;
        for row in reply {
            match row? {
                local::Record::FmtYes(_) => yes = true,
                _ => return Err(ERR_INVALID),
            }
        }
        Ok(yes)
    }

    /// Op 4 `close`: best effort, nothing the format says can fail it.
    pub(crate) fn close(&self, target: Target, document: u32, out: &mut [u8]) {
        let record = local::FmtCall::new(CLOSE, document, 0, 0, 0, 0, 0, 0, 0, 0, 0, "", &[]);
        let _ = self.run(target, &record, &super::doc_service::Options::NONE, out);
    }

    /// Encode, call, and leave the format's counted batch at the front of
    /// `out`; returns its length.
    fn run(
        &self,
        target: Target,
        record: &local::FmtCall<'_>,
        options: &super::doc_service::Options<'_>,
        out: &mut [u8],
    ) -> Result<usize, i32> {
        let mut rows: Vec<local::Record<'_>> = Vec::new();
        rows.try_reserve_exact(1 + OPTIONS_MAX)
            .map_err(|_| ERR_NO_MEMORY)?;
        rows.push(local::Record::FmtCall(*record));
        let mut refused = false;
        options.each(|option| {
            if rows.len() > OPTIONS_MAX {
                refused = true;
            } else {
                rows.push(local::Record::FmtOption(local::FmtOption::new(
                    option.key,
                    option.value,
                )));
            }
        });
        if refused {
            return Err(ERR_NO_SPACE);
        }
        let mut payload = buffer(if rows.len() > 1 {
            PAYLOAD_MAX
        } else {
            CALL_MAX
        })?;
        // HOOK (#385 `input_rows`): the batch encoder, see the module doc.
        let used = local::records_encode(&rows, &mut payload)?;
        drop(rows);
        payload.truncate(used);
        self.route.call(target, &payload, out)
    }

    /// The format's batch in `out[..length]` as a bare stream in service tags.
    fn translate(
        &self,
        who: Opener<'_>,
        call: &FormatCall<'_>,
        out: &mut [u8],
        length: usize,
    ) -> Result<usize, i32> {
        let allowed = answers(call.op);
        // Checked whole before a byte moves: tags the op answers with.
        let batch = out.get(..length).ok_or(ERR_INVALID)?;
        let rows = local::reply_decode(batch)?.len();
        let mut at = {
            let mut probe = Reader::new(batch);
            probe.varint()?;
            probe.at
        };
        // The answer goes to the end of `out`; each record is written from the
        // front, never past the one it replaces (a tag grows by one byte).
        let start = out.len().checked_sub(length).ok_or(ERR_NO_SPACE)?;
        if start < rows {
            return Err(ERR_NO_SPACE);
        }
        out.copy_within(..length, start);
        let (end, mut put) = (out.len(), 0);
        at += start;
        for _ in 0..rows {
            let mut reader = Reader::new(&out[at..end]);
            let tag = reader.varint()?;
            let size = reader.varint()? as usize;
            let (header, body) = (reader.at, reader.at.checked_add(size).ok_or(ERR_INVALID)?);
            if body > end - at {
                return Err(ERR_INVALID);
            }
            let (record, rest) = (at, at + body);
            if !allowed.contains(&tag) {
                return Err(ERR_INVALID);
            }
            if tag == local::FMT_IMAGE_TAG {
                put = self.image(who, call, out, (record, rest), put)?;
            } else if let Some(service) = service_tag(tag) {
                put = reframe(out, (record + header, rest), service, put, record)?;
            } else {
                // `fmt.yes` outside of sniff has no service record.
                return Err(ERR_INVALID);
            }
            at = rest;
        }
        Ok(put)
    }

    /// A `fmt.image` frame `out[frame.0..frame.1]` as the opener's `doc_image`
    /// (or nothing) at `put`; returns the new `put`.
    fn image(
        &self,
        who: Opener<'_>,
        call: &FormatCall<'_>,
        out: &mut [u8],
        frame: (usize, usize),
        put: usize,
    ) -> Result<usize, i32> {
        let size = frame.1 - frame.0;
        if size > IMAGE_FRAME_MAX {
            return Err(ERR_INVALID);
        }
        // The one record decoded in a batch of its own (count 1, then it).
        let mut single = [0_u8; IMAGE_FRAME_MAX + 1];
        single[0] = 1;
        single[1..=size].copy_from_slice(&out[frame.0..frame.1]);
        let mut stream = local::reply_decode(&single[..=size])?;
        let Some(Ok(local::Record::FmtImage(image))) = stream.next() else {
            return Err(ERR_INVALID);
        };
        let mapped = self
            .images
            .map(who.owner, call.document, &image, call.op == fmt::BLOCK)?;
        let Some(doc) = mapped else {
            // A block keeps its text without the picture; the cover or a
            // picture asked for alone is none: `ERR_NOT_FOUND` (`document.md`
            // 5), the format's file is at fault, not the caller's request.
            return if call.op == fmt::BLOCK {
                Ok(put)
            } else {
                Err(ERR_NOT_FOUND)
            };
        };
        let mut scratch = [0_u8; 64];
        let written = codec::encode_one(&Record::ServiceDocImage(doc), &mut scratch)?;
        // Never over what is still to be read.
        if put + written > frame.1 {
            return Err(ERR_NO_SPACE);
        }
        out[put..put + written].copy_from_slice(&scratch[..written]);
        Ok(put + written)
    }
}

/// The record body `out[body.0..body.1]` framed under `tag` at `put`; the
/// record it replaces began at `from` (`put <= from`). Returns the new `put`.
fn reframe(
    out: &mut [u8],
    body: (usize, usize),
    tag: u32,
    put: usize,
    from: usize,
) -> Result<usize, i32> {
    let length = body.1 - body.0;
    let (tag_len, len_len) = (varint_len(tag), varint_len(length as u32));
    let end = put + tag_len + len_len + length;
    if end > body.1 || put > from {
        return Err(ERR_NO_SPACE);
    }
    out.copy_within(body.0..body.1, put + tag_len + len_len);
    let mut at = put;
    write_varint(tag, out, &mut at);
    write_varint(length as u32, out, &mut at);
    Ok(end)
}

fn varint_len(mut value: u32) -> usize {
    let mut n = 1;
    while value >= 0x80 {
        value >>= 7;
        n += 1;
    }
    n
}

fn write_varint(mut value: u32, out: &mut [u8], at: &mut usize) {
    while value >= 0x80 {
        out[*at] = (value & 0x7f) as u8 | 0x80;
        *at += 1;
        value >>= 7;
    }
    out[*at] = value as u8;
    *at += 1;
}

/// A varint reader over a framed batch.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    /// A varint of at most 5 bytes. Lenient on purpose: a non-minimal
    /// encoding is accepted and bits above `u32` in the fifth byte are
    /// dropped, as this only walks frames `local::reply_decode` has already
    /// checked whole; a sixth byte or a cut varint is `ERR_INVALID`.
    fn varint(&mut self) -> Result<u32, i32> {
        let mut value = 0_u32;
        for shift in (0..35).step_by(7) {
            let byte = *self.bytes.get(self.at).ok_or(ERR_INVALID)?;
            self.at += 1;
            value |= u32::from(byte & 0x7f)
                .checked_shl(shift)
                .ok_or(ERR_INVALID)?;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(ERR_INVALID)
    }
}

#[cfg(test)]
#[path = "fmt_call_tests.rs"]
mod tests;

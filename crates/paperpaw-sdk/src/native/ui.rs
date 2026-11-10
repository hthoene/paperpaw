//! Typed `core.ui` calls. Handles retain no callback binding; every operation
//! borrows the current host and caller-owned input/output buffers.
use super::declare::records_wire as wire;
use super::wire::services_wire::core_ui_v1 as schema;
use super::{Error, Host, services::Service};

pub(crate) mod navigation;
pub use navigation::{Notify, Open, ScreenPolicy, SpeechRequest};
mod status;
pub use status::{StatusLine, StatusValue};
mod queries;
pub use queries::{HintInput, UiHint, UiScreen};
mod handover;
pub mod stages;
mod widgets;
pub use widgets::{
    Compose, MadeWidget, StatusHandle, StatusLineHandle, WidgetHandle, WidgetInputKind, WidgetSize,
    WidgetTarget,
};
mod params;
pub use params::Param;
mod paging;
pub use paging::{UiCursor, UiPage, UiPageEnd};
mod types;
pub use types::{
    Answer, HandoverId, Instance, Question, QuestionInfo, QuestionKind, RequestId, TextId,
    TextMode, TextPart, TextWrite,
};

/// An owned `core.ui@1` handle, reusable across callbacks.
#[derive(Debug)]
#[must_use = "release the UI service when no longer needed"]
pub struct Ui {
    service: Service,
}
impl Ui {
    /// Open the UI service declared in this plugin's metadata.
    ///
    /// # Errors
    /// Returns the host's schema, admission or allocation refusal.
    pub fn open(host: &Host<'_>) -> Result<Self, Error> {
        Service::open(host, schema::KEY, schema::MAJOR, 0).map(|service| Self { service })
    }
    /// Release this service handle using the current callback's host.
    ///
    /// # Errors
    /// Returns the host's resource refusal.
    pub fn release(self, host: &Host<'_>) -> Result<(), Error> {
        self.service.release(host)
    }

    /// Record a question and return its request identifier immediately.
    /// The host opens its dialog after this callback returns.
    ///
    /// # Errors
    /// Returns local framing/limit errors or the host's refusal. An invalid or
    /// zero request identifier in a successful reply is Invalid.
    pub fn ask(
        &self,
        host: &Host<'_>,
        question: Question<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<RequestId, Error> {
        let record = question.record()?;
        let count = self.call(host, schema::OP_ASK, &[&record], input, out)?;
        let row: wire::ServiceUiRequest = expect(one(&out[..count])?)?;
        RequestId::new(row.request).ok_or(Error::Invalid)
    }

    /// Read a question from the dialog instance opened for it.
    ///
    /// # Errors
    /// Returns State for a mismatched reply, Invalid for malformed fields,
    /// or the host's ownership refusal.
    pub fn question<'a>(
        &self,
        host: &Host<'_>,
        request: RequestId,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<QuestionInfo<'a>, Error> {
        let count = self.call(
            host,
            schema::OP_QUESTION,
            &[&wire::ServiceUiRequest {
                request: request.get(),
            }],
            input,
            out,
        )?;
        let row: wire::ServiceUiQuestion<'_> = expect(one(&out[..count])?)?;
        let kind = QuestionKind::decode(row.kind)?;
        types::validate_question(kind, row.flags, row.choices)?;
        Ok(QuestionInfo {
            kind,
            flags: row.flags,
            max_bytes: row.max_bytes,
            title: row.title,
            text: row.text,
            choices: row.choices,
        })
    }

    /// Write an append-only part; offset zero starts a replacement. Commit
    /// with Last before returning from the asking callback or answering.
    ///
    /// # Errors
    /// Returns local limits or host ownership, storage and commit errors.
    #[allow(
        clippy::too_many_arguments,
        reason = "current host, caller buffers and one host-text part"
    )]
    pub fn text_write(
        &self,
        host: &Host<'_>,
        text: TextId,
        offset: u32,
        data: &str,
        finish: TextWrite,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        self.write(
            host,
            schema::OP_TEXT_WRITE,
            text.get(),
            offset,
            data,
            finish,
            input,
            out,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "shared request/clipboard text transport"
    )]
    fn write(
        &self,
        host: &Host<'_>,
        op: u32,
        text: u32,
        offset: u32,
        data: &str,
        finish: TextWrite,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        let length = self.call(
            host,
            op,
            &[&wire::ServiceUiText {
                text,
                offset,
                total: 0,
                flags: finish.flags(),
                data,
            }],
            input,
            out,
        )?;
        if length != 0 {
            return Err(Error::State);
        }
        Ok(())
    }

    /// Read a whole UTF-8 part. The result borrows only out, not this handle.
    ///
    /// # Errors
    /// Returns State for a mismatched text or offset, Invalid for an incorrect
    /// size or non-progressing reply, or a host refusal (including IO for a
    /// removed text card).
    pub fn text_read<'a>(
        &self,
        host: &Host<'_>,
        text: TextId,
        offset: u32,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<TextPart<'a>, Error> {
        self.read(host, schema::OP_TEXT_READ, text.get(), offset, input, out)
    }
    fn read<'a>(
        &self,
        host: &Host<'_>,
        op: u32,
        text: u32,
        offset: u32,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<TextPart<'a>, Error> {
        let count = self.call(
            host,
            op,
            &[&wire::ServiceUiTextAt { text, offset }],
            input,
            out,
        )?;
        let row: wire::ServiceUiText<'_> = expect(one(&out[..count])?)?;
        let end = row
            .offset
            .checked_add(u32::try_from(row.data.len()).map_err(|_| Error::Invalid)?)
            .ok_or(Error::Invalid)?;
        if row.text != text || row.offset != offset {
            return Err(Error::State);
        }
        if end > row.total || row.data.is_empty() && end != row.total {
            return Err(Error::Invalid);
        }
        Ok(TextPart {
            text,
            offset,
            total: row.total,
            data: row.data,
        })
    }

    /// Answer a request from the dialog instance that the host opened for it.
    ///
    /// # Errors
    /// Returns the host's ownership, choice or uncommitted-text refusal.
    pub fn answer(
        &self,
        host: &Host<'_>,
        request: RequestId,
        answer: Answer,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        let (outcome, choice) = answer.fields();
        let length = self.call(
            host,
            schema::OP_ANSWER,
            &[&wire::ServiceUiAnswer {
                request: request.get(),
                outcome,
                choice,
            }],
            input,
            out,
        )?;
        if length != 0 {
            return Err(Error::State);
        }
        Ok(())
    }
    fn operation(&self, op: u32) -> Result<&super::wire::services_wire::ServiceOp, Error> {
        self.service.operation(op).ok_or(Error::Unsupported)
    }
    fn call(
        &self,
        host: &Host<'_>,
        op: u32,
        records: &[&dyn wire::WireEncode],
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let operation = self.operation(op)?;
        let count = wire::records_encode_counted_of(
            records.iter().copied(),
            usize::from(operation.input_rows),
            operation.max_input,
            input,
        )
        .map_err(Error::from_status)?;
        self.service.call(host, op, &input[..count], out)
    }
}

fn one(bytes: &[u8]) -> Result<wire::RawRecord<'_>, Error> {
    let (record, length) = wire::RawRecord::prefix(bytes).map_err(Error::from_status)?;
    if length != bytes.len() {
        return Err(Error::Invalid);
    }
    Ok(record)
}

/// The reply row `raw` as `R`: another record is State (a reply this op never gives).
fn expect<'a, R: wire::WireRecord<'a>>(raw: wire::RawRecord<'a>) -> Result<R, Error> {
    if raw.tag() != R::TAG {
        return Err(Error::State);
    }
    raw.decode().map_err(Error::from_status)
}

#[cfg(test)]
mod tests;

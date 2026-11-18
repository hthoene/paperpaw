//! Allocation-free catalog formatting with caller-owned argument and output buffers.
use super::{Error, Host, declare::records_wire as wire};
use paperpaw_native::host_glue::{PpBuffer, PpBytes};

/// Maximum UTF-8 bytes in a catalog key.
pub const MAX_KEY_BYTES: usize = wire::I18N_FORMAT_MAX_KEY_BYTES;
/// Maximum encoded formatting argument batch size.
pub const MAX_ARGUMENT_BYTES: usize = wire::I18N_FORMAT_MAX_ARGUMENT_BYTES;
/// Maximum UTF-8 bytes passed to digit conversion.
pub const MAX_DIGIT_BYTES: usize = wire::I18N_DIGITS_MAX_BYTES;
/// Maximum byte size accepted by i18n.copy (slot 6) for keys and messages.
pub const MAX_MESSAGE_BYTES: usize = 4096;

/// One formatting argument. Signed integers use the wire's two's complement encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Arg<'a> {
    /// A named string substitution.
    Text { name: &'a str, value: &'a str },
    /// A named integer substitution.
    Int { name: &'a str, value: i64 },
    /// The plural selector, also bound to the name `count`.
    Count(i64),
}

/// Caller-owned storage for formatting a catalog message without allocating.
/// `scratch` contains only the encoded arguments; `out` receives the message.
#[derive(Debug)]
#[non_exhaustive]
pub struct FormatBuffers<'scratch, 'out> {
    /// Temporary argument encoding storage, bounded by [`MAX_ARGUMENT_BYTES`].
    pub scratch: &'scratch mut [u8],
    /// Destination for the translated message's byte prefix.
    pub out: &'out mut [u8],
}
impl<'scratch, 'out> FormatBuffers<'scratch, 'out> {
    /// Assign separate buffers to argument encoding and translated output.
    /// The buffers can be reused after the formatter's mutable borrow ends.
    #[must_use]
    pub fn new(scratch: &'scratch mut [u8], out: &'out mut [u8]) -> Self {
        Self { scratch, out }
    }
}

impl Arg<'_> {
    fn name(&self) -> &str {
        match self {
            Self::Text { name, .. } | Self::Int { name, .. } => name,
            Self::Count(_) => "count",
        }
    }
    fn record(&self) -> ArgRecord<'_> {
        match self {
            Self::Text { name, value } => ArgRecord::Text(wire::I18nArgStr { name, value }),
            Self::Int { name, value } => ArgRecord::Int(wire::I18nArgInt {
                name,
                value: *value as u64,
            }),
            Self::Count(value) => ArgRecord::Count(wire::I18nCount {
                value: *value as u64,
            }),
        }
    }
}

/// An argument's wire record, encoded by type: only these three codecs are linked.
#[derive(Clone, Copy)]
enum ArgRecord<'a> {
    Text(wire::I18nArgStr<'a>),
    Int(wire::I18nArgInt<'a>),
    Count(wire::I18nCount),
}

impl ArgRecord<'_> {
    fn wire(&self) -> &dyn wire::WireEncode {
        match self {
            Self::Text(record) => record,
            Self::Int(record) => record,
            Self::Count(record) => record,
        }
    }
}
fn bytes(input: &[u8]) -> PpBytes {
    PpBytes {
        ptr: input.as_ptr(),
        len: input.len(),
    }
}
fn buffer(output: &mut [u8]) -> PpBuffer {
    PpBuffer {
        ptr: output.as_mut_ptr(),
        cap: output.len(),
    }
}
fn length(result: i32) -> Result<usize, Error> {
    usize::try_from(result).map_err(|_| Error::from_status(result))
}

/// Copy a complete catalog translation into caller-owned output without allocating.
/// Returns text borrowing `out`; a missing key leaves the buffer unchanged.
/// Messages are bounded by [`MAX_MESSAGE_BYTES`]. A buffer of that size fits
/// every permitted message; shorter buffers return NoSpace when necessary.
///
/// ```compile_fail
/// use paperpaw_sdk::native::{Host, formatting};
/// fn escape(host: &Host<'_>) -> &'static str {
///     let mut out = [0; 64];
///     formatting::copy_message(host, "battery.title", &mut out).unwrap()
/// }
/// ```
///
/// # Errors
/// Returns NoSpace when the complete message does not fit, Utf8 for malformed
/// host text, Invalid for an oversized key, or the host's refusal, including
/// NotFound for a missing key.
pub fn copy_message<'a>(
    host: &Host<'_>,
    key: impl AsRef<str>,
    out: &'a mut [u8],
) -> Result<&'a str, Error> {
    let key = key.as_ref();
    if key.len() > MAX_MESSAGE_BYTES {
        return Err(Error::Invalid);
    }
    let capacity = out.len().min(MAX_MESSAGE_BYTES);
    // SAFETY: callback binding, key and exclusively borrowed output remain live.
    let count = length(unsafe {
        (host.table.i18n_copy)(
            host.ctx,
            bytes(key.as_bytes()),
            buffer(&mut out[..capacity]),
        )
    })?;
    if count > capacity {
        return Err(Error::NoSpace);
    }
    let message = out.get(..count).ok_or(Error::NoSpace)?;
    core::str::from_utf8(message).map_err(|_| Error::Utf8)
}

/// Format a catalog key, optionally selecting its plural form with [`Arg::Count`].
/// [`FormatBuffers::scratch`] holds only encoded arguments;
/// [`FormatBuffers::out`] receives the translated message. Returns the full
/// UTF-8 byte length, even when only the output buffer's byte prefix fits.
/// Retry with a larger output buffer when the returned length exceeds its capacity.
/// A partial output can end inside a UTF-8 character.
///
/// # Errors
/// Returns NoSpace for field, argument or scratch limits; Invalid for duplicate
/// names or plural selectors; or the host refusal. A missing key leaves `out` unchanged.
pub fn format_message(
    host: &Host<'_>,
    key: impl AsRef<str>,
    args: &[Arg<'_>],
    buffers: &mut FormatBuffers<'_, '_>,
) -> Result<usize, Error> {
    let scratch = &mut *buffers.scratch;
    let out = &mut *buffers.out;
    let key = key.as_ref();
    if key.len() > MAX_KEY_BYTES {
        return Err(Error::NoSpace);
    }
    let count = encode_args(args, scratch)?;
    // SAFETY: all inputs and the exclusively borrowed output live through this call.
    length(unsafe {
        (host.table.i18n_format)(
            host.ctx,
            bytes(key.as_bytes()),
            bytes(&scratch[..count]),
            buffer(out),
        )
    })
}

/// Encodes `args` as the argument batch of a catalog message (slot 27's `args`,
/// the `label_args` of `command.set_item_state`) into the front of `scratch`;
/// returns the bytes written.
///
/// # Errors
/// NoSpace for more than 16 arguments or a batch over [`MAX_ARGUMENT_BYTES`]
/// or `scratch`, Invalid for duplicate names or a second plural selector.
pub(crate) fn encode_args(args: &[Arg<'_>], scratch: &mut [u8]) -> Result<usize, Error> {
    if args.len() > wire::I18N_FORMAT_MAX_RECORDS {
        return Err(Error::NoSpace);
    }
    for (index, arg) in args.iter().enumerate() {
        if args[..index]
            .iter()
            .any(|previous| previous.name() == arg.name())
        {
            return Err(Error::Invalid);
        }
    }
    let mut records =
        [ArgRecord::Count(wire::I18nCount { value: 0 }); wire::I18N_FORMAT_MAX_RECORDS];
    for (record, arg) in records.iter_mut().zip(args) {
        *record = arg.record();
    }
    let capacity = scratch.len().min(MAX_ARGUMENT_BYTES);
    wire::records_encode_counted_of(
        records[..args.len()].iter().map(ArgRecord::wire),
        wire::I18N_FORMAT_MAX_RECORDS,
        MAX_ARGUMENT_BYTES,
        &mut scratch[..capacity],
    )
    .map_err(Error::from_status)
}

/// Convert ASCII digits to the UI language's digits without allocating.
/// Returns the full byte length; an output shorter than that holds a byte prefix,
/// which can end inside a UTF-8 character. Retry with a larger buffer if needed.
///
/// # Errors
/// Returns NoSpace for overlong input or the host's refusal.
pub fn digits(host: &Host<'_>, text: impl AsRef<str>, out: &mut [u8]) -> Result<usize, Error> {
    let text = text.as_ref();
    if text.len() > MAX_DIGIT_BYTES {
        return Err(Error::NoSpace);
    }
    // SAFETY: callback binding, input and exclusively borrowed output remain live.
    length(unsafe { (host.table.i18n_digits)(host.ctx, bytes(text.as_bytes()), buffer(out)) })
}

#[cfg(test)]
#[path = "formatting_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "formatting_limits_tests.rs"]
mod limits_tests;

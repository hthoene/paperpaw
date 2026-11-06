//! Bounded, allocation-free catalog copy, interpolation and digit conversion.

use super::{environment::EnvironmentScope, registrations as wire};
use core::mem::MaybeUninit;
use paperpaw_i18n::{Arg, Translator};
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_UNSUPPORTED, PpBuffer, PpBytes, buffers,
};

/// Copy one translated message, preserving the output if the key is absent.
///
/// # Safety
/// Inputs must be initialized and readable for the call. Output must be
/// writable for its capacity, exclusively borrowed and disjoint from inputs
/// and host state. It may be uninitialized. Null with zero length is accepted.
/// No pointer is retained.
#[must_use]
pub unsafe fn copy(scope: &EnvironmentScope, key: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: the caller provides valid, disjoint input and output regions.
    let key = match unsafe { buffers::text_in(key, buffers::TEXT_MAX) } {
        Ok(key) => key,
        Err(error) => return error,
    };
    // SAFETY: output is exclusively writable for the duration of this call.
    let out = match unsafe { buffers::buffer_out(out, buffers::TEXT_MAX) } {
        Ok(out) => out,
        Err(error) => return error,
    };
    scope
        .with_frame(|frame| {
            frame
                .translator()
                .get(key)
                .map_or(ERR_NOT_FOUND, |message| {
                    buffers::copy_required(message.as_bytes(), out, buffers::TEXT_MAX)
                })
        })
        .unwrap_or_else(|error| error)
}

/// Format a translated message using the generated argument record codec.
/// All argument validation finishes before the first catalog lookup.
/// The complete result must fit the 4 KiB scratch; only its fitting byte prefix
/// is copied to output, and the full required length is returned.
///
/// # Safety
/// Same pointer validity and exclusion requirements as [`copy`], including
/// that output is disjoint from both key and argument input regions.
#[must_use]
pub unsafe fn format(scope: &EnvironmentScope, key: PpBytes, args: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: all pairs satisfy the documented lifetime and exclusion contract.
    let key = match unsafe { buffers::text_in(key, wire::I18N_FORMAT_MAX_KEY_BYTES) } {
        Ok(key) => key,
        Err(error) => return error,
    };
    // SAFETY: arguments are initialized and readable through the call.
    let args = match unsafe { buffers::bytes_in(args, wire::I18N_FORMAT_MAX_ARGUMENT_BYTES) } {
        Ok(args) => args,
        Err(error) => return error,
    };
    // SAFETY: output is exclusively writable and disjoint from all inputs.
    let out = match unsafe { buffers::buffer_out(out, buffers::TEXT_MAX) } {
        Ok(out) => out,
        Err(error) => return error,
    };
    let args = match Arguments::decode(args) {
        Ok(args) => args,
        Err(error) => return error,
    };
    scope
        .with_frame(|frame| format_message(frame.translator(), key, &args, out))
        .unwrap_or_else(|error| error)
}

/// Convert ASCII digits using the current UI language's digit characters.
/// The full result must fit 4 KiB; the return value is its complete byte length.
///
/// # Safety
/// Same pointer validity and exclusion requirements as [`copy`].
#[must_use]
pub unsafe fn digits(scope: &EnvironmentScope, text: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: the input is readable and stable for the call.
    let text = match unsafe { buffers::text_in(text, wire::I18N_DIGITS_MAX_BYTES) } {
        Ok(text) => text,
        Err(error) => return error,
    };
    // SAFETY: output is exclusively writable and disjoint from input and host state.
    let out = match unsafe { buffers::buffer_out(out, buffers::TEXT_MAX) } {
        Ok(out) => out,
        Err(error) => return error,
    };
    scope
        .with_frame(|frame| {
            let digits = frame.translator().digits();
            let mut scratch = Scratch::new();
            for character in text.chars() {
                let character = if character.is_ascii_digit() {
                    digits[character as usize - '0' as usize]
                } else {
                    character
                };
                if let Err(error) = scratch.character(character) {
                    return error;
                }
            }
            scratch.copy(out)
        })
        .unwrap_or_else(|error| error)
}

struct Arguments<'a> {
    named: [(&'a str, Arg<'a>); wire::I18N_FORMAT_MAX_RECORDS],
    len: usize,
    count: Option<i64>,
}

impl<'a> Arguments<'a> {
    fn decode(bytes: &'a [u8]) -> Result<Self, i32> {
        let batch = wire::records_decode(bytes)?;
        if batch.len() > wire::I18N_FORMAT_MAX_RECORDS {
            return Err(ERR_NO_SPACE);
        }
        let mut args = Self {
            named: [("", Arg::Str("")); wire::I18N_FORMAT_MAX_RECORDS],
            len: 0,
            count: None,
        };
        for record in batch.iter() {
            let (name, value) = match record {
                wire::Record::I18nArgStr(arg) => (arg.name, Arg::Str(arg.value)),
                wire::Record::I18nArgInt(arg) => (arg.name, Arg::Int(arg.value as i64)),
                wire::Record::I18nCount(arg) => {
                    if args.count.is_some()
                        || args.named[..args.len]
                            .iter()
                            .any(|(name, _)| *name == "count")
                    {
                        return Err(ERR_INVALID);
                    }
                    args.count = Some(arg.value as i64);
                    continue;
                }
                _ => return Err(ERR_UNSUPPORTED),
            };
            if (name == "count" && args.count.is_some())
                || args.named[..args.len].iter().any(|(used, _)| *used == name)
            {
                return Err(ERR_INVALID);
            }
            args.named[args.len] = (name, value);
            args.len += 1;
        }
        Ok(args)
    }
}

fn format_message(
    translator: &Translator,
    key: &str,
    args: &Arguments<'_>,
    out: &mut [MaybeUninit<u8>],
) -> i32 {
    let mut candidate = [0; wire::I18N_FORMAT_MAX_KEY_BYTES + 6];
    let message = match args.count {
        Some(count) => translator.get_plural(key, count, &mut candidate),
        None => translator.get(key),
    };
    let Some(message) = message else {
        return ERR_NOT_FOUND;
    };
    let mut scratch = Scratch::new();
    match interpolate(message, args, &translator.digits(), &mut scratch) {
        Ok(()) => scratch.copy(out),
        Err(error) => error,
    }
}

// Match Translator's interpolation rules without its allocating String result.
fn interpolate(
    mut rest: &str,
    args: &Arguments<'_>,
    digits: &[char; 10],
    out: &mut Scratch,
) -> Result<(), i32> {
    while let Some(open) = rest.find(['{', '}']) {
        out.text(&rest[..open])?;
        let tail = &rest[open..];
        if let Some(after) = tail.strip_prefix("{{") {
            out.character('{')?;
            rest = after;
            continue;
        }
        if let Some(after) = tail.strip_prefix("}}") {
            out.character('}')?;
            rest = after;
            continue;
        }
        if tail.starts_with('{')
            && let Some(close) = tail.find('}')
        {
            let name = &tail[1..close];
            let value = args.named[..args.len]
                .iter()
                .find(|(used, _)| *used == name)
                .map(|(_, value)| *value)
                .or_else(|| args.count.filter(|_| name == "count").map(Arg::Int));
            match value {
                Some(Arg::Str(text)) => out.text(text)?,
                Some(Arg::Int(number)) => out.integer(number, digits)?,
                None => out.text(&tail[..=close])?,
            }
            rest = &tail[close + 1..];
            continue;
        }
        out.text(&tail[..1])?;
        rest = &tail[1..];
    }
    out.text(rest)
}

struct Scratch {
    bytes: [u8; buffers::TEXT_MAX],
    len: usize,
}

impl Scratch {
    fn new() -> Self {
        Self {
            bytes: [0; buffers::TEXT_MAX],
            len: 0,
        }
    }

    fn text(&mut self, text: &str) -> Result<(), i32> {
        let end = self.len.checked_add(text.len()).ok_or(ERR_NO_SPACE)?;
        let destination = self.bytes.get_mut(self.len..end).ok_or(ERR_NO_SPACE)?;
        destination.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }

    fn character(&mut self, character: char) -> Result<(), i32> {
        self.text(character.encode_utf8(&mut [0; 4]))
    }

    fn integer(&mut self, number: i64, digits: &[char; 10]) -> Result<(), i32> {
        if number < 0 {
            self.character('-')?;
        }
        let mut magnitude = number.unsigned_abs();
        let mut places = [0u8; 20];
        let mut start = places.len();
        loop {
            start -= 1;
            places[start] = (magnitude % 10) as u8;
            magnitude /= 10;
            if magnitude == 0 {
                break;
            }
        }
        for &place in &places[start..] {
            self.character(digits[usize::from(place)])?;
        }
        Ok(())
    }

    fn copy(&self, out: &mut [MaybeUninit<u8>]) -> i32 {
        buffers::copy_required(&self.bytes[..self.len], out, buffers::TEXT_MAX)
    }
}

#[cfg(test)]
#[path = "formatting_tests.rs"]
mod tests;

//! The sample of a font before it is downloaded: a font pack of one size with
//! only the characters of the font's name and of a sentence in the reader's
//! language, made by the font library on request (a few kilobytes), kept in the
//! data folder and drawn in its own face (`Font::acquire_data`). The interface-6
//! `samples.rs` is the reference; one font's sample is held, for the font's page.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        callbacks::DrawFrame,
        fonts::{Font, FontRole},
        snapshot,
    },
};
use paperpaw_ui_kit::{
    Style,
    paint::{self, Paint},
    text,
};

/// The size the sample is made and drawn in, in pixels: a little larger than
/// menu text, for the letters to show.
pub const SIZE: u32 = 26;
/// The largest sample taken from a library, in bytes.
pub const MAX_FILE: u64 = 64 * 1024;
/// The folder of samples in the data folder.
pub const DIR: &str = "samples";
/// Longest path the host takes for a font of the data folder, in bytes.
pub const MAX_PATH: usize = 123;
/// Most characters of a sample (name and sentence together).
const MAX_CHARS: usize = 512;

/// Where the sample of font `id` at `version` is kept, one for each version,
/// size and language; letters, digits and `.` of the version only.
///
/// # Errors
/// `NoMemory`, and `Invalid` where the path would be too long.
pub fn path(id: &str, version: &str, language: &str) -> Result<String, Error> {
    let version = version
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.');
    let language = language.chars().filter(char::is_ascii_alphanumeric);
    // The length first: catalog strings may be of any size, and nothing is
    // pushed before they are known to fit (digits of the size: at most 10).
    let total = DIR.len()
        + 1
        + id.len()
        + 1
        + version.clone().count()
        + 1
        + 10
        + 1
        + language.clone().count()
        + ".ppfp".len();
    if total > MAX_PATH || id.contains(['/', '\\']) || id.contains("..") {
        return Err(Error::Invalid);
    }
    let mut out = String::new();
    out.try_reserve_exact(total).map_err(|_| Error::NoMemory)?;
    out.push_str(DIR);
    out.push('/');
    out.push_str(id);
    out.push('-');
    out.extend(version);
    out.push('-');
    push_number(&mut out, SIZE);
    out.push('-');
    out.extend(language);
    out.push_str(".ppfp");
    // Two dots in a row would look like a step up.
    if out.contains("..") {
        return Err(Error::Invalid);
    }
    Ok(out)
}

/// The characters of `name` and `sentence`, each once and in order, and `…` for
/// a name cut short; no control characters.
///
/// # Errors
/// `NoMemory`.
pub fn characters(name: &str, sentence: &str) -> Result<String, Error> {
    let mut chars: Vec<char> = Vec::new();
    for c in name.chars().chain(sentence.chars()).chain(['…']) {
        if c.is_control() || chars.contains(&c) {
            continue;
        }
        if chars.len() >= MAX_CHARS {
            break;
        }
        chars.try_reserve(1).map_err(|_| Error::NoMemory)?;
        chars.push(c);
    }
    chars.sort_unstable();
    let mut out = String::new();
    out.try_reserve(chars.len() * 4)
        .map_err(|_| Error::NoMemory)?;
    out.extend(chars);
    Ok(out)
}

/// The address of a font's sample: `sample` with the size and the characters
/// asked for (`?` or `&` as the address has a query already).
///
/// # Errors
/// `NoMemory`.
pub fn url(sample: &str, text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve(sample.len() + 24 + text.len() * 9)
        .map_err(|_| Error::NoMemory)?;
    out.push_str(sample);
    out.push(if sample.contains('?') { '&' } else { '?' });
    out.push_str("size=");
    push_number(&mut out, SIZE);
    out.push_str("&text=");
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            b' ' => out.push('+'),
            _ => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                out.push('%');
                out.push(char::from(HEX[usize::from(byte >> 4)]));
                out.push(char::from(HEX[usize::from(byte & 15)]));
            }
        }
    }
    Ok(out)
}

/// Draws `sentence` in the face of the sample kept at `path` over `area` (the
/// line of the font's page that holds the sentence in the menus' face): the
/// area is cleared and the sentence set in it. `false` where the file is no
/// font any more, or changed (nothing is drawn: the page keeps the plain line).
///
/// # Errors
/// `NoMemory` and the host's refusals other than of the sample's font.
pub fn draw(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    area: Rect,
    (sentence, path): (&str, &str),
) -> Result<bool, Error> {
    let sample = match Font::acquire_data(host, path) {
        Ok(font) => font,
        Err(Error::NotFound | Error::Invalid | Error::State) => return Ok(false),
        Err(error) => return Err(error),
    };
    let drawn = draw_in(host, frame, area, sentence, &sample);
    let released = sample.release();
    drawn.and(released).map(|()| true)
}

fn draw_in(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    area: Rect,
    sentence: &str,
    sample: &Font<'_>,
) -> Result<(), Error> {
    let ui = Font::acquire_role(host, FontRole::Ui)?;
    let style = snapshot::capture_snapshot(host).and_then(|d| Style::read(host, &d, &ui));
    let released = ui.release();
    let style = style?;
    released?;
    let size = SIZE as f32;
    let room = f32::from(area.width) - 2.0 * f32::from(style.margin);
    let fitted = text::fit(sample, size, sentence, room)?;
    let mut ops: Vec<Paint> = Vec::new();
    paint::push(&mut ops, Paint::Fill(area, style.paper))?;
    text::label(
        &mut ops,
        sample,
        &fitted,
        size,
        f32::from(area.x) + f32::from(style.margin),
        area,
        style.ink,
        Some(area),
    )?;
    paint::replay(frame, sample, ops.iter())
}

fn push_number(to: &mut String, mut n: u32) {
    let mut digits = [0u8; 10];
    let mut at = digits.len();
    loop {
        at -= 1;
        digits[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for d in &digits[at..] {
        to.push(char::from(*d));
    }
}

#[cfg(test)]
mod tests;

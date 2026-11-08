//! The device's own network on interface 7 (slice 3): the name it opens
//! for a phone to join, the password drawn for it, the token that guards
//! its page and the text of the QR code that joins it. The rules of
//! `phone.rs` and of the SDK's `Random`, `device_name::ssid` and
//! `wifi::join_code`, host-free: bytes come from a `draw` the screen backs
//! with `core.secrets` `random` (up to 256 bytes a call, a flag saying
//! whether the device source was fully random), allocation is fallible, and
//! the bytes and the password are wiped when done.
use alloc::string::String;

use paperpaw_sdk::device_name::MAX_SSID;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{mem, text};

use crate::native_policy::Password;

/// The letters of [`readable`]: lower case letters and digits a person reads
/// off a screen and types without taking one for another.
pub const READABLE: &str = "acdefhkmnprtwxy347";
/// The letters of [`token`] (Crockford's base 32).
const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
/// The name of a network when the device's has none.
const FALLBACK: &str = "PaperPaw";
/// The most one draw gives (`core.secrets` `random`).
pub const DRAW_MAX: usize = 256;

/// Fills a buffer (1 to [`DRAW_MAX`] bytes) with random bytes and says
/// whether the source was fully random.
pub type Draw<'a> = dyn FnMut(&mut [u8]) -> Result<bool, Error> + 'a;

/// A drawn text, with whether the source was fully random for all of it.
#[derive(PartialEq, Eq)]
pub struct Drawn {
    pub text: Password,
    pub strong: bool,
}

impl Drawn {
    /// The text, only where the whole draw was fully random: a weak draw is
    /// dropped (wiped) and gives `None`. The own network's password and the
    /// page's token are taken only through this.
    #[must_use]
    pub fn into_strong(self) -> Option<Password> {
        self.strong.then_some(self.text)
    }
}

impl core::fmt::Debug for Drawn {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Drawn")
            .field("strong", &self.strong)
            .finish_non_exhaustive()
    }
}

/// A random text of `chars` digits and lower case letters, 5 bits each (so
/// 13 for at least 64 bits).
///
/// # Errors
/// [`Error::Invalid`] for more than 409 characters (one draw of 256 bytes);
/// [`Error::NoMemory`]; what `draw` says.
pub fn token(chars: usize, draw: &mut Draw<'_>) -> Result<Drawn, Error> {
    let wanted = (chars * 5).div_ceil(8);
    if wanted > DRAW_MAX {
        return Err(Error::Invalid);
    }
    let mut out = String::new();
    out.try_reserve_exact(chars).map_err(|_| Error::NoMemory)?;
    // At least one byte, as the pair read below may reach one past.
    let mut bytes = mem::buffer(wanted.max(1))?;
    let strong = if wanted == 0 {
        Ok(true)
    } else {
        draw(&mut bytes[..wanted])
    };
    let strong = match strong {
        Ok(strong) => strong,
        Err(error) => {
            mem::wipe(&mut bytes);
            return Err(error);
        }
    };
    for i in 0..chars {
        // Bits 5i to 5i + 4, the first byte's highest first.
        let at = i * 5;
        let pair =
            u16::from(bytes[at / 8]) << 8 | u16::from(bytes.get(at / 8 + 1).copied().unwrap_or(0));
        let index = (pair >> (11 - at % 8)) & 0x1f;
        out.push(char::from(ALPHABET[usize::from(index)]));
    }
    mem::wipe(&mut bytes);
    let text = Password::new(&out);
    mem::wipe_text(out);
    Ok(Drawn {
        text: text?,
        strong,
    })
}

/// A random text of `chars` letters of [`READABLE`], each as likely as any
/// other (bytes that would favour a letter are drawn again); `None` where
/// sixteen draws gave too few usable bytes.
///
/// # Errors
/// [`Error::Invalid`] for more than 128 characters (one draw is twice as many
/// bytes); [`Error::NoMemory`]; what `draw` says.
pub fn readable(chars: usize, draw: &mut Draw<'_>) -> Result<Option<Drawn>, Error> {
    let letters = READABLE.as_bytes();
    let count = letters.len();
    let below = 256 - 256 % count;
    let size = chars.max(1) * 2;
    if size > DRAW_MAX {
        return Err(Error::Invalid);
    }
    let mut out = String::new();
    out.try_reserve_exact(chars).map_err(|_| Error::NoMemory)?;
    let mut bytes = mem::buffer(size)?;
    let mut strong = true;
    for _ in 0..16 {
        if out.len() == chars {
            break;
        }
        match draw(&mut bytes) {
            Ok(this) => strong &= this,
            Err(error) => {
                mem::wipe(&mut bytes);
                mem::wipe_text(out);
                return Err(error);
            }
        }
        for &byte in &bytes {
            if out.len() == chars {
                break;
            }
            if usize::from(byte) < below {
                out.push(char::from(letters[usize::from(byte) % count]));
            }
        }
    }
    mem::wipe(&mut bytes);
    if out.len() != chars {
        mem::wipe_text(out);
        return Ok(None);
    }
    let text = Password::new(&out);
    mem::wipe_text(out);
    Ok(Some(Drawn {
        text: text?,
        strong,
    }))
}

/// `code` as it is shown to be read and typed: in groups of five letters, a
/// space between them ("acdef hkmnp rt").
///
/// # Errors
/// [`Error::NoMemory`].
pub fn grouped(code: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(code.len() + code.len() / 5)
        .map_err(|_| Error::NoMemory)?;
    for (i, c) in code.chars().enumerate() {
        if i > 0 && i % 5 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    Ok(out)
}

/// The text of a QR code that joins network `ssid` with `password` (WPA), as
/// phones' cameras read it: `\`, `;`, `,`, `:` and `"` in them escaped. The
/// text holds the password: the screen wipes it with the code's modules.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn join_code(ssid: &str, password: &str) -> Result<String, Error> {
    let marks = |text: &str| {
        text.chars()
            .filter(|c| matches!(c, '\\' | ';' | ',' | ':' | '"'))
            .count()
    };
    const OPEN: &str = "WIFI:T:WPA;S:";
    let len = OPEN.len() + ssid.len() + marks(ssid) + 3 + password.len() + marks(password) + 2;
    let mut out = String::new();
    out.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    out.push_str(OPEN);
    push_escaped(&mut out, ssid);
    out.push_str(";P:");
    push_escaped(&mut out, password);
    out.push_str(";;");
    Ok(out)
}

fn push_escaped(out: &mut String, text: &str) {
    for c in text.chars() {
        if matches!(c, '\\' | ';' | ',' | ':' | '"') {
            out.push('\\');
        }
        out.push(c);
    }
}

/// `name` as the name of a network the device opens (for a phone to join):
/// as it is, cut at 32 bytes on a character's edge, `PaperPaw` where nothing
/// is left.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn ssid(name: &str) -> Result<String, Error> {
    let mut end = name.len().min(MAX_SSID);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    match name[..end].trim() {
        "" => text::copy(FALLBACK),
        ssid => text::copy(ssid),
    }
}

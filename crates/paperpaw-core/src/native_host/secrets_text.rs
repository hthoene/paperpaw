//! Interface 6's view of the secrets file on a device whose `core.secrets`
//! seals (decision 138; `docs/abi/secrets.md` section 3).
//!
//! The file holds sealed (`s1:`) and veiled lines. The kernel's interface-6
//! code (`Command::SetSecret`, `Env::secrets`, Wi-Fi and mail passwords) keeps
//! the passwords in memory and reads and writes the veiled hex form. These
//! two functions stand between that code and the file: [`open_text`] turns
//! each sealed line the sealer opens into a veiled one, to be decoded as ever;
//! [`seal_text`] turns each veiled line of the text to be written into a
//! sealed one. A sealed line that does not open here (another device, a lost
//! key, a damaged line) is left as it is, and so is every line these
//! functions do not understand. Nothing is written veiled by a device that
//! seals (decision 140), so a sealer that cannot seal fails the write.
use super::secret_bytes::wipe;
use super::secrets_file::{
    NAME_MAX, SEALED_MARK, SEALED_MAX, Sealer, VALUE_MAX, aad_of, is_hex, nibble, veil,
};
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_IO, ERR_NO_MEMORY};

/// One line split into its fields (`id`, `label`, flags, value), as the
/// interface-6 text writes them.
fn fields(line: &str) -> Option<[&str; 4]> {
    let mut parts = line.splitn(4, '\t');
    Some([parts.next()?, parts.next()?, parts.next()?, parts.next()?])
}

/// The owner and the name of an escaped line id (`owner.name`), unescaped.
fn owner_name(id: &str) -> Option<(String, String)> {
    let mut plain = String::new();
    let mut chars = id.chars();
    while let Some(c) = chars.next() {
        plain.push(if c == '\\' {
            match chars.next()? {
                't' => '\t',
                'n' => '\n',
                'r' => '\r',
                '\\' => '\\',
                _ => return None,
            }
        } else {
            c
        });
    }
    let (owner, name) = plain.split_once('.')?;
    (!owner.is_empty() && !name.is_empty()).then(|| (owner.into(), name.into()))
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !is_hex(bytes) {
        return None;
    }
    Some(
        bytes
            .chunks(2)
            .filter_map(|pair| Some((nibble(pair[0])? << 4) | nibble(*pair.get(1)?)?))
            .collect(),
    )
}

fn push_hex(out: &mut String, bytes: &[u8]) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
}

/// The two working buffers of a pass over the text, one allocation each (a
/// value, a sealed value), wiped when dropped.
struct Scratch {
    plain: Vec<u8>,
    blob: Vec<u8>,
}

impl Scratch {
    fn new() -> Result<Self, i32> {
        let mut buffer = |size: usize| -> Result<Vec<u8>, i32> {
            let mut bytes = Vec::new();
            bytes.try_reserve_exact(size).map_err(|_| ERR_NO_MEMORY)?;
            bytes.resize(size, 0);
            Ok(bytes)
        };
        Ok(Self {
            plain: buffer(SEALED_MAX)?,
            blob: buffer(SEALED_MAX)?,
        })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        wipe(&mut self.plain);
        wipe(&mut self.blob);
    }
}

/// `text` with every sealed line `sealer` opens turned into a veiled one.
pub(crate) fn open_text(text: &str, sealer: &dyn Sealer) -> String {
    let mut out = String::new();
    // Without room for the buffers nothing opens: the lines stay as they are.
    let mut scratch = Scratch::new().ok();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        match scratch.as_mut().and_then(|s| opened(line, sealer, s)) {
            Some(veiled) => out.push_str(&veiled),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// The veiled form of the sealed `line`, if it is one and opens here.
fn opened(line: &str, sealer: &dyn Sealer, scratch: &mut Scratch) -> Option<String> {
    let [id, label, flags, value] = fields(line)?;
    let sealed = unhex(value.strip_prefix("s1:")?)?;
    if sealed.len() > SEALED_MAX {
        return None;
    }
    let (owner, name) = owner_name(id)?;
    let mut aad = [0_u8; 2 * NAME_MAX + 1];
    let aad = aad_of(&owner, &name, &mut aad).ok()?;
    let length = sealer.open(aad, &sealed, &mut scratch.plain).ok();
    let veiled = length.filter(|&n| n <= VALUE_MAX).map(|n| {
        let value = &mut scratch.plain[..n];
        veil(value);
        let mut text = String::new();
        for part in [id, "\t", label, "\t", flags, "\t"] {
            text.push_str(part);
        }
        push_hex(&mut text, value);
        text
    });
    wipe(&mut scratch.plain);
    veiled
}

/// `text` (interface 6's, values veiled) with every veiled line sealed. A line
/// whose value is the one a sealed line of `previous` (the file as it was read)
/// holds is kept as that line: what did not change is not sealed again, so
/// removing a secret needs no random numbers.
///
/// # Errors
/// What the sealer answers when it cannot seal (`ERR_STATE`: no key, or no
/// random source now); `ERR_INVALID` for a secret whose id cannot be sealed
/// (no `.`): no value is ever written veiled. Nothing is returned then.
pub(crate) fn seal_text(text: &str, previous: &str, sealer: &dyn Sealer) -> Result<String, i32> {
    let mut out = String::new();
    let mut scratch = Scratch::new()?;
    for line in text.lines() {
        match kept(line, previous, sealer, &mut scratch) {
            Some(old) => out.push_str(old),
            None => match sealed(line, sealer, &mut scratch) {
                Some(Ok(line)) => out.push_str(&line),
                Some(Err(status)) => return Err(status),
                None => out.push_str(line),
            },
        }
        out.push('\n');
    }
    Ok(out)
}

/// The sealed line of `previous` that holds what the veiled `line` holds.
fn kept<'a>(
    line: &str,
    previous: &'a str,
    sealer: &dyn Sealer,
    scratch: &mut Scratch,
) -> Option<&'a str> {
    let [id, label, flags, _] = fields(line)?;
    previous
        .lines()
        .map(|old| old.trim_end_matches('\r'))
        .filter(|old| {
            fields(old).is_some_and(|[i, l, f, v]| {
                (i, l, f) == (id, label, flags) && v.as_bytes().starts_with(SEALED_MARK)
            })
        })
        .find(|old| opened(old, sealer, scratch).as_deref() == Some(line))
}

/// The sealed form of the veiled `line`; `None` for a line that is not one.
fn sealed(line: &str, sealer: &dyn Sealer, scratch: &mut Scratch) -> Option<Result<String, i32>> {
    let [id, label, flags, value] = fields(line)?;
    if value.as_bytes().starts_with(SEALED_MARK) || !matches!(flags, "" | "c") {
        return None;
    }
    let mut plain = unhex(value)?;
    veil(&mut plain);
    // A secret that cannot be sealed is never written veiled instead.
    let Some((owner, name)) = owner_name(id) else {
        wipe(&mut plain);
        return Some(Err(ERR_INVALID));
    };
    let mut aad = [0_u8; 2 * NAME_MAX + 1];
    let Ok(aad) = aad_of(&owner, &name, &mut aad) else {
        wipe(&mut plain);
        return Some(Err(ERR_IO));
    };
    let made = sealer.seal(aad, &plain, &mut scratch.blob);
    wipe(&mut plain);
    let result = match made {
        Ok(length) if length <= SEALED_MAX => {
            let mut text = String::new();
            for part in [id, "\t", label, "\t", flags, "\t", "s1:"] {
                text.push_str(part);
            }
            push_hex(&mut text, &scratch.blob[..length]);
            Ok(text)
        }
        Ok(_) => Err(ERR_IO),
        Err(status) => Err(status),
    };
    wipe(&mut scratch.blob);
    Some(result)
}

#[cfg(test)]
#[path = "secrets_text_tests.rs"]
pub(crate) mod tests;

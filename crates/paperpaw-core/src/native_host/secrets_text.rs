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
use paperpaw_native::host_glue::ERR_IO;

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

/// `text` with every sealed line `sealer` opens turned into a veiled one.
pub(crate) fn open_text(text: &str, sealer: &dyn Sealer) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        match opened(line, sealer) {
            Some(veiled) => out.push_str(&veiled),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// The veiled form of the sealed `line`, if it is one and opens here.
fn opened(line: &str, sealer: &dyn Sealer) -> Option<String> {
    let [id, label, flags, value] = fields(line)?;
    let sealed = unhex(value.strip_prefix("s1:")?)?;
    if sealed.len() > SEALED_MAX {
        return None;
    }
    let (owner, name) = owner_name(id)?;
    let mut aad = [0_u8; 2 * NAME_MAX + 1];
    let aad = aad_of(&owner, &name, &mut aad).ok()?;
    let mut plain = alloc::vec![0_u8; SEALED_MAX];
    let length = sealer.open(aad, &sealed, &mut plain).ok();
    let veiled = length.filter(|&n| n <= VALUE_MAX).map(|n| {
        let mut value = plain[..n].to_vec();
        veil(&mut value);
        let mut text = String::new();
        for part in [id, "\t", label, "\t", flags, "\t"] {
            text.push_str(part);
        }
        push_hex(&mut text, &value);
        wipe(&mut value);
        text
    });
    wipe(&mut plain);
    veiled
}

/// `text` (interface 6's, values veiled) with every veiled line sealed.
///
/// # Errors
/// What the sealer answers when it cannot seal (`ERR_STATE`: no key, or no
/// random source now); nothing is returned then.
pub(crate) fn seal_text(text: &str, sealer: &dyn Sealer) -> Result<String, i32> {
    let mut out = String::new();
    for line in text.lines() {
        match sealed(line, sealer) {
            Some(Ok(line)) => out.push_str(&line),
            Some(Err(status)) => return Err(status),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    Ok(out)
}

/// The sealed form of the veiled `line`; `None` for a line that is not one.
fn sealed(line: &str, sealer: &dyn Sealer) -> Option<Result<String, i32>> {
    let [id, label, flags, value] = fields(line)?;
    if value.as_bytes().starts_with(SEALED_MARK) || !matches!(flags, "" | "c") {
        return None;
    }
    let mut plain = unhex(value)?;
    veil(&mut plain);
    let (owner, name) = owner_name(id)?;
    let mut aad = [0_u8; 2 * NAME_MAX + 1];
    let Ok(aad) = aad_of(&owner, &name, &mut aad) else {
        wipe(&mut plain);
        return Some(Err(ERR_IO));
    };
    let mut blob = alloc::vec![0_u8; SEALED_MAX];
    let made = sealer.seal(aad, &plain, &mut blob);
    wipe(&mut plain);
    let result = match made {
        Ok(length) if length <= SEALED_MAX => {
            let mut text = String::new();
            for part in [id, "\t", label, "\t", flags, "\t", "s1:"] {
                text.push_str(part);
            }
            push_hex(&mut text, &blob[..length]);
            Ok(text)
        }
        Ok(_) => Err(ERR_IO),
        Err(status) => Err(status),
    };
    wipe(&mut blob);
    Some(result)
}

#[cfg(test)]
#[path = "secrets_text_tests.rs"]
pub(crate) mod tests;

//! What the screen lock reads and keeps through the host: the code (among the
//! secrets, as `kind:salt:hash`), the wrong tries (a number in the data folder),
//! and the two settings. The rules are in `native_model`; here are only the
//! calls. Every buffer that held a code or the kept hash is wiped before it is
//! freed.
use alloc::string::String;

use paperpaw_sdk::native::{
    Error, Host,
    secrets::{SecretName, SecretWrite, Secrets},
    settings::{self, SettingValue},
    storage::{self, Root},
};
use paperpaw_ui_kit::{
    mem::{buffer, wipe},
    text::copy,
};

use crate::native_model::{self as model, Code};

/// The secret's name among the lock's own (interface 6 called it `lock.code`).
pub const CODE: &str = "code";
/// What the settings editor shows for it.
const LABEL: &str = "lock.secret";
/// The file of the wrong tries in the plugin's data folder.
pub const TRIES_FILE: &str = "tries.txt";
/// The settings.
pub const KIND: &str = "lock.kind";
pub const ON_WAKE: &str = "lock.on-wake";
/// What a secret call needs: the encoded input, and a reply with the value
/// (at most 1,024 bytes) and its names.
const SECRET_IO: usize = 2048;

/// Runs `call` with the secrets service open and releases it afterwards,
/// whatever came of the call.
fn with_secrets<T>(
    host: &Host<'_>,
    call: impl FnOnce(&Secrets, &mut [u8], &mut [u8]) -> Result<T, Error>,
) -> Result<T, Error> {
    let secrets = Secrets::open(host)?;
    let result = (|| {
        let mut input = buffer(SECRET_IO)?;
        let mut out = buffer(SECRET_IO)?;
        let result = call(&secrets, &mut input, &mut out);
        wipe(&mut input);
        wipe(&mut out);
        result
    })();
    let released = secrets.release(host);
    let value = result?;
    released?;
    Ok(value)
}

/// The code kept (`kind:salt:hash`), if there is one.
///
/// # Errors
/// The host's refusal; `NoMemory` when it cannot be held.
pub fn stored(host: &Host<'_>) -> Result<Option<Code>, Error> {
    with_secrets(host, |secrets, input, out| {
        match secrets.get(host, CODE, input, out) {
            Ok(value) => Ok(Some(Code::new(copy(value.value())?))),
            Err(Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    })
}

/// Keeps `code` as the code of a lock of `kind`, with a salt of its own.
///
/// # Errors
/// `NoMemory`, or the host's refusal (the old code stays then).
pub fn keep(host: &Host<'_>, kind: &str, code: &Code) -> Result<(), Error> {
    let salt = salt(host)?;
    let value = model::kept_value(kind, &salt, code)?;
    let value = Code::new(value);
    with_secrets(host, |secrets, input, out| {
        // A hash: nothing to type anew in the list.
        let write = SecretWrite::new(SecretName::new(CODE)?, &value)?.label(LABEL)?;
        secrets.keep(host, write, input, out)
    })
}

/// Forgets the code.
///
/// # Errors
/// The host's refusal.
pub fn forget(host: &Host<'_>) -> Result<(), Error> {
    with_secrets(host, |secrets, input, out| {
        secrets.remove(host, SecretName::new(CODE)?, input, out)
    })
}

/// The letters a salt is made of: random, 16 of them (about 82 bits), if the
/// device has a source of random numbers, else the device's own (the time,
/// never the code).
fn salt(host: &Host<'_>) -> Result<String, Error> {
    const LETTERS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let drawn = with_secrets(host, |secrets, input, out| {
        match secrets.random(host, model::SALT_LETTERS, input, out) {
            Ok(random) => {
                let mut letters = String::new();
                letters
                    .try_reserve_exact(model::SALT_LETTERS)
                    .map_err(|_| Error::NoMemory)?;
                for byte in random.bytes() {
                    letters.push(char::from(LETTERS[usize::from(*byte) % LETTERS.len()]));
                }
                Ok(Some(letters))
            }
            Err(Error::Unsupported) => Ok(None),
            Err(error) => Err(error),
        }
    })?;
    let now = paperpaw_sdk::native::snapshot::capture_snapshot(host)?;
    model::salt(drawn.as_deref(), "", now.instant().millis())
}

/// The wrong tries in a row as the card keeps them; 0 where there are none.
///
/// # Errors
/// The host's refusal other than a missing file.
pub fn tries(host: &Host<'_>) -> Result<u8, Error> {
    let mut bytes = [0; model::MAX_TRIES_FILE + 1];
    match storage::read_at(host, Root::Data, TRIES_FILE, 0, &mut bytes) {
        Ok(n) => Ok(model::parse_tries(&bytes[..n])),
        Err(Error::NotFound) => Ok(0),
        Err(error) => Err(error),
    }
}

/// Keeps `tries` on the card (none: the file goes). Not kept, the count still
/// holds until PaperPaw starts anew; the caller has nothing to tell.
///
/// # Errors
/// `NoMemory`, or the host's refusal.
pub fn keep_tries(host: &Host<'_>, tries: u8) -> Result<(), Error> {
    if tries == 0 {
        return storage::remove(host, Root::Data, TRIES_FILE);
    }
    let text = model::format_tries(tries)?;
    storage::write(host, Root::Data, TRIES_FILE, text.as_bytes())
}

/// The kind setting, as it is stored (`lock.kind.pin`); empty where unset.
///
/// # Errors
/// `NoMemory`, or the host's refusal.
pub fn kind_setting(host: &Host<'_>) -> Result<String, Error> {
    let mut scratch = [0; 64];
    match settings::read_setting(host, KIND, &mut scratch)? {
        SettingValue::Text(text) => copy(text),
        _ => Ok(String::new()),
    }
}

/// Whether waking up asks for the lock (on unless set otherwise).
///
/// # Errors
/// The host's refusal.
pub fn on_wake(host: &Host<'_>) -> Result<bool, Error> {
    let mut scratch = [0; 16];
    Ok(match settings::read_setting(host, ON_WAKE, &mut scratch)? {
        SettingValue::Bool(on) => on,
        _ => true,
    })
}

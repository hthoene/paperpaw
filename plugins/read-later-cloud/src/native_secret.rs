//! The access token as the plugin holds it: a text that wipes itself when it
//! is dropped and shows nothing of itself.
use alloc::string::String;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{mem, text};

/// The longest token that is kept and sent; Readwise's are far shorter.
pub const MAX_TOKEN: usize = 512;

/// A text that wipes itself when it is dropped: the token.
pub struct Secret(String);

impl Secret {
    /// A copy of `text` that wipes itself when it is dropped.
    ///
    /// # Errors
    /// `NoMemory`; `NoSpace` over [`MAX_TOKEN`] bytes.
    pub fn new(text: &str) -> Result<Self, Error> {
        if text.len() > MAX_TOKEN {
            return Err(Error::NoSpace);
        }
        text::copy(text).map(Self)
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl core::fmt::Debug for Secret {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Secret(..)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        mem::wipe_text(core::mem::take(&mut self.0));
    }
}

/// The `Authorization` header's value for `token`, wiped when dropped.
///
/// # Errors
/// `NoMemory`.
pub fn authorization(token: &Secret) -> Result<Secret, Error> {
    let line = text::format(format_args!("Token {}", token.as_str()))?;
    let secret = Secret::new(&line);
    mem::wipe_text(line);
    secret
}

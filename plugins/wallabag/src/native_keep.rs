//! The password and the API client's secret among the plugin's secrets
//! (`core.secrets`, the names `password` and `client-secret`, `secrets.md`
//! section 7): read, kept and wiped by the kit's `dav::secret`, as every
//! plugin with a server's password does; the token's `Authorization` header
//! is Wallabag's own.
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{mem::wipe_text, text::concat};

pub use paperpaw_ui_kit::dav::secret::{Secret, keep, present, read};

/// The account's password.
pub const PASSWORD: &str = "password";
/// The API client's secret.
pub const CLIENT_SECRET: &str = "client-secret";

/// The value of an `Authorization` header that carries `token`:
/// `Bearer <token>`; the text it is made in is wiped.
///
/// # Errors
/// `NoMemory`.
pub fn bearer(token: &Secret) -> Result<Secret, Error> {
    let header = concat(&["Bearer ", token.as_str()])?;
    let secret = Secret::copy(&header);
    wipe_text(header);
    secret
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;

    #[test]
    fn a_secret_is_never_shown() {
        let secret = Secret::copy("hunter2").unwrap();
        assert_eq!(secret.as_str(), "hunter2");
        assert_eq!(alloc::format!("{secret:?}"), "Secret(..)");
        assert!(!alloc::format!("{:?}", Some(secret)).contains("hunter"));
    }

    #[test]
    fn a_token_is_sent_as_a_bearer() {
        let token = Secret::copy("abc").unwrap();
        assert_eq!(bearer(&token).unwrap().as_str(), "Bearer abc");
    }
}

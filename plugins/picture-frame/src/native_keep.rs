//! The server's password among the plugin's secrets (the name `password`) and
//! the `Authorization` header made of the user and the password: the shared
//! WebDAV account of the UI kit.
pub use paperpaw_ui_kit::dav::secret::{Secret, keep, present, read};

/// The server's password.
pub const PASSWORD: &str = "password";

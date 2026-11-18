//! A WebDAV server as plugins use it (Picture frame, File sync): addresses
//! ([`url`]), the account's `Authorization` header and its password among the
//! plugin's secrets ([`secret`]) and the listing a `PROPFIND` answers, read as
//! it streams from the file it was saved to ([`listing`]).
pub mod listing;
pub mod secret;
pub mod url;

//! Installing a plugin that comes from this side of the device: from a
//! computer over the Wi-Fi transfer or USB, e.g. one's own build. The
//! same steps whichever way it comes, and the same rules as a store's
//! install where data is at stake:
//!
//! 1. Each file goes into a folder PaperPaw does not load
//!    (`Staging::folder`), checked by its SHA-256 before it is taken
//!    (`Staging::put`): a file cut short or changed on the way is
//!    refused, and nothing installed changes.
//! 2. Once all are there, `Staging::commit` checks the folder holds the
//!    plugin it says and, for a plugin with code, that its code is whole
//!    and built for this PaperPaw. Code runs only when the reader agreed
//!    on the device to that very code (`Commit::approved`, by its
//!    SHA-256: code put in its place after the question is refused); the
//!    code is then named as installed, as a store names what it installs,
//!    once the plugin is in place ([`marks`](super::marks)).
//! 3. The kernel installs it in one step (`Ask::InstallPlugin`):
//!    the version before stays until the new one is all there, the
//!    plugin's own data (in `.paperpaw/plugins/<id>`) is not touched, and
//!    it is loaded at once (a plugin with code: after a restart, as an
//!    update from a store).
//!
//! It is the reader's own from then on: no store updates it
//! ([`forget_store_install`](super::forget_store_install)).

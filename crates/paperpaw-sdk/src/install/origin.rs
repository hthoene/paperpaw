//! Where an installed plugin came from: a store, or any other way (the
//! Wi-Fi transfer, a memory card, USB, one's own build). A store updates
//! only what it installed itself, so a plugin of one's own by the same id
//! is never replaced by surprise.
//!
//! The versions a store installed are kept beside the code's names
//! ([`native::installed_code`](crate::native::installed_code)), where
//! neither the Wi-Fi transfer nor plugins' folders reach: a plugin counts
//! as the store's only while its version is one of those. One put in
//! place otherwise counts as local even before its installer said so
//! ([`forget_store_install`]), as long as its version differs. On a
//! device that kept none yet, the plugins installed are taken for the
//! store's once ([`adopt`]): those it installed before keep its updates.
















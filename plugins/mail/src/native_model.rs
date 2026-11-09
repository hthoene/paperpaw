//! The host-free parts of Mail that read what servers and mails send, for the
//! native plugin: the interface-6 `codec` and `header` (with the secrets that
//! wipe themselves) made fallible (a refused allocation is [`Error::NoMemory`](mem::Error), never
//! an abort), bounded (a mail from anyone must not make anything grow without
//! end) and free of indexing that could panic. Each is tested to give the same
//! answers as the old one on what both take (`tests/native_model.rs`),
//! to survive any bytes (the same file) and every refused allocation
//! (`tests/native_model_alloc_refusal.rs`). `html`, `imap` and `smtp` follow the same rules;
//! the connection code that uses them follows (`PORTING-7.md`).
//!
//! The interface-6 modules stay until the port is complete (`PORTING-7.md`).

pub mod account;
pub mod browser;
pub mod codec;
pub mod fail;
pub mod header;
pub mod html;
pub mod imap;
pub mod job;
pub mod link;
pub mod mem;
pub mod render;
pub mod session;
pub mod smtp;
pub mod store;
pub mod texts;

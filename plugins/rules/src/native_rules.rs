//! The rules of interface 7, without the host: the file (`rule`) and what the
//! rules do on an event (`engine`). Fallible allocation throughout; the
//! interface-6 plugin in `lib.rs` is removed with the activation (see
//! `PORTING-7.md`).
pub mod engine;
pub mod rule;

#[cfg(test)]
mod tests;

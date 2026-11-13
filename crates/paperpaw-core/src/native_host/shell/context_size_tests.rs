//! What the context of one hosted plugin costs (#248): every hosted plugin
//! keeps one in its pinned owner, so a word here is a word per plugin.
use super::super::Context;
use super::Heap;
use core::mem::size_of;

/// The bytes a context may take besides its services, on a 64-bit host
/// (the emulator); the X4 takes about half.
#[cfg(target_pointer_width = "64")]
const CONTEXT_BYTES: usize = 256;

#[cfg(target_pointer_width = "64")]
#[test]
fn a_plugins_context_stays_within_its_bytes() {
    let bytes = size_of::<Context<Heap>>() - size_of::<Heap>();
    assert!(bytes <= CONTEXT_BYTES, "{bytes} bytes");
}

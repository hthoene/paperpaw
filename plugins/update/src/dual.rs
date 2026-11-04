//! Dual boot (#95): where the firmware found that PaperPaw shares the flash
//! with another firmware, started by a tool (`dual-boot = on` in
//! `paperpaw.ini`, or a bootloader that says so), it installs nothing into
//! the other partition, and leaves a note on the card that says so. The page
//! says why there is nothing to install, and the plugin asks nobody for
//! updates it could not install.

use paperpaw_sdk::Storage;

/// The file the firmware leaves (`paperpaw_x4::dual_boot::NOTE`; a test of
/// the firmware's crate keeps the two together).
pub const NOTE: &str = ".paperpaw/boot.txt";

/// Whether the note says a dual-boot setup is declared: its first word is
/// `dual-boot`. Anything else, a missing or cut or odd file, says it is not.
#[inline(never)]
pub fn declared(storage: &dyn Storage) -> bool {
    let mut head = [0u8; 64];
    let Ok(n) = storage.read_at(NOTE, 0, &mut head) else {
        return false;
    };
    core::str::from_utf8(&head[..n])
        .ok()
        .and_then(|text| text.lines().next())
        .is_some_and(|line| line.split_whitespace().next() == Some("dual-boot"))
}

#[cfg(test)]
mod tests {
    use paperpaw_sdk::MemoryStorage;

    use super::*;

    #[test]
    fn only_a_note_that_says_it_declares_a_dual_boot_setup() {
        let note = |text: &[u8]| MemoryStorage::new().with(NOTE, text.to_vec());
        assert!(declared(&note(b"dual-boot source=marker slot=1\n")));
        assert!(declared(&note(b"dual-boot")));
        assert!(!declared(&MemoryStorage::new()));
        assert!(!declared(&note(b"")));
        assert!(!declared(&note(b"\n dual-boot")));
        assert!(!declared(&note(b"dualboot source=ini")));
        assert!(!declared(&note(&[0xff, 0xfe, 0x00])));
        // A long file is read as far as the first line needs.
        let mut long = b"dual-boot source=ini slot=0\n".to_vec();
        long.extend(core::iter::repeat_n(b'x', 10_000));
        assert!(declared(&note(&long)));
    }
}

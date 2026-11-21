//! `wifi.ini` at the top of the card (`docs/data-files.md`): networks to join,
//! written there by the firmware from what the web flasher gave
//! (`shells/xteink/src/provision.rs`) or by the reader by hand. Beta 3's Wi-Fi
//! plugin read it there; the interface-7 one reads `wifi.ini` in its own data
//! folder and has no access to the card's files. So at a start, before any
//! plugin hears of it, the host adds the file to the plugin's and removes it
//! from the top of the card: the plugin keeps the networks with the passwords
//! and removes its file, and no plaintext password stays on the card (#454).
use crate::Kernel;
use paperpaw_platform::StorageError;
use paperpaw_sdk::data_path;

/// The file at the top of the card.
const CARD_FILE: &str = "wifi.ini";
/// The Wi-Fi plugin's file (`plugins/wifi/src/native_ini.rs`), and its folders.
const PLUGIN_FILE: &str = data_path!("plugins/wifi/wifi.ini");
const PLUGIN_FOLDERS: [&str; 3] = [
    data_path!(""),
    data_path!("plugins"),
    data_path!("plugins/wifi"),
];
/// The largest file the plugin reads (`native_ini::MOST_BYTES`); a larger one
/// is removed unread, as the plugin does, so that no password stays.
const MOST_BYTES: usize = 16 * 1024;

impl Kernel {
    /// Hands `wifi.ini` at the top of the card to the Wi-Fi plugin. A file that
    /// cannot be read, or not handed over, stays for the next start.
    pub(crate) fn hand_card_wifi_over(&mut self) {
        let storage = self.shared.storage.as_ref();
        let mut text = match paperpaw_sdk::read_file(storage, CARD_FILE, MOST_BYTES) {
            Ok(text) => text,
            Err(StorageError::NotFound) => return,
            Err(StorageError::TooLarge) => {
                let removed = storage.remove(CARD_FILE).and_then(|()| storage.sync());
                self.warn_runtime(alloc::format!(
                    "{CARD_FILE} is larger than {MOST_BYTES} bytes: removed unread ({removed:?})"
                ));
                return;
            }
            Err(error) => {
                self.warn_runtime(alloc::format!(
                    "{CARD_FILE} cannot be read ({error}): kept for the next start"
                ));
                return;
            }
        };
        for folder in PLUGIN_FOLDERS {
            // One there already is fine; a folder that cannot be made fails
            // the write below.
            storage.create_dir(folder.trim_end_matches('/')).ok();
        }
        // On a line of its own after what the plugin's file holds.
        let handed = storage
            .append(PLUGIN_FILE, b"\n")
            .and_then(|()| storage.append(PLUGIN_FILE, &text))
            .and_then(|()| storage.sync())
            .and_then(|()| storage.remove(CARD_FILE))
            .and_then(|()| storage.sync());
        // The passwords are not left on the heap.
        text.fill(0);
        if let Err(error) = handed {
            self.warn_runtime(alloc::format!(
                "{CARD_FILE} not handed to the Wi-Fi plugin ({error}): kept for the next start"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
    use super::*;
    use alloc::string::String;
    use paperpaw_platform::Instant;
    use paperpaw_sdk::MemoryStorage;

    fn started(storage: MemoryStorage) -> Kernel {
        let mut kernel = Kernel::builder(crate::tests::DEVICE)
            .storage(storage)
            .build()
            .unwrap();
        let _ = kernel.start(Instant(0));
        kernel
    }

    fn text(kernel: &Kernel, path: &str) -> Option<String> {
        paperpaw_sdk::read_file(kernel.storage(), path, 64 * 1024)
            .ok()
            .map(|bytes| String::from_utf8(bytes).unwrap())
    }

    #[test]
    fn the_card_file_goes_to_the_wifi_plugin_before_it_starts_and_is_removed() {
        let card = "ssid = Home\npassword = secret\n";
        let kernel = started(
            MemoryStorage::new()
                .with(CARD_FILE, card)
                .with(PLUGIN_FILE, "ssid = Office\npassword = other"),
        );
        assert_eq!(
            kernel.storage().size(CARD_FILE),
            Err(StorageError::NotFound)
        );
        assert_eq!(
            text(&kernel, PLUGIN_FILE).unwrap(),
            "ssid = Office\npassword = other\nssid = Home\npassword = secret\n"
        );
    }

    #[test]
    fn without_a_card_file_nothing_is_made_and_a_large_one_is_removed_unread() {
        let kernel = started(MemoryStorage::new());
        assert_eq!(text(&kernel, PLUGIN_FILE), None);
        let large = "x".repeat(MOST_BYTES + 1);
        let kernel = started(MemoryStorage::new().with(CARD_FILE, large.as_str()));
        assert_eq!(
            kernel.storage().size(CARD_FILE),
            Err(StorageError::NotFound)
        );
        assert_eq!(text(&kernel, PLUGIN_FILE), None);
    }
}

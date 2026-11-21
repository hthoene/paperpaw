//! Wi-Fi given with the firmware by the web flasher (see
//! `paperpaw_x4::provision`): read once at a start from the `nvs`
//! partition, given to the card as `wifi.ini` (added to one already
//! there), which the kernel hands to the `wifi` plugin as it starts
//! (`paperpaw_core` `card_wifi.rs`), and the plugin keeps the networks with
//! the passwords and removes it;
//! the record is erased once the card has it.

use alloc::boxed::Box;
use alloc::vec::Vec;

use esp_bootloader_esp_idf::partitions::{
    self, DataPartitionSubType, PARTITION_TABLE_MAX_LEN, PartitionType,
};
use log::{info, warn};
use paperpaw_platform::Storage;
use paperpaw_x4::provision::{MAX, record};

use crate::ota::Flash;

/// The file the `wifi` plugin reads at a start.
const WIFI_FILE: &str = "wifi.ini";

/// Gives the networks the flasher wrote to the card, and erases them
/// from the flash.
// A frame of its own, off the stack before the kernel is built.
#[inline(never)]
pub fn take(flash: &Flash, storage: &dyn Storage) {
    let mut flash = flash.borrow_mut();
    let mut table = Box::new([0u8; PARTITION_TABLE_MAX_LEN]);
    let Some(offset) = partitions::read_partition_table(&mut flash, &mut table[..])
        .ok()
        .and_then(|pt| {
            pt.find_partition(PartitionType::Data(DataPartitionSubType::Nvs))
                .ok()
                .flatten()
        })
        .map(|p| p.offset())
    else {
        return;
    };
    let mut sector = alloc::vec![0u8; MAX];
    if flash.read(offset, &mut sector).is_err() {
        return;
    }
    let Some(text) = record(&sector) else {
        return;
    };
    // A file there that cannot be read is not written over: the record
    // waits for the next start.
    let mut file: Vec<u8> = match paperpaw_sdk::read_file(storage, WIFI_FILE, 16 * 1024) {
        Ok(file) => file,
        Err(paperpaw_platform::StorageError::NotFound) => Vec::new(),
        Err(e) => {
            warn!(
                "Wi-Fi from the flasher: {WIFI_FILE} cannot be read ({e}); kept for the next start"
            );
            return;
        }
    };
    if !file.is_empty() && !file.ends_with(b"\n") {
        file.push(b'\n');
    }
    file.extend_from_slice(text.as_bytes());
    if storage.write(WIFI_FILE, &file).is_err() || storage.sync().is_err() {
        warn!("Wi-Fi from the flasher: not written to the card; kept for the next start");
        return;
    }
    match flash.erase(offset, offset + MAX as u32) {
        Ok(()) => info!("Wi-Fi from the flasher: given to the card"),
        Err(e) => warn!("Wi-Fi from the flasher: not erased: {e:?}"),
    }
}

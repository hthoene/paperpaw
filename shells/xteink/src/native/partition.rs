//! What lies behind the running firmware's image in the app partition it
//! runs from: the room for the X4's plugins (`flash`), and where the web
//! flasher left its bundle of plugins (`bundle`) before it could put it
//! into the other app partition ([`other_app`]), on both chips.

use esp_bootloader_esp_idf::partitions::{self, FlashStorage, PARTITION_TABLE_MAX_LEN};
use esp_hal::peripherals::MMU_TABLE;
use paperpaw_native::index::SECTOR;
use paperpaw_native::{CODE_OFFSET, ImageExtent, LINK_BASE};

/// An MMU page: 64 KiB of flash per entry.
pub const PAGE: u32 = 0x1_0000;

/// The flash behind the running firmware's image.
#[derive(Debug, Clone, Copy)]
pub struct Behind {
    /// The first sector after the image.
    pub start: u32,
    /// The end of the partition.
    pub end: u32,
    /// The firmware: the SHA-256 its image ends with.
    pub firmware: [u8; 32],
}

/// The flash behind the running firmware's image, or why it is not known.
pub fn behind_image(flash: &mut FlashStorage<'static>) -> Result<Behind, &'static str> {
    let (offset, size) = running_partition(flash)?;
    let extent = ImageExtent::read(&mut |at, buf| flash.read(offset + at, buf).is_ok())
        .filter(|e| e.sha256 && e.len < size)
        .ok_or("the running image cannot be measured")?;
    let mut firmware = [0u8; 32];
    flash
        .read(offset + extent.len - 32, &mut firmware)
        .map_err(|_| "the flash cannot be read")?;
    Ok(Behind {
        start: offset + extent.len.next_multiple_of(SECTOR),
        end: offset + size,
        firmware,
    })
}

/// The other app partition of the two the OTA data chooses between (its
/// offset and size): where the web flasher puts its bundle of plugins on a
/// first flash, when nothing is in it yet (`bundle`).
pub fn other_app(flash: &mut FlashStorage<'static>) -> Result<(u32, u32), &'static str> {
    let (running, _) = running_partition(flash)?;
    let mut table = alloc::vec![0u8; PARTITION_TABLE_MAX_LEN];
    let table = partitions::read_partition_table(flash, &mut table)
        .map_err(|_| "the partition table cannot be read")?;
    table
        .iter()
        .filter(|p| {
            matches!(
                p.partition_type(),
                partitions::PartitionType::App(
                    partitions::AppPartitionSubType::Ota0 | partitions::AppPartitionSubType::Ota1
                )
            )
        })
        .map(|p| (p.offset(), p.len()))
        .find(|&(offset, _)| offset != running)
        .ok_or("no other app partition")
}

/// Where in the flash the code running is: the address of the page the MMU
/// maps this function's page to, with the offset in it. Read from the
/// MMU's entry for this code, not from its first (which is the bootloader's
/// to map as it likes, as a dualbooter's does, #167).
pub fn code_address() -> Result<u32, &'static str> {
    let here = code_address as *const () as u32;
    let entry = here
        .checked_sub(LINK_BASE + CODE_OFFSET)
        .ok_or("the firmware does not run from flash")?
        / PAGE;
    let page = MMU_TABLE::regs().entry(entry as usize).read();
    if page.invalid().bit() {
        return Err("the firmware's code is not mapped");
    }
    Ok(u32::from(page.paddr().bits()) * PAGE + (here % PAGE))
}

/// The app partition this code runs from (its offset and size): the one
/// holding the flash page the MMU maps this function's address to (the
/// bootloader's choice; the OTA data may say another after an update).
pub fn running_partition(flash: &mut FlashStorage<'static>) -> Result<(u32, u32), &'static str> {
    let paddr = code_address()?;
    let mut table = alloc::vec![0u8; PARTITION_TABLE_MAX_LEN];
    let table = partitions::read_partition_table(flash, &mut table)
        .map_err(|_| "the partition table cannot be read")?;
    table
        .iter()
        .filter(|p| matches!(p.partition_type(), partitions::PartitionType::App(_)))
        .map(|p| (p.offset(), p.len()))
        .find(|&(offset, len)| (offset..offset + len).contains(&paddr))
        .ok_or("no app partition holds the firmware's code")
}

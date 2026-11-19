//! Formats an exFAT volume, for tests and host tools (the firmware never
//! formats): the layout `mkfs.exfat` and the SD Association's formatter
//! write, with the recommended up-case table.

use super::layout::{ENTRY, ENTRY_BITMAP, ENTRY_UPCASE, FAT_END, UPCASE, UPCASE_CHECKSUM};
use crate::error::Error;
use crate::fs::ReadWriteSeek;
use crate::io::SeekFrom;

/// Formats `disk` as an exFAT volume of `bytes` bytes in clusters of
/// `cluster` bytes (a power of two, 512 at least). Only what is not zero is
/// written, and the root directory's cluster: the disk is taken to be
/// zeroed (a new image file), so an image of many gigabytes stays sparse.
///
/// # Errors
///
/// `Error::InvalidInput` for a size no volume can have, `Error::Io` where
/// the disk fails.
pub fn format_exfat<IO: ReadWriteSeek>(disk: &mut IO, bytes: u64, cluster: u32) -> Result<(), Error<IO::Error>> {
    const SECTOR: u64 = 512;
    if !cluster.is_power_of_two() || u64::from(cluster) < SECTOR || bytes < (1 << 20) {
        return Err(Error::InvalidInput);
    }
    let per_cluster = u64::from(cluster) / SECTOR;
    let sectors = bytes / SECTOR;
    let fat_offset = 128u64.max(per_cluster);
    let guess = (sectors - fat_offset) / per_cluster;
    let fat_length = ((guess + 2) * 4).div_ceil(SECTOR);
    let heap_offset = (fat_offset + fat_length).next_multiple_of(per_cluster);
    let count = (sectors.saturating_sub(heap_offset)) / per_cluster;
    let bitmap_len = count.div_ceil(8);
    let bitmap_clusters = bitmap_len.div_ceil(u64::from(cluster));
    let upcase_clusters = (UPCASE.len() as u64).div_ceil(u64::from(cluster));
    let used = bitmap_clusters + upcase_clusters + 1;
    if count < used + 1 || count > u64::from(super::layout::MAX_CLUSTER_COUNT) {
        return Err(Error::InvalidInput);
    }
    let bitmap_first = 2u64;
    let upcase_first = bitmap_first + bitmap_clusters;
    let root = upcase_first + upcase_clusters;
    let heap = heap_offset * SECTOR;
    let at = |n: u64| heap + (n - 2) * u64::from(cluster);

    let mut boot = [0u8; 512];
    boot[..3].copy_from_slice(&[0xEB, 0x76, 0x90]);
    boot[3..11].copy_from_slice(b"EXFAT   ");
    boot[72..80].copy_from_slice(&sectors.to_le_bytes());
    boot[80..84].copy_from_slice(&(fat_offset as u32).to_le_bytes());
    boot[84..88].copy_from_slice(&(fat_length as u32).to_le_bytes());
    boot[88..92].copy_from_slice(&(heap_offset as u32).to_le_bytes());
    boot[92..96].copy_from_slice(&(count as u32).to_le_bytes());
    boot[96..100].copy_from_slice(&(root as u32).to_le_bytes());
    boot[100..104].copy_from_slice(&0x5050_4157u32.to_le_bytes());
    boot[104..106].copy_from_slice(&0x0100u16.to_le_bytes());
    boot[108] = 9;
    boot[109] = per_cluster.trailing_zeros() as u8;
    boot[110] = 1;
    boot[111] = 0x80;
    boot[510] = 0x55;
    boot[511] = 0xAA;
    // The main boot region and its copy: the boot sector, eight extended
    // boot sectors, two empty ones and the checksum of them all.
    let mut sum = 0u32;
    let mut add = |index: u64, sector: &[u8; 512]| {
        for (i, &byte) in sector.iter().enumerate() {
            if index == 0 && matches!(i, 106 | 107 | 112) {
                continue;
            }
            sum = sum.rotate_right(1).wrapping_add(u32::from(byte));
        }
    };
    let mut extended = [0u8; 512];
    extended[510] = 0x55;
    extended[511] = 0xAA;
    add(0, &boot);
    (1..9).for_each(|i| add(i, &extended));
    let mut sums = [0u8; 512];
    for word in sums.chunks_exact_mut(4) {
        word.copy_from_slice(&sum.to_le_bytes());
    }
    for region in [0u64, 12] {
        disk.seek(SeekFrom::Start(region * SECTOR))?;
        disk.write_all(&boot)?;
        for _ in 1..9 {
            disk.write_all(&extended)?;
        }
        disk.write_all(&[0u8; 1024])?;
        disk.write_all(&sums)?;
    }

    // The FAT: the media type, then the chains of the three files.
    disk.seek(SeekFrom::Start(fat_offset * SECTOR))?;
    disk.write_all(&0xFFFF_FFF8u32.to_le_bytes())?;
    disk.write_all(&FAT_END.to_le_bytes())?;
    for (first, len) in [(bitmap_first, bitmap_clusters), (upcase_first, upcase_clusters), (root, 1)] {
        for n in first..first + len {
            let next = if n + 1 < first + len { (n + 1) as u32 } else { FAT_END };
            disk.write_all(&next.to_le_bytes())?;
        }
    }

    // The bitmap: its own clusters, the table's and the root's.
    disk.seek(SeekFrom::Start(at(bitmap_first)))?;
    for byte in 0..used.div_ceil(8) {
        let bits = (used - byte * 8).min(8);
        disk.write_all(&[(0xFFu16 >> (8 - bits)) as u8])?;
    }
    disk.seek(SeekFrom::Start(at(upcase_first)))?;
    disk.write_all(UPCASE)?;

    disk.seek(SeekFrom::Start(at(root)))?;
    crate::fs::write_zeros(disk, u64::from(cluster))?;
    let mut entries = [0u8; 2 * ENTRY];
    entries[0] = ENTRY_BITMAP;
    entries[20..24].copy_from_slice(&(bitmap_first as u32).to_le_bytes());
    entries[24..32].copy_from_slice(&bitmap_len.to_le_bytes());
    let table = &mut entries[ENTRY..];
    table[0] = ENTRY_UPCASE;
    table[4..8].copy_from_slice(&UPCASE_CHECKSUM.to_le_bytes());
    table[20..24].copy_from_slice(&(upcase_first as u32).to_le_bytes());
    table[24..32].copy_from_slice(&(UPCASE.len() as u64).to_le_bytes());
    disk.seek(SeekFrom::Start(at(root)))?;
    disk.write_all(&entries)?;
    disk.flush()?;
    Ok(())
}

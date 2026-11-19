//! Formatting a volume (feature `format`: tests and tools, not the firmware).

use core::slice;

use crate::boot_sector::{BiosParameterBlock, BootSector};
use crate::dir_entry::{DIR_ENTRY_SIZE, DirFileEntryData, FileAttributes, SFN_SIZE};
use crate::error::{Error, IoError};
use crate::fs::ReadWriteSeek;
use crate::fs::{FatType, write_zeros};
use crate::fs::{FsInfoSector, fat_slice};
use crate::io::SeekFrom;
use crate::table::alloc_cluster;
use crate::table::{RESERVED_FAT_ENTRIES, format_fat};
use core::cell::Cell;

const BITS_PER_BYTE: u32 = 8;
const KB_32: u32 = 1024;
const KB_64: u64 = 1024;
const MB_64: u64 = KB_64 * 1024;
const GB_64: u64 = MB_64 * 1024;

pub(crate) fn estimate_fat_type(total_bytes: u64) -> FatType {
    // Used only to select cluster size if FAT type has not been overriden in options
    if total_bytes < 4200 * KB_64 {
        FatType::Fat12
    } else if total_bytes < 512 * MB_64 {
        FatType::Fat16
    } else {
        FatType::Fat32
    }
}

fn determine_bytes_per_cluster(
    total_bytes: u64,
    bytes_per_sector: u16,
    fat_type: Option<FatType>,
) -> u32 {
    const MAX_CLUSTER_SIZE: u32 = 32 * KB_32;

    let fat_type = fat_type.unwrap_or_else(|| estimate_fat_type(total_bytes));
    let bytes_per_cluster = match fat_type {
        FatType::Fat12 => {
            // approximate cluster count: (1024, 2048]
            (total_bytes.next_power_of_two() / MB_64 * 512) as u32
        }
        FatType::Fat16 => {
            if total_bytes <= 16 * MB_64 {
                // cluster size: 1 KB
                // approximate cluster count: (4096, 16384]
                KB_32
            } else if total_bytes <= 128 * MB_64 {
                // cluster size: 2 KB
                // approximate cluster count: (8192, 65536]
                2 * KB_32
            } else {
                // cluster size: 4 KB or more
                // approximate cluster count: (32768, 65536]
                ((total_bytes.next_power_of_two() / (64 * MB_64)) as u32) * KB_32
            }
        }
        FatType::Fat32 => {
            if total_bytes <= 260 * MB_64 {
                // cluster size: 512 B
                // approximate cluster count: (65_536, 532_480]
                512
            } else if total_bytes <= 8 * GB_64 {
                // cluster size: 4 KB
                // approximate cluster count: (66_560, 2_097_152]
                // Note: for minimal volume size (260 MB + 1 sector) it always results in enough cluster for this FAT type
                // unless there is more than ~7000 reserved sectors
                4 * KB_32
            } else {
                // cluster size: 8 KB or more
                // approximate cluster count: (1_048_576, 2_097_152]
                // at 32 GB it already uses maximal cluster size and then cluster count goes out of the above range
                ((total_bytes.next_power_of_two() / (2 * GB_64)) as u32) * KB_32
            }
        }
    };
    let bytes_per_cluster_clamped =
        bytes_per_cluster.clamp(bytes_per_sector.into(), MAX_CLUSTER_SIZE);
    debug_assert!(bytes_per_cluster_clamped.is_power_of_two());
    bytes_per_cluster_clamped
}

fn determine_sectors_per_fat(
    total_sectors: u32,
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    fat_type: FatType,
    reserved_sectors: u16,
    root_dir_sectors: u32,
    fats: u8,
) -> u32 {
    //
    // FAT size formula transformations:
    //
    // Initial basic formula:
    // size of FAT in bits >= (total number of clusters + 2) * bits per FAT entry
    //
    // Note: when computing number of clusters from number of sectors rounding down is used because partial clusters
    // are not allowed
    // Note: in those transformations '/' is a floating-point division (not a rounding towards zero division)
    //
    // data_sectors = total_sectors - reserved_sectors - fats * sectors_per_fat - root_dir_sectors
    // total_clusters = floor(data_sectors / sectors_per_cluster)
    // bits_per_sector = bytes_per_sector * 8
    // sectors_per_fat * bits_per_sector >= (total_clusters + 2) * bits_per_fat_entry
    // sectors_per_fat * bits_per_sector >= (floor(data_sectors / sectors_per_cluster) + 2) * bits_per_fat_entry
    //
    // Note: omitting the floor function can cause the FAT to be bigger by 1 entry - negligible
    //
    // sectors_per_fat * bits_per_sector >= (data_sectors / sectors_per_cluster + 2) * bits_per_fat_entry
    // t0 = total_sectors - reserved_sectors - root_dir_sectors
    // sectors_per_fat * bits_per_sector >= ((t0 - fats * sectors_per_fat) / sectors_per_cluster + 2) * bits_per_fat_entry
    // sectors_per_fat * bits_per_sector / bits_per_fat_entry >= (t0 - fats * sectors_per_fat) / sectors_per_cluster + 2
    // sectors_per_fat * bits_per_sector / bits_per_fat_entry >= t0 / sectors_per_cluster + 2 - fats * sectors_per_fat / sectors_per_cluster
    // sectors_per_fat * bits_per_sector / bits_per_fat_entry + fats * sectors_per_fat / sectors_per_cluster >= t0 / sectors_per_cluster + 2
    // sectors_per_fat * (bits_per_sector / bits_per_fat_entry + fats / sectors_per_cluster) >= t0 / sectors_per_cluster + 2
    // sectors_per_fat >= (t0 / sectors_per_cluster + 2) / (bits_per_sector / bits_per_fat_entry + fats / sectors_per_cluster)
    //
    // Note: MS specification omits the constant 2 in calculations. This library is taking a better approach...
    //
    // sectors_per_fat >= ((t0 + 2 * sectors_per_cluster) / sectors_per_cluster) / (bits_per_sector / bits_per_fat_entry + fats / sectors_per_cluster)
    // sectors_per_fat >= (t0 + 2 * sectors_per_cluster) / (sectors_per_cluster * bits_per_sector / bits_per_fat_entry + fats)
    //
    // Note: compared to MS formula this one can suffer from an overflow problem if u32 type is used
    //
    // When converting formula to integer types round towards a bigger FAT:
    // * first division towards infinity
    // * second division towards zero (it is in a denominator of the first division)

    let t0: u32 = total_sectors - u32::from(reserved_sectors) - root_dir_sectors;
    let t1: u64 = u64::from(t0) + u64::from(2 * u32::from(sectors_per_cluster));
    let bits_per_cluster =
        u32::from(sectors_per_cluster) * u32::from(bytes_per_sector) * BITS_PER_BYTE;
    let t2 = u64::from(bits_per_cluster / fat_type.bits_per_fat_entry() + u32::from(fats));
    let sectors_per_fat = (t1 + t2 - 1) / t2;
    // Note: casting is safe here because number of sectors per FAT cannot be bigger than total sectors number
    sectors_per_fat as u32
}

fn try_fs_layout(
    total_sectors: u32,
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    fat_type: FatType,
    root_dir_sectors: u32,
    fats: u8,
    reserved_sectors: Option<u16>,
) -> Result<(u16, u32), Error<()>> {
    // Note: most of implementations use 32 reserved sectors for FAT32 but it's wasting of space
    // This implementation uses only 8 by default. This is enough to fit in two boot sectors (main and backup) with
    // additional bootstrap code and one FSInfo sector. It also makes FAT alligned to 4096 which is a nice number.
    let min_reserved_sectors: u16 = if fat_type == FatType::Fat32 { 7 } else { 1 };
    let reserved_sectors =
        reserved_sectors.unwrap_or(if fat_type == FatType::Fat32 { 8 } else { 1 });
    if reserved_sectors < min_reserved_sectors {
        error!(
            "Too few reserved sectors for {:?}: {}",
            fat_type, reserved_sectors
        );
        return Err(Error::InvalidInput);
    }

    // Check if volume has enough space to accomodate reserved sectors, FAT, root directory and some data space
    // Having less than 8 sectors for FAT and data would make a little sense
    if total_sectors <= u32::from(reserved_sectors) + root_dir_sectors + 8 {
        error!("Volume is too small");
        return Err(Error::InvalidInput);
    }

    // calculate File Allocation Table size
    let sectors_per_fat = determine_sectors_per_fat(
        total_sectors,
        bytes_per_sector,
        sectors_per_cluster,
        fat_type,
        reserved_sectors,
        root_dir_sectors,
        fats,
    );

    let data_sectors = total_sectors
        - u32::from(reserved_sectors)
        - root_dir_sectors
        - sectors_per_fat * u32::from(fats);
    let total_clusters = data_sectors / u32::from(sectors_per_cluster);
    if fat_type != FatType::from_clusters(total_clusters) {
        error!(
            "Invalid FAT type (expect {:?} due to {} clusters",
            FatType::from_clusters(total_clusters),
            total_clusters
        );
        return Err(Error::InvalidInput);
    }
    debug_assert!(total_clusters >= fat_type.min_clusters());
    if total_clusters > fat_type.max_clusters() {
        // Note: it can happen for FAT32
        error!("Too many clusters");
        return Err(Error::InvalidInput);
    }

    Ok((reserved_sectors, sectors_per_fat))
}

fn determine_root_dir_sectors(
    root_dir_entries: u16,
    bytes_per_sector: u16,
    fat_type: FatType,
) -> u32 {
    if fat_type == FatType::Fat32 {
        0
    } else {
        let root_dir_bytes = u32::from(root_dir_entries) * DIR_ENTRY_SIZE;
        (root_dir_bytes + u32::from(bytes_per_sector) - 1) / u32::from(bytes_per_sector)
    }
}

struct FsLayout {
    fat_type: FatType,
    reserved_sectors: u16,
    sectors_per_fat: u32,
    sectors_per_cluster: u8,
}

fn determine_fs_layout<E: IoError>(
    options: &FormatVolumeOptions,
    total_sectors: u32,
) -> Result<FsLayout, Error<E>> {
    let bytes_per_cluster = options.bytes_per_cluster.unwrap_or_else(|| {
        let total_bytes = u64::from(total_sectors) * u64::from(options.bytes_per_sector);
        determine_bytes_per_cluster(total_bytes, options.bytes_per_sector, options.fat_type)
    });

    let sectors_per_cluster_32 = bytes_per_cluster / u32::from(options.bytes_per_sector);
    let Ok(sectors_per_cluster) = sectors_per_cluster_32.try_into() else {
        error!("Too many sectors per cluster, please try a different volume size");
        return Err(Error::InvalidInput);
    };

    let allowed_fat_types: &[FatType] = options.fat_type.as_ref().map_or(
        &[FatType::Fat32, FatType::Fat16, FatType::Fat12],
        slice::from_ref,
    );

    // Note: this loop is needed because in case of user-provided cluster size it is hard to reliably determine
    // a proper FAT type. In case of automatic cluster size actual FAT type is determined in `estimate_fat_type`
    for &fat_type in allowed_fat_types {
        let root_dir_sectors = determine_root_dir_sectors(
            options.max_root_dir_entries,
            options.bytes_per_sector,
            fat_type,
        );
        let result = try_fs_layout(
            total_sectors,
            options.bytes_per_sector,
            sectors_per_cluster,
            fat_type,
            root_dir_sectors,
            options.fats,
            options.reserved_sectors,
        );
        if let Ok((reserved_sectors, sectors_per_fat)) = result {
            return Ok(FsLayout {
                fat_type,
                reserved_sectors,
                sectors_per_fat,
                sectors_per_cluster,
            });
        }
    }

    error!("Cannot select FAT type - unfortunate storage size");
    Err(Error::InvalidInput)
}

fn format_bpb<E: IoError>(
    options: &FormatVolumeOptions,
    total_sectors: u32,
) -> Result<(BiosParameterBlock, FatType), Error<E>> {
    let layout = determine_fs_layout(options, total_sectors)?;

    // drive_num should be 0 for floppy disks and 0x80 for hard disks - determine it using FAT type
    let drive_num = options.drive_num.unwrap_or_else(|| {
        if layout.fat_type == FatType::Fat12 {
            0
        } else {
            0x80
        }
    });

    // setup volume label
    let volume_label = options.volume_label.unwrap_or(*b"NO NAME    ");

    // setup fs_type_label field
    let fs_type_label = *match layout.fat_type {
        FatType::Fat12 => b"FAT12   ",
        FatType::Fat16 => b"FAT16   ",
        FatType::Fat32 => b"FAT32   ",
    };

    // create Bios Parameter Block struct
    let is_fat32 = layout.fat_type == FatType::Fat32;
    let sectors_per_fat_16 = if is_fat32 {
        0
    } else {
        let Ok(sectors_per_fat_16) = layout.sectors_per_fat.try_into() else {
            error!("FAT is too big, please try a different volume size");
            return Err(Error::InvalidInput);
        };
        sectors_per_fat_16
    };
    let sectors_per_fat_32 = if is_fat32 { layout.sectors_per_fat } else { 0 };

    let root_entries = if is_fat32 {
        0
    } else {
        options.max_root_dir_entries
    };

    let total_sectors_16 = if is_fat32 {
        0
    } else {
        total_sectors.try_into().unwrap_or(0)
    };
    let total_sectors_32 = if total_sectors_16 == 0 {
        total_sectors
    } else {
        0
    };

    let bpb = BiosParameterBlock {
        bytes_per_sector: options.bytes_per_sector,
        sectors_per_cluster: layout.sectors_per_cluster,
        reserved_sectors: layout.reserved_sectors,
        fats: options.fats,
        root_entries,
        total_sectors_16,
        media: options.media,
        sectors_per_fat_16,
        sectors_per_track: options.sectors_per_track,
        heads: options.heads,
        hidden_sectors: options.hidden_sectors,
        total_sectors_32,
        // FAT32 fields start
        sectors_per_fat_32,
        extended_flags: 0, // mirroring enabled
        fs_version: 0,
        root_dir_first_cluster: if is_fat32 { 2 } else { 0 },
        fs_info_sector: if is_fat32 { 1 } else { 0 },
        backup_boot_sector: if is_fat32 { 6 } else { 0 },
        reserved_0: [0_u8; 12],
        // FAT32 fields end
        drive_num,
        reserved_1: 0,
        ext_sig: 0x29,
        volume_id: options.volume_id,
        volume_label,
        fs_type_label,
    };

    // Check if number of clusters is proper for used FAT type
    if FatType::from_clusters(bpb.total_clusters()) != layout.fat_type {
        error!(
            "Total number of clusters and FAT type does not match, please try a different volume size"
        );
        return Err(Error::InvalidInput);
    }

    Ok((bpb, layout.fat_type))
}

pub(crate) fn format_boot_sector<E: IoError>(
    options: &FormatVolumeOptions,
    total_sectors: u32,
) -> Result<(BootSector, FatType), Error<E>> {
    let mut boot = BootSector::default();
    let (bpb, fat_type) = format_bpb(options, total_sectors)?;
    boot.bpb = bpb;
    boot.oem_name.copy_from_slice(b"MSWIN4.1");
    // Boot code copied from FAT32 boot sector initialized by mkfs.fat
    boot.bootjmp = [0xEB, 0x58, 0x90];
    let boot_code: [u8; 129] = [
        0x0E, 0x1F, 0xBE, 0x77, 0x7C, 0xAC, 0x22, 0xC0, 0x74, 0x0B, 0x56, 0xB4, 0x0E, 0xBB, 0x07,
        0x00, 0xCD, 0x10, 0x5E, 0xEB, 0xF0, 0x32, 0xE4, 0xCD, 0x16, 0xCD, 0x19, 0xEB, 0xFE, 0x54,
        0x68, 0x69, 0x73, 0x20, 0x69, 0x73, 0x20, 0x6E, 0x6F, 0x74, 0x20, 0x61, 0x20, 0x62, 0x6F,
        0x6F, 0x74, 0x61, 0x62, 0x6C, 0x65, 0x20, 0x64, 0x69, 0x73, 0x6B, 0x2E, 0x20, 0x20, 0x50,
        0x6C, 0x65, 0x61, 0x73, 0x65, 0x20, 0x69, 0x6E, 0x73, 0x65, 0x72, 0x74, 0x20, 0x61, 0x20,
        0x62, 0x6F, 0x6F, 0x74, 0x61, 0x62, 0x6C, 0x65, 0x20, 0x66, 0x6C, 0x6F, 0x70, 0x70, 0x79,
        0x20, 0x61, 0x6E, 0x64, 0x0D, 0x0A, 0x70, 0x72, 0x65, 0x73, 0x73, 0x20, 0x61, 0x6E, 0x79,
        0x20, 0x6B, 0x65, 0x79, 0x20, 0x74, 0x6F, 0x20, 0x74, 0x72, 0x79, 0x20, 0x61, 0x67, 0x61,
        0x69, 0x6E, 0x20, 0x2E, 0x2E, 0x2E, 0x20, 0x0D, 0x0A,
    ];
    boot.boot_code[..boot_code.len()].copy_from_slice(&boot_code);
    boot.boot_sig = [0x55, 0xAA];

    // fix offsets in bootjmp and boot code for non-FAT32 filesystems (bootcode is on a different offset)
    if fat_type != FatType::Fat32 {
        // offset of boot code
        const BOOT_CODE_OFFSET: u8 = 0x36 + 8;
        // offset of message
        const MESSAGE_OFFSET: u16 = 29;
        boot.bootjmp[1] = BOOT_CODE_OFFSET - 2;
        let message_offset_in_sector = u16::from(BOOT_CODE_OFFSET) + MESSAGE_OFFSET + 0x7c00;
        boot.boot_code[3] = (message_offset_in_sector & 0xff) as u8;
        boot.boot_code[4] = (message_offset_in_sector >> 8) as u8;
    }

    Ok((boot, fat_type))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_fat_type() {
        assert_eq!(estimate_fat_type(4 * MB_64), FatType::Fat12);
        assert_eq!(estimate_fat_type(5 * MB_64), FatType::Fat16);
        assert_eq!(estimate_fat_type(511 * MB_64), FatType::Fat16);
        assert_eq!(estimate_fat_type(512 * MB_64), FatType::Fat32);
    }

    #[test]
    fn test_determine_bytes_per_cluster_fat12() {
        assert_eq!(
            determine_bytes_per_cluster(128 * KB_64, 512, Some(FatType::Fat12)),
            512
        );
        assert_eq!(
            determine_bytes_per_cluster(MB_64, 512, Some(FatType::Fat12)),
            512
        );
        assert_eq!(
            determine_bytes_per_cluster(MB_64 + 1, 512, Some(FatType::Fat12)),
            1024
        );
        assert_eq!(
            determine_bytes_per_cluster(MB_64, 4096, Some(FatType::Fat12)),
            4096
        );
    }

    #[test]
    #[rustfmt::skip]
    fn test_determine_bytes_per_cluster_fat16() {
        assert_eq!(determine_bytes_per_cluster(MB_64,            512, Some(FatType::Fat16)), KB_32);
        assert_eq!(determine_bytes_per_cluster(MB_64,            4096, Some(FatType::Fat16)), 4 * KB_32);
        assert_eq!(determine_bytes_per_cluster(16 * MB_64,       512, Some(FatType::Fat16)), KB_32);
        assert_eq!(determine_bytes_per_cluster(16 * MB_64 + 1,   512, Some(FatType::Fat16)), 2 * KB_32);
        assert_eq!(determine_bytes_per_cluster(128 * MB_64,      512, Some(FatType::Fat16)), 2 * KB_32);
        assert_eq!(determine_bytes_per_cluster(128 * MB_64 + 1,  512, Some(FatType::Fat16)), 4 * KB_32);
        assert_eq!(determine_bytes_per_cluster(256 * MB_64,      512, Some(FatType::Fat16)), 4 * KB_32);
        assert_eq!(determine_bytes_per_cluster(256 * MB_64 + 1,  512, Some(FatType::Fat16)), 8 * KB_32);
        assert_eq!(determine_bytes_per_cluster(512 * MB_64,      512, Some(FatType::Fat16)), 8 * KB_32);
        assert_eq!(determine_bytes_per_cluster(512 * MB_64 + 1,  512, Some(FatType::Fat16)), 16 * KB_32);
        assert_eq!(determine_bytes_per_cluster(1024 * MB_64,     512, Some(FatType::Fat16)), 16 * KB_32);
        assert_eq!(determine_bytes_per_cluster(1024 * MB_64 + 1, 512, Some(FatType::Fat16)), 32 * KB_32);
        assert_eq!(determine_bytes_per_cluster(99999 * MB_64,    512, Some(FatType::Fat16)), 32 * KB_32);
    }

    #[test]
    #[rustfmt::skip]
    fn test_determine_bytes_per_cluster_fat32() {
        assert_eq!(determine_bytes_per_cluster(260 * MB_64,     512, Some(FatType::Fat32)), 512);
        assert_eq!(determine_bytes_per_cluster(260 * MB_64,     4096, Some(FatType::Fat32)), 4 * KB_32);
        assert_eq!(determine_bytes_per_cluster(260 * MB_64 + 1, 512, Some(FatType::Fat32)), 4 * KB_32);
        assert_eq!(determine_bytes_per_cluster(8   * GB_64,     512, Some(FatType::Fat32)), 4 * KB_32);
        assert_eq!(determine_bytes_per_cluster(8   * GB_64 + 1, 512, Some(FatType::Fat32)), 8 * KB_32);
        assert_eq!(determine_bytes_per_cluster(16  * GB_64,     512, Some(FatType::Fat32)), 8 * KB_32);
        assert_eq!(determine_bytes_per_cluster(16  * GB_64 + 1, 512, Some(FatType::Fat32)), 16 * KB_32);
        assert_eq!(determine_bytes_per_cluster(32  * GB_64,     512, Some(FatType::Fat32)), 16 * KB_32);
        assert_eq!(determine_bytes_per_cluster(32  * GB_64 + 1, 512, Some(FatType::Fat32)), 32 * KB_32);
        assert_eq!(determine_bytes_per_cluster(999 * GB_64,     512, Some(FatType::Fat32)), 32 * KB_32);
    }

    fn test_determine_sectors_per_fat_single(
        total_bytes: u64,
        bytes_per_sector: u16,
        bytes_per_cluster: u32,
        fat_type: FatType,
        reserved_sectors: u16,
        fats: u8,
        root_dir_entries: u32,
    ) {
        let total_sectors = total_bytes / u64::from(bytes_per_sector);
        debug_assert!(total_sectors <= u64::from(u32::MAX), "{:x}", total_sectors);
        let total_sectors = total_sectors as u32;

        let sectors_per_cluster = (bytes_per_cluster / u32::from(bytes_per_sector)) as u8;
        let root_dir_size = root_dir_entries * DIR_ENTRY_SIZE;
        let root_dir_sectors =
            (root_dir_size + u32::from(bytes_per_sector) - 1) / u32::from(bytes_per_sector);
        let sectors_per_fat = determine_sectors_per_fat(
            total_sectors,
            bytes_per_sector,
            sectors_per_cluster,
            fat_type,
            reserved_sectors,
            root_dir_sectors,
            fats,
        );

        let sectors_per_all_fats = u32::from(fats) * sectors_per_fat;
        let total_data_sectors =
            total_sectors - u32::from(reserved_sectors) - sectors_per_all_fats - root_dir_sectors;
        let total_clusters = total_data_sectors / u32::from(sectors_per_cluster);
        if FatType::from_clusters(total_clusters) != fat_type {
            // Skip impossible FAT configurations
            return;
        }
        let bits_per_sector = u32::from(bytes_per_sector) * BITS_PER_BYTE;
        let bits_per_fat = u64::from(sectors_per_fat) * u64::from(bits_per_sector);
        let total_fat_entries = (bits_per_fat / u64::from(fat_type.bits_per_fat_entry())) as u32;
        let fat_clusters = total_fat_entries - RESERVED_FAT_ENTRIES;
        // Note: fat_entries_per_sector is rounded down for FAT12
        let fat_entries_per_sector = bits_per_sector / fat_type.bits_per_fat_entry();
        let desc = format!(
            "total_clusters {}, fat_clusters {}, total_sectors {}, bytes/sector {}, sectors/cluster {}, fat_type {:?}, reserved_sectors {}, root_dir_sectors {}, sectors_per_fat {}",
            total_clusters,
            fat_clusters,
            total_sectors,
            bytes_per_sector,
            sectors_per_cluster,
            fat_type,
            reserved_sectors,
            root_dir_sectors,
            sectors_per_fat
        );
        assert!(fat_clusters >= total_clusters, "Too small FAT: {}", desc);
        let expected_max_fat_clusters = total_clusters + 2 * fat_entries_per_sector;
        assert!(
            fat_clusters <= expected_max_fat_clusters,
            "Too big FAT: {}",
            desc
        );
    }

    fn test_determine_sectors_per_fat_for_multiple_sizes(
        bytes_per_sector: u16,
        fat_type: FatType,
        reserved_sectors: u16,
        fats: u8,
        root_dir_entries: u32,
    ) {
        let mut bytes_per_cluster = u32::from(bytes_per_sector);
        while bytes_per_cluster <= 64 * KB_32 {
            let mut size: u64 = MB_64;
            while size < 2048 * GB_64 {
                test_determine_sectors_per_fat_single(
                    size,
                    bytes_per_sector,
                    bytes_per_cluster,
                    fat_type,
                    reserved_sectors,
                    fats,
                    root_dir_entries,
                );
                size = size + size / 7;
            }
            size = 2048 * GB_64 - 1;
            test_determine_sectors_per_fat_single(
                size,
                bytes_per_sector,
                bytes_per_cluster,
                fat_type,
                reserved_sectors,
                fats,
                root_dir_entries,
            );
            bytes_per_cluster *= 2;
        }
    }

    #[test]
    fn test_determine_sectors_per_fat() {
        test_determine_sectors_per_fat_for_multiple_sizes(512, FatType::Fat12, 1, 2, 512);
        test_determine_sectors_per_fat_for_multiple_sizes(512, FatType::Fat12, 1, 1, 512);
        test_determine_sectors_per_fat_for_multiple_sizes(512, FatType::Fat12, 1, 2, 8192);
        test_determine_sectors_per_fat_for_multiple_sizes(4096, FatType::Fat12, 1, 2, 512);

        test_determine_sectors_per_fat_for_multiple_sizes(512, FatType::Fat16, 1, 2, 512);
        test_determine_sectors_per_fat_for_multiple_sizes(512, FatType::Fat16, 1, 1, 512);
        test_determine_sectors_per_fat_for_multiple_sizes(512, FatType::Fat16, 1, 2, 8192);
        test_determine_sectors_per_fat_for_multiple_sizes(4096, FatType::Fat16, 1, 2, 512);

        test_determine_sectors_per_fat_for_multiple_sizes(512, FatType::Fat32, 32, 2, 0);
        test_determine_sectors_per_fat_for_multiple_sizes(512, FatType::Fat32, 32, 1, 0);
        test_determine_sectors_per_fat_for_multiple_sizes(4096, FatType::Fat32, 32, 2, 0);
    }

    #[test]
    fn test_format_boot_sector() {
        let bytes_per_sector = 512_u16;
        // test all partition sizes from 1MB to 2TB (u32::MAX sectors is 2TB - 1 for 512 byte sectors)
        let mut total_sectors_vec = Vec::new();
        let mut size = MB_64;
        while size < 2048 * GB_64 {
            total_sectors_vec.push((size / u64::from(bytes_per_sector)) as u32);
            size = size + size / 7;
        }
        total_sectors_vec.push(u32::MAX);
        total_sectors_vec.push(8227);
        for total_sectors in total_sectors_vec {
            let (boot, _) = format_boot_sector::<()>(&FormatVolumeOptions::new(), total_sectors)
                .unwrap_or_else(|_| panic!("format_boot_sector total_sectors: {}", total_sectors));
            boot.validate::<()>(true).expect("validate");
        }
    }

    fn test_determine_fs_layout(fat_type: FatType, min_size: u64, max_size: u64) {
        let bytes_per_sector = 512_u16;
        // test all partition sizes from 24 KB to 127 MB
        let mut total_sectors_vec = Vec::new();
        let mut size = min_size;
        while size < max_size {
            total_sectors_vec.push((size / u64::from(bytes_per_sector)).try_into().unwrap());
            size += size / 7;
        }
        total_sectors_vec.push((max_size / u64::from(bytes_per_sector)).try_into().unwrap());
        for total_sectors in total_sectors_vec {
            let options = FormatVolumeOptions::new().fat_type(fat_type);
            let layout = determine_fs_layout::<()>(&options, total_sectors).unwrap_or_else(|e| {
                panic!(
                    "determine_fs_layout(total_sectors={}): {:?}",
                    total_sectors, e
                )
            });
            assert_eq!(layout.fat_type, fat_type);
        }
    }

    #[test]
    fn test_determine_fs_layout_fat12() {
        // approximately: 21 KB - 127 MB
        test_determine_fs_layout(FatType::Fat12, 21 * KB_64, 127 * MB_64);
    }

    #[test]
    fn test_determine_fs_layout_fat16() {
        // approximately: 4.1 MB - 1.9 GB
        test_determine_fs_layout(FatType::Fat16, 4120 * KB_64, 2047 * MB_64);
    }

    #[test]
    fn test_determine_fs_layout_fat32() {
        // approximately: 33 MB - 1.9 TB
        test_determine_fs_layout(FatType::Fat32, 33 * MB_64, 2048 * GB_64 - 1);
    }
}

fn write_zeros_until_end_of_sector<IO: ReadWriteSeek>(
    disk: &mut IO,
    bytes_per_sector: u16,
) -> Result<(), IO::Error> {
    let pos = disk.seek(SeekFrom::Current(0))?;
    let total_bytes_to_write = u64::from(bytes_per_sector) - (pos % u64::from(bytes_per_sector));
    if total_bytes_to_write != u64::from(bytes_per_sector) {
        write_zeros(disk, total_bytes_to_write)?;
    }
    Ok(())
}

/// A FAT filesystem formatting options
///
/// This struct implements a builder pattern.
/// Options are specified as an argument for `format_volume` function.
#[derive(Debug, Clone)]
pub struct FormatVolumeOptions {
    pub(crate) bytes_per_sector: u16,
    pub(crate) total_sectors: Option<u32>,
    pub(crate) bytes_per_cluster: Option<u32>,
    pub(crate) fat_type: Option<FatType>,
    pub(crate) max_root_dir_entries: u16,
    pub(crate) fats: u8,
    pub(crate) media: u8,
    pub(crate) sectors_per_track: u16,
    pub(crate) heads: u16,
    pub(crate) drive_num: Option<u8>,
    pub(crate) hidden_sectors: u32,
    pub(crate) reserved_sectors: Option<u16>,
    pub(crate) volume_id: u32,
    pub(crate) volume_label: Option<[u8; SFN_SIZE]>,
}

impl Default for FormatVolumeOptions {
    fn default() -> Self {
        Self {
            bytes_per_sector: 512,
            total_sectors: None,
            bytes_per_cluster: None,
            fat_type: None,
            max_root_dir_entries: 512,
            fats: 2,
            media: 0xF8,
            sectors_per_track: 0x20,
            heads: 0x40,
            drive_num: None,
            hidden_sectors: 0,
            reserved_sectors: None,
            volume_id: 0x1234_5678,
            volume_label: None,
        }
    }
}

impl FormatVolumeOptions {
    /// Create options struct for `format_volume` function
    ///
    /// Allows to overwrite many filesystem parameters.
    /// In normal use-case defaults should suffice.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set size of cluster in bytes (must be dividable by sector size)
    ///
    /// Cluster size must be a power of two and be greater or equal to sector size.
    /// If option is not specified optimal cluster size is selected based on partition size and
    /// optionally FAT type override (if specified using `fat_type` method).
    ///
    /// # Panics
    ///
    /// Panics if `bytes_per_cluster` is not a power of two or is lower than `512`.
    #[must_use]
    pub fn bytes_per_cluster(mut self, bytes_per_cluster: u32) -> Self {
        assert!(
            bytes_per_cluster.is_power_of_two() && bytes_per_cluster >= 512,
            "Invalid bytes_per_cluster"
        );
        self.bytes_per_cluster = Some(bytes_per_cluster);
        self
    }

    /// Set File Allocation Table type
    ///
    /// Option allows to override File Allocation Table (FAT) entry size.
    /// It is unrecommended to set this option unless you know what you are doing.
    /// Note: FAT type is determined from total number of clusters. Changing this option can cause formatting to fail
    /// if the volume cannot be divided into proper number of clusters for selected FAT type.
    #[must_use]
    pub fn fat_type(mut self, fat_type: FatType) -> Self {
        self.fat_type = Some(fat_type);
        self
    }

    /// Set sector size in bytes
    ///
    /// Sector size must be a power of two and be in range 512 - 4096.
    /// Default is `512`.
    ///
    /// # Panics
    ///
    /// Panics if `bytes_per_sector` is not a power of two or is lower than `512`.
    #[must_use]
    pub fn bytes_per_sector(mut self, bytes_per_sector: u16) -> Self {
        assert!(
            bytes_per_sector.is_power_of_two() && bytes_per_sector >= 512,
            "Invalid bytes_per_sector"
        );
        self.bytes_per_sector = bytes_per_sector;
        self
    }

    /// Set total number of sectors
    ///
    /// If option is not specified total number of sectors is calculated as storage device size divided by sector size.
    #[must_use]
    pub fn total_sectors(mut self, total_sectors: u32) -> Self {
        self.total_sectors = Some(total_sectors);
        self
    }

    /// Set maximal numer of entries in root directory for FAT12/FAT16 volumes
    ///
    /// Total root directory size should be dividable by sectors size so keep it a multiple of 16 (for default sector
    /// size).
    /// Note: this limit is not used on FAT32 volumes.
    /// Default is `512`.
    #[must_use]
    pub fn max_root_dir_entries(mut self, max_root_dir_entries: u16) -> Self {
        self.max_root_dir_entries = max_root_dir_entries;
        self
    }

    /// Set number of File Allocation Tables
    ///
    /// The only allowed values are `1` and `2`. If value `2` is used the FAT is mirrored.
    /// Default is `2`.
    ///
    /// # Panics
    ///
    /// Panics if `fats` is outside of the range [1, 2].
    #[must_use]
    pub fn fats(mut self, fats: u8) -> Self {
        assert!((1..=2).contains(&fats), "Invalid number of FATs");
        self.fats = fats;
        self
    }

    /// Set media field for Bios Parameters Block
    ///
    /// Default is `0xF8`.
    #[must_use]
    pub fn media(mut self, media: u8) -> Self {
        self.media = media;
        self
    }

    /// Set number of physical sectors per track for Bios Parameters Block (INT 13h CHS geometry)
    ///
    /// Default is `0x20`.
    #[must_use]
    pub fn sectors_per_track(mut self, sectors_per_track: u16) -> Self {
        self.sectors_per_track = sectors_per_track;
        self
    }

    /// Set number of heads for Bios Parameters Block (INT 13h CHS geometry)
    ///
    /// Default is `0x40`.
    #[must_use]
    pub fn heads(mut self, heads: u16) -> Self {
        self.heads = heads;
        self
    }

    /// Set drive number for Bios Parameters Block
    ///
    /// Default is `0` for FAT12, `0x80` for FAT16/FAT32.
    #[must_use]
    pub fn drive_num(mut self, drive_num: u8) -> Self {
        self.drive_num = Some(drive_num);
        self
    }

    /// Set the number of hidden sectors for Bios Parameters Block: the sectors on the disk before
    /// the volume, i.e. the start of its partition.
    ///
    /// Boot code that loads the operating system through the BIOS reads it to find the volume, so
    /// it must be the partition's first sector on a bootable partition. Default is `0`.
    #[must_use]
    pub fn hidden_sectors(mut self, hidden_sectors: u32) -> Self {
        self.hidden_sectors = hidden_sectors;
        self
    }

    /// Set the number of reserved sectors: the sectors before the first FAT, which hold the boot
    /// sector and on FAT32 also the FS Information Sector, the backup boot sector (sector 6) and
    /// boot code.
    ///
    /// Default is `8` for FAT32 and `1` for FAT12 and FAT16; FAT32 needs at least `7`. Microsoft's
    /// own format uses `32` for FAT32, and Windows NT's FAT32 boot code keeps a second sector at
    /// sector 12, so a FAT32 volume that Windows NT setup makes bootable needs `32`.
    #[must_use]
    pub fn reserved_sectors(mut self, reserved_sectors: u16) -> Self {
        self.reserved_sectors = Some(reserved_sectors);
        self
    }

    /// Set volume ID for Bios Parameters Block
    ///
    /// Default is `0x12345678`.
    #[must_use]
    pub fn volume_id(mut self, volume_id: u32) -> Self {
        self.volume_id = volume_id;
        self
    }

    /// Set volume label
    ///
    /// Default is empty label.
    #[must_use]
    pub fn volume_label(mut self, volume_label: [u8; SFN_SIZE]) -> Self {
        self.volume_label = Some(volume_label);
        self
    }
}

/// Create FAT filesystem on a disk or partition (format a volume)
///
/// Warning: this function overrides internal FAT filesystem structures and causes a loss of all data on provided
/// partition. Please use it with caution.
/// Only quick formatting is supported. To achieve a full format zero entire partition before calling this function.
/// Supplied `storage` parameter cannot be seeked (internal pointer must be on position 0).
/// To format a fragment of a disk image (e.g. partition) library user should wrap the file struct in a struct
/// limiting access to partition bytes only e.g. `fscommon::StreamSlice`.
///
/// # Errors
///
/// Errors that can be returned:
///
/// * `Error::InvalidInput` will be returned if `options` describes an invalid file system that cannot be created.
///   Possible reason can be requesting a fat type that is not compatible with the total number of clusters or
///   formatting a too big storage. If sectors/clusters related options in `options` structure were left set to
///   defaults this error is very unlikely to happen.
/// * `Error::Io` will be returned if the provided storage object returned an I/O error.
///
/// # Panics
///
/// Panics in non-optimized build if `storage` position returned by `seek` is not zero.
#[allow(clippy::needless_pass_by_value)]
pub fn format_volume<S: ReadWriteSeek>(
    storage: &mut S,
    options: FormatVolumeOptions,
) -> Result<(), Error<S::Error>> {
    trace!("format_volume");
    debug_assert_eq!(storage.seek(SeekFrom::Current(0))?, 0);

    let total_sectors = if let Some(total_sectors) = options.total_sectors {
        total_sectors
    } else {
        let total_bytes: u64 = storage.seek(SeekFrom::End(0))?;
        let total_sectors_64 = total_bytes / u64::from(options.bytes_per_sector);
        storage.seek(SeekFrom::Start(0))?;
        if total_sectors_64 > u64::from(u32::MAX) {
            error!("Volume has too many sectors: {}", total_sectors_64);
            return Err(Error::InvalidInput);
        }
        total_sectors_64 as u32 // safe case: possible overflow is handled above
    };

    // Create boot sector, validate and write to storage device
    let (boot, fat_type) = format_boot_sector(&options, total_sectors)?;
    if boot.validate::<S::Error>(true).is_err() {
        return Err(Error::InvalidInput);
    }
    boot.serialize(storage)?;
    // Make sure entire logical sector is updated (serialize method always writes 512 bytes)
    let bytes_per_sector = boot.bpb.bytes_per_sector;
    write_zeros_until_end_of_sector(storage, bytes_per_sector)?;

    let bpb = &boot.bpb;
    if bpb.is_fat32() {
        // backup boot sector
        storage.seek(SeekFrom::Start(
            bpb.bytes_from_sectors(bpb.backup_boot_sector()),
        ))?;
        boot.serialize(storage)?;
        write_zeros_until_end_of_sector(storage, bytes_per_sector)?;
    }

    // format File Allocation Table
    let reserved_sectors = bpb.reserved_sectors();
    let fat_pos = bpb.bytes_from_sectors(reserved_sectors);
    let sectors_per_all_fats = bpb.sectors_per_all_fats();
    storage.seek(SeekFrom::Start(fat_pos))?;
    write_zeros(storage, bpb.bytes_from_sectors(sectors_per_all_fats))?;
    {
        let mut fat_slice = fat_slice::<S, &mut S>(storage, bpb);
        let sectors_per_fat = bpb.sectors_per_fat();
        let bytes_per_fat = bpb.bytes_from_sectors(sectors_per_fat);
        format_fat(
            &mut fat_slice,
            fat_type,
            bpb.media,
            bytes_per_fat,
            bpb.total_clusters(),
        )?;
    }

    // init root directory - zero root directory region for FAT12/16 and alloc first root directory cluster for FAT32
    let root_dir_first_sector = reserved_sectors + sectors_per_all_fats;
    let root_dir_sectors = bpb.root_dir_sectors();
    let root_dir_pos = bpb.bytes_from_sectors(root_dir_first_sector);
    storage.seek(SeekFrom::Start(root_dir_pos))?;
    write_zeros(storage, bpb.bytes_from_sectors(root_dir_sectors))?;
    if fat_type == FatType::Fat32 {
        let root_dir_first_cluster = {
            let mut fat_slice = fat_slice::<S, &mut S>(storage, bpb);
            alloc_cluster(&mut fat_slice, fat_type, None, None, 1, &Cell::new(0))?
        };
        assert_eq!(root_dir_first_cluster, bpb.root_dir_first_cluster);
        let first_data_sector = reserved_sectors + sectors_per_all_fats + root_dir_sectors;
        let data_sectors_before_root_dir =
            bpb.sectors_from_clusters(root_dir_first_cluster - RESERVED_FAT_ENTRIES);
        let fat32_root_dir_first_sector = first_data_sector + data_sectors_before_root_dir;
        let fat32_root_dir_pos = bpb.bytes_from_sectors(fat32_root_dir_first_sector);
        storage.seek(SeekFrom::Start(fat32_root_dir_pos))?;
        write_zeros(storage, u64::from(bpb.cluster_size()))?;

        let free_cluster_count = bpb.total_clusters() - 1;
        let fs_info_sector = FsInfoSector {
            free_cluster_count: Some(free_cluster_count),
            next_free_cluster: Some(root_dir_first_cluster + 1),
            dirty: false,
            claimed: false,
        };
        storage.seek(SeekFrom::Start(
            bpb.bytes_from_sectors(bpb.fs_info_sector()),
        ))?;
        fs_info_sector.serialize(storage)?;
        write_zeros_until_end_of_sector(storage, bytes_per_sector)?;
    }

    // Create volume label directory entry if volume label is specified in options
    if let Some(volume_label) = options.volume_label {
        storage.seek(SeekFrom::Start(root_dir_pos))?;
        let volume_entry = DirFileEntryData::new(volume_label, FileAttributes::VOLUME_ID);
        volume_entry.serialize(storage)?;
    }

    storage.seek(SeekFrom::Start(0))?;
    trace!("format_volume end");
    Ok(())
}

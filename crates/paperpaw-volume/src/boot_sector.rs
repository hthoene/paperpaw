use crate::dir_entry::DIR_ENTRY_SIZE;
use crate::error::{Error, IoError};
use crate::fs::{FatType, FsStatusFlags};
use crate::io::{Read, ReadLeExt, Write, WriteLeExt};
use crate::table::RESERVED_FAT_ENTRIES;

#[derive(Default, Debug, Clone)]
pub(crate) struct BiosParameterBlock {
    pub(crate) bytes_per_sector: u16,
    pub(crate) sectors_per_cluster: u8,
    pub(crate) reserved_sectors: u16,
    pub(crate) fats: u8,
    pub(crate) root_entries: u16,
    pub(crate) total_sectors_16: u16,
    pub(crate) media: u8,
    pub(crate) sectors_per_fat_16: u16,
    pub(crate) sectors_per_track: u16,
    pub(crate) heads: u16,
    pub(crate) hidden_sectors: u32,
    pub(crate) total_sectors_32: u32,

    // Extended BIOS Parameter Block
    pub(crate) sectors_per_fat_32: u32,
    pub(crate) extended_flags: u16,
    pub(crate) fs_version: u16,
    pub(crate) root_dir_first_cluster: u32,
    pub(crate) fs_info_sector: u16,
    pub(crate) backup_boot_sector: u16,
    pub(crate) reserved_0: [u8; 12],
    pub(crate) drive_num: u8,
    pub(crate) reserved_1: u8,
    pub(crate) ext_sig: u8,
    pub(crate) volume_id: u32,
    pub(crate) volume_label: [u8; 11],
    pub(crate) fs_type_label: [u8; 8],
}

impl BiosParameterBlock {
    fn deserialize<R: Read>(rdr: &mut R) -> Result<Self, R::Error> {
        let mut bpb = Self {
            bytes_per_sector: rdr.read_u16_le()?,
            sectors_per_cluster: rdr.read_u8()?,
            reserved_sectors: rdr.read_u16_le()?,
            fats: rdr.read_u8()?,
            root_entries: rdr.read_u16_le()?,
            total_sectors_16: rdr.read_u16_le()?,
            media: rdr.read_u8()?,
            sectors_per_fat_16: rdr.read_u16_le()?,
            sectors_per_track: rdr.read_u16_le()?,
            heads: rdr.read_u16_le()?,
            hidden_sectors: rdr.read_u32_le()?,
            total_sectors_32: rdr.read_u32_le()?,
            ..Self::default()
        };

        if bpb.is_fat32() {
            bpb.sectors_per_fat_32 = rdr.read_u32_le()?;
            bpb.extended_flags = rdr.read_u16_le()?;
            bpb.fs_version = rdr.read_u16_le()?;
            bpb.root_dir_first_cluster = rdr.read_u32_le()?;
            bpb.fs_info_sector = rdr.read_u16_le()?;
            bpb.backup_boot_sector = rdr.read_u16_le()?;
            rdr.read_exact(&mut bpb.reserved_0)?;
        }

        bpb.drive_num = rdr.read_u8()?;
        bpb.reserved_1 = rdr.read_u8()?;
        bpb.ext_sig = rdr.read_u8()?; // 0x29
        bpb.volume_id = rdr.read_u32_le()?;
        rdr.read_exact(&mut bpb.volume_label)?;
        rdr.read_exact(&mut bpb.fs_type_label)?;

        // when the extended boot signature is anything other than 0x29, the fields are invalid
        if bpb.ext_sig != 0x29 {
            // fields after ext_sig are not used - clean them
            bpb.volume_id = 0;
            bpb.volume_label = [0; 11];
            bpb.fs_type_label = [0; 8];
        }

        Ok(bpb)
    }

    fn serialize<W: Write>(&self, wrt: &mut W) -> Result<(), W::Error> {
        wrt.write_u16_le(self.bytes_per_sector)?;
        wrt.write_u8(self.sectors_per_cluster)?;
        wrt.write_u16_le(self.reserved_sectors)?;
        wrt.write_u8(self.fats)?;
        wrt.write_u16_le(self.root_entries)?;
        wrt.write_u16_le(self.total_sectors_16)?;
        wrt.write_u8(self.media)?;
        wrt.write_u16_le(self.sectors_per_fat_16)?;
        wrt.write_u16_le(self.sectors_per_track)?;
        wrt.write_u16_le(self.heads)?;
        wrt.write_u32_le(self.hidden_sectors)?;
        wrt.write_u32_le(self.total_sectors_32)?;

        if self.is_fat32() {
            wrt.write_u32_le(self.sectors_per_fat_32)?;
            wrt.write_u16_le(self.extended_flags)?;
            wrt.write_u16_le(self.fs_version)?;
            wrt.write_u32_le(self.root_dir_first_cluster)?;
            wrt.write_u16_le(self.fs_info_sector)?;
            wrt.write_u16_le(self.backup_boot_sector)?;
            wrt.write_all(&self.reserved_0)?;
        }

        wrt.write_u8(self.drive_num)?;
        wrt.write_u8(self.reserved_1)?;
        wrt.write_u8(self.ext_sig)?; // 0x29
        wrt.write_u32_le(self.volume_id)?;
        wrt.write_all(&self.volume_label)?;
        wrt.write_all(&self.fs_type_label)?;
        Ok(())
    }

    fn validate_bytes_per_sector<E: IoError>(&self) -> Result<(), Error<E>> {
        if !self.bytes_per_sector.is_power_of_two() {
            error!(
                "invalid bytes_per_sector value in BPB: expected a power of two but got {}",
                self.bytes_per_sector
            );
            return Err(Error::CorruptedFileSystem);
        }
        if self.bytes_per_sector < 512 || self.bytes_per_sector > 4096 {
            error!(
                "invalid bytes_per_sector value in BPB: expected value in range [512, 4096] but got {}",
                self.bytes_per_sector
            );
            return Err(Error::CorruptedFileSystem);
        }
        Ok(())
    }

    fn validate_sectors_per_cluster<E: IoError>(&self) -> Result<(), Error<E>> {
        if !self.sectors_per_cluster.is_power_of_two() {
            error!(
                "invalid sectors_per_cluster value in BPB: expected a power of two but got {}",
                self.sectors_per_cluster
            );
            return Err(Error::CorruptedFileSystem);
        }

        // bytes per sector is u16, sectors per cluster is u8, so guaranteed no overflow in multiplication
        let bytes_per_cluster =
            u32::from(self.bytes_per_sector) * u32::from(self.sectors_per_cluster);
        let maximum_compatibility_bytes_per_cluster: u32 = 32 * 1024;

        if bytes_per_cluster > maximum_compatibility_bytes_per_cluster {
            // 32k is the largest value to maintain greatest compatibility
            // Many implementations appear to support 64k per cluster, and some may support 128k or larger
            // However, >32k is not as thoroughly tested...
            warn!(
                "fs compatibility: bytes_per_cluster value '{}' in BPB exceeds '{}', and thus may be incompatible with some implementations",
                bytes_per_cluster, maximum_compatibility_bytes_per_cluster
            );
        }
        Ok(())
    }

    fn validate_reserved_sectors<E: IoError>(&self) -> Result<(), Error<E>> {
        let is_fat32 = self.is_fat32();
        if self.reserved_sectors < 1 {
            error!(
                "invalid reserved_sectors value in BPB: {}",
                self.reserved_sectors
            );
            return Err(Error::CorruptedFileSystem);
        }
        if !is_fat32 && self.reserved_sectors != 1 {
            // Microsoft document indicates fat12 and fat16 code exists that presume this value is 1
            warn!(
                "fs compatibility: reserved_sectors value '{}' in BPB is not '1', and thus is incompatible with some implementations",
                self.reserved_sectors
            );
        }
        if is_fat32 && self.backup_boot_sector >= self.reserved_sectors {
            error!(
                "Invalid BPB: expected backup boot-sector to be in the reserved region (sector < {}) but got sector {}",
                self.reserved_sectors, self.backup_boot_sector
            );
            return Err(Error::CorruptedFileSystem);
        }
        if is_fat32 && self.fs_info_sector >= self.reserved_sectors {
            error!(
                "Invalid BPB: expected FSInfo sector to be in the reserved region (sector < {}) but got sector {}",
                self.reserved_sectors, self.fs_info_sector
            );
            return Err(Error::CorruptedFileSystem);
        }
        Ok(())
    }

    fn validate_fats<E: IoError>(&self) -> Result<(), Error<E>> {
        if self.fats == 0 {
            error!("invalid fats value in BPB: {}", self.fats);
            return Err(Error::CorruptedFileSystem);
        }
        if self.fats > 2 {
            // Microsoft document indicates that few implementations support any values other than 1 or 2
            warn!(
                "fs compatibility: numbers of FATs '{}' in BPB is greater than '2', and thus is incompatible with some implementations",
                self.fats
            );
        }
        Ok(())
    }

    fn validate_root_entries<E: IoError>(&self) -> Result<(), Error<E>> {
        let is_fat32 = self.is_fat32();
        if is_fat32 && self.root_entries != 0 {
            error!(
                "Invalid root_entries value in FAT32 BPB: expected 0 but got {}",
                self.root_entries
            );
            return Err(Error::CorruptedFileSystem);
        }
        if !is_fat32 && self.root_entries == 0 {
            error!(
                "Invalid root_entries value in FAT12/FAT16 BPB: expected non-zero value but got {}",
                self.root_entries
            );
            return Err(Error::CorruptedFileSystem);
        }
        if (u32::from(self.root_entries) * DIR_ENTRY_SIZE) % u32::from(self.bytes_per_sector) != 0 {
            warn!("Root entries should fill sectors fully");
        }
        Ok(())
    }

    fn validate_total_sectors<E: IoError>(&self) -> Result<(), Error<E>> {
        let is_fat32 = self.is_fat32();
        if is_fat32 && self.total_sectors_16 != 0 {
            error!(
                "Invalid total_sectors_16 value in FAT32 BPB: expected 0 but got {}",
                self.total_sectors_16
            );
            return Err(Error::CorruptedFileSystem);
        }
        if self.total_sectors_16 == 0 && self.total_sectors_32 == 0 {
            error!("Invalid BPB: total_sectors_16 or total_sectors_32 should be non-zero");
            return Err(Error::CorruptedFileSystem);
        }
        if self.total_sectors_16 != 0
            && self.total_sectors_32 != 0
            && u32::from(self.total_sectors_16) != self.total_sectors_32
        {
            error!(
                "Invalid BPB: total_sectors_16 and total_sectors_32 are non-zero and have conflicting values"
            );
            return Err(Error::CorruptedFileSystem);
        }
        let total_sectors = self.total_sectors();
        // Modified for PaperPaw: summed checked, each number is the card's: overflowed, it was a
        // panic in debug builds, and wrapped, it placed the FAT over the data. Checked here, the
        // unchecked `first_data_sector` is used after.
        let first_data_sector = u32::from(self.fats)
            .checked_mul(self.sectors_per_fat())
            .and_then(|fats| fats.checked_add(self.reserved_sectors()))
            .and_then(|sectors| sectors.checked_add(self.root_dir_sectors()));
        let Some(first_data_sector) = first_data_sector else {
            error!("Invalid BPB: the FATs and the sectors before them overflow");
            return Err(Error::CorruptedFileSystem);
        };
        if total_sectors <= first_data_sector {
            error!(
                "Invalid total_sectors value in BPB: expected value > {} but got {}",
                first_data_sector, total_sectors
            );
            return Err(Error::CorruptedFileSystem);
        }
        Ok(())
    }

    fn validate_sectors_per_fat<E: IoError>(&self) -> Result<(), Error<E>> {
        let is_fat32 = self.is_fat32();
        if is_fat32 && self.sectors_per_fat_32 == 0 {
            error!(
                "Invalid sectors_per_fat_32 value in FAT32 BPB: expected non-zero value but got {}",
                self.sectors_per_fat_32
            );
            return Err(Error::CorruptedFileSystem);
        }
        Ok(())
    }

    fn validate_total_clusters<E: IoError>(&self) -> Result<(), Error<E>> {
        let is_fat32 = self.is_fat32();
        let total_clusters = self.data_clusters();
        let fat_type = FatType::from_clusters(total_clusters);
        if is_fat32 != (fat_type == FatType::Fat32) {
            error!(
                "Invalid BPB: result of FAT32 determination from total number of clusters and sectors_per_fat_16 field differs"
            );
            return Err(Error::CorruptedFileSystem);
        }
        if fat_type == FatType::Fat32 && total_clusters > 0x0FFF_FFFF {
            error!("Invalid BPB: too many clusters {}", total_clusters);
            return Err(Error::CorruptedFileSystem);
        }

        // Modified for PaperPaw: a FAT too small for the clusters (it maps fewer than the data
        // area holds) is taken as Linux takes it: the volume has the clusters the FAT maps
        // (`total_clusters`), so the search for a free cluster does not run off its end.
        if self.fat_clusters() < self.data_clusters() {
            warn!(
                "FAT is too small (allows allocation of {} clusters) compared to the total number of clusters ({}): only those are used",
                self.fat_clusters(),
                self.data_clusters()
            );
        }
        Ok(())
    }

    fn validate<E: IoError>(&self) -> Result<(), Error<E>> {
        if self.fs_version != 0 {
            error!(
                "Unsupported filesystem version: expected 0 but got {}",
                self.fs_version
            );
            return Err(Error::CorruptedFileSystem);
        }
        self.validate_bytes_per_sector()?;
        self.validate_sectors_per_cluster()?;
        self.validate_reserved_sectors()?;
        self.validate_fats()?;
        self.validate_root_entries()?;
        self.validate_total_sectors()?;
        self.validate_sectors_per_fat()?;
        self.validate_total_clusters()?;
        Ok(())
    }

    pub(crate) fn mirroring_enabled(&self) -> bool {
        self.extended_flags & 0x80 == 0
    }

    pub(crate) fn active_fat(&self) -> u16 {
        // The zero-based number of the active FAT is only valid if mirroring is disabled.
        if self.mirroring_enabled() {
            0
        } else {
            self.extended_flags & 0x0F
        }
    }

    pub(crate) fn status_flags(&self) -> FsStatusFlags {
        FsStatusFlags::decode(self.reserved_1)
    }

    pub(crate) fn is_fat32(&self) -> bool {
        // because this field must be zero on FAT32, and
        // because it must be non-zero on FAT12/FAT16,
        // this provides a simple way to detect FAT32
        self.sectors_per_fat_16 == 0
    }

    pub(crate) fn sectors_per_fat(&self) -> u32 {
        if self.is_fat32() {
            self.sectors_per_fat_32
        } else {
            u32::from(self.sectors_per_fat_16)
        }
    }

    pub(crate) fn total_sectors(&self) -> u32 {
        if self.total_sectors_16 == 0 {
            self.total_sectors_32
        } else {
            u32::from(self.total_sectors_16)
        }
    }

    pub(crate) fn reserved_sectors(&self) -> u32 {
        u32::from(self.reserved_sectors)
    }

    pub(crate) fn root_dir_sectors(&self) -> u32 {
        let root_dir_bytes = u32::from(self.root_entries) * DIR_ENTRY_SIZE;
        (root_dir_bytes + u32::from(self.bytes_per_sector) - 1) / u32::from(self.bytes_per_sector)
    }

    pub(crate) fn sectors_per_all_fats(&self) -> u32 {
        u32::from(self.fats) * self.sectors_per_fat()
    }

    pub(crate) fn first_data_sector(&self) -> u32 {
        let root_dir_sectors = self.root_dir_sectors();
        let fat_sectors = self.sectors_per_all_fats();
        self.reserved_sectors() + fat_sectors + root_dir_sectors
    }

    /// The clusters the data area holds; what the FAT's type is told by
    /// (`FatType::from_clusters`).
    pub(crate) fn data_clusters(&self) -> u32 {
        let total_sectors = self.total_sectors();
        let first_data_sector = self.first_data_sector();
        let data_sectors = total_sectors - first_data_sector;
        data_sectors / u32::from(self.sectors_per_cluster)
    }

    /// The clusters the FAT has an entry for (counted in 64 bits: the
    /// product of the card's numbers overflows `u32`).
    fn fat_clusters(&self) -> u32 {
        let bits_per_fat_entry =
            u64::from(FatType::from_clusters(self.data_clusters()).bits_per_fat_entry());
        let entries = u64::from(self.sectors_per_fat()) * u64::from(self.bytes_per_sector) * 8
            / bits_per_fat_entry;
        let usable = entries.saturating_sub(u64::from(RESERVED_FAT_ENTRIES));
        u32::try_from(usable).unwrap_or(u32::MAX)
    }

    /// The clusters of the volume: those of the data area the FAT maps.
    // Modified for PaperPaw: at most as many as the FAT has entries for (see
    // `validate_total_clusters`).
    pub(crate) fn total_clusters(&self) -> u32 {
        self.data_clusters().min(self.fat_clusters())
    }

    pub(crate) fn bytes_from_sectors(&self, sectors: u32) -> u64 {
        // Note: total number of sectors is a 32 bit number so offsets have to be 64 bit
        u64::from(sectors) * u64::from(self.bytes_per_sector)
    }

    pub(crate) fn sectors_from_clusters(&self, clusters: u32) -> u32 {
        // Note: total number of sectors is a 32 bit number so it should not overflow
        clusters * u32::from(self.sectors_per_cluster)
    }

    pub(crate) fn cluster_size(&self) -> u32 {
        u32::from(self.sectors_per_cluster) * u32::from(self.bytes_per_sector)
    }

    pub(crate) fn clusters_from_bytes(&self, bytes: u64) -> u32 {
        let cluster_size = u64::from(self.cluster_size());
        ((bytes + cluster_size - 1) / cluster_size) as u32
    }

    pub(crate) fn fs_info_sector(&self) -> u32 {
        u32::from(self.fs_info_sector)
    }

    pub(crate) fn backup_boot_sector(&self) -> u32 {
        u32::from(self.backup_boot_sector)
    }
}

pub(crate) struct BootSector {
    pub(crate) bootjmp: [u8; 3],
    pub(crate) oem_name: [u8; 8],
    pub(crate) bpb: BiosParameterBlock,
    pub(crate) boot_code: [u8; 448],
    pub(crate) boot_sig: [u8; 2],
}

impl BootSector {
    pub(crate) fn deserialize<R: Read>(rdr: &mut R) -> Result<Self, R::Error> {
        let mut boot = Self::default();
        rdr.read_exact(&mut boot.bootjmp)?;
        rdr.read_exact(&mut boot.oem_name)?;
        boot.bpb = BiosParameterBlock::deserialize(rdr)?;

        if boot.bpb.is_fat32() {
            rdr.read_exact(&mut boot.boot_code[0..420])?;
        } else {
            rdr.read_exact(&mut boot.boot_code[0..448])?;
        }
        rdr.read_exact(&mut boot.boot_sig)?;
        Ok(boot)
    }

    pub(crate) fn serialize<W: Write>(&self, wrt: &mut W) -> Result<(), W::Error> {
        wrt.write_all(&self.bootjmp)?;
        wrt.write_all(&self.oem_name)?;
        self.bpb.serialize(&mut *wrt)?;

        if self.bpb.is_fat32() {
            wrt.write_all(&self.boot_code[0..420])?;
        } else {
            wrt.write_all(&self.boot_code[0..448])?;
        }
        wrt.write_all(&self.boot_sig)?;
        Ok(())
    }

    pub(crate) fn validate<E: IoError>(&self, strict: bool) -> Result<(), Error<E>> {
        if strict && self.boot_sig != [0x55, 0xAA] {
            error!(
                "Invalid boot sector signature: expected [0x55, 0xAA] but got {:?}",
                self.boot_sig
            );
            return Err(Error::CorruptedFileSystem);
        }
        if strict && self.bootjmp[0] != 0xEB && self.bootjmp[0] != 0xE9 {
            warn!(
                "Unknown opcode {:x} in bootjmp boot sector field",
                self.bootjmp[0]
            );
        }
        self.bpb.validate()?;
        Ok(())
    }
}

impl Default for BootSector {
    fn default() -> Self {
        Self {
            bootjmp: Default::default(),
            oem_name: Default::default(),
            bpb: BiosParameterBlock::default(),
            boot_code: [0; 448],
            boot_sig: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{FormatVolumeOptions, format_volume};
    use crate::io::{Seek, SeekFrom, StdIoWrapper};
    use std::io::Cursor;

    #[test]
    fn a_boot_sector_whose_sums_overflow_is_refused_and_a_small_fat_bounds_the_clusters() {
        let mut disk = StdIoWrapper::new(Cursor::new(vec![0u8; 40 << 20]));
        format_volume(
            &mut disk,
            FormatVolumeOptions::new()
                .fat_type(FatType::Fat32)
                .bytes_per_cluster(512),
        )
        .unwrap();
        disk.seek(SeekFrom::Start(0)).unwrap();
        let formatted = BootSector::deserialize(&mut disk).unwrap();
        assert!(formatted.validate::<()>(true).is_ok());
        // The FATs' sectors overflow.
        let mut boot = BootSector {
            bpb: formatted.bpb.clone(),
            boot_sig: formatted.boot_sig,
            ..BootSector::default()
        };
        boot.bpb.fats = 255;
        boot.bpb.sectors_per_fat_32 = 0x0100_0000;
        assert!(matches!(
            boot.validate::<()>(true),
            Err(Error::CorruptedFileSystem)
        ));
        // The FAT's bits overflow `u32` (2^27 entries): counted in 64 bits,
        // they bound the clusters.
        let mut boot = BootSector {
            bpb: formatted.bpb.clone(),
            boot_sig: formatted.boot_sig,
            ..BootSector::default()
        };
        boot.bpb.fats = 1;
        boot.bpb.sectors_per_fat_32 = 1 << 20;
        boot.bpb.sectors_per_cluster = 16;
        boot.bpb.total_sectors_32 = u32::MAX;
        assert!(boot.validate::<()>(true).is_ok());
        assert_eq!(boot.bpb.total_clusters(), (1 << 27) - 2);
        // One sector of FAT (126 clusters of FAT32 entries) for many more
        // clusters: the volume is read, with the clusters the FAT maps.
        let mut small = formatted.bpb.clone();
        small.sectors_per_fat_32 = 1;
        let boot = BootSector {
            bpb: small,
            boot_sig: formatted.boot_sig,
            ..BootSector::default()
        };
        assert!(boot.validate::<()>(true).is_ok());
        assert!(boot.bpb.data_clusters() > 126);
        assert_eq!(boot.bpb.total_clusters(), 126);
    }
}

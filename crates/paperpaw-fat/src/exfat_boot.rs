//! Why an exFAT volume could not be mounted: [`MountFailure`], told in
//! the log so that a card that fails says why without a computer.

use alloc::string::String;
use core::fmt;

use crate::disk::{BLOCK, Blocks, SharedDisk};

/// What an exFAT boot sector says about the volume's sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootSizes {
    /// `VolumeLength`, in sectors.
    pub volume_sectors: u64,
    /// `BytesPerSectorShift` (9 to 12).
    pub sector_shift: u8,
    /// `SectorsPerClusterShift`.
    pub cluster_shift: u8,
    /// `FatOffset` and `FatLength`, in sectors.
    pub fat: (u32, u32),
    /// `ClusterHeapOffset`, in sectors.
    pub heap: u32,
    /// `ClusterCount`.
    pub clusters: u32,
    /// `NumberOfFats`.
    pub fats: u8,
    /// `VolumeFlags`.
    pub flags: u16,
    /// `FileSystemRevision` (major in the high byte).
    pub revision: u16,
}

impl BootSizes {
    /// The sizes in `sector`, a volume's first; none where it does not
    /// name exFAT.
    pub fn read(sector: &[u8; BLOCK]) -> Option<Self> {
        if sector.get(3..11) != Some(b"EXFAT   ".as_slice()) {
            return None;
        }
        let u16_at = |at: usize| u16::from_le_bytes([sector[at], sector[at + 1]]);
        let u32_at = |at: usize| {
            u32::from_le_bytes([sector[at], sector[at + 1], sector[at + 2], sector[at + 3]])
        };
        let u64_at = |at: usize| u64::from(u32_at(at)) | (u64::from(u32_at(at + 4)) << 32);
        Some(Self {
            volume_sectors: u64_at(72),
            sector_shift: sector[108],
            cluster_shift: sector[109],
            fat: (u32_at(80), u32_at(84)),
            heap: u32_at(88),
            clusters: u32_at(92),
            fats: sector[110],
            flags: u16_at(106),
            revision: u16_at(104),
        })
    }

    /// The sizes in the first block of `disk`; none where it cannot be
    /// read or does not name exFAT.
    pub(crate) fn of<B: Blocks>(disk: &SharedDisk<B>) -> Option<Self> {
        let mut sector = [0u8; BLOCK];
        disk.read_at(0, &mut sector).ok()?;
        Self::read(&sector)
    }

    /// The volume's length in bytes (`u64::MAX` where it overflows).
    pub fn volume_bytes(&self) -> u64 {
        self.volume_sectors
            .checked_shl(u32::from(self.sector_shift))
            .filter(|bytes| bytes >> self.sector_shift == self.volume_sectors)
            .unwrap_or(u64::MAX)
    }
}

/// Why an exFAT volume could not be mounted: the driver's error, the
/// boot sector's sizes and the device's, so that the log of a card that
/// fails names the reason (e.g. a volume longer than the card).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountFailure {
    /// The driver's error, as it tells it.
    pub error: String,
    /// What the boot sector says; none where it could not be read or
    /// does not name exFAT.
    pub boot: Option<BootSizes>,
    /// Blocks of 512 bytes the device (the partition) holds.
    pub device_blocks: u64,
}

impl MountFailure {
    /// The failure `error` of mounting the volume on `disk`, with what its
    /// boot sector says.
    pub(crate) fn new<B: Blocks>(error: String, disk: &SharedDisk<B>) -> Self {
        Self {
            error,
            boot: BootSizes::of(disk),
            device_blocks: disk.blocks(),
        }
    }

    /// Whether the boot sector names a volume longer than the device.
    pub fn longer_than_device(&self) -> bool {
        self.boot
            .is_some_and(|b| b.volume_bytes() > self.device_blocks.saturating_mul(BLOCK as u64))
    }
}

impl fmt::Display for MountFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)?;
        if self.longer_than_device() {
            f.write_str(" (the volume is longer than the card)")?;
        }
        write!(f, "; device {} blocks", self.device_blocks)?;
        match self.boot {
            Some(b) => write!(
                f,
                "; boot: volume {} sectors of {} B, FAT at {} ({} sectors) x{}, heap at {}, {} clusters of {} sectors, flags {:#x}, revision {:#x}",
                b.volume_sectors,
                1u32.checked_shl(u32::from(b.sector_shift)).unwrap_or(0),
                b.fat.0,
                b.fat.1,
                b.fats,
                b.heap,
                b.clusters,
                1u64.checked_shl(u32::from(b.cluster_shift)).unwrap_or(0),
                b.flags,
                b.revision,
            ),
            None => f.write_str("; no exFAT boot sector read"),
        }
    }
}

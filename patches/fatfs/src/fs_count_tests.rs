//! Mutations of corrupt chains must not over-report free clusters.
use super::*;
use crate::io::StdIoWrapper;
use std::io::Cursor;

type TestFs = FileSystem<StdIoWrapper<Cursor<Vec<u8>>>>;

fn filesystem() -> TestFs {
    let mut disk = StdIoWrapper::new(Cursor::new(vec![0u8; 8 << 20]));
    format_volume(&mut disk, FormatVolumeOptions::new().fat_type(FatType::Fat16)).unwrap();
    disk.seek(SeekFrom::Start(0)).unwrap();
    FileSystem::new(disk, FsOptions::new()).unwrap()
}

fn set_entry(fs: &TestFs, cluster: u32, value: u16) {
    let mut fat = fs.fat_slice();
    fat.seek(SeekFrom::Start(u64::from(cluster) * 2)).unwrap();
    fat.write_all(&value.to_le_bytes()).unwrap();
}

#[test]
fn freeing_a_link_into_free_space_does_not_double_count() {
    let fs = filesystem();
    set_entry(&fs, 2, 3);
    let before = fs.recalc_free_clusters().unwrap();
    fs.free_cluster_chain(2).unwrap();
    assert_eq!(fs.cluster_changes(), (0, 1));
    assert_eq!(fs.stats().unwrap().free_clusters(), before + 1);
    assert_eq!(fs.recalc_free_clusters().unwrap(), before + 1);
    // A second file cross-linked into that freed chain.
    set_entry(&fs, 4, 2);
    let before = fs.recalc_free_clusters().unwrap();
    fs.free_cluster_chain(4).unwrap();
    assert_eq!(fs.cluster_changes(), (0, 2));
    assert_eq!(fs.stats().unwrap().free_clusters(), before + 1);
    assert_eq!(fs.recalc_free_clusters().unwrap(), before + 1);
}

#[test]
fn truncating_an_already_free_cluster_records_it_as_taken() {
    let fs = filesystem();
    let before = fs.recalc_free_clusters().unwrap();
    fs.truncate_cluster_chain(2).unwrap();
    assert_eq!(fs.cluster_changes(), (1, 0));
    assert_eq!(fs.stats().unwrap().free_clusters(), before - 1);
    assert_eq!(fs.recalc_free_clusters().unwrap(), before - 1);
    fs.truncate_cluster_chain(2).unwrap();
    assert_eq!(fs.cluster_changes(), (1, 0));
}

/// The first FAT copy accepts writes; the mirror fails.
struct FailMirror(Cursor<Vec<u8>>);

impl std::io::Read for FailMirror {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.0, bytes)
    }
}

impl std::io::Seek for FailMirror {
    fn seek(&mut self, at: std::io::SeekFrom) -> std::io::Result<u64> {
        std::io::Seek::seek(&mut self.0, at)
    }
}

impl std::io::Write for FailMirror {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.position() >= 64 {
            return Err(std::io::Error::other("mirror write failed"));
        }
        std::io::Write::write(&mut self.0, bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_failed_mirror_allocation_keeps_the_count_on_the_low_side() {
    let mut disk = StdIoWrapper::new(FailMirror(Cursor::new(vec![0u8; 128])));
    let mut fat = DiskSlice::<_, StdIoWrapper<FailMirror>>::new(0, 64, 2, &mut disk);
    let before = crate::table::count_free_clusters::<_, std::io::Error>(&mut fat, FatType::Fat16, 30).unwrap();
    let taken = Cell::new(0);
    assert!(
        crate::table::alloc_cluster::<_, std::io::Error>(&mut fat, FatType::Fat16, None, None, 30, &taken).is_err()
    );
    let actual = crate::table::count_free_clusters::<_, std::io::Error>(&mut fat, FatType::Fat16, 30).unwrap();
    assert_eq!(actual, before - 1);
    assert_eq!(taken.get(), 1);
    assert!(u64::from(before).saturating_sub(taken.get()) <= u64::from(actual));
}

#[test]
fn a_failed_mirror_truncate_counts_the_cluster_as_taken() {
    let mut disk = StdIoWrapper::new(FailMirror(Cursor::new(vec![0u8; 128])));
    let mut fat = DiskSlice::<_, StdIoWrapper<FailMirror>>::new(0, 64, 2, &mut disk);
    let mut iter =
        ClusterIterator::<_, std::io::Error, DiskSlice<_, StdIoWrapper<FailMirror>>>::new(&mut fat, FatType::Fat16, 2);
    assert!(iter.truncate().is_err());
    assert_eq!((iter.taken(), iter.freed()), (1, 0));
}

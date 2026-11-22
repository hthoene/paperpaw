//! Mutations of corrupt chains must not over-report free clusters.
use super::*;
use crate::format::{FormatVolumeOptions, format_volume};
use crate::io::StdIoWrapper;
use std::io::Cursor;

type TestFs = FileSystem<StdIoWrapper<Cursor<Vec<u8>>>>;

fn filesystem() -> TestFs {
    let mut disk = StdIoWrapper::new(Cursor::new(vec![0u8; 8 << 20]));
    format_volume(
        &mut disk,
        FormatVolumeOptions::new().fat_type(FatType::Fat16),
    )
    .unwrap();
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
    let before =
        crate::table::count_free_clusters::<_, std::io::Error>(&mut fat, FatType::Fat16, 30)
            .unwrap();
    let taken = Cell::new(0);
    assert!(
        crate::table::alloc_cluster::<_, std::io::Error>(
            &mut fat,
            FatType::Fat16,
            None,
            None,
            30,
            &taken
        )
        .is_err()
    );
    let actual =
        crate::table::count_free_clusters::<_, std::io::Error>(&mut fat, FatType::Fat16, 30)
            .unwrap();
    assert_eq!(actual, before - 1);
    assert_eq!(taken.get(), 1);
    assert!(u64::from(before).saturating_sub(taken.get()) <= u64::from(actual));
}

#[test]
fn a_failed_mirror_truncate_counts_the_cluster_as_taken() {
    let mut disk = StdIoWrapper::new(FailMirror(Cursor::new(vec![0u8; 128])));
    let mut fat = DiskSlice::<_, StdIoWrapper<FailMirror>>::new(0, 64, 2, &mut disk);
    let mut iter =
        ClusterIterator::<_, std::io::Error, DiskSlice<_, StdIoWrapper<FailMirror>>>::new(
            &mut fat,
            FatType::Fat16,
            2,
            30,
        );
    assert!(iter.truncate().is_err());
    assert_eq!((iter.taken(), iter.freed()), (1, 0));
}

/// The bytes of the first FAT.
fn fat_bytes(fs: &TestFs) -> Vec<u8> {
    let mut fat = fs.fat_slice();
    let mut bytes = vec![0u8; fs.bpb.bytes_from_sectors(fs.bpb.sectors_per_fat()) as usize];
    fat.read_exact(&mut bytes).unwrap();
    bytes
}

/// File `name` in the root, one cluster of data, made to begin at `first`.
fn file_at<'a>(
    fs: &'a TestFs,
    name: &str,
    first: u32,
) -> File<'a, StdIoWrapper<Cursor<Vec<u8>>>, DefaultTimeProvider, LossyOemCpConverter> {
    let root = fs.root_dir();
    let mut file = root.create_file(name).unwrap();
    file.write_all(&[7; 100]).unwrap();
    drop(file);
    let entry = root
        .iter()
        .map(Result::unwrap)
        .find(|e| e.file_name() == name)
        .unwrap();
    entry.set_first_cluster(Some(first)).unwrap();
    root.open_file(name).unwrap()
}

#[test]
fn a_cluster_number_out_of_the_volume_is_an_error_not_a_place_on_the_disk() {
    let fs = filesystem();
    let past = fs.total_clusters() + RESERVED_FAT_ENTRIES;
    for (n, first) in [1, past].into_iter().enumerate() {
        let mut file = file_at(&fs, &format!("first{n}.txt"), first);
        let fat = fat_bytes(&fs);
        assert!(matches!(
            file.write(&[1; 10]),
            Err(Error::CorruptedFileSystem)
        ));
        assert!(matches!(
            file.read(&mut [0; 10]),
            Err(Error::CorruptedFileSystem)
        ));
        assert_eq!(fat_bytes(&fs), fat, "the FAT stays as it was");
    }
    // A link in the FAT to no cluster: the second cluster is not written.
    for (n, next) in [1, past].into_iter().enumerate() {
        let mut file = fs.root_dir().create_file(&format!("link{n}.txt")).unwrap();
        let size = fs.cluster_size() as usize;
        file.write_all(&vec![7; 2 * size]).unwrap();
        let first = file.first_cluster().unwrap();
        set_entry(&fs, first, next as u16);
        file.seek(SeekFrom::Start(0)).unwrap();
        let fat = fat_bytes(&fs);
        let mut buf = vec![0; 2 * size];
        assert!(matches!(
            file.read_exact(&mut buf),
            Err(Error::CorruptedFileSystem)
        ));
        file.seek(SeekFrom::Start(0)).unwrap();
        assert!(matches!(
            file.write_all(&buf),
            Err(Error::CorruptedFileSystem)
        ));
        assert_eq!(fat_bytes(&fs), fat, "the FAT stays as it was");
    }
}

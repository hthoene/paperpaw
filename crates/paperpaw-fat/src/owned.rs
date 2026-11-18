//! The allocation behind an owning FAT storage. rust-fatfs handles
//! borrow their file system; our private caches need that borrow across
//! calls. The allocation stays in place until all those caches are gone.

use alloc::boxed::Box;
use core::marker::PhantomData;
use core::ptr::NonNull;

use fatfs::{FileSystem, ReadWriteSeek};

use crate::FatStorage;

pub(super) struct FileSystemOwner<IO: ReadWriteSeek> {
    pointer: NonNull<FileSystem<IO>>,
    allocation: PhantomData<Box<FileSystem<IO>>>,
}

pub(super) fn storage<IO: ReadWriteSeek + 'static>(fs: FileSystem<IO>) -> FatStorage<IO> {
    let pointer = Box::into_raw(Box::new(fs));
    // SAFETY: Box::into_raw returns a non-null, aligned allocation. It
    // stays at this address, owned only by FileSystemOwner below.
    let owner = FileSystemOwner {
        pointer: unsafe { NonNull::new_unchecked(pointer) },
        allocation: PhantomData,
    };
    // SAFETY: the extended borrow is confined to FatStorage and its
    // private handles. Its public API exposes neither. The owner is the
    // last field, so every handle drops before the allocation. Keeping a
    // raw owner avoids moving or mutably borrowing a Box while its
    // contents have shared references. The only reconstruction is below,
    // after those references can no longer be used.
    let mut storage = FatStorage::new(unsafe { &*pointer });
    storage.owner = Some(owner);
    storage
}

impl<IO: ReadWriteSeek> Drop for FileSystemOwner<IO> {
    fn drop(&mut self) {
        // SAFETY: this is the sole owner of the original Box allocation.
        // FatStorage's fields are dropped in declaration order: all
        // handles borrowing it are gone before this last field drops.
        unsafe { drop(Box::from_raw(self.pointer.as_ptr())) };
    }
}

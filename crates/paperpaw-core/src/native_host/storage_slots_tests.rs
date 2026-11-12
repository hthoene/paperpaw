use super::*;
use alloc::rc::Rc;
use core::{cell::Cell, mem::MaybeUninit, pin::pin};
use paperpaw_native::host_glue::{ERR_FULL, ERR_IO, ERR_NOT_FOUND, ERR_UTF8};
use paperpaw_platform::{DirEntry, Instant, Storage, StorageError};
use paperpaw_sdk::MemoryStorage;

const OWNER: &str = "battery";
const FILE: &str = ".paperpaw/plugins/battery/state";
const FOREIGN: &str = ".paperpaw/plugins/other/state";

#[derive(Clone, Default)]
pub(super) struct Backend {
    pub(super) files: Rc<MemoryStorage>,
    free: Rc<Cell<Option<u64>>>,
    pub(super) fail: Rc<Cell<Option<StorageError>>>,
    bad_count: Rc<Cell<bool>>,
    partial_error: Rc<Cell<bool>>,
    /// The adapter writes over the whole buffer before a good read.
    scribble: Rc<Cell<bool>>,
}

impl Backend {
    fn check(&self) -> Result<(), StorageError> {
        self.fail.get().map_or(Ok(()), Err)
    }
}

impl Storage for Backend {
    fn read_at(&self, path: &str, offset: u64, out: &mut [u8]) -> Result<usize, StorageError> {
        self.check()?;
        if self.bad_count.get() {
            out.fill(0xee);
            return Ok(out.len() + 1);
        }
        if self.partial_error.get() {
            if let Some(first) = out.first_mut() {
                *first = 0xee;
            }
            return Err(StorageError::Io);
        }
        if self.scribble.get() {
            out.fill(0xee);
        }
        self.files.read_at(path, offset, out)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.check()?;
        self.files.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.check()?;
        self.files.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.check()?;
        self.files.append(path, data)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.check()?;
        self.files.rename(from, to)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.check()?;
        self.files.remove(path)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.check()
    }
    fn list(&self, path: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(path, visit)
    }
    fn free_space(&self) -> Option<u64> {
        self.free.get()
    }
}

pub(super) fn bytes(value: &[u8]) -> PpBytes {
    PpBytes {
        ptr: value.as_ptr(),
        len: value.len(),
    }
}
pub(super) fn output(value: &mut [u8]) -> PpBuffer {
    PpBuffer {
        ptr: value.as_mut_ptr(),
        cap: value.len(),
    }
}
pub(super) fn scoped(backend: &Backend, call: impl FnOnce(&EnvironmentScope, &crate::Kernel)) {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .storage(backend.clone())
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let scope = pin!(EnvironmentScope::new());
    scope
        .as_ref()
        .scope(&env, || call(&scope, &kernel))
        .unwrap();
}

#[test]
fn raw_signatures_match_generated_slot_arguments_after_scope_and_owner() {
    let _: unsafe fn(&EnvironmentScope, &str, Option<Format<'_>>, u32, PpBytes, *mut u64) -> i32 =
        size;
    let _: unsafe fn(
        &EnvironmentScope,
        &str,
        Option<Format<'_>>,
        u32,
        PpBytes,
        u64,
        PpBuffer,
    ) -> i32 = read_at;
    let _: unsafe fn(
        &EnvironmentScope,
        &str,
        Option<Format<'_>>,
        &PathsInUse,
        u32,
        PpBytes,
        PpBytes,
    ) -> i32 = write;
    let _: unsafe fn(
        &EnvironmentScope,
        &str,
        Option<Format<'_>>,
        &PathsInUse,
        u32,
        PpBytes,
        PpBytes,
    ) -> i32 = append;
    let _: unsafe fn(
        &EnvironmentScope,
        &str,
        Option<Format<'_>>,
        &PathsInUse,
        u32,
        PpBytes,
        PpBytes,
    ) -> i32 = rename;
    let _: unsafe fn(
        &EnvironmentScope,
        &str,
        Option<Format<'_>>,
        &PathsInUse,
        u32,
        PpBytes,
    ) -> i32 = remove;
    let _: fn(&EnvironmentScope, &str) -> i32 = sync;
}

#[test]
#[allow(clippy::too_many_lines, reason = "one walk through all seven slots")]
fn all_slots_use_the_scoped_storage_and_only_the_owner_folder() {
    let backend = Backend::default();
    backend.files.write(FOREIGN, b"foreign").unwrap();
    scoped(&backend, |scope, kernel| {
        let _running = kernel.shared.meter.enter(OWNER);
        let mut len = 91;
        // SAFETY: inputs and output are live, exclusive and disjoint.
        unsafe {
            assert_eq!(
                size(scope, OWNER, None, 0, bytes(b"state"), &mut len),
                ERR_NOT_FOUND
            );
            assert_eq!(len, 91);
            assert_eq!(
                write(
                    scope,
                    OWNER,
                    None,
                    &PathsInUse::default(),
                    0,
                    bytes(b"state"),
                    bytes(b"abc")
                ),
                OK
            );
            assert_eq!(
                append(
                    scope,
                    OWNER,
                    None,
                    &PathsInUse::default(),
                    0,
                    bytes(b"state"),
                    bytes(b"def")
                ),
                OK
            );
            assert_eq!(size(scope, OWNER, None, 0, bytes(b"state"), &mut len), OK);
            assert_eq!(len, 6);
            let mut out = [0xa5; 8];
            assert_eq!(
                read_at(scope, OWNER, None, 0, bytes(b"state"), 2, output(&mut out)),
                4
            );
            assert_eq!(&out, b"cdef\0\0\0\0");
            assert_eq!(
                read_at(
                    scope,
                    OWNER,
                    None,
                    0,
                    bytes(b"state"),
                    u64::MAX,
                    output(&mut out)
                ),
                0
            );
            assert_eq!(
                rename(
                    scope,
                    OWNER,
                    None,
                    &PathsInUse::default(),
                    0,
                    bytes(b"state"),
                    bytes(b"moved")
                ),
                OK
            );
            assert_eq!(
                remove(
                    scope,
                    OWNER,
                    None,
                    &PathsInUse::default(),
                    0,
                    bytes(b"moved")
                ),
                OK
            );
            assert_eq!(
                remove(
                    scope,
                    OWNER,
                    None,
                    &PathsInUse::default(),
                    0,
                    bytes(b"moved")
                ),
                OK
            );
            assert_eq!(
                write(
                    scope,
                    OWNER,
                    None,
                    &PathsInUse::default(),
                    0,
                    bytes(b"state"),
                    bytes(b"")
                ),
                OK
            );
        }
        assert_eq!(sync(scope, OWNER), OK);
        let rows = kernel.shared.meter.table.borrow();
        let row = rows
            .iter()
            .find(|row| row.plugin.as_str() == OWNER)
            .unwrap();
        assert_eq!((row.read, row.written), (4, 6));
        assert_eq!(kernel.shared.meter.syncs_of(OWNER), 1);
    });
    assert_eq!(backend.files.get(FILE).unwrap(), b"");
    assert_eq!(backend.files.get(FOREIGN).unwrap(), b"foreign");
}

#[test]
#[allow(clippy::too_many_lines, reason = "one table of refusals")]
fn malformed_paths_utf8_and_reserved_roots_leave_files_unchanged() {
    let backend = Backend::default();
    backend.files.write(FILE, b"keep").unwrap();
    scoped(&backend, |scope, _| {
        for bad in [
            &b"../state"[..],
            b"/state",
            b"a//state",
            b"a/./state",
            b"a\\state",
            b"",
            b"a\0b",
            b"a:",
            b"con",
            b"state/",
        ] {
            // SAFETY: inputs are live; malformed text is rejected before storage.
            unsafe {
                assert_eq!(
                    write(
                        scope,
                        OWNER,
                        None,
                        &PathsInUse::default(),
                        0,
                        bytes(bad),
                        bytes(b"new")
                    ),
                    ERR_INVALID
                );
                assert_eq!(
                    append(
                        scope,
                        OWNER,
                        None,
                        &PathsInUse::default(),
                        0,
                        bytes(bad),
                        bytes(b"new")
                    ),
                    ERR_INVALID
                );
                assert_eq!(
                    remove(scope, OWNER, None, &PathsInUse::default(), 0, bytes(bad)),
                    ERR_INVALID
                );
                assert_eq!(
                    rename(
                        scope,
                        OWNER,
                        None,
                        &PathsInUse::default(),
                        0,
                        bytes(b"state"),
                        bytes(bad)
                    ),
                    ERR_INVALID
                );
                assert_eq!(
                    rename(
                        scope,
                        OWNER,
                        None,
                        &PathsInUse::default(),
                        0,
                        bytes(bad),
                        bytes(b"state")
                    ),
                    ERR_INVALID
                );
            }
        }
        // SAFETY: byte arrays and output are live and disjoint.
        unsafe {
            let mut len = 91;
            assert_eq!(
                size(scope, OWNER, None, 0, bytes(&[0xff]), &mut len),
                ERR_UTF8
            );
            assert_eq!(
                write(
                    scope,
                    OWNER,
                    None,
                    &PathsInUse::default(),
                    0,
                    bytes(&[0xff]),
                    bytes(b"new")
                ),
                ERR_UTF8
            );
            assert_eq!(len, 91);
            for root in [1, 2, u32::MAX] {
                let mut out = [0xa5; 1];
                assert_eq!(
                    size(scope, OWNER, None, root, bytes(b"state"), &mut len),
                    ERR_UNSUPPORTED
                );
                assert_eq!(
                    read_at(
                        scope,
                        OWNER,
                        None,
                        root,
                        bytes(b"state"),
                        0,
                        output(&mut out)
                    ),
                    ERR_UNSUPPORTED
                );
                assert_eq!(
                    write(
                        scope,
                        OWNER,
                        None,
                        &PathsInUse::default(),
                        root,
                        bytes(b"state"),
                        bytes(b"new")
                    ),
                    ERR_UNSUPPORTED
                );
                assert_eq!(
                    append(
                        scope,
                        OWNER,
                        None,
                        &PathsInUse::default(),
                        root,
                        bytes(b"state"),
                        bytes(b"new")
                    ),
                    ERR_UNSUPPORTED
                );
                assert_eq!(
                    rename(
                        scope,
                        OWNER,
                        None,
                        &PathsInUse::default(),
                        root,
                        bytes(b"state"),
                        bytes(b"new")
                    ),
                    ERR_UNSUPPORTED
                );
                assert_eq!(
                    remove(
                        scope,
                        OWNER,
                        None,
                        &PathsInUse::default(),
                        root,
                        bytes(b"state")
                    ),
                    ERR_UNSUPPORTED
                );
                assert_eq!(out, [0xa5]);
            }
        }
    });
    assert_eq!(backend.files.get(FILE).unwrap(), b"keep");
}

#[test]
#[allow(clippy::too_many_lines, reason = "one table of refusals")]
fn bad_pairs_and_bounds_are_rejected_before_storage_or_output() {
    let backend = Backend::default();
    backend.files.write(FILE, b"keep").unwrap();
    scoped(&backend, |scope, _| {
        let null = PpBytes {
            ptr: core::ptr::null(),
            len: 1,
        };
        let long = PpBytes {
            ptr: b"x".as_ptr(),
            len: MAX_PATH + 1,
        };
        let big = PpBytes {
            ptr: b"x".as_ptr(),
            len: MAX_DATA + 1,
        };
        let overflow = PpBytes {
            ptr: usize::MAX as *const u8,
            len: 1,
        };
        let mut out = [0xa5; 8];
        let mut scalar = [91u64; 2];
        // SAFETY: valid pairs name live disjoint storage. Malformed pairs are
        // deliberately checked before dereferencing their invalid pointers.
        unsafe {
            for bad in [null, overflow] {
                assert_eq!(
                    write(
                        scope,
                        OWNER,
                        None,
                        &PathsInUse::default(),
                        0,
                        bad,
                        bytes(b"new")
                    ),
                    ERR_INVALID
                );
            }
            assert_eq!(
                write(
                    scope,
                    OWNER,
                    None,
                    &PathsInUse::default(),
                    0,
                    long,
                    bytes(b"new")
                ),
                ERR_NO_SPACE
            );
            assert_eq!(
                write(
                    scope,
                    OWNER,
                    None,
                    &PathsInUse::default(),
                    0,
                    bytes(b"state"),
                    big
                ),
                ERR_NO_SPACE
            );
            assert_eq!(
                append(
                    scope,
                    OWNER,
                    None,
                    &PathsInUse::default(),
                    0,
                    bytes(b"state"),
                    big
                ),
                ERR_NO_SPACE
            );
            assert_eq!(
                write(
                    scope,
                    OWNER,
                    None,
                    &PathsInUse::default(),
                    0,
                    bytes(b"state"),
                    null
                ),
                ERR_INVALID
            );
            assert_eq!(
                size(
                    scope,
                    OWNER,
                    None,
                    0,
                    bytes(b"state"),
                    core::ptr::null_mut()
                ),
                ERR_INVALID
            );
            let unaligned = scalar.as_mut_ptr().cast::<u8>().add(1).cast();
            assert_eq!(
                size(scope, OWNER, None, 0, bytes(b"state"), unaligned),
                ERR_INVALID
            );
            assert_eq!(
                read_at(
                    scope,
                    OWNER,
                    None,
                    0,
                    bytes(b"state"),
                    0,
                    PpBuffer {
                        ptr: out.as_mut_ptr(),
                        cap: MAX_DATA + 1
                    }
                ),
                ERR_NO_SPACE
            );
            assert_eq!(
                read_at(
                    scope,
                    OWNER,
                    None,
                    0,
                    bytes(b"state"),
                    0,
                    PpBuffer {
                        ptr: core::ptr::null_mut(),
                        cap: 1
                    }
                ),
                ERR_INVALID
            );
            let empty = PpBuffer {
                ptr: core::ptr::null_mut(),
                cap: 0,
            };
            assert_eq!(read_at(scope, OWNER, None, 0, bytes(b"state"), 0, empty), 0);
        }
        assert_eq!(out, [0xa5; 8]);
        assert_eq!(scalar, [91; 2]);
    });
    assert_eq!(backend.files.get(FILE).unwrap(), b"keep");
}

#[test]
fn reserve_and_read_errors_preserve_files_and_zero_the_output() {
    let backend = Backend::default();
    // Stay above the 4096-byte recall limit so an invalid backend count
    // reaches the raw adapter instead of being consumed by the file cache.
    let data = alloc::vec![0x5a; MAX_DATA + 1];
    backend.files.write(FILE, &data).unwrap();
    scoped(&backend, |scope, _| {
        backend.free.set(Some(storage::MIN_RESERVE));
        let mut out = [0xa5; 4];
        let mut len = 91;
        // SAFETY: live inputs and exclusive outputs are disjoint.
        unsafe {
            assert_eq!(
                write(
                    scope,
                    OWNER,
                    None,
                    &PathsInUse::default(),
                    0,
                    bytes(b"state"),
                    bytes(b"new")
                ),
                ERR_FULL
            );
            assert_eq!(
                append(
                    scope,
                    OWNER,
                    None,
                    &PathsInUse::default(),
                    0,
                    bytes(b"state"),
                    bytes(b"new")
                ),
                ERR_FULL
            );
            backend.fail.set(Some(StorageError::Io));
            assert_eq!(
                size(scope, OWNER, None, 0, bytes(b"state"), &mut len),
                ERR_IO
            );
            assert_eq!(
                read_at(scope, OWNER, None, 0, bytes(b"state"), 0, output(&mut out)),
                ERR_IO
            );
            assert_eq!(
                remove(
                    scope,
                    OWNER,
                    None,
                    &PathsInUse::default(),
                    0,
                    bytes(b"state")
                ),
                ERR_IO
            );
            assert_eq!(
                rename(
                    scope,
                    OWNER,
                    None,
                    &PathsInUse::default(),
                    0,
                    bytes(b"state"),
                    bytes(b"other")
                ),
                ERR_IO
            );
            assert_eq!(sync(scope, OWNER), ERR_IO);
            backend.fail.set(None);
            backend.bad_count.set(true);
            assert_eq!(
                read_at(scope, OWNER, None, 0, bytes(b"state"), 0, output(&mut out)),
                ERR_INVALID
            );
        }
        assert_eq!(len, 91);
        // A failed read leaves zeros, never what the card wrote before it failed.
        assert_eq!(out, [0; 4]);
    });
    assert_eq!(backend.files.get(FILE).unwrap(), data);
}

#[test]
fn raw_read_initializes_only_the_successful_prefix() {
    let backend = Backend::default();
    backend.files.write(FILE, b"ok").unwrap();
    scoped(&backend, |scope, _| {
        let mut out = [MaybeUninit::<u8>::uninit(); 4];
        // SAFETY: caller storage is writable and exclusive. Only the reported
        // successful prefix is read after the adapter has initialized it.
        unsafe {
            assert_eq!(
                read_at(
                    scope,
                    OWNER,
                    None,
                    0,
                    bytes(b"state"),
                    0,
                    PpBuffer {
                        ptr: out.as_mut_ptr().cast(),
                        cap: out.len()
                    }
                ),
                2
            );
            assert_eq!([out[0].assume_init(), out[1].assume_init()], *b"ok");
        }
    });
}

#[test]
fn a_backend_that_partly_writes_then_fails_leaves_zeros_and_no_card_bytes() {
    let backend = Backend::default();
    // Bypass the small-file cache so this reaches the failing backend.
    backend
        .files
        .write(FILE, &alloc::vec![0x5a; MAX_DATA + 1])
        .unwrap();
    backend.partial_error.set(true);
    scoped(&backend, |scope, _| {
        let mut out = [0xa5; 4];
        // SAFETY: caller output is live, exclusive and disjoint from inputs.
        unsafe {
            assert_eq!(
                read_at(scope, OWNER, None, 0, bytes(b"state"), 0, output(&mut out)),
                ERR_IO
            );
            // What the card's adapter wrote before it failed (0xee) is gone.
            assert_eq!(out, [0; 4]);
        }
    });
}

#[test]
fn a_short_read_returns_its_count_and_zeros_the_rest() {
    let backend = Backend::default();
    backend.files.write(FILE, b"keep").unwrap();
    scoped(&backend, |scope, _| {
        let mut out = [0xa5; 8];
        // SAFETY: valid disjoint input and exclusive initialized output.
        assert_eq!(
            unsafe { read_at(scope, OWNER, None, 0, bytes(b"state"), 0, output(&mut out)) },
            4
        );
        assert_eq!(out, *b"keep\0\0\0\0");
    });
}

#[test]
fn an_adapter_that_used_the_buffer_as_scratch_leaves_zeros_after_the_count() {
    let backend = Backend::default();
    // Larger than the small-file cache, so the read reaches the backend.
    backend
        .files
        .write(FILE, &alloc::vec![0x5a; MAX_DATA + 1])
        .unwrap();
    backend.scribble.set(true);
    scoped(&backend, |scope, _| {
        let mut out = [0xa5; 4];
        let at = MAX_DATA as u64;
        // SAFETY: valid disjoint input and exclusive initialized output.
        assert_eq!(
            unsafe { read_at(scope, OWNER, None, 0, bytes(b"state"), at, output(&mut out)) },
            1
        );
        // The scratch bytes (0xee) after the one byte read are gone.
        assert_eq!(out, [0x5a, 0, 0, 0]);
    });
}

#[test]
fn an_output_that_overlaps_the_path_is_invalid_and_untouched() {
    let backend = Backend::default();
    backend.files.write(FILE, b"keep").unwrap();
    scoped(&backend, |scope, _| {
        let mut both = *b"state....";
        let at = both.as_mut_ptr();
        let path = PpBytes { ptr: at, len: 5 };
        // SAFETY: inside `both` (9 bytes); the overlap is what is refused.
        let over = PpBuffer {
            ptr: unsafe { at.add(3) },
            cap: 4,
        };
        // SAFETY: both ranges are live; the host must refuse the overlap.
        assert_eq!(
            unsafe { read_at(scope, OWNER, None, 0, path, 0, over) },
            ERR_INVALID
        );
        assert_eq!(&both, b"state....");
        // Right after the path is no overlap.
        // SAFETY: inside `both`, disjoint from the path's five bytes.
        let next = PpBuffer {
            ptr: unsafe { at.add(5) },
            cap: 4,
        };
        // SAFETY: disjoint live input and exclusive output.
        assert_eq!(unsafe { read_at(scope, OWNER, None, 0, path, 0, next) }, 4);
        assert_eq!(&both, b"statekeep");
    });
}

#[test]
fn a_read_holds_no_copy_of_the_callers_buffer() {
    let backend = Backend::default();
    backend
        .files
        .write(FILE, &alloc::vec![0x5a; MAX_DATA + 1])
        .unwrap();
    scoped(&backend, |scope, _| {
        let mut out = alloc::vec![0_u8; MAX_DATA];
        // The kernel remembers a file's size for the rest of a call, and small
        // files (up to 4 KiB) whole (`recall.rs`): the file is larger, and its
        // size is asked first, so what is measured is the slot's own heap.
        let mut len = 0;
        // SAFETY: live input and an exclusive output.
        assert_eq!(
            unsafe { size(scope, OWNER, None, 0, bytes(b"state"), &mut len) },
            OK
        );
        let before = crate::native_host::shell::tests::HEAP_LIVE.with(Cell::get);
        crate::native_host::shell::tests::HEAP_PEAK.with(|peak| peak.set(before));
        // SAFETY: valid disjoint input and exclusive initialized output.
        let read = unsafe { read_at(scope, OWNER, None, 0, bytes(b"state"), 0, output(&mut out)) };
        let held = crate::native_host::shell::tests::HEAP_PEAK.with(Cell::get) - before;
        assert_eq!(read, MAX_DATA as i32);
        assert!(out.iter().all(|byte| *byte == 0x5a));
        assert!(
            held < MAX_DATA / 4,
            "{held} bytes held for a {MAX_DATA}-byte read"
        );
    });
}

#[test]
fn outside_dispatch_and_invalid_owner_routes_have_no_effect() {
    let scope = EnvironmentScope::new();
    let mut len = 91;
    let mut out = [0xa5; 4];
    // SAFETY: inputs and exclusive outputs are live and disjoint.
    unsafe {
        assert_eq!(
            size(&scope, OWNER, None, 0, bytes(b"state"), &mut len),
            ERR_STATE
        );
        assert_eq!(
            read_at(&scope, OWNER, None, 0, bytes(b"state"), 0, output(&mut out)),
            ERR_STATE
        );
        assert_eq!(
            write(
                &scope,
                OWNER,
                None,
                &PathsInUse::default(),
                0,
                bytes(b"state"),
                bytes(b"new")
            ),
            ERR_STATE
        );
        assert_eq!(
            append(
                &scope,
                OWNER,
                None,
                &PathsInUse::default(),
                0,
                bytes(b"state"),
                bytes(b"new")
            ),
            ERR_STATE
        );
        assert_eq!(
            rename(
                &scope,
                OWNER,
                None,
                &PathsInUse::default(),
                0,
                bytes(b"state"),
                bytes(b"new")
            ),
            ERR_STATE
        );
        assert_eq!(
            remove(
                &scope,
                OWNER,
                None,
                &PathsInUse::default(),
                0,
                bytes(b"state")
            ),
            ERR_STATE
        );
    }
    assert_eq!(sync(&scope, OWNER), ERR_STATE);
    assert_eq!(len, 91);
    assert_eq!(out, [0xa5; 4]);
    let backend = Backend::default();
    scoped(&backend, |scope, _| {
        for owner in ["", "..", "../other", "a/b", "a\\b", "a:", "con"] {
            // SAFETY: inputs are live; invalid owner routes never reach storage.
            assert_eq!(
                unsafe {
                    write(
                        scope,
                        owner,
                        None,
                        &PathsInUse::default(),
                        0,
                        bytes(b"state"),
                        bytes(b"new"),
                    )
                },
                ERR_INVALID
            );
        }
        let long_owner = "a".repeat(MAX_FOLDER);
        assert_eq!(sync(scope, &long_owner), ERR_INVALID);
        assert!(StorageAdapter::borrowed_folder(&backend, ".paperpaw/plugins/../", None).is_none());
        assert!(StorageAdapter::borrowed_folder(&backend, "plugins/battery/", None).is_none());
    });
    assert_eq!(backend.files.get(FILE), None);
}

#[test]
fn an_empty_read_needs_no_allocation_and_reads_nothing() {
    let backend = Backend::default();
    backend.files.write(FILE, b"keep").unwrap();
    scoped(&backend, |scope, _| {
        // The kernel keeps a small file whole for the rest of a call
        // (`recall.rs`); one read first, so only the slot's own heap is measured.
        let mut first = [0_u8; 1];
        // SAFETY: live input and an exclusive output.
        assert_eq!(
            unsafe {
                read_at(
                    scope,
                    OWNER,
                    None,
                    0,
                    bytes(b"state"),
                    0,
                    output(&mut first),
                )
            },
            1
        );
        let before = crate::native_host::shell::tests::HEAP_LIVE.with(Cell::get);
        crate::native_host::shell::tests::HEAP_PEAK.with(|peak| peak.set(before));
        // SAFETY: live input and a non-null, exclusive empty output buffer.
        assert_eq!(
            unsafe { read_at(scope, OWNER, None, 0, bytes(b"state"), 0, output(&mut [])) },
            0
        );
        assert_eq!(
            crate::native_host::shell::tests::HEAP_PEAK.with(Cell::get),
            before
        );
    });
}

#[test]
fn read_validation_refusals_preserve_sentinel_output() {
    let backend = Backend::default();
    scoped(&backend, |scope, _| {
        for (owner, path, expected) in [
            (OWNER, bytes(&[0xff]), ERR_UTF8),
            (OWNER, bytes(b"../state"), ERR_INVALID),
            (
                OWNER,
                PpBytes {
                    ptr: core::ptr::null(),
                    len: 1,
                },
                ERR_INVALID,
            ),
            ("../other", bytes(b"state"), ERR_INVALID),
        ] {
            let mut out = [0xa5; 7];
            // SAFETY: output is live and exclusive. Malformed input pairs must
            // be refused before dereference or output initialization.
            assert_eq!(
                unsafe { read_at(scope, owner, None, 0, path, 0, output(&mut out)) },
                expected
            );
            assert_eq!(out, [0xa5; 7]);
        }
    });
}

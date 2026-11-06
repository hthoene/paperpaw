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
struct Backend {
    files: Rc<MemoryStorage>,
    free: Rc<Cell<Option<u64>>>,
    fail: Rc<Cell<Option<StorageError>>>,
    bad_count: Rc<Cell<bool>>,
    partial_error: Rc<Cell<bool>>,
    read_failure: Rc<Cell<Option<StorageError>>>,
}

impl Backend {
    fn check(&self) -> Result<(), StorageError> {
        self.fail.get().map_or(Ok(()), Err)
    }
}

impl Storage for Backend {
    fn read_at(&self, path: &str, offset: u64, out: &mut [u8]) -> Result<usize, StorageError> {
        self.check()?;
        if let Some(error) = self.read_failure.get() {
            out.fill(0xee);
            return Err(error);
        }
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

fn bytes(value: &[u8]) -> PpBytes {
    PpBytes {
        ptr: value.as_ptr(),
        len: value.len(),
    }
}
fn output(value: &mut [u8]) -> PpBuffer {
    PpBuffer {
        ptr: value.as_mut_ptr(),
        cap: value.len(),
    }
}
fn scoped(backend: &Backend, call: impl FnOnce(&EnvironmentScope, &crate::Kernel)) {
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
    let _: unsafe fn(&EnvironmentScope, &str, u32, PpBytes, *mut u64) -> i32 = size;
    let _: unsafe fn(&EnvironmentScope, &str, u32, PpBytes, u64, PpBuffer) -> i32 = read_at;
    let _: unsafe fn(&EnvironmentScope, &str, u32, PpBytes, PpBytes) -> i32 = write;
    let _: unsafe fn(&EnvironmentScope, &str, u32, PpBytes, PpBytes) -> i32 = append;
    let _: unsafe fn(&EnvironmentScope, &str, u32, PpBytes, PpBytes) -> i32 = rename;
    let _: unsafe fn(&EnvironmentScope, &str, u32, PpBytes) -> i32 = remove;
    let _: fn(&EnvironmentScope, &str) -> i32 = sync;
}

#[test]
fn all_slots_use_the_scoped_storage_and_only_the_owner_folder() {
    let backend = Backend::default();
    backend.files.write(FOREIGN, b"foreign").unwrap();
    scoped(&backend, |scope, kernel| {
        let _running = kernel.shared.meter.enter(OWNER);
        let mut len = 91;
        // SAFETY: inputs and output are live, exclusive and disjoint.
        unsafe {
            assert_eq!(
                size(scope, OWNER, 0, bytes(b"state"), &mut len),
                ERR_NOT_FOUND
            );
            assert_eq!(len, 91);
            assert_eq!(write(scope, OWNER, 0, bytes(b"state"), bytes(b"abc")), OK);
            assert_eq!(append(scope, OWNER, 0, bytes(b"state"), bytes(b"def")), OK);
            assert_eq!(size(scope, OWNER, 0, bytes(b"state"), &mut len), OK);
            assert_eq!(len, 6);
            let mut out = [0xa5; 8];
            assert_eq!(
                read_at(scope, OWNER, 0, bytes(b"state"), 2, output(&mut out)),
                4
            );
            assert_eq!(&out, b"cdef\0\0\0\0");
            assert_eq!(
                read_at(scope, OWNER, 0, bytes(b"state"), u64::MAX, output(&mut out)),
                0
            );
            assert_eq!(
                rename(scope, OWNER, 0, bytes(b"state"), bytes(b"moved")),
                OK
            );
            assert_eq!(remove(scope, OWNER, 0, bytes(b"moved")), OK);
            assert_eq!(remove(scope, OWNER, 0, bytes(b"moved")), OK);
            assert_eq!(write(scope, OWNER, 0, bytes(b"state"), bytes(b"")), OK);
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
                    write(scope, OWNER, 0, bytes(bad), bytes(b"new")),
                    ERR_INVALID
                );
                assert_eq!(
                    append(scope, OWNER, 0, bytes(bad), bytes(b"new")),
                    ERR_INVALID
                );
                assert_eq!(remove(scope, OWNER, 0, bytes(bad)), ERR_INVALID);
                assert_eq!(
                    rename(scope, OWNER, 0, bytes(b"state"), bytes(bad)),
                    ERR_INVALID
                );
                assert_eq!(
                    rename(scope, OWNER, 0, bytes(bad), bytes(b"state")),
                    ERR_INVALID
                );
            }
        }
        // SAFETY: byte arrays and output are live and disjoint.
        unsafe {
            let mut len = 91;
            assert_eq!(size(scope, OWNER, 0, bytes(&[0xff]), &mut len), ERR_UTF8);
            assert_eq!(
                write(scope, OWNER, 0, bytes(&[0xff]), bytes(b"new")),
                ERR_UTF8
            );
            assert_eq!(len, 91);
            for root in [1, 2, u32::MAX] {
                let mut out = [0xa5; 1];
                assert_eq!(
                    size(scope, OWNER, root, bytes(b"state"), &mut len),
                    ERR_UNSUPPORTED
                );
                assert_eq!(
                    read_at(scope, OWNER, root, bytes(b"state"), 0, output(&mut out)),
                    ERR_UNSUPPORTED
                );
                assert_eq!(
                    write(scope, OWNER, root, bytes(b"state"), bytes(b"new")),
                    ERR_UNSUPPORTED
                );
                assert_eq!(
                    append(scope, OWNER, root, bytes(b"state"), bytes(b"new")),
                    ERR_UNSUPPORTED
                );
                assert_eq!(
                    rename(scope, OWNER, root, bytes(b"state"), bytes(b"new")),
                    ERR_UNSUPPORTED
                );
                assert_eq!(remove(scope, OWNER, root, bytes(b"state")), ERR_UNSUPPORTED);
                assert_eq!(out, [0xa5]);
            }
        }
    });
    assert_eq!(backend.files.get(FILE).unwrap(), b"keep");
}

#[test]
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
                assert_eq!(write(scope, OWNER, 0, bad, bytes(b"new")), ERR_INVALID);
            }
            assert_eq!(write(scope, OWNER, 0, long, bytes(b"new")), ERR_NO_SPACE);
            assert_eq!(write(scope, OWNER, 0, bytes(b"state"), big), ERR_NO_SPACE);
            assert_eq!(append(scope, OWNER, 0, bytes(b"state"), big), ERR_NO_SPACE);
            assert_eq!(write(scope, OWNER, 0, bytes(b"state"), null), ERR_INVALID);
            assert_eq!(
                size(scope, OWNER, 0, bytes(b"state"), core::ptr::null_mut()),
                ERR_INVALID
            );
            let unaligned = scalar.as_mut_ptr().cast::<u8>().add(1).cast();
            assert_eq!(
                size(scope, OWNER, 0, bytes(b"state"), unaligned),
                ERR_INVALID
            );
            assert_eq!(
                read_at(
                    scope,
                    OWNER,
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
            assert_eq!(read_at(scope, OWNER, 0, bytes(b"state"), 0, empty), 0);
        }
        assert_eq!(out, [0xa5; 8]);
        assert_eq!(scalar, [91; 2]);
    });
    assert_eq!(backend.files.get(FILE).unwrap(), b"keep");
}

#[test]
fn reserve_errors_preserve_files_and_bad_read_counts_zero_output() {
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
                write(scope, OWNER, 0, bytes(b"state"), bytes(b"new")),
                ERR_FULL
            );
            assert_eq!(
                append(scope, OWNER, 0, bytes(b"state"), bytes(b"new")),
                ERR_FULL
            );
            backend.fail.set(Some(StorageError::Io));
            assert_eq!(size(scope, OWNER, 0, bytes(b"state"), &mut len), ERR_IO);
            assert_eq!(
                read_at(scope, OWNER, 0, bytes(b"state"), 0, output(&mut out)),
                ERR_IO
            );
            assert_eq!(out, [0xa5; 4], "preflight failure preserves output");
            assert_eq!(remove(scope, OWNER, 0, bytes(b"state")), ERR_IO);
            assert_eq!(
                rename(scope, OWNER, 0, bytes(b"state"), bytes(b"other")),
                ERR_IO
            );
            assert_eq!(sync(scope, OWNER), ERR_IO);
            backend.fail.set(None);
            backend.bad_count.set(true);
            assert_eq!(
                read_at(scope, OWNER, 0, bytes(b"state"), 0, output(&mut out)),
                ERR_INVALID
            );
        }
        assert_eq!(len, 91);
        assert_eq!(out, [0; 4]);
    });
    assert_eq!(backend.files.get(FILE).unwrap(), data);
}

#[test]
fn raw_read_initializes_the_output_in_place_before_storage() {
    let backend = Backend::default();
    backend.files.write(FILE, b"ok").unwrap();
    scoped(&backend, |scope, _| {
        let mut out = [MaybeUninit::<u8>::uninit(); 4];
        // SAFETY: caller storage is writable and exclusive. The adapter
        // initializes the full output before borrowing it as bytes.
        unsafe {
            assert_eq!(
                read_at(
                    scope,
                    OWNER,
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
            assert_eq!(out.map(|byte| byte.assume_init()), *b"ok\0\0");
        }
    });
}

#[test]
fn a_backend_that_partly_writes_then_fails_leaves_initialized_zero_output() {
    let backend = Backend::default();
    // Bypass the small-file cache so this reaches the failing backend.
    backend
        .files
        .write(FILE, &alloc::vec![0x5a; MAX_DATA + 1])
        .unwrap();
    backend.partial_error.set(true);
    scoped(&backend, |scope, _| {
        let mut out = [MaybeUninit::<u8>::uninit(); 4];
        // SAFETY: caller output is live and exclusive. Even on backend error,
        // the adapter initializes and clears its full capacity before returning.
        unsafe {
            assert_eq!(
                read_at(
                    scope,
                    OWNER,
                    0,
                    bytes(b"state"),
                    0,
                    PpBuffer {
                        ptr: out.as_mut_ptr().cast(),
                        cap: out.len()
                    }
                ),
                ERR_IO
            );
            assert_eq!(out.map(|byte| byte.assume_init()), [0; 4]);
        }
    });
}

#[test]
fn preflight_failures_preserve_output_and_read_failures_zero_it() {
    let backend = Backend::default();
    // Above the recall limit: each read reaches the actual backend.
    backend
        .files
        .write(FILE, &alloc::vec![0x5a; MAX_DATA + 1])
        .unwrap();
    scoped(&backend, |scope, kernel| {
        for (error, expected) in [
            (StorageError::NotFound, ERR_NOT_FOUND),
            (StorageError::Io, ERR_IO),
            (StorageError::InvalidPath, ERR_INVALID),
        ] {
            kernel.shared.recall.clear();
            let mut out = [0xa5; 7];
            backend.fail.set(Some(error));
            // SAFETY: valid disjoint input and exclusive output.
            assert_eq!(
                unsafe { read_at(scope, OWNER, 0, bytes(b"state"), 0, output(&mut out)) },
                expected
            );
            assert_eq!(out, [0xa5; 7], "preflight must not write");
            backend.fail.set(None);
            // Each phase models a fresh call, without cached preflight errors.
            kernel.shared.recall.clear();
            backend.read_failure.set(Some(error));
            // SAFETY: same live output; the read begins after successful preflight.
            assert_eq!(
                unsafe { read_at(scope, OWNER, 0, bytes(b"state"), 0, output(&mut out)) },
                expected
            );
            assert_eq!(out, [0; 7], "read failure must leave no partial bytes");
            backend.read_failure.set(None);
        }
        let mut out = [0xa5; 7];
        // SAFETY: valid call, but no file exists for this path.
        assert_eq!(
            unsafe { read_at(scope, OWNER, 0, bytes(b"absent"), 0, output(&mut out)) },
            ERR_NOT_FOUND
        );
        assert_eq!(out, [0xa5; 7]);
        backend.bad_count.set(true);
        // SAFETY: backend over-reporting is validated before returning success.
        assert_eq!(
            unsafe { read_at(scope, OWNER, 0, bytes(b"state"), 0, output(&mut out)) },
            ERR_INVALID
        );
        assert_eq!(out, [0; 7]);
    });
}

#[test]
fn outside_dispatch_and_invalid_owner_routes_have_no_effect() {
    let scope = EnvironmentScope::new();
    let mut len = 91;
    let mut out = [0xa5; 4];
    // SAFETY: inputs and exclusive outputs are live and disjoint.
    unsafe {
        assert_eq!(size(&scope, OWNER, 0, bytes(b"state"), &mut len), ERR_STATE);
        assert_eq!(
            read_at(&scope, OWNER, 0, bytes(b"state"), 0, output(&mut out)),
            ERR_STATE
        );
        assert_eq!(
            write(&scope, OWNER, 0, bytes(b"state"), bytes(b"new")),
            ERR_STATE
        );
        assert_eq!(
            append(&scope, OWNER, 0, bytes(b"state"), bytes(b"new")),
            ERR_STATE
        );
        assert_eq!(
            rename(&scope, OWNER, 0, bytes(b"state"), bytes(b"new")),
            ERR_STATE
        );
        assert_eq!(remove(&scope, OWNER, 0, bytes(b"state")), ERR_STATE);
    }
    assert_eq!(sync(&scope, OWNER), ERR_STATE);
    assert_eq!(len, 91);
    assert_eq!(out, [0xa5; 4]);
    let backend = Backend::default();
    scoped(&backend, |scope, _| {
        for owner in ["", "..", "../other", "a/b", "a\\b", "a:", "con"] {
            // SAFETY: inputs are live; invalid owner routes never reach storage.
            assert_eq!(
                unsafe { write(scope, owner, 0, bytes(b"state"), bytes(b"new")) },
                ERR_INVALID
            );
        }
        let long_owner = "a".repeat(MAX_FOLDER);
        assert_eq!(sync(scope, &long_owner), ERR_INVALID);
        assert!(StorageAdapter::borrowed_folder(&backend, ".paperpaw/plugins/../").is_none());
        assert!(StorageAdapter::borrowed_folder(&backend, "plugins/battery/").is_none());
    });
    assert_eq!(backend.files.get(FILE), None);
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
                unsafe { read_at(scope, owner, 0, path, 0, output(&mut out)) },
                expected
            );
            assert_eq!(out, [0xa5; 7]);
        }
    });
}

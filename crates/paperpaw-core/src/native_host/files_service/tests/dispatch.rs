//! `service.open` and `service.call` end to end: the device registry, the
//! admitted features per instance, the wired host view, the kernel's own
//! storage as the frame's view, and the re-entry rule.
use super::*;
use crate::native_host::{
    HostResources, call_services, declared_services::DeclaredServices,
    environment::EnvironmentScope, resources,
};
use core::pin::pin;
use paperpaw_native::{
    host_glue::{ERR_BUSY, ERR_DENIED, OK, PpBuffer, PpBytes},
    metadata::Declared,
};

/// The fake card as the kernel's storage.
struct Shared(Rc<Card>);

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.0.list_from(dir, from, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn remove_listed(&self, dir: &str, place: u64) -> Result<(), StorageError> {
        self.0.remove_listed(dir, place)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove_dir_all(path)
    }
    fn modified(&self, path: &str) -> Option<i64> {
        self.0.modified(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
    fn ignores_case(&self) -> bool {
        self.0.ignores_case()
    }
    fn free_space(&self) -> Option<u64> {
        self.0.free_space()
    }
    fn total_space(&self) -> Option<u64> {
        self.0.total_space()
    }
}

fn declared(features: AdmittedFeatures) -> DeclaredServices {
    DeclaredServices::try_from_iter(core::iter::once(Declared {
        key: "core.files",
        major: 1,
        semantic_revision: 0,
    }))
    .unwrap()
    .with_features(features)
}

fn open(
    scope: &EnvironmentScope,
    owner: &resources::OwnerResources,
    declared: &DeclaredServices,
) -> Result<u32, i32> {
    let key = "core.files";
    let mut handle = 0;
    // SAFETY: the key and the scalar output are live and disjoint.
    let status = unsafe {
        call_services::open(
            scope,
            owner,
            declared,
            PpBytes {
                ptr: key.as_ptr(),
                len: key.len(),
            },
            1,
            0,
            &mut handle,
        )
    };
    if status == OK {
        Ok(handle)
    } else {
        Err(status)
    }
}

fn call_op(
    scope: &EnvironmentScope,
    owner: &resources::OwnerResources,
    handle: u32,
    id: u32,
    record: Record<'_>,
) -> Result<Vec<u8>, i32> {
    let op = schema_op(id);
    let input = input(id, record);
    let mut out = std::vec![0; op.max_reply];
    // SAFETY: the fixture's input and output are live and disjoint.
    let status = unsafe {
        call_services::call(
            scope,
            owner,
            PLUGIN,
            handle,
            id,
            PpBytes {
                ptr: input.as_ptr(),
                len: input.len(),
            },
            PpBuffer {
                ptr: out.as_mut_ptr(),
                cap: out.len(),
            },
        )
    };
    if status < 0 {
        return Err(status);
    }
    out.truncate(status as usize);
    Ok(out)
}

#[test]
fn a_plugin_opens_the_family_and_calls_every_op_through_the_dispatcher() {
    let card = Rc::new(Card::new());
    let host = TestHost::new(&card);
    card.file("Books/a.epub", 10);
    let pool = HostResources::try_new().unwrap();
    pool.hosts().wire_files(host.clone()).unwrap();
    assert_eq!(
        pool.hosts().wire_files(host.clone()),
        Err(ERR_STATE),
        "wired once"
    );
    let owner = pool.try_owner().unwrap();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .storage(Shared(Rc::clone(&card)))
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let view = owner.try_view().unwrap();
    let scope = pin!(EnvironmentScope::new());
    // SAFETY: the view outlives the scope's callbacks.
    unsafe { scope.expire_view(&view) };
    scope
        .as_ref()
        .scope(&env, || {
            // Not asked for `uses.storage`: root 0 is fine, root 1 is denied.
            let plain = declared(AdmittedFeatures::test(0, WIRED));
            let handle = open(&scope, &owner, &plain).unwrap();
            assert_eq!(
                call_op(&scope, &owner, handle, 2, path(1, "Books/a.epub")),
                Err(ERR_DENIED)
            );
            assert!(call_op(&scope, &owner, handle, 3, path(0, "")).is_ok());
            owner.release(handle).unwrap();
            let granted = declared(AdmittedFeatures::test(1, WIRED));
            let handle = open(&scope, &owner, &granted).unwrap();
            let entry = call_op(&scope, &owner, handle, 2, path(1, "Books/a.epub")).unwrap();
            assert_eq!(rows(&entry).0[0].2, 10);
            let list = call_op(&scope, &owner, handle, 1, list_record(1, "Books", 0)).unwrap();
            assert_eq!(rows(&list).0.len(), 1);
            assert_eq!(
                call_op(&scope, &owner, handle, 4, path(1, "New/Dir")),
                Ok(Vec::new())
            );
            assert!(card.is_dir("New/Dir"));
            let moved = call_op(
                &scope,
                &owner,
                handle,
                6,
                move_record(1, "Books/a.epub", 1, "New/a.epub", 0),
            );
            assert_eq!(moved, Ok(Vec::new()));
            host.finish_job();
            let progress = call_op(&scope, &owner, handle, 5, list_record(1, "New", 0)).unwrap();
            assert!(matches!(
                wire::record_decode_one(&progress).unwrap(),
                Record::ServiceFsProgress(p) if p.flags == 1
            ));
            assert!(!card.has("New"));
            // A malformed batch is refused before the host sees it.
            let op = schema_op(2);
            let mut out = std::vec![0; op.max_reply];
            // SAFETY: live, disjoint buffers.
            let status = unsafe {
                call_services::call(
                    &scope,
                    &owner,
                    PLUGIN,
                    handle,
                    2,
                    PpBytes {
                        ptr: [0xffu8].as_ptr(),
                        len: 1,
                    },
                    PpBuffer {
                        ptr: out.as_mut_ptr(),
                        cap: out.len(),
                    },
                )
            };
            assert!(status < 0);
            // A call made while the family is in a call of its own is busy.
            let key = owner.service(handle).unwrap();
            let inner = owner.enter_service(key).unwrap();
            assert_eq!(
                call_op(&scope, &owner, handle, 3, path(0, "")),
                Err(ERR_BUSY)
            );
            drop(inner);
            assert!(call_op(&scope, &owner, handle, 3, path(0, "")).is_ok());
        })
        .unwrap();
}

#[test]
fn a_family_the_shell_has_not_wired_answers_unsupported() {
    let card = Rc::new(Card::new());
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .storage(Shared(card))
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let view = owner.try_view().unwrap();
    let scope = pin!(EnvironmentScope::new());
    // SAFETY: the view outlives the scope's callbacks.
    unsafe { scope.expire_view(&view) };
    scope
        .as_ref()
        .scope(&env, || {
            let handle = open(&scope, &owner, &declared(AdmittedFeatures::test(1, WIRED))).unwrap();
            assert_eq!(
                call_op(&scope, &owner, handle, 3, path(0, "")),
                Err(ERR_UNSUPPORTED)
            );
        })
        .unwrap();
}

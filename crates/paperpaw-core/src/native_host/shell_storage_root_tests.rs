//! The storage slots of a hosted plugin open root 1, the user's files, by the
//! plugin's own admitted `uses.storage` (`files.md` 1.1, decision 125), as
//! the shell hands it to them.
use super::*;
use crate::native_host::admitted_features::AdmittedFeatures;
use core::cell::Cell;
use paperpaw_native::{
    host_glue::{ERR_DENIED, ERR_UNSUPPORTED},
    metadata::Feature,
};
use paperpaw_platform::Storage as _;
use paperpaw_sdk::{Id, MemoryStorage, ScreenKey, StackEntry};

std::thread_local! {
    /// What the plugin's `read_at` of root 1 answered, and the bytes it read.
    static READ: Cell<(i32, [u8; 5])> = const { Cell::new((i32::MIN, [0; 5])) };
}

const HOST: &[Feature<'static>] = &[Feature {
    key: "uses.storage",
    revision: 1,
}];

unsafe extern "C" fn make_reader(
    host: *const PpHost,
    ctx: *mut c_void,
    out: *mut *const PpPlugin,
) -> i32 {
    // SAFETY: forwarded factory arguments satisfy the original fixture's contract.
    let status = unsafe { make(host, ctx, out) };
    if status == OK {
        // SAFETY: make just initialized this uniquely owned fixture table. It is
        // not published to the host until this factory returns.
        unsafe { (*(*out).cast_mut()).input = Some(input_read) };
    }
    status
}

unsafe extern "C" fn input_read(state: *mut c_void, _: u32, _: PpBytes) -> i32 {
    let path = b"words/cards.txt";
    let mut out = [0_u8; 5];
    // SAFETY: owner admission keeps the fixture and table alive; the path is
    // readable and `out` writable and disjoint for this synchronous call.
    let status = unsafe {
        let fake = &*state.cast::<Fake>();
        ((*fake.host).storage_read_at)(
            fake.ctx,
            1,
            PpBytes {
                ptr: path.as_ptr(),
                len: path.len(),
            },
            0,
            PpBuffer {
                ptr: out.as_mut_ptr(),
                cap: out.len(),
            },
        )
    };
    READ.with(|read| read.set((status, out)));
    OK
}

/// The plugin's root-1 read with its admitted `features`.
fn read_with(features: AdmittedFeatures) -> (i32, [u8; 5]) {
    reset(NORMAL);
    READ.with(|read| read.set((i32::MIN, [0; 5])));
    let card = MemoryStorage::new();
    card.write("words/cards.txt", b"CARDS").unwrap();
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .storage(card)
        .build()
        .unwrap();
    kernel.shared.stack.push(StackEntry::new(
        Id::new_static("fake.screen"),
        None,
        false,
        ScreenKey::new(7),
    ));
    // SAFETY: this fixture's factory is callable for the retained module lifetime.
    let hosted = unsafe {
        Hosted::new(
            Heap,
            Module::new(),
            make_reader,
            "fake",
            (ptr::null(), 0),
            report,
            crate::native_host::HostResources::try_new().unwrap(),
            DeclaredServices::none().with_features(features),
        )
    }
    .unwrap();
    let batch = hosted
        .make_registered(hosted.begin().unwrap())
        .unwrap()
        .commit()
        .unwrap();
    assert_eq!(
        kernel.input_native_for_test(
            &batch,
            TOKEN,
            &DispatchDepth::new(),
            &[],
            paperpaw_platform::Instant(0),
            0,
        ),
        Ok(OK)
    );
    drop(batch);
    finish(&hosted);
    READ.with(Cell::get)
}

#[test]
fn a_hosted_plugin_reads_the_users_files_only_with_its_own_uses_storage() {
    assert_eq!(read_with(AdmittedFeatures::test(1, HOST)), (5, *b"CARDS"));
    assert_eq!(read_with(AdmittedFeatures::test(0, HOST)).0, ERR_DENIED);
    // A host that offers no card files.
    assert_eq!(read_with(AdmittedFeatures::test(1, &[])).0, ERR_UNSUPPORTED);
}

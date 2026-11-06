//! Real dlopen/factory/destruction proof. Original cases use typed host records;
//! the foreign child module exercises reviewed wire submission and callbacks.
use super::*;
use paperpaw_core::native_host::DispatchDepth;
use std::{cell::RefCell, process::Command, rc::Rc};

#[derive(Default)]
struct Evidence {
    events: RefCell<Vec<&'static str>>,
    live: Cell<bool>,
    allocations: Cell<usize>,
    frees: Cell<usize>,
    clocks: Cell<usize>,
    records: Cell<usize>,
    callback_markers: Cell<usize>,
    callback_frees: Cell<usize>,
}

struct ServicesProbe {
    evidence: Rc<Evidence>,
    path: std::path::PathBuf,
}

fn mapped(path: &Path) -> bool {
    std::fs::read_to_string("/proc/self/maps")
        .unwrap()
        .contains(path.to_str().unwrap())
}

impl Services for ServicesProbe {
    fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        assert!(self.evidence.live.get());
        assert!(mapped(&self.path));
        if size == 1 && align == 1 {
            self.evidence
                .callback_markers
                .set(self.evidence.callback_markers.get() + 1);
            self.evidence.events.borrow_mut().push("callback");
        } else {
            self.evidence
                .allocations
                .set(self.evidence.allocations.get() + 1);
            self.evidence.events.borrow_mut().push("allocate");
        }
        // SAFETY: host glue validated this nonzero layout.
        unsafe { std::alloc::alloc(Layout::from_size_align(size, align).unwrap()) }
    }

    unsafe fn dealloc(&self, block: *mut u8, size: usize, align: usize) {
        assert!(self.evidence.live.get());
        assert!(mapped(&self.path));
        if size == 1 && align == 1 {
            self.evidence
                .callback_frees
                .set(self.evidence.callback_frees.get() + 1);
        } else {
            self.evidence.frees.set(self.evidence.frees.get() + 1);
            self.evidence.events.borrow_mut().push("free");
        }
        // SAFETY: fixture returns exactly the allocation and layout above.
        unsafe { std::alloc::dealloc(block, Layout::from_size_align(size, align).unwrap()) };
    }

    unsafe fn realloc(&self, _: *mut u8, _: usize, _: usize, _: usize) -> *mut u8 {
        panic!("fixture must not reallocate")
    }

    fn micros(&self) -> u64 {
        assert!(self.evidence.live.get());
        assert!(mapped(&self.path));
        let old = self.evidence.clocks.replace(self.evidence.clocks.get() + 1);
        self.evidence
            .events
            .borrow_mut()
            .push(if old == 0 { "make" } else { "destroy" });
        old as u64
    }
}

impl Drop for ServicesProbe {
    fn drop(&mut self) {
        assert!(self.evidence.live.replace(false));
        assert!(mapped(&self.path), "context must drop before module unload");
        assert_eq!(self.evidence.allocations.get(), self.evidence.frees.get());
        assert_eq!(
            self.evidence.callback_markers.get(),
            self.evidence.callback_frees.get()
        );
        self.evidence.events.borrow_mut().push("context");
    }
}

struct Record(Rc<Evidence>);
impl Drop for Record {
    fn drop(&mut self) {
        assert!(self.0.live.get());
        assert_eq!(
            self.0.frees.get(),
            0,
            "staging drains before foreign destroy"
        );
        self.0.records.set(self.0.records.get() + 1);
        self.0.events.borrow_mut().push("record");
    }
}

struct Fixture {
    root: std::path::PathBuf,
    path: std::path::PathBuf,
    digest: [u8; 32],
}

impl Fixture {
    fn build(mode: u32) -> Self {
        let root = crate::private::dir("paperpaw-owner718").unwrap();
        let path = root.join("fixture.so");
        let abi = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../abi/generated/paperpaw_abi.rs")
            .canonicalize()
            .unwrap();
        let bytes = super::super::tests::metadata_bytes();
        let plugin_table = if mode == 3 { "Prefix" } else { "PpPlugin" };
        let callback_fields = if mode == 3 {
            ""
        } else {
            ", dispatch_event: None, draw: None"
        };
        let source = format!(
            r#"
#![no_std]
#![allow(dead_code)]
include!({abi:?});
#[panic_handler] fn panic(_: &core::panic::PanicInfo) -> ! {{ loop {{}} }}
#[unsafe(no_mangle)] pub static PAPERPAW_METADATA: [u8; {len}] = {bytes:?};
#[repr(transparent)] pub struct Entry(PpEntry);
unsafe impl Sync for Entry {{}}
#[unsafe(no_mangle)] pub static PAPERPAW_ENTRY: Entry = Entry(PpEntry {{
 size: core::mem::size_of::<PpEntry>() as u32, abi_major: 7, needs_minor: 1,
 metadata: PpBytes {{ ptr: PAPERPAW_METADATA.as_ptr(), len: {len} }}, make: Some(make),
}});
#[repr(C)] struct Prefix {{ size: u32, state: *mut core::ffi::c_void, drop: Option<unsafe extern "C" fn(*mut core::ffi::c_void)> }}
const _: () = assert!(core::mem::size_of::<Prefix>() == PLUGIN_PREFIX_BYTES);
#[repr(C)] struct State {{ host: *const PpHost, ctx: *mut core::ffi::c_void, plugin: {plugin_table} }}
unsafe extern "C" fn make(host: *const PpHost, ctx: *mut core::ffi::c_void, out: *mut *const PpPlugin) -> i32 {{
 let h = unsafe {{ &*host }};
 let mut stamp = 0;
 unsafe {{ (h.clock_micros)(ctx, &mut stamp); }}
 if {mode} == 1 {{ return ERR_INVALID; }}
 if {mode} == 2 {{ return OK; }}
 let mut block = core::ptr::null_mut();
 let status = unsafe {{ (h.heap_alloc)(ctx, core::mem::size_of::<State>(), core::mem::align_of::<State>(), &mut block) }};
 if status != OK {{ return status; }}
 let state = block.cast::<State>();
 unsafe {{ state.write(State {{ host, ctx, plugin: {plugin_table} {{ size: core::mem::size_of::<{plugin_table}>() as u32, state: block, drop: Some(destroy){callback_fields} }} }});
 out.write((&raw const (*state).plugin).cast()); }}
 OK
}}
unsafe extern "C" fn destroy(state: *mut core::ffi::c_void) {{
 let s = unsafe {{ &*state.cast::<State>() }};
 let h = unsafe {{ &*s.host }};
 let mut stamp = 0;
 unsafe {{ (h.clock_micros)(s.ctx, &mut stamp);
 (h.heap_dealloc)(s.ctx, state, core::mem::size_of::<State>(), core::mem::align_of::<State>()); }}
}}
"#,
            len = bytes.len()
        );
        let input = root.join("fixture.rs");
        std::fs::write(&input, source).unwrap();
        let result = Command::new("rustc")
            .args([
                "--edition=2024",
                "--crate-type=cdylib",
                "-Cpanic=abort",
                "-Copt-level=2",
                "-Clink-arg=-nostartfiles",
            ])
            .arg(&input)
            .arg("-o")
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let digest = paperpaw_core::sdk::digest::sha256::digest(&std::fs::read(&path).unwrap());
        Self { root, path, digest }
    }

    fn open(&self) -> (PinnedPlugin<ServicesProbe>, Rc<Evidence>) {
        let evidence = Rc::new(Evidence::default());
        evidence.live.set(true);
        let services = ServicesProbe {
            evidence: evidence.clone(),
            path: self.path.clone(),
        };
        // SAFETY: locally compiled fixture has no initializers and follows the
        // factory contract. Its private inode stays immutable through unload.
        let plugin =
            unsafe { PinnedPlugin::open(&self.path, &self.digest, "fixture", services) }.unwrap();
        assert!(mapped(&self.path));
        (plugin, evidence)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(
            !mapped(&self.path),
            "real module must be unmapped after owner release"
        );
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn real_library_owner_batch_queue_revoke_and_exactly_once_teardown() {
    let fixture = Fixture::build(0);
    let (plugin, evidence) = fixture.open();
    let mut transaction = plugin.begin().unwrap();
    assert!(transaction.stage(Record(evidence.clone())).is_ok());
    assert!(matches!(
        plugin.owner.as_ref().lease(),
        Err(native_host::Error::State)
    ));
    let batch = plugin.construct(transaction).unwrap();
    let queue = plugin.owner.as_ref().lease().unwrap();
    let depth = DispatchDepth::new();
    batch
        .dispatch(0, &depth, |_, _| {
            evidence.events.borrow_mut().push("dispatch")
        })
        .unwrap();
    plugin.owner.as_ref().revoke();
    assert_eq!(
        batch.dispatch(0, &depth, |_, _| panic!("revoked")),
        Err(native_host::Error::State)
    );
    assert_eq!(
        plugin.owner.as_ref().finish(),
        Err(native_host::Error::Busy)
    );
    drop(batch);
    assert_eq!(
        plugin.owner.as_ref().finish(),
        Err(native_host::Error::Busy)
    );
    drop(queue);
    plugin.owner.as_ref().finish().unwrap();
    plugin.owner.as_ref().finish().unwrap();
    assert_eq!(evidence.clocks.get(), 2);
    assert_eq!(evidence.frees.get(), 1);
    assert!(mapped(&fixture.path));
    drop(plugin);
    assert!(!mapped(&fixture.path));
    assert_eq!(
        &*evidence.events.borrow(),
        &[
            "make", "allocate", "dispatch", "record", "destroy", "free", "context"
        ]
    );
}

#[test]
fn real_library_valid_output_guard_cleans_poisoned_and_revoked_commits() {
    let fixture = Fixture::build(0);
    for revoke in [false, true] {
        let (plugin, evidence) = fixture.open();
        let mut transaction = plugin.begin().unwrap();
        let count = if revoke { 1 } else { 8 };
        for _ in 0..count {
            assert!(transaction.stage(Record(evidence.clone())).is_ok());
        }
        if !revoke {
            let (_, refused) = transaction.stage(Record(evidence.clone())).err().unwrap();
            drop(refused);
        }
        let pending = plugin.make(transaction).unwrap();
        if revoke {
            plugin.owner.as_ref().revoke();
        }
        assert!(matches!(pending.commit(), Err(Error::Lifecycle(_))));
        assert_eq!(evidence.clocks.get(), 2);
        assert_eq!(evidence.frees.get(), 1);
        assert_eq!(evidence.records.get(), count + usize::from(!revoke));
        assert!(plugin.owner.as_ref().context().attempted.get());
        drop(plugin);
        assert!(!mapped(&fixture.path));
        assert_eq!(evidence.events.borrow().last(), Some(&"context"));
    }
}

#[test]
fn real_library_factory_failure_and_null_success_abort_without_unvalidated_drop() {
    for (mode, expected) in [
        (1, RuntimeError::Factory(host_glue::ERR_INVALID)),
        (2, RuntimeError::Plugin),
    ] {
        let fixture = Fixture::build(mode);
        let (plugin, evidence) = fixture.open();
        let mut transaction = plugin.begin().unwrap();
        assert!(transaction.stage(Record(evidence.clone())).is_ok());
        assert!(
            matches!(plugin.construct(transaction), Err(Error::Runtime(actual)) if actual == expected)
        );
        assert_eq!(
            evidence.clocks.get(),
            1,
            "make ran, no unvalidated destructor call"
        );
        assert_eq!(evidence.records.get(), 1);
        assert_eq!(evidence.allocations.get(), 0);
        drop(plugin);
        assert!(!mapped(&fixture.path));
        assert_eq!(&*evidence.events.borrow(), &["make", "record", "context"]);
    }
}

#[test]
fn real_library_owner_drop_runs_validated_destructor_before_unmap() {
    let fixture = Fixture::build(0);
    let (plugin, evidence) = fixture.open();
    let transaction = plugin.begin::<()>().unwrap();
    let batch = plugin.construct(transaction).unwrap();
    drop(batch);
    drop(plugin);
    assert_eq!(
        &*evidence.events.borrow(),
        &["make", "allocate", "destroy", "free", "context"]
    );
    assert!(!mapped(&fixture.path));
}

#[test]
fn real_library_abandoned_pending_guard_drains_records_before_foreign_drop() {
    let fixture = Fixture::build(0);
    let (plugin, evidence) = fixture.open();
    let mut transaction = plugin.begin().unwrap();
    assert!(transaction.stage(Record(evidence.clone())).is_ok());
    let pending = plugin.make(transaction).unwrap();
    drop(pending);
    drop(plugin);
    assert_eq!(
        &*evidence.events.borrow(),
        &["make", "allocate", "record", "destroy", "free", "context"]
    );
    assert!(!mapped(&fixture.path));
}

#[test]
fn real_short_prefix_and_null_full_tail_construct_without_optional_callbacks() {
    for mode in [0, 3] {
        let fixture = Fixture::build(mode);
        let (plugin, evidence) = fixture.open();
        let transaction = plugin.begin::<()>().unwrap();
        let batch = plugin.construct(transaction).unwrap();
        let instance = plugin.owner.as_ref().context().instance.get().unwrap();
        assert!(instance.event.is_none());
        assert!(instance.draw.is_none());
        drop(batch);
        drop(plugin);
        assert_eq!(evidence.clocks.get(), 2);
        assert_eq!(evidence.allocations.get(), 1);
        assert_eq!(evidence.frees.get(), 1);
        assert!(!mapped(&fixture.path));
    }
}

#[test]
fn real_library_panicking_staged_drop_still_destroys_validated_foreign_output() {
    struct PanickingRecord {
        record: Option<Record>,
        panic_on_drop: bool,
    }
    impl Drop for PanickingRecord {
        fn drop(&mut self) {
            drop(self.record.take());
            assert!(!self.panic_on_drop, "expected staged record panic");
        }
    }
    let fixture = Fixture::build(0);
    let (plugin, evidence) = fixture.open();
    let mut transaction = plugin.begin().unwrap();
    for panic_on_drop in [true, false] {
        assert!(
            transaction
                .stage(PanickingRecord {
                    record: Some(Record(evidence.clone())),
                    panic_on_drop,
                })
                .is_ok()
        );
    }
    let pending = plugin.make(transaction).unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(pending)));
    assert!(result.is_err());
    assert_eq!(
        evidence.records.get(),
        2,
        "all records drain before foreign cleanup"
    );
    assert_eq!(
        evidence.clocks.get(),
        2,
        "make and foreign destroy each ran once"
    );
    assert_eq!(evidence.allocations.get(), 1);
    assert_eq!(evidence.frees.get(), 1);
    assert!(plugin.owner.as_ref().context().instance.get().is_none());
    assert!(evidence.live.get());
    assert!(mapped(&fixture.path));
    assert_eq!(
        &*evidence.events.borrow(),
        &["make", "allocate", "record", "record", "destroy", "free"]
    );
    drop(plugin);
    assert!(!evidence.live.get());
    assert!(!mapped(&fixture.path));
    assert_eq!(
        evidence.clocks.get(),
        2,
        "owner drop must not destroy twice"
    );
    assert_eq!(evidence.events.borrow().last(), Some(&"context"));
}

mod canvas;
mod e2e;
mod environment;
mod foreign;

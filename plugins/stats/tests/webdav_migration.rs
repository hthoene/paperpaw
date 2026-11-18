//! The password in the `webdav.ini` of Beta 1 to 3 (`testdata/webdav.ini.v1`) on
//! the real host (#323, the host gap of #229): the first start keeps it in the
//! passwords and keys and leaves the file as it was, the second start removes
//! the file, and no later start moves anything again.
//!
//! `PAPERPAW_WEBDAV_FIXTURE` names a file to use instead of this tree's
//! fixture: `tools/qualify-update-paths.sh` passes each beta tag's own.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, clippy::expect_used, reason = "test fixture")]
use paperpaw_core::native_host::shell_wiring::{
    CatalogEnv, HttpJob, NetInputs, NetLimits, Sealer, SecretsInputs, ServedReply, ServerReason,
    ShellHttp, ShellNetwork, WorkDevice, WorkInputs,
};
use paperpaw_core::{
    Kernel,
    native_host::{FixedTables, HostResources, HostWiring, flat::FlatDepth},
};
use paperpaw_native::{
    host_glue::{PpEntry, Services},
    metadata::{Feature, Host as AdmissionHost, Profile, Service},
};
use paperpaw_sdk::{DirEntry, Id, Instant, MemoryClass, MemoryStorage, Storage, StorageError};
use std::{
    alloc::Layout,
    cell::{Cell, RefCell},
    rc::Rc,
};

const OLD_FILE: &str = ".paperpaw/plugins/stats/webdav.ini";
const SECRETS: &str = ".paperpaw/secrets.txt";

#[derive(Clone, Debug)]
struct Card(Rc<MemoryStorage>);
impl Storage for Card {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.0.size(p)
    }
    fn write(&self, p: &str, b: &[u8]) -> Result<(), StorageError> {
        self.0.write(p, b)
    }
    fn append(&self, p: &str, b: &[u8]) -> Result<(), StorageError> {
        self.0.append(p, b)
    }
    fn list(&self, p: &str, f: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(p, f)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn sync(&self) -> Result<(), StorageError> {
        self.0.sync()
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.0.remove(p)
    }
}

#[derive(Default)]
struct Network {
    now: Cell<u64>,
}
impl ShellNetwork for Network {
    fn now_ms(&self) -> u64 {
        self.now.get()
    }
    fn network_up(&self) -> bool {
        true
    }
    fn network_demand(&self, _: bool) {}
}
impl WorkDevice for Network {
    fn now_ms(&self) -> u64 {
        self.now.get()
    }
    fn network_up(&self) -> bool {
        true
    }
    fn radio_on(&self) -> bool {
        true
    }
}
impl ShellHttp for Network {
    fn start(&self, _: &HttpJob<'_>) -> Result<(), i32> {
        Ok(())
    }
    fn abort(&self, _: u32) {}
    fn listen(&self, _: u32, _: u32, _: &mut [u8]) -> Result<usize, ServerReason> {
        Err(ServerReason::Refused)
    }
    fn unlisten(&self, _: u32) {}
    fn reply(&self, _: u32, _: &ServedReply<'_>) -> Result<(), i32> {
        Err(-1)
    }
    fn close_served(&self, _: u32, _: u32) {}
}

struct Device;
impl paperpaw_core::native_host::device_service::DeviceHost for Device {
    fn device(
        &self,
        visit: &mut dyn FnMut(&paperpaw_core::native_host::device_service::DeviceFacts<'_>),
    ) -> Result<(), i32> {
        visit(&paperpaw_core::native_host::device_service::DeviceFacts {
            id: "test-x4",
            model: "Test X4",
            catalog: "",
            class: 0,
            caps: 0,
            controls: 0,
            tls_sessions: 0,
            start: 0,
            slept_ms: None,
        });
        Ok(())
    }
}

struct Catalog;
impl CatalogEnv for Catalog {
    fn walking(&self) -> Option<u32> {
        None
    }
    fn today(&self) -> Option<u32> {
        Some(20_000)
    }
}

// This reversible fixture cipher is only a fake device keystore for dummy accounts.
struct TestSeal;
impl Sealer for TestSeal {
    fn seals(&self) -> bool {
        true
    }
    fn seal(&self, _: &[u8], plain: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        if out.len() < plain.len() {
            return Err(-3);
        }
        for (to, from) in out.iter_mut().zip(plain) {
            *to = *from ^ 0x5a;
        }
        Ok(plain.len())
    }
    fn open(&self, aad: &[u8], sealed: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        self.seal(aad, sealed, out)
    }
}

#[derive(Clone, Default)]
struct Heap(Rc<Cell<isize>>, Rc<RefCell<Vec<(u32, i32)>>>);
impl Services for Heap {
    fn micros(&self) -> u64 {
        0
    }
    fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        // SAFETY: host supplies a validated nonzero layout.
        let ptr = unsafe { std::alloc::alloc(Layout::from_size_align(size, align).unwrap()) };
        if !ptr.is_null() {
            self.0.set(self.0.get() + 1);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, size: usize, align: usize) {
        self.0.set(self.0.get() - 1);
        // SAFETY: allocation and layout came from this heap.
        unsafe { std::alloc::dealloc(ptr, Layout::from_size_align(size, align).unwrap()) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, size: usize, align: usize, new: usize) -> *mut u8 {
        // SAFETY: allocation and layout came from this heap.
        unsafe { std::alloc::realloc(ptr, Layout::from_size_align(size, align).unwrap(), new) }
    }
}
fn dropped(heap: &Heap, _: &str, token: u32, status: i32) {
    heap.1.borrow_mut().push((token, status));
}
fn teardown(
    heap: &Heap,
    _: &str,
    _: paperpaw_core::native_host::TeardownStep,
    _: paperpaw_core::native_host::Error,
) {
    heap.1.borrow_mut().push((u32::MAX, -6));
}

fn test_device() -> paperpaw_sdk::DeviceInfo {
    paperpaw_sdk::DeviceInfo::new(
        "test-x4",
        "Test X4",
        paperpaw_sdk::DisplayInfo::new(
            480,
            800,
            220,
            paperpaw_sdk::PixelFormat::Gray4,
            Default::default(),
            None,
        ),
        paperpaw_sdk::InputCaps::new(
            true,
            false,
            false,
            &[
                "back",
                "confirm",
                "left",
                "right",
                "page-back",
                "page-forward",
            ],
        ),
        MemoryClass::Small,
    )
}

struct World {
    kernel: Option<Kernel>,
    resources: Option<HostResources>,
    depth: Option<FlatDepth>,
    heap: Heap,
}

impl World {
    /// Starts the kernel on `card` with the stats plugin running.
    fn on_card(card: Card) -> Self {
        let network = Rc::new(Network::default());
        let resources = HostResources::try_new().unwrap();
        let wired = resources
            .wire(
                HostWiring::new()
                    .device(Rc::new(Device))
                    .ui(MemoryClass::Small)
                    .documents(MemoryClass::Small, 800)
                    .library(Rc::new(card.clone()), Rc::new(Catalog))
                    .files(Rc::new(card.clone()))
                    .secrets(SecretsInputs::new(Rc::new(card.clone()), Rc::new(TestSeal)))
                    .work(WorkInputs::new(network.clone(), MemoryClass::Small))
                    .net(NetInputs::new(
                        Rc::new(card.clone()),
                        network.clone(),
                        network.clone(),
                        NetLimits::new(45_056, 8),
                    )),
            )
            .unwrap();
        drop(wired);
        let config = "[core]\nlanguage=en\n[plugins]\nstats=on\n".to_owned();
        let kernel = Kernel::builder(test_device())
            .storage(card.clone())
            .user_config(config)
            .build()
            .unwrap();
        let mut world = Self {
            kernel: Some(kernel),
            resources: Some(resources),
            depth: Some(FlatDepth::try_new().unwrap()),
            heap: Heap::default(),
        };
        world.register(
            paperpaw_plugin_stats::NATIVE_ENTRY.entry(),
            "stats",
            FixedTables::NONE,
        );
        world.kernel().start(Instant(0));
        world
    }

    fn kernel(&mut self) -> &mut Kernel {
        self.kernel.as_mut().unwrap()
    }

    fn register(&mut self, entry: &'static PpEntry, name: &'static str, tables: FixedTables) {
        let config = self.kernel().native_config();
        let decision = self.kernel().native_decide(&config, name, true, false);
        if !decision.hosts() {
            self.list(name, paperpaw_sdk::PluginState::Off, decision.enabled());
            return;
        }
        let services: &'static [Service<'static>] = Box::leak(
            [
                "core.device",
                "core.document",
                "core.files",
                "core.library",
                "core.net",
                "core.registry",
                "core.secrets",
                "core.ui",
                "core.work",
            ]
            .map(|key| Service {
                key,
                major: 1,
                semantic_revision: 0,
                min_table_bytes: 0,
            })
            .to_vec()
            .into_boxed_slice(),
        );
        let features: &'static [Feature<'static>] = Box::leak(
            [
                "uses.library",
                "uses.storage",
                "uses.network",
                "uses.secrets",
                "uses.power",
            ]
            .map(|key| Feature { key, revision: 1 })
            .to_vec()
            .into_boxed_slice(),
        );
        let host = AdmissionHost {
            abi_major: 7,
            minor: 1,
            profile: Profile {
                target: "linux-x86_64-sysv64-le",
                entry_layout: 1,
                entry_bytes: 32,
            },
            services,
            features,
        };
        // SAFETY: linked code and metadata remain live throughout the test.
        let metadata =
            unsafe { std::slice::from_raw_parts(entry.metadata.ptr, entry.metadata.len) };
        let resources = self.resources.as_ref().unwrap().try_clone().unwrap();
        let heap = self.heap.clone();
        let depth = self.depth.as_ref().unwrap().try_clone().unwrap();
        // SAFETY: linked entry and shared host objects outlive the kernel registration.
        unsafe {
            self.kernel().register_linked(
                entry,
                metadata,
                &host,
                name,
                heap,
                resources,
                &depth,
                teardown,
                dropped,
                (std::ptr::null(), 0),
                tables,
            )
        }
        .expect(name);
        self.list(name, paperpaw_sdk::PluginState::Running, true);
    }
    fn list(&mut self, name: &'static str, state: paperpaw_sdk::PluginState, enabled: bool) {
        use paperpaw_sdk::{Manifest, ManifestRef, PluginInfo, PluginKind, RequiredFor, Version};
        let manifest = ManifestRef::try_new(
            Manifest::new(Id::new_static(name), name, Version::parse("1.0.0").unwrap()),
            None,
        )
        .unwrap();
        self.kernel()
            .list_native(PluginInfo::new(
                manifest,
                state,
                enabled,
                RequiredFor::default(),
                PluginKind::BuiltIn,
                None,
                None,
            ))
            .unwrap();
    }
}

impl Drop for World {
    fn drop(&mut self) {
        drop(self.kernel.take());
        drop(self.resources.take());
        drop(self.depth.take());
        if !std::thread::panicking() {
            assert_eq!(self.heap.0.get(), 0, "native blocks after teardown");
            assert!(
                self.heap.1.borrow().is_empty(),
                "{:?}",
                self.heap.1.borrow()
            );
        }
    }
}

fn fixture() -> Vec<u8> {
    match std::env::var_os("PAPERPAW_WEBDAV_FIXTURE") {
        Some(path) => std::fs::read(path).unwrap(),
        None => include_bytes!("../testdata/webdav.ini.v1").to_vec(),
    }
}

fn start(card: &Card) {
    drop(World::on_card(card.clone()));
}

fn bytes(card: &Card, path: &str) -> Option<Vec<u8>> {
    let size = usize::try_from(card.size(path).ok()?).unwrap();
    let mut data = vec![0; size];
    assert_eq!(card.read_at(path, 0, &mut data).unwrap(), size);
    Some(data)
}

/// The password kept for the plugin, as the secret store holds it.
fn kept(card: &Card) -> Option<String> {
    let text = String::from_utf8(bytes(card, SECRETS)?).unwrap();
    let mut found = text.lines().filter(|line| line.starts_with("stats."));
    let line = found.next()?;
    assert!(found.next().is_none(), "one secret only: {text}");
    let veiled = line
        .rsplit('\t')
        .next()
        .unwrap()
        .strip_prefix("s1:")
        .unwrap();
    let plain: Vec<u8> = (0..veiled.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&veiled[at..at + 2], 16).unwrap() ^ 0x5a)
        .collect();
    Some(String::from_utf8(plain).unwrap())
}

#[test]
fn the_beta_password_moves_to_the_secrets_once_and_the_file_goes_at_the_next_start() {
    let old = fixture();
    let card = Card(Rc::new(MemoryStorage::new()));
    card.write(OLD_FILE, &old).unwrap();

    // First start: kept, and the file is as the beta left it (the only copy until now).
    start(&card);
    assert_eq!(kept(&card).as_deref(), Some("secret 1"));
    assert_eq!(bytes(&card, OLD_FILE).as_deref(), Some(old.as_slice()));
    let secrets = bytes(&card, SECRETS);

    // Second start: the password is there, so the file goes, and the secret stays.
    start(&card);
    assert_eq!(bytes(&card, OLD_FILE), None);
    assert_eq!(bytes(&card, SECRETS), secrets);

    // Later starts move nothing: a password the reader chose since stays.
    start(&card);
    assert_eq!(bytes(&card, OLD_FILE), None);
    assert_eq!(bytes(&card, SECRETS), secrets);
}

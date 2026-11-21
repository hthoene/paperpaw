//! Real native host, reader and network dispatch over an in-memory card and transport.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test fixture"
)]
use paperpaw_core::native_host::shell_wiring::{
    CatalogEnv, HttpDone, HttpJob, HttpReason, NetInputs, NetLimits, NetReports, Sealer,
    SecretsInputs, ServedReply, ServerReason, ShellHttp, ShellNetwork, WorkDevice, WorkInputs,
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

#[derive(Clone, Debug)]
pub struct Card(pub Rc<MemoryStorage>);
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

pub struct Request {
    pub id: u32,
    pub method: String,
    pub url: String,
    pub body: Vec<u8>,
}
#[derive(Default)]
pub struct Network {
    pub now: Cell<u64>,
    pub requests: RefCell<Vec<Request>>,
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
    fn start(&self, job: &HttpJob<'_>) -> Result<(), i32> {
        self.requests.borrow_mut().push(Request {
            id: job.id,
            method: job.method.to_owned(),
            url: job.url.to_owned(),
            body: job.body.unwrap_or_default().to_vec(),
        });
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

pub struct World {
    kernel: Option<Kernel>,
    resources: Option<HostResources>,
    depth: Option<FlatDepth>,
    heap: Heap,
    pub card: Card,
    pub network: Rc<Network>,
    reports: NetReports,
}
impl World {
    pub fn new() -> Self {
        Self::with_account(false)
    }
    pub fn signed_in() -> Self {
        Self::with_account(true)
    }
    pub fn restart(card: Card) -> Self {
        Self::on_card(card, false, true, true)
    }
    pub fn menu(installed: bool, enabled: bool) -> Self {
        Self::with_plugins(false, installed, enabled)
    }
    fn with_account(signed_in: bool) -> Self {
        Self::with_plugins(signed_in, true, true)
    }
    /// Signed out, in the Offline power mode (the core's `core.offline`,
    /// worn again at the start as the reader left it).
    pub fn offline() -> Self {
        let card = Self::card(false);
        card.write(".paperpaw/power.ini", b"mode = core.offline\n")
            .unwrap();
        Self::on_card(card, false, true, true)
    }
    fn with_plugins(signed_in: bool, installed: bool, enabled: bool) -> Self {
        Self::on_card(Self::card(signed_in), signed_in, installed, enabled)
    }
    fn card(signed_in: bool) -> Card {
        let card = Card(Rc::new(
            MemoryStorage::new().with(
                "book.txt",
                "A paragraph for the native reader handover.\n\n"
                    .repeat(2000)
                    .into_bytes(),
            ),
        ));
        if signed_in {
            let sealed: String = b"test-only-key"
                .iter()
                .map(|b| format!("{:02x}", b ^ 0x5a))
                .collect();
            card.write(
                ".paperpaw/secrets.txt",
                format!("kosync.key\tTest account\t\ts1:{sealed}\n").as_bytes(),
            )
            .unwrap();
        }
        card
    }
    fn on_card(card: Card, signed_in: bool, installed: bool, enabled: bool) -> Self {
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
        let reports = wired.net().unwrap().clone();
        let device = test_device();
        let config = format!(
            "[core]\nlanguage=en\n[kosync]\n{}server=https://fake.invalid/kosync\nauto=true\n[reader]\nopen-last=false\n[plugins]\nkosync={}\n",
            if signed_in { "user=test-reader\n" } else { "" },
            if enabled { "on" } else { "off" }
        );
        let kernel = Kernel::builder(device)
            .storage(card.clone())
            .built_in_fonts(&paperpaw_plugin_fonts_default::DEFAULT_FONT_PACKS)
            .user_config(config)
            .build()
            .unwrap();
        let mut world = Self {
            kernel: Some(kernel),
            resources: Some(resources),
            depth: Some(FlatDepth::try_new().unwrap()),
            heap: Heap::default(),
            card,
            network,
            reports,
        };
        world.register(
            paperpaw_plugin_reader::NATIVE_ENTRY.entry(),
            "reader",
            FixedTables::new(
                paperpaw_plugin_reader::NATIVE_SETTINGS,
                paperpaw_plugin_reader::NATIVE_GROUPS,
            )
            .with_ids(paperpaw_plugin_reader::NATIVE_IDS)
            .with_titles(paperpaw_plugin_reader::NATIVE_TITLES),
        );
        world.register(
            paperpaw_plugin_format_txt::NATIVE_ENTRY.entry(),
            "format-txt",
            FixedTables::NONE,
        );
        world.register(
            paperpaw_plugin_keyboard::NATIVE_ENTRY.entry(),
            "keyboard",
            FixedTables::new(
                paperpaw_plugin_keyboard::NATIVE_SETTINGS,
                paperpaw_plugin_keyboard::NATIVE_GROUPS,
            )
            .with_ids(paperpaw_plugin_keyboard::NATIVE_IDS)
            .with_titles(paperpaw_plugin_keyboard::NATIVE_TITLES),
        );
        world.register(
            paperpaw_plugin_menu::NATIVE_ENTRY.entry(),
            "menu",
            FixedTables::new(
                paperpaw_plugin_menu::NATIVE_SETTINGS,
                paperpaw_plugin_menu::NATIVE_GROUPS,
            )
            .with_ids(paperpaw_plugin_menu::NATIVE_IDS)
            .with_titles(paperpaw_plugin_menu::NATIVE_TITLES),
        );
        if installed {
            world.register(
                paperpaw_plugin_kosync::NATIVE_ENTRY.entry(),
                "kosync",
                FixedTables::NONE,
            );
        }
        world.add_catalogs();
        world.kernel().start(Instant(0));
        world
    }

    /// The texts the plugins ask for, with the core's (`core.on`, `core.off`).
    fn add_catalogs(&mut self) {
        for text in [
            include_str!("../../../../crates/paperpaw-core/i18n/en.ini"),
            include_str!("../../i18n/en.ini"),
            include_str!("../../../reader/i18n/en.ini"),
            include_str!("../../../keyboard/i18n/en.ini"),
            include_str!("../../../menu/i18n/en.ini"),
        ] {
            self.kernel().add_catalog(text);
        }
    }

    pub fn kernel(&mut self) -> &mut Kernel {
        self.kernel.as_mut().unwrap()
    }
    pub fn register(&mut self, entry: &'static PpEntry, name: &'static str, tables: FixedTables) {
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
                // `command.go_online` (the offer to go online, #459).
                "uses.wifi",
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
    pub fn action(&mut self, action: &'static str) {
        let now = self.network.now.get() + 100;
        self.network.now.set(now);
        self.kernel()
            .run_action(Instant(now), Id::new_static(action));
        self.pump();
    }
    /// A short press of the button `name`: a user input, which the host's
    /// window for `command.go_online` needs (an action run is none).
    pub fn press(&mut self, name: &'static str) {
        use paperpaw_platform::{ButtonState, InputEvent};
        let now = self.network.now.get() + 100;
        self.network.now.set(now + 50);
        for (at, state) in [
            (now, ButtonState::Pressed),
            (now + 50, ButtonState::Released),
        ] {
            self.kernel()
                .handle(Instant(at), InputEvent::Button { name, state });
        }
        self.pump();
    }
    pub fn pump(&mut self) {
        for _ in 0..30 {
            let now = self.network.now.get() + 10;
            self.network.now.set(now);
            self.kernel().tick(Instant(now));
        }
    }
    pub fn complete(&mut self, index: usize, status: u32, body: &[u8]) {
        let id = self.network.requests.borrow()[index].id;
        let mut done = HttpDone::new(status, HttpReason::Ok);
        done.body = body.to_vec();
        self.reports.finished(id, done).unwrap();
        self.pump();
    }
    pub fn assert_clean(&self) {
        assert!(
            self.heap.1.borrow().is_empty(),
            "{:?}",
            self.heap.1.borrow()
        );
    }
}
impl Drop for World {
    fn drop(&mut self) {
        drop(self.kernel.take());
        drop(self.resources.take());
        drop(self.depth.take());
        if !std::thread::panicking() {
            assert_eq!(self.heap.0.get(), 0, "native blocks after teardown");
            self.assert_clean();
        }
    }
}

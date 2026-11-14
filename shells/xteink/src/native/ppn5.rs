//! The X4's plugin files for interface 7 (format 5, `abi/ppn5.md`), as far
//! as the core can take them today (feature `ppn5`, off by default).
//!
//! What this does, in the note's order (`Header5` and `stream5` of
//! `paperpaw_native` hold every rule and are tested on the host):
//!
//! 1. the header is read and checked against this firmware (steps 1 to 7);
//!    the file must be the one installed;
//! 2. the room is found and the image placed (flash on the ESP32-C3; PSRAM on
//!    the ESP32-S3, target 9: the X4 Pro and the X4 Classic, decision 142);
//! 3. the digest, every word to relocate, and the entry and metadata as they
//!    will lie once placed (relocated in RAM) are checked on the file,
//!    before a sector of the flash is erased (`Flash::put5`, steps 8 and 9):
//!    a deterministic refusal writes nothing, and the layout forgets the
//!    file as if it had not been placed;
//! 4. the image is relocated for its place and written, a sector at a time,
//!    through a buffer of one sector: nothing is held in proportion to the
//!    file, the image or the number of words;
//! 5. the entry the written image holds is checked once more against the
//!    header and the metadata record (`Header5::check_entry`, step 9), which
//!    catches only a flash that did not keep what it was given; a refusal
//!    there rewinds the layout.
//!
//! 6. the entry's `make` runs (X2): in the registration scope of the
//!    core's shared owner (`paperpaw_core::native_host::shell`), as the
//!    first plugin code the X4 firmware runs for interface 7. Its output is
//!    committed, wrapped as a core `FlatPlugin` and returned to the kernel.
//!
//! **Plugin code and the firmware.** The plugin has no sandbox, as format 4.
//! Around every call into its code (`make`, a callback, its destructor) the
//! shell keeps the plugin's id in RTC memory (`safe_mode::calling`) and,
//! when the call returns, checks the stack's end (`stack::check`, a hit
//! restarts the X4 with the id still set) before clearing it
//! (`safe_mode::returned`). A reset anywhere in a call, whenever in the
//! session, switches that plugin off at the next start. The host entries a
//! plugin can call (`clock_micros`, `registrations_submit`) do **not** feed
//! the watchdog: a plugin that loops on them is a hang the watchdog resets,
//! and the id says whose it was. A file refused before `make` leaves nothing
//! behind: the layout takes it back. After `make` ran the layout is never
//! rewound in this session (the code may be referenced by what the plugin
//! made); the region is reclaimed at the next start.
//!
//! Off by default (`ppn5`); not yet run on a device.

use alloc::boxed::Box;
use log::warn;
use paperpaw_core::native_host::HostResources;
use paperpaw_core::native_host::flat::FlatDepth;
use paperpaw_core::{CodeFile, NativeAdmission};
use paperpaw_native::MAGIC;
use paperpaw_native::header5::{HEADER_LEN, Header5, Host, Refusal};
use paperpaw_platform::Storage;
use paperpaw_sdk::PluginProblem;

/// What this firmware is, to compare a file with: the X4 (target 5) or the X4
/// Pro and X4 Classic (target 9) at the interface minor the generated ABI says.
#[cfg(feature = "esp32c3")]
pub(super) const HOST: Host = Host::x4(paperpaw_native::host_glue::MINOR);
#[cfg(feature = "esp32s3")]
pub(super) const HOST: Host = Host::x4_pro(paperpaw_native::host_glue::MINOR);

/// The metadata profile of this firmware's chip (`abi/entry.md`).
#[cfg(feature = "esp32c3")]
const PROFILE: paperpaw_native::metadata::Profile<'static> = paperpaw_native::header5::X4_PROFILE;
#[cfg(feature = "esp32s3")]
const PROFILE: paperpaw_native::metadata::Profile<'static> = paperpaw_native::header5::S3_PROFILE;

/// The newest format a header may carry for [`is_format5`] to take the file
/// as one of ours: the reader below says what it is.
const FIRST_FORMAT: u16 = paperpaw_native::header5::FORMAT;

/// Whether `head` (the file's first bytes) is a plugin file of format 5 or
/// later, which is this module's to read; older ones are the format-4
/// loader's.
pub fn is_format5(head: &[u8]) -> bool {
    head.len() >= 6 && head[..4] == MAGIC && u16::from_le_bytes([head[4], head[5]]) >= FIRST_FORMAT
}

/// What the plugin manager says for a header this firmware refuses.
fn refused(path: &str, head: &[u8], why: Refusal) -> PluginProblem {
    warn!("native: {path}: format 5: {why:?}");
    match why {
        Refusal::OlderInterface => PluginProblem::OlderInterface {
            built_for: head
                .get(8..10)
                .map_or(0, |b| u16::from_le_bytes([b[0], b[1]])),
        },
        Refusal::NewerFormat
        | Refusal::NewerInterface
        | Refusal::NeedsNewerMinor
        | Refusal::Reserved => PluginProblem::NeedsNewerPaperPaw,
        Refusal::UnknownChip | Refusal::OtherChip | Refusal::NoProfile => PluginProblem::OtherBuild,
        _ => PluginProblem::Damaged,
    }
}

/// A format-5 file checked and placed: its header, where it lies, its entry
/// and what it declares to need, and when its loading began.
struct Verified {
    header: Header5,
    placed: crate::native::Placed5,
    make: u32,
    declared: paperpaw_core::native_host::shell::DeclaredServices,
}

/// Steps 1 to 5 of the [module documentation](self): the header read and
/// checked against this firmware, the file the installed one, the image
/// placed and its entry checked. Nothing of the plugin has run.
fn verify(
    place: &mut super::Place,
    storage: &dyn Storage,
    code: &CodeFile<'_>,
) -> Result<Verified, PluginProblem> {
    let path = code.path;
    #[cfg(feature = "esp32s3")]
    if !scanned_table_is_handed() {
        warn!(
            "native: {path} not run: the host table the frame scan read is not the one handed to plugins"
        );
        return Err(PluginProblem::Unsupported);
    }
    let started = crate::micros();
    let size = storage.size(path).map_err(|_| PluginProblem::Damaged)?;
    let mut head = [0u8; HEADER_LEN];
    let have = size.min(HEADER_LEN as u64) as usize;
    super::read_exact(storage, path, 0, &mut head[..have]).map_err(|()| PluginProblem::Damaged)?;
    let header =
        Header5::read(&head[..have], size, &HOST).map_err(|why| refused(path, &head, why))?;
    // The file installed, not one put in its folder since.
    if header.sha256 != code.sha256 {
        warn!("native: {path} is not the file installed");
        return Err(PluginProblem::Changed);
    }
    let (placed, make, declared) = x4::verified(place, storage, code, &header, started)?;
    Ok(Verified {
        header,
        placed,
        make,
        declared,
    })
}

/// Hosts the format-5 file `code` for the kernel to admit with everything it
/// declares (`CodeLoader::load_native`): the checks and placement, then the hosted plugin, not made yet (see [`x4::host`]).
pub fn host(
    place: &mut super::Place,
    depth: &FlatDepth,
    resources: &mut Option<HostResources>,
    device: &alloc::rc::Rc<paperpaw_x4::device_host::X4Device>,
    storage: &dyn Storage,
    code: &CodeFile<'_>,
) -> Result<Box<dyn NativeAdmission>, PluginProblem> {
    let Verified {
        header,
        placed,
        make,
        declared,
        ..
    } = verify(place, storage, code)?;
    x4::host(
        place, code, &header, &placed, make, depth, resources, device, declared,
    )
}

#[cfg(feature = "native-on")]
pub(crate) use x4::register_linked;
#[cfg(feature = "esp32s3")]
pub(super) use x4::scanned_table_is_handed;
pub(super) use x4::{StorageSource, entry_check_failed, heap_word};

/// The loader both chips share (the module keeps its first name): the image is
/// placed (flash on the ESP32-C3, PSRAM on the ESP32-S3), its entry read, and
/// `make` run. What is the chip's is [`HOST`], [`PROFILE`] and
/// `crate::native::Placed5`.
mod x4 {
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    use core::alloc::Layout;

    use log::{info, warn};
    use paperpaw_core::CodeFile;
    use paperpaw_core::NativeAdmission;
    use paperpaw_core::native_host::HostResources;
    use paperpaw_core::native_host::flat::FlatDepth;
    use paperpaw_core::native_host::hosted_admission::HostedAdmission;
    use paperpaw_core::native_host::shell::{self, Hosted, Make};
    use paperpaw_native::header5::{
        EntryFacts, EntryRefusal, HEADER_LEN, Header5, RawEntry, RecordFacts,
    };
    use paperpaw_native::host_glue::Services;
    use paperpaw_native::metadata;
    use paperpaw_platform::Storage;
    use paperpaw_sdk::PluginProblem;

    use paperpaw_native::stream5::{EntryCheck, ReadError, Source};

    use super::HOST;

    /// A file of the card read at offsets, for `stream5`. Every read tells the
    /// watchdogs the work goes on: the digest of a whole file takes seconds.
    pub struct StorageSource<'a> {
        storage: &'a dyn Storage,
        path: &'a str,
    }

    impl<'a> StorageSource<'a> {
        pub fn new(storage: &'a dyn Storage, path: &'a str) -> Self {
            Self { storage, path }
        }
    }

    impl Source for StorageSource<'_> {
        fn read_at(&self, at: u64, buf: &mut [u8]) -> Result<(), ReadError> {
            crate::power::feed();
            crate::native::read_exact(self.storage, self.path, at, buf).map_err(|()| ReadError)
        }
    }

    /// Why the file's entry, checked in RAM before the flash was touched, is
    /// refused: a damaged file.
    pub(in crate::native) fn entry_check_failed(path: &str, why: EntryCheck) -> PluginProblem {
        warn!("native: {path}: format 5: refused before writing: {why:?}");
        PluginProblem::Damaged
    }

    /// An entry the image holds that is not right: a damaged file.
    fn entry_refused(path: &str, why: EntryRefusal) -> PluginProblem {
        warn!("native: {path}: format 5: the entry: {why:?}");
        PluginProblem::Damaged
    }

    /// What the plugin manager says for a record the admission refuses.
    fn admission_refused(path: &str, why: shell::admission::Admission<'_>) -> PluginProblem {
        use shell::admission::Admission;
        // A short code and the key, not the whole `Debug` (flash).
        match why {
            Admission::Refused(metadata::Refusal::Service { key }) => {
                warn!("native: {path}: format 5: metadata: no such service {key}");
            }
            Admission::Refused(metadata::Refusal::RequiredFeature { key, .. }) => {
                warn!("native: {path}: format 5: metadata: no such feature {key}");
            }
            Admission::Refused(_) => warn!("native: {path}: format 5: metadata: refused"),
            Admission::Invalid | Admission::NoRoom => {
                warn!("native: {path}: format 5: metadata: services not kept");
            }
        }
        match why {
            Admission::NoRoom => PluginProblem::NoRoom,
            Admission::Refused(metadata::Refusal::Incompatible) => PluginProblem::OtherBuild,
            // A service or feature this firmware does not offer: a newer
            // PaperPaw might.
            Admission::Refused(
                metadata::Refusal::Service { .. } | metadata::Refusal::RequiredFeature { .. },
            ) => PluginProblem::NeedsNewerPaperPaw,
            Admission::Refused(metadata::Refusal::Malformed) | Admission::Invalid => {
                PluginProblem::Damaged
            }
        }
    }

    /// Places the image of `header`, checks its entry, and takes it back if
    /// the entry is refused.
    pub(super) fn verified(
        place: &mut crate::native::Place,
        storage: &dyn Storage,
        code: &CodeFile<'_>,
        header: &Header5,
        started: u64,
    ) -> Result<(crate::native::Placed5, u32, shell::DeclaredServices), PluginProblem> {
        let path = code.path;
        // The metadata record as the file has it: at most 2 KiB, however large
        // the file.
        let mut record = Vec::new();
        record
            .try_reserve_exact(header.meta_len as usize)
            .map_err(|_| PluginProblem::NoRoom)?;
        record.resize(header.meta_len as usize, 0);
        crate::native::read_exact(
            storage,
            path,
            HEADER_LEN as u64 + u64::from(header.meta_at),
            &mut record,
        )
        .map_err(|()| PluginProblem::Damaged)?;
        // The record's body against what this firmware offers, before the
        // image is written: a plugin that asks for a service this firmware
        // lacks costs no flash. What it declares is kept for the host, which
        // refuses a query of any other service; the prefix facts the header
        // is compared with are the check's own, read once.
        // What a card plugin is offered: the services of every family this
        // start wired, as the linked plugins' (`paperpaw_x4::native_offer`),
        // and of the features only the families it may reach
        // (`card_features`: not `uses.secrets`, which reaches other plugins'
        // secrets). A plugin that needs more is refused here, costing no flash.
        // The radio's commands are not offered (`wifi` false).
        let wired = crate::device_host::wired_families(false);
        let offered =
            paperpaw_x4::native_offer::card_host(wired, HOST.abi_major, HOST.minor, super::PROFILE);
        let admitted = shell::admission::admit(&record, &offered)
            .map_err(|why| admission_refused(path, why))?;
        let facts = RecordFacts {
            abi_major: admitted.abi_major,
            needs_minor: admitted.needs_minor,
            entry_bytes: u32::from(admitted.entry_bytes),
        };
        let declared = admitted.declared;
        let placed = place.put5(storage, path, header)?;
        let make = match check_placed(path, header, &placed, &facts, &record) {
            Ok(make) => make,
            Err(why) => {
                place.take_back(placed);
                return Err(why);
            }
        };
        info!(
            "native: {path}: format 5, {} B at {:#x}, {} words moved, {} patched; \
             placed and its entry checked in {} us",
            header.image_len,
            placed.base,
            header.relocs,
            header.patches,
            crate::micros() - started
        );
        Ok((placed, make, declared))
    }

    /// Step 9 on the image as it lies in flash: reads the entry and the
    /// metadata view where the header says they are, and gives the header what
    /// it found.
    fn check_placed(
        path: &str,
        header: &Header5,
        placed: &crate::native::Placed5,
        facts: &RecordFacts,
        record: &[u8],
    ) -> Result<u32, PluginProblem> {
        // SAFETY: the entry and the metadata lie in the image just placed
        // (`Header5::read` checked `entry + entry_bytes <= code_at <= image_len`
        // and `meta_at + meta_len <= code_at`), written, its cache lines
        // invalidated, in the region mapped at `placed.base`; read-only bytes.
        let (entry, metadata) = unsafe {
            (
                core::slice::from_raw_parts(
                    (placed.base + header.entry) as *const u8,
                    paperpaw_native::header5::X4_ENTRY_BYTES as usize,
                ),
                core::slice::from_raw_parts(
                    (placed.base + header.meta_at) as *const u8,
                    header.meta_len as usize,
                ),
            )
        };
        let raw = RawEntry::read(entry).ok_or(PluginProblem::Damaged)?;
        let found = EntryFacts {
            size: raw.size,
            abi_major: raw.abi_major,
            needs_minor: raw.needs_minor,
            metadata_ptr: raw.metadata_ptr,
            metadata_len: raw.metadata_len,
            metadata,
            make_at: header.make_offset(raw.make, placed.base, placed.hot),
        };
        // `make` was read from the written flash and is checked there: inside
        // the image's code, even (`Header5::check_entry`, step 9).
        header
            .check_entry(placed.base, &HOST, facts, record, &found)
            .map_err(|why| entry_refused(path, why))?;
        Ok(raw.make)
    }

    /// The program's read-only data in flash as a mapping (address and length),
    /// from the linker's own symbols (`esp-hal`'s `rodata.x`: `.rodata` and
    /// `.srodata`, where the linked plugins' declaration bytes sit). It stays
    /// mapped for the whole run and is never written. `(null, 0)` if the
    /// symbols are not in order (end not after start): the declarations are
    /// then copied, as before.
    #[cfg(feature = "native-on")]
    fn flash_rodata() -> (*const u8, usize) {
        unsafe extern "C" {
            static _rodata_start: u8;
            static _rodata_end: u8;
        }
        let start = core::ptr::addr_of!(_rodata_start);
        let end = core::ptr::addr_of!(_rodata_end);
        match (end as usize).checked_sub(start as usize) {
            Some(len) if len > 0 => (start, len),
            _ => (core::ptr::null(), 0),
        }
    }

    /// The X4's heap and clock for a plugin: the firmware's allocator, so a
    /// plugin's allocations count against the heap like everything else.
    pub(super) struct X4Services;

    /// The firmware's `PpHeap` table for the heap word of a plugin's image
    /// (`abi/heap-word.md` section 3); no `abort`: a plugin panic ends in the
    /// trap, which the crash slot blames on the frame owner.
    static HEAP_TABLE: paperpaw_native::host_glue::PpHeap = shell::heap_table::<X4Services>();

    /// The address of [`HEAP_TABLE`], written into a placed image's heap word.
    pub(crate) fn heap_word() -> u32 {
        core::ptr::addr_of!(HEAP_TABLE).addr() as u32
    }

    /// The host table of this firmware's services as a named, read-only
    /// static, for `paperpaw-sign frames-s3`: the scan reads a table's function
    /// pointers from the ELF by the symbol's name (the table the loader hands a
    /// plugin is a promoted constant with no symbol of its own). Built by the
    /// same `host_table::<X4Services>()` that makes that constant, so the
    /// roots it gives are the ones a plugin reaches; it is never read at run
    /// time (`#[used]` keeps it, about 150 bytes of flash on the S3 only).
    #[cfg(feature = "esp32s3")]
    #[used]
    #[unsafe(no_mangle)]
    pub static PAPERPAW_HOST_TABLE_ANCHOR: paperpaw_native::host_glue::PpHost =
        shell::host_table::<X4Services>();

    /// Whether the static the S3 scan reads ([`PAPERPAW_HOST_TABLE_ANCHOR`])
    /// is the table the loader hands plugins (`handed_table`), slot by slot.
    /// If they ever differed the scan would check the roots of another table
    /// than the one a plugin calls through; checked before the first file is
    /// placed, a few words compared.
    #[cfg(feature = "esp32s3")]
    pub(in crate::native) fn scanned_table_is_handed() -> bool {
        shell::same_table(
            &PAPERPAW_HOST_TABLE_ANCHOR,
            shell::handed_table::<X4Services>(),
        )
    }

    impl Services for X4Services {
        fn alloc(&self, size: usize, align: usize) -> *mut u8 {
            match Layout::from_size_align(size, align) {
                // SAFETY: the layout is valid and its size is not zero.
                Ok(layout) if size != 0 => unsafe { alloc::alloc::alloc(layout) },
                _ => core::ptr::null_mut(),
            }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, size: usize, align: usize) {
            if let Ok(layout) = Layout::from_size_align(size, align) {
                // SAFETY: the trait's contract: this allocator's block, once.
                unsafe { alloc::alloc::dealloc(ptr, layout) };
            }
        }

        unsafe fn realloc(
            &self,
            ptr: *mut u8,
            old_size: usize,
            align: usize,
            new_size: usize,
        ) -> *mut u8 {
            match (
                Layout::from_size_align(old_size, align),
                Layout::from_size_align(new_size, align),
            ) {
                // SAFETY: as `dealloc`; both layouts are valid, `new_size` not zero.
                (Ok(layout), Ok(_)) if new_size != 0 => unsafe {
                    alloc::alloc::realloc(ptr, layout, new_size)
                },
                _ => core::ptr::null_mut(),
            }
        }

        fn micros(&self) -> u64 {
            // Not the watchdog's business: a plugin that loops on the clock
            // is a hang the watchdog resets.
            crate::micros()
        }

        /// A plugin from a card file runs only inside the call hooks that
        /// name it to safe mode and check the stack's end afterwards (the
        /// linked plugins are the firmware's own and are not announced).
        fn enter(&self, id: &str) -> bool {
            call_begin(id)
        }

        fn leave(&self, id: &str) {
            call_end(id);
        }
    }

    /// What the owner keeps alive for a plugin on the X4: nothing to unload.
    /// The image stays placed and mapped for the session (and a region is
    /// reused only after a restart, so nothing here can be freed under a
    /// plugin that still runs).
    pub(super) struct FlashModule;

    /// Set while the linked plugins are made (`register_linked`): every call
    /// into one feeds the watchdogs, so that making them all, with the card
    /// and the heap busy, is not taken for a hang (test image, Senior's
    /// condition on #2462). Not set later: a plugin that loops in a call is
    /// still the watchdog's.
    #[cfg(feature = "native-on")]
    static CREATING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

    fn call_begin(id: &str) -> bool {
        // The ESP32-S3's band needs the call to start at interrupt level 0
        // on the first core (todo 185); the ESP32-C3's band must be armed and
        // not lost: otherwise it is refused, and the plugin is not run.
        if !crate::stack::dispatch_allowed() {
            return false;
        }
        #[cfg(feature = "native-on")]
        if CREATING.load(core::sync::atomic::Ordering::Relaxed) {
            crate::power::feed();
        }
        crate::safe_mode::calling(id);
        true
    }

    fn call_end(_id: &str) {
        // A hit restarts the X4 by a panic with the id still set.
        crate::stack::check();
        crate::safe_mode::returned();
    }

    /// Hosts the linked plugins (`paperpaw_app::NATIVE_PLUGINS`): the app's one
    /// path (`paperpaw_app::register_native_plugins`, the desktop's too), with
    /// the services and features this start wired (`paperpaw_x4::native_offer`,
    /// admitted against every linked plugin by the desktop's tests). Called by
    /// the main loop once the stack's band is armed (`abi/heap-word.md`
    /// section 6): without it no plugin of interface 7 runs. A plugin the host
    /// refuses is logged and does not stop the others.
    #[cfg(feature = "native-on")]
    #[inline(never)]
    pub(crate) fn register_linked(
        kernel: &mut paperpaw_core::Kernel,
        table: &crate::native::DeviceTable,
    ) {
        fn linked_report(_: &X4Services, plugin: &str, tag: u32, status: i32) {
            warn!("native: linked: {plugin}: call {tag} ended with {status}");
        }
        use paperpaw_x4::native_offer::{features, services};
        let wired = crate::device_host::wired_families(kernel.wifi_model().is_some());
        let offered = metadata::Host {
            abi_major: HOST.abi_major,
            minor: HOST.minor,
            profile: super::PROFILE,
            services: services(wired),
            features: features(wired),
        };
        // The loader's own table of handles and callback-depth counter (one of
        // each for the device), not a second: the kernel refuses a plugin that
        // brings another counter.
        let (Some(depth), Some(resources)) = (table.depth(), table.share()) else {
            warn!("native: no room for the linked plugins' table: none hosted");
            return;
        };
        // This start loads native plugins: counted as one that may fail until
        // its first frame is shown (`safe_mode::started`); after two in a
        // row the next goes without them (`paperpaw_x4::start_count`).
        crate::safe_mode::loading("native-linked");
        let began = crate::micros();
        CREATING.store(true, core::sync::atomic::Ordering::Relaxed);
        // SAFETY: the entries are `native_plugin!(..., linked = NAME)` of crates
        // linked into this firmware (their code stays for the whole run); the
        // services offered are the ones wired at this start
        // (`device_host::wired_families`); the image is the firmware's read-only
        // data in flash, mapped for the whole run.
        let outcome = unsafe {
            paperpaw_app::register_native_plugins(
                kernel,
                &offered,
                &resources,
                &depth,
                || X4Services,
                report,
                linked_report,
                |id| paperpaw_app::native_default_on(id, paperpaw_app::StorePlugins::Left),
                flash_rodata(),
                // The plugins that may sleep are registered parked (#248, D1).
                paperpaw_app::sleeps,
            )
        };
        CREATING.store(false, core::sync::atomic::Ordering::Relaxed);
        let mut hosted = 0_u32;
        for (id, status) in outcome {
            match status {
                Ok(()) => {
                    hosted += 1;
                    info!("native: linked {id}: running");
                }
                Err(why) => warn!("native: linked {id}: not hosted: {why:?}"),
            }
        }
        if hosted > 0 {
            // A lost band after a sleep now ends in a start without them.
            crate::stack::native_loaded();
        }
        info!(
            "native: {hosted} linked plugins hosted in {} ms",
            (crate::micros() - began) / 1000
        );
    }

    /// The owner's fallback report of a teardown step that failed (called
    /// once, without allocating or calling plugin code): the serial log is
    /// all the X4 has to say it with.
    fn report(
        _: &X4Services,
        id: &str,
        step: paperpaw_core::native_host::TeardownStep,
        why: paperpaw_core::native_host::Error,
    ) {
        warn!("native: {id}: teardown step {step:?} failed: {why:?}");
    }

    /// The steps before `make` with their real allocations
    /// (`paperpaw_x4::premake::PreMake`).
    struct Prepare<'a> {
        id: &'a str,
        header: &'a Header5,
        placed: &'a crate::native::Placed5,
        place: &'a mut crate::native::Place,
        resources: &'a mut Option<HostResources>,
        device: &'a alloc::rc::Rc<paperpaw_x4::device_host::X4Device>,
        name: Option<alloc::string::String>,
        share: Option<HostResources>,
    }

    impl paperpaw_x4::premake::PreMake for Prepare<'_> {
        type Refusal = PluginProblem;

        fn copy_name(&mut self) -> Result<(), PluginProblem> {
            self.name = Some(try_string(self.id)?);
            Ok(())
        }

        fn table(&mut self) -> Result<(), PluginProblem> {
            if self.resources.is_none() {
                let device: alloc::rc::Rc<
                    dyn paperpaw_core::native_host::device_service::DeviceHost,
                > = self.device.clone();
                *self.resources =
                    Some(crate::native::new_resources(Some(device)).ok_or(PluginProblem::NoRoom)?);
            }
            Ok(())
        }

        fn share(&mut self) -> Result<(), PluginProblem> {
            self.share = self.resources.as_ref().and_then(HostResources::try_clone);
            if self.share.is_none() {
                warn!(
                    "native: {}: no room for a share of the host's table",
                    self.id
                );
                return Err(PluginProblem::NoRoom);
            }
            Ok(())
        }

        fn list_image(&mut self) -> Result<(), PluginProblem> {
            let (id, header, placed) = (self.id, self.header, self.placed);
            let Some(name) = self.name.take() else {
                return Err(PluginProblem::NoRoom);
            };
            let listed = critical_section::with(|cs| {
                let mut images = crate::native::IMAGES.borrow_ref_mut(cs);
                if images.try_reserve(1).is_err() {
                    return false;
                }
                images.push(crate::native::Image {
                    id: name,
                    code_base: placed.base + paperpaw_native::CODE_OFFSET,
                    len: header.image_len,
                    hot: placed.hot,
                    hot_at: header.hot_at,
                    hot_len: header.hot_len,
                    took: None,
                });
                true
            });
            if listed {
                Ok(())
            } else {
                warn!("native: {id}: no room to list the image");
                Err(PluginProblem::NoRoom)
            }
        }

        fn give_back_region(&mut self) {
            self.place.take_back(*self.placed);
        }
    }

    /// Makes the plugin of the placed image and returns it as the core's
    /// flat plugin. Called after `make` is read from the written flash and
    /// checked: from here the layout is never rewound.
    /// What `make` needs once everything that can still be refused before it
    /// has been (`paperpaw_x4::premake`), in the order that leaves nothing
    /// behind.
    struct Ready {
        /// The device's one counter, a counted share of it.
        depth: Option<FlatDepth>,
        /// A share of the device's table of handles.
        share: HostResources,
        make: Make,
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the loader's one entry: the flash layout, the image and the device's tables"
    )]
    fn ready(
        place: &mut crate::native::Place,
        code: &CodeFile<'_>,
        header: &Header5,
        placed: &crate::native::Placed5,
        make: u32,
        depth: &FlatDepth,
        resources: &mut Option<HostResources>,
        device: &alloc::rc::Rc<paperpaw_x4::device_host::X4Device>,
    ) -> Result<Ready, PluginProblem> {
        let id = code.id.as_str();
        // The device's one counter, a counted share of it, taken before anything
        // is made: the core's `into_plugin` then never makes a counter of its own
        // (the kernel refuses a plugin that brings another).
        let Some(depth) = depth.try_clone() else {
            return Err(PluginProblem::NoRoom);
        };
        let depth = Some(depth);
        // Plugin calls into the host's slots are told to the record of calls,
        // so the Power-hold escape never blames a plugin for waiting on the host.
        paperpaw_core::native_host::host_slot::set(
            crate::safe_mode::host_begin,
            crate::safe_mode::host_end,
        );
        // Everything that can still be refused before `make`, in the order
        // that leaves nothing behind (`paperpaw_x4::premake`): a refusal gives
        // the flash region back to the layout and lists nothing. Every
        // allocation is fallible: a full heap is `NoRoom`, never an abort.
        let mut steps = Prepare {
            id,
            header,
            placed,
            place,
            resources,
            device,
            name: None,
            share: None,
        };
        paperpaw_x4::premake::prepare(&mut steps)?;
        let Some(share) = steps.share.take() else {
            return Err(PluginProblem::NoRoom);
        };
        // SAFETY: `make` was read from the image just written to flash and
        // checked there (inside its code, even), so it is the plugin's
        // entry, callable for as long as the image stays placed: all session.
        let make: Make = unsafe { core::mem::transmute::<usize, Make>(make as usize) };
        Ok(Ready { depth, share, make })
    }

    /// Hosts the plugin of the placed image, not made yet, for the kernel to
    /// admit with everything it declares (`CodeLoader::load_native`): `make`
    /// runs in the kernel's registration transaction, and the declarations
    /// (screens, settings, actions, fonts, ...) are registered with the
    /// callbacks it committed, all or none. A refusal there destroys the host
    /// and says why as the plugin's problem.
    #[allow(
        clippy::too_many_arguments,
        reason = "the loader's one entry: the flash layout, the image and the device's tables"
    )]
    pub(super) fn host(
        place: &mut crate::native::Place,
        code: &CodeFile<'_>,
        header: &Header5,
        placed: &crate::native::Placed5,
        make: u32,
        depth: &FlatDepth,
        resources: &mut Option<HostResources>,
        device: &alloc::rc::Rc<paperpaw_x4::device_host::X4Device>,
        declared_services: shell::DeclaredServices,
    ) -> Result<Box<dyn NativeAdmission>, PluginProblem> {
        let id = code.id.as_str();
        let (heap, internal, started) = (
            esp_alloc::HEAP.used(),
            crate::native::internal_free(),
            crate::micros(),
        );
        let Ready {
            depth, share, make, ..
        } = ready(place, code, header, placed, make, depth, resources, device)?;
        let Some(depth) = depth else {
            return Err(PluginProblem::NoRoom);
        };
        // The image's first `code_at` bytes are the read-only data;
        // `Header5::read` checked `code_at <= image_len`, and the whole image
        // lies in the placed room, so the slice cannot pass it.
        let data_len = header.code_at.min(header.image_len) as usize;
        let image_ptr = core::ptr::with_exposed_provenance::<u8>(placed.base as usize);
        // SAFETY: `make` was read from the image just placed and checked there,
        // so it is the plugin's entry, callable all session; the image is
        // flash (PSRAM on the S3) mapped at `placed.base` for the session:
        // only `put5` writes the region, and only before `finish`, into free
        // room, so nothing erases it under the plugin (`FlashModule`).
        let hosted = unsafe {
            Hosted::new(
                X4Services,
                FlashModule,
                make,
                id,
                (image_ptr, data_len),
                report,
                share,
                declared_services,
            )
        }
        .map_err(|why| {
            warn!("native: {id}: format 5: refused while hosting: {why:?}");
            match why {
                shell::Error::Allocation => PluginProblem::NoRoom,
                _ => PluginProblem::Damaged,
            }
        })?;
        let hot = if placed.hot == 0 { 0 } else { header.hot_len };
        Ok(Box::new(Measured {
            inner: HostedAdmission::new(hosted, depth, admitted_report, refused),
            before: (heap, internal, started),
            hot,
        }))
    }

    /// The admission with what the plugin cost: the heap and the time from
    /// placing it through `make` and the registration of everything it
    /// declares, which the remote's memory page and the log show per plugin.
    struct Measured {
        inner: HostedAdmission<X4Services, FlashModule>,
        /// The heap and internal RAM used, and the time, when hosting began
        /// (after `verify` placed the image: placing costs flash, not heap).
        before: (usize, usize, u64),
        hot: u32,
    }

    impl NativeAdmission for Measured {
        fn admit(
            self: Box<Self>,
            kernel: &mut paperpaw_core::Kernel,
            folder: &str,
        ) -> Result<(), PluginProblem> {
            let Self { inner, before, hot } = *self;
            let made = crate::micros();
            let admitted = inner.admit_in(kernel, folder);
            let took = crate::native::Took {
                place_us: made - before.2,
                make_us: crate::micros() - made,
                heap: esp_alloc::HEAP.used() as isize - before.0 as isize,
                internal: before.1 as isize - crate::native::internal_free() as isize,
                hot,
            };
            info!(
                "native: {folder}: admitted in {} us (placing {} us), heap {:+} B, internal RAM {:+} B, hot code {} B: {admitted:?}",
                took.make_us, took.place_us, took.heap, took.internal, took.hot
            );
            critical_section::with(|cs| {
                if let Some(image) = crate::native::IMAGES.borrow_ref_mut(cs).last_mut() {
                    image.took = Some(took);
                }
            });
            admitted
        }
    }

    /// What the kernel tells of an admitted plugin's callbacks.
    fn admitted_report(_: &X4Services, plugin: &str, tag: u32, status: i32) {
        warn!("native: {plugin}: call {tag} ended with {status}");
    }

    /// Why the kernel refused a plugin it was handed: nothing of it stays.
    fn refused(folder: &str, why: &shell::Error) {
        warn!("native: {folder}: not admitted: {why:?}");
    }

    /// `text` copied, or `NoRoom`.
    fn try_string(text: &str) -> Result<alloc::string::String, PluginProblem> {
        let mut copy = alloc::string::String::new();
        copy.try_reserve_exact(text.len())
            .map_err(|_| PluginProblem::NoRoom)?;
        copy.push_str(text);
        Ok(copy)
    }
}

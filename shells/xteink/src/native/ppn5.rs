//! The X4's plugin files for interface 7 (format 5, `abi/ppn5.md`), as far
//! as the core can take them today (feature `ppn5`, off by default).
//!
//! What this does, in the note's order (`Header5` and `stream5` of
//! `paperpaw_native` hold every rule and are tested on the host):
//!
//! 1. the header is read and checked against this firmware (steps 1 to 7);
//!    the file must be the one installed;
//! 2. the room is found and the image placed (ESP32-C3 only: the X4 Pro and
//!    the X4 Classic refuse every format-5 file at step 3, target 9 having
//!    no profile yet);
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
use paperpaw_core::CodeFile;
use paperpaw_core::native_host::HostResources;
use paperpaw_core::native_host::flat::FlatDepth;
use paperpaw_native::MAGIC;
use paperpaw_native::header5::{HEADER_LEN, Header5, Host, Refusal};
use paperpaw_platform::Storage;
use paperpaw_sdk::{Plugin, PluginProblem};

/// What this firmware is, to compare a file with: the X4 at the interface
/// minor the generated ABI says.
pub(super) const HOST: Host = Host::x4(paperpaw_native::host_glue::MINOR);

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

/// Loads the format-5 file `code` as far as the core takes it (see the
/// [module documentation](self)). `place` puts the image where it runs.
pub fn load(
    place: &mut super::Place,
    depth: &mut Option<FlatDepth>,
    resources: &mut Option<HostResources>,
    storage: &dyn Storage,
    code: &CodeFile<'_>,
) -> Result<Box<dyn Plugin>, PluginProblem> {
    let path = code.path;
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
    #[cfg(feature = "esp32c3")]
    {
        let (placed, make, declared) = x4::verified(place, storage, code, &header, started)?;
        x4::run(
            place, code, &header, &placed, make, depth, resources, declared, started,
        )
    }
    #[cfg(not(feature = "esp32c3"))]
    {
        // Unreachable in practice: the X4 Pro's files are target 9, refused
        // by the header; any other chip is refused as another build.
        let _ = (place, depth, resources, started);
        Err(PluginProblem::OtherBuild)
    }
}

#[cfg(feature = "esp32c3")]
pub(super) use x4::{StorageSource, entry_check_failed};

/// The ESP32-C3's part: the image is placed in flash and its entry read.
#[cfg(feature = "esp32c3")]
mod x4 {
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    use core::alloc::Layout;

    use log::{info, warn};
    use paperpaw_core::CodeFile;
    use paperpaw_core::native_host::HostResources;
    use paperpaw_core::native_host::flat::{DropReason, FlatDepth, FlatError, FlatPlugin};
    use paperpaw_core::native_host::shell::instance::{Hooks, Instance};
    use paperpaw_core::native_host::shell::{self, Make};
    use paperpaw_native::header5::{
        EntryFacts, EntryRefusal, HEADER_LEN, Header5, RawEntry, RecordFacts,
    };
    use paperpaw_native::host_glue::Services;
    use paperpaw_native::metadata;
    use paperpaw_platform::Storage;
    use paperpaw_sdk::Plugin;
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
        warn!("native: {path}: format 5: metadata: {why:?}");
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
    ) -> Result<(crate::native::flash::Placed5, u32, shell::DeclaredServices), PluginProblem> {
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
        let facts = RecordFacts::read(&record).ok_or_else(|| {
            warn!("native: {path}: format 5: the metadata record is not one this firmware reads");
            PluginProblem::Damaged
        })?;
        // The record's body against what this firmware offers, before the
        // image is written: a plugin that asks for a service this firmware
        // lacks costs no flash. What it declares is kept for the host, which
        // refuses a query of any other service.
        let offered = metadata::Host {
            abi_major: HOST.abi_major,
            minor: HOST.minor,
            profile: paperpaw_native::header5::X4_PROFILE,
            services: shell::DATA_SERVICES,
            features: &[],
        };
        let (declared, needs_minor) = shell::admission::admit(&record, &offered)
            .map_err(|why| admission_refused(path, why))?;
        if needs_minor != facts.needs_minor {
            warn!("native: {path}: format 5: the record's two readings disagree");
            return Err(PluginProblem::Damaged);
        }
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
        placed: &crate::native::flash::Placed5,
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

    /// The X4's heap and clock for a plugin: the firmware's allocator, so a
    /// plugin's allocations count against the heap like everything else.
    pub(super) struct X4Services;

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
    }

    /// What the owner keeps alive for a plugin on the X4: nothing to unload.
    /// The image stays placed and mapped for the session (and a region is
    /// reused only after a restart, so nothing here can be freed under a
    /// plugin that still runs).
    pub(super) struct FlashModule;

    fn call_begin(id: &str) {
        crate::safe_mode::calling(id);
    }

    fn call_end(_id: &str) {
        // A hit restarts the X4 by a panic with the id still set.
        crate::stack::check();
        crate::safe_mode::returned();
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

    fn event_dropped(id: &str, tag: u32, why: DropReason) {
        warn!("native: {id}: event {tag} dropped: {why:?}");
    }

    fn teardown_failed(id: &str, why: paperpaw_core::native_host::Error) {
        warn!("native: {id}: unload step failed: {why:?}");
    }

    fn leaked(id: &str) {
        warn!("native: {id}: still in use at unload, kept");
    }

    /// The steps before `make` with their real allocations
    /// (`paperpaw_x4::premake::PreMake`).
    struct Prepare<'a> {
        id: &'a str,
        header: &'a Header5,
        placed: &'a crate::native::flash::Placed5,
        place: &'a mut crate::native::Place,
        resources: &'a mut Option<HostResources>,
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
                *self.resources = Some(HostResources::try_new().map_err(|_| {
                    warn!(
                        "native: {}: no room for the host's table of handles",
                        self.id
                    );
                    PluginProblem::NoRoom
                })?);
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
    #[allow(
        clippy::too_many_arguments,
        reason = "the loader's one entry: the flash layout, the image, the device's tables and the start time"
    )]
    pub(super) fn run(
        place: &mut crate::native::Place,
        code: &CodeFile<'_>,
        header: &Header5,
        placed: &crate::native::flash::Placed5,
        make: u32,
        depth: &mut Option<FlatDepth>,
        resources: &mut Option<HostResources>,
        declared_services: shell::DeclaredServices,
        started: u64,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        let id = code.id.as_str();
        let heap = esp_alloc::HEAP.used();
        let internal = crate::native::internal_free();
        let made = crate::micros();
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
        let hooks = Hooks {
            event_dropped,
            leaked,
            teardown_failed,
            call_begin,
            call_end,
        };
        // SAFETY: as above; the read-only data [0, code_at) of the image is
        // mapped at `placed.base` for the session (nothing erases the region
        // before a restart: it is only written by `put5` into free room).
        let loaded = unsafe {
            Instance::load(
                X4Services,
                FlashModule,
                make,
                id,
                (placed.base as usize, header.code_at as usize),
                report,
                hooks,
                share,
                declared_services,
            )
        };
        let instance = loaded.map_err(|why| {
            warn!("native: {id}: format 5: refused while making: {why:?}");
            match why {
                shell::Error::Allocation => PluginProblem::NoRoom,
                _ => PluginProblem::Damaged,
            }
        })?;
        if instance.declared_records() != 0 {
            warn!(
                "native: {id}: declares {} records, which cannot be applied yet: not loaded",
                instance.declared_records()
            );
            return Err(PluginProblem::Unsupported);
        }
        // Checked copies of the id and the name; a refusal drops the instance
        // (the unload, once) as any other post-`make` failure.
        let manifest = FlatPlugin::try_manifest(code.id, id, paperpaw_sdk::Version::new(0, 0, 0))
            .map_err(|_| {
            warn!("native: {id}: no room for the manifest");
            PluginProblem::NoRoom
        })?;
        // The instance box, the device-wide depth (made with the first plugin,
        // each plugin holds a counted share), the adapter and its box: all
        // fallible, a full heap is `NoRoom`, never an abort. A refusal drops
        // the instance exactly once.
        let plugin = instance
            .into_plugin(manifest, depth, code.host)
            .map_err(|why| {
                warn!("native: {id}: format 5: {why:?}");
                match why {
                    FlatError::Allocation => PluginProblem::NoRoom,
                    _ => PluginProblem::Unsupported,
                }
            })?;
        let took = crate::native::Took {
            place_us: made - started,
            make_us: crate::micros() - made,
            heap: esp_alloc::HEAP.used() as isize - heap as isize,
            internal: internal as isize - crate::native::internal_free() as isize,
            hot: if placed.hot == 0 { 0 } else { header.hot_len },
        };
        info!(
            "native: {id}: format 5 made in {} us, heap {:+} B, internal RAM {:+} B, hot code {} B",
            took.make_us, took.heap, took.internal, took.hot
        );
        critical_section::with(|cs| {
            if let Some(image) = crate::native::IMAGES.borrow_ref_mut(cs).last_mut() {
                image.took = Some(took);
            }
        });
        Ok(plugin)
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

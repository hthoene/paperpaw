//! The web flasher's bundle of plugins (`paperpaw_native::bundle`), found
//! at the firmware's first start at the start of the other app partition
//! (the flasher puts it there for firmware that says it looks there,
//! [`OTHER_SLOT`]: room for the whole store), or behind the firmware's
//! image (where it put it before). Its plugins are copied
//! onto the card into folders PaperPaw does not load (`plugins/.<id>.part`),
//! each file checked by its SHA-256 as it is copied and a plugin's code by
//! its author's signature, for the kernel to install before it loads
//! plugins (`KernelBuilder::install_plugin`), so they run at this start.
//! Its origin, author's key and code digest are staged with the files
//! (`install::marks`) and kept only once installed; code signed by another
//! author than the version installed before is not installed, and the plugin manager says
//! so (`Native::not_installed`): the reader decides in the store, warned.
//! Behind the image, the bundle's first sector is erased once they are
//! all on the card (on the X4 its code plugins go there, and are placed
//! as the kernel installs them). In the other app partition it is erased
//! only once the kernel installed them ([`forget`], `main`): a power cut
//! before that, while copying or installing, leaves it as it was, and the
//! next start copies and installs it again, each plugin in place of what
//! it left; never half of one. Without a card it waits; an update, which
//! goes into that partition, needs the card and comes from the kernel,
//! so only after it was installed. A partition that holds no bundle (an
//! image, another firmware's, CrossPoint's) is never written.
//!
//! A bundle for another image, or one that is damaged, is erased without
//! installing anything; one that cannot be copied (no card, a card full)
//! waits for the next start. On the X4 the bundle lies where its code
//! plugins go: they get room once it is installed.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use esp_bootloader_esp_idf::partitions::FlashStorage;
use log::{info, warn};
use paperpaw_digest::sha256::Sha256;
use paperpaw_native::bundle::{BUNDLE_MAGIC, Bundle, BundleFile, BundlePlugin, Source};
use paperpaw_native::index::SECTOR;
use paperpaw_platform::Storage;
use paperpaw_sdk::Id;
use paperpaw_sdk::builtin::PLUGIN_DIR;
use paperpaw_sdk::native::{NotInstalled, NotInstalledReason};

use super::partition::{behind_image, other_app};
use crate::ota::Flash;

/// Read and copied a piece at a time.
const PIECE: usize = 4096;

/// Says in the image that this firmware takes a bundle from the start of
/// the other app partition: the web flasher looks for these bytes before
/// putting it there, and puts it behind the image for a firmware without
/// them.
#[used]
pub static OTHER_SLOT: [u8; 32] = *b"PaperPaw bundle: other slot, 1\0\0";

/// Where a bundle lies, if one does: from `start` to at most `end`.
///
/// `other_slot`: the other app partition is looked in too; not in a dual-boot
/// setup, where it holds another firmware, which is never read for this.
fn found(
    flash: &mut FlashStorage<'static>,
    behind: (u32, u32),
    other_slot: bool,
) -> Option<(u32, u32)> {
    let mut head = [0u8; 4];
    let other = other_slot
        .then(|| other_app(flash).ok())
        .flatten()
        .map(|(offset, size)| (offset, offset + size));
    [Some(behind), other]
        .into_iter()
        .flatten()
        .find(|&(start, _)| flash.read(start, &mut head).is_ok() && head == BUNDLE_MAGIC)
}

/// The bundle's bytes, from flash.
struct InFlash<'a> {
    flash: &'a mut FlashStorage<'static>,
    at: u32,
}

impl Source for InFlash<'_> {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> bool {
        crate::power::feed();
        self.flash.read(self.at + at, buf).is_ok()
    }
}

/// What a bundle brought: each plugin to install, by its id and the
/// folder it is in, and those not installed; and where the bundle lies in
/// the other app partition, to [`forget`] once the kernel installed them.
#[derive(Default)]
pub struct Staged {
    pub installs: Vec<(Id, String)>,
    pub not_installed: Vec<NotInstalled>,
    pub other_slot: Option<u32>,
}

/// Work before the kernel has a screen of its own.
#[derive(Debug)]
pub enum Progress<'a> {
    Checking,
    Copying {
        plugin: &'a str,
        index: usize,
        total: usize,
        done: u32,
        bytes: u32,
    },
    Starting,
    Damaged,
    Failed,
}

/// The bundle at `at`, in the other app partition, installed: erased.
pub fn forget(flash: &Flash, at: u32) {
    erase(&mut flash.borrow_mut(), at);
    info!("bundle: installed, erased at {at:#x}");
}

/// Copies the plugins of the bundle behind the image onto the card, if
/// there is one, and erases it (see the [module documentation](self)).
// A frame of its own, off the stack before the kernel is built.
#[inline(never)]
pub fn stage(
    flash: &Flash,
    storage: &dyn Storage,
    other_slot: bool,
    progress: &mut dyn FnMut(Progress<'_>),
) -> Option<Staged> {
    let mut flash = flash.borrow_mut();
    let behind = behind_image(&mut flash).ok()?;
    let (start, end) = found(&mut flash, (behind.start, behind.end), other_slot)?;
    info!("bundle: at {start:#x}");
    progress(Progress::Checking);
    let checking = crate::micros();
    let mut source = InFlash {
        flash: &mut flash,
        at: start,
    };
    let bundle = match Bundle::read(&mut source, &behind.firmware, end - start) {
        Ok(bundle) => bundle,
        Err(why) => {
            warn!("bundle: {why}: erased, nothing installed");
            progress(Progress::Damaged);
            erase(&mut flash, start);
            return None;
        }
    };
    info!(
        "bundle: checked {} bytes in {} ms",
        bundle.len,
        (crate::micros() - checking) / 1000
    );
    let started = crate::micros();
    let mut source = InFlash {
        flash: &mut flash,
        at: start,
    };
    let mut staged = copy_all(&mut source, storage, &bundle, progress)?;
    if start == behind.start {
        erase(&mut flash, start);
    } else {
        staged.other_slot = Some(start);
    }
    info!(
        "bundle: {} plugins copied onto the card in {} ms, {} not installed",
        staged.installs.len(),
        (crate::micros() - started) / 1000,
        staged.not_installed.len()
    );
    progress(Progress::Starting);
    Some(staged)
}

/// Copies and prepares the checked bundle, without changing installed plugins.
fn copy_all(
    source: &mut InFlash<'_>,
    storage: &dyn Storage,
    bundle: &Bundle,
    progress: &mut dyn FnMut(Progress<'_>),
) -> Option<Staged> {
    let mut buf = Vec::new();
    if buf.try_reserve_exact(PIECE).is_err() {
        warn!("bundle: no copy buffer; kept for the next start");
        progress(Progress::Failed);
        return None;
    }
    buf.resize(PIECE, 0);
    let mut staged = Staged::default();
    for (index, plugin) in bundle.plugins.iter().enumerate() {
        // The bundle's names are those of plugins (`Bundle::read`).
        let Ok(id) = Id::parse(&plugin.id) else {
            continue;
        };
        let folder = format!("{PLUGIN_DIR}/.{}.part", plugin.id);
        let copied = crate::micros();
        let mut report = |done, bytes| {
            progress(Progress::Copying {
                plugin: &plugin.id,
                index: index + 1,
                total: bundle.plugins.len(),
                done,
                bytes,
            })
        };
        if let Err(why) = copy(source, storage, plugin, &folder, &mut buf, &mut report) {
            // The card, most likely: tried again at the next start.
            warn!("bundle: {}: {why}; kept for the next start", plugin.id);
            for (_, folder) in &staged.installs {
                let _ = storage.remove_dir_all(folder);
            }
            let _ = storage.remove_dir_all(&folder);
            progress(Progress::Failed);
            return None;
        }
        match paperpaw_app::flasher::prepare(storage, &plugin.id, &folder, &plugin.version) {
            Ok(()) => {
                info!(
                    "bundle: {} prepared in {} ms",
                    plugin.id,
                    (crate::micros() - copied) / 1000
                );
                staged.installs.push((id, folder));
            }
            Err(paperpaw_app::flasher::Problem::Storage) => {
                warn!(
                    "bundle: {}: installation marks not kept; retry next start",
                    plugin.id
                );
                progress(Progress::Failed);
                return None;
            }
            Err(reason) => {
                warn!("bundle: {}: {reason:?}; not installed", plugin.id);
                let _ = storage.remove_dir_all(&folder);
                let reason = match reason {
                    paperpaw_app::flasher::Problem::OtherAuthor => NotInstalledReason::OtherAuthor,
                    _ => NotInstalledReason::Damaged,
                };
                staged.not_installed.push(NotInstalled::new(id, reason));
            }
        }
    }
    Some(staged)
}

/// Copies `plugin`'s files into `folder`, each checked by its SHA-256.
fn copy(
    source: &mut InFlash<'_>,
    storage: &dyn Storage,
    plugin: &BundlePlugin,
    folder: &str,
    buf: &mut [u8],
    progress: &mut dyn FnMut(u32, u32),
) -> Result<(), &'static str> {
    let _ = storage.remove_dir_all(folder);
    let _ = storage.create_dir(PLUGIN_DIR);
    storage.create_dir(folder).map_err(|_| "no folder for it")?;
    let total = plugin.files.iter().map(|file| file.size).sum();
    let mut done = 0;
    progress(0, total);
    for file in &plugin.files {
        crate::power::feed();
        let path = format!("{folder}/{}", file.path);
        // Its folders first (`i18n/de.ini`).
        let mut at = folder.len();
        while let Some(slash) = path[at + 1..].find('/') {
            at += 1 + slash;
            let _ = storage.create_dir(&path[..at]);
        }
        copy_file(source, storage, file, &path, buf, &mut |bytes| {
            progress(done + bytes, total)
        })?;
        done += file.size;
    }
    Ok(())
}

fn copy_file(
    source: &mut InFlash<'_>,
    storage: &dyn Storage,
    file: &BundleFile,
    path: &str,
    buf: &mut [u8],
    progress: &mut dyn FnMut(u32),
) -> Result<(), &'static str> {
    storage.write(path, &[]).map_err(|_| "not written")?;
    let mut sha = Sha256::new();
    let mut done = 0u32;
    while done < file.size {
        let len = (file.size - done).min(buf.len() as u32) as usize;
        if !source.read(file.at + done, &mut buf[..len]) {
            return Err("the flash cannot be read");
        }
        sha.update(&buf[..len]);
        storage
            .append(path, &buf[..len])
            .map_err(|_| "not written")?;
        done += len as u32;
        progress(done);
    }
    if sha.finish() != file.sha256 {
        return Err("a file does not hold");
    }
    Ok(())
}

/// Erases the bundle's first sector: what is behind it is no bundle.
fn erase(flash: &mut FlashStorage<'static>, at: u32) {
    if let Err(e) = flash.erase(at, at + SECTOR) {
        warn!("bundle: not erased ({e:?})");
    }
}

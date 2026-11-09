//! `check_file` of kind 0 (`docs/abi/device.md` 5.1): the check of a plugin
//! source folder, which changes nothing on the card.
//!
//! A job of the queue, run in bounded steps from the main loop:
//!
//! 1. **Start:** the 4 KiB I/O buffer and the folder walk (no memory: reason 1).
//! 2. **Walk:** the folder in pieces of [`plugin_folder::PIECE`] entries: file
//!    count, `bytes`, `plugin.ini`, the code file.
//! 3. **Manifest:** id, name, version; the reserved `core` and the plugins
//!    built in are taken (15); a data plugin (no code file) ends here, one
//!    whose only code is a computer's is not on this device (19).
//! 4. **Header:** `code.ppn` is format 5 (`paperpaw_native::header5`): the
//!    loader's own refusals (4, 10, 11, 12) and the room for code (13).
//! 5. **Body:** every byte after the header, 4 KiB a step, into the digest and
//!    the signature check; the digest, then the signature, are decided in a
//!    step of their own (4).
//! 6. **Metadata:** the record the header names, checked against what the
//!    device offers (10 for a service or feature it lacks, 12, 4).
//! 7. **Trust:** unsigned code is 8; the pinned key, its recovery key and a
//!    revocation come from the pin files (`plugin_trust`); the key's name is
//!    one the firmware knows, else empty. A revoked pinned key is 7; a pin
//!    that is another key is a flag (an install needs a rotation), not a refusal.
//!
//! A check says what an install would end with at that moment; the install
//! checks again. It reads only: a cut changes nothing.
use alloc::{string::String, vec::Vec};
use paperpaw_native::{
    header5::{HEADER_LEN, Header5, Refusal},
    metadata,
    signature::{SIGNATURE_LEN, Signature, SignatureCheck},
};
use paperpaw_platform::{Storage, StorageError, firmware::Text};
use paperpaw_sdk::digest::sha256::Sha256;
use paperpaw_sdk::native::{self, CODE_FILES};

use super::plugin_folder::{self, Found, Manifest, Walk, Walked};
use super::plugin_job::{
    FLAG_DATA_PLUGIN, FLAG_PINNED_HERE, FLAG_PINNED_OTHER, Finished, JobEnv, JobStep, KIND_CHECK,
    PluginFacts, PluginJob, io_buffer, reason,
};
use super::plugin_trust;

/// The code file this device reads (`CODE_FILES[0]`).
const CODE: &str = "code.ppn";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Start,
    Walk,
    Manifest,
    Header,
    Body,
    Verify,
    Metadata,
    Trust,
    Ended,
}

/// A plugin check in the queue.
pub(crate) struct PluginCheck {
    owner: u64,
    request: u32,
    phase: Phase,
    /// The source folder, until the walk takes it.
    source: String,
    walk: Option<Walk>,
    /// The path of the code file, once the header step built it.
    code: String,
    buf: Vec<u8>,
    found: Option<Found>,
    manifest: Option<Manifest>,
    header: Option<Header5>,
    /// The file's length, signature included.
    size: u64,
    signature: Option<Signature>,
    sha: Option<Sha256>,
    check: Option<SignatureCheck>,
    at: u64,
    /// The code's header digest, once it held.
    digest: Option<[u8; 32]>,
    /// Set by [`PluginJob::abort`]: the next step ends with it.
    stopped: Option<u8>,
}

impl PluginCheck {
    /// A check of the plugin folder at `source` (the card path the caller
    /// resolved from its root).
    ///
    /// # Errors
    /// `None` for no memory.
    pub(crate) fn new(owner: u64, request: u32, source: &str) -> Option<Self> {
        let mut path = String::new();
        path.try_reserve_exact(source.len()).ok()?;
        path.push_str(source);
        Some(Self {
            owner,
            request,
            phase: Phase::Start,
            source: path,
            walk: None,
            code: String::new(),
            buf: Vec::new(),
            found: None,
            manifest: None,
            header: None,
            size: 0,
            signature: None,
            sha: None,
            check: None,
            at: 0,
            digest: None,
            stopped: None,
        })
    }

    fn end(&mut self, reason: u8) -> JobStep {
        self.release();
        self.phase = Phase::Ended;
        JobStep::Done(Finished::refused(reason))
    }

    /// Drops what the job holds, so a finished or aborted job is small.
    fn release(&mut self) {
        self.buf = Vec::new();
        self.code = String::new();
        self.walk = None;
        self.sha = None;
        self.check = None;
    }

    fn code_path(&self) -> Option<String> {
        let root = self.walk.as_ref()?.root();
        let mut path = String::new();
        path.try_reserve_exact(root.len() + 1 + CODE.len()).ok()?;
        path.push_str(root);
        path.push('/');
        path.push_str(CODE);
        Some(path)
    }

    fn start(&mut self) -> JobStep {
        let Some(buf) = io_buffer() else {
            return self.end(reason::OTHER);
        };
        let Some(walk) = Walk::new(&self.source) else {
            return self.end(reason::OTHER);
        };
        self.buf = buf;
        self.walk = Some(walk);
        self.source = String::new();
        self.phase = Phase::Walk;
        JobStep::More
    }

    fn walk(&mut self, env: &JobEnv<'_>) -> JobStep {
        let Some(walk) = self.walk.as_mut() else {
            return self.end(reason::OTHER);
        };
        match walk.step(env.storage) {
            Walked::More => JobStep::More,
            Walked::Refused(why) => self.end(why),
            Walked::Done(found) => {
                // A source without a manifest is no plugin folder.
                if !found.manifest {
                    return self.end(reason::DAMAGED);
                }
                self.found = Some(found);
                self.phase = Phase::Manifest;
                JobStep::More
            }
        }
    }

    fn manifest(&mut self, env: &JobEnv<'_>) -> JobStep {
        let (Some(walk), Some(found)) = (self.walk.as_ref(), self.found) else {
            return self.end(reason::OTHER);
        };
        let manifest = match plugin_folder::read_manifest(env.storage, walk.root(), walk.folder_name())
        {
            Ok(manifest) => manifest,
            Err(why) => return self.end(why),
        };
        let id = manifest.id.as_str();
        if id == "core" || env.built_ins.index_of(id).is_some() {
            return self.end(reason::ID_TAKEN);
        }
        self.manifest = Some(manifest);
        match found.code {
            None => {
                // A data plugin: no code, no signature, nothing to pin.
                let facts = Self::facts(&found, &manifest, None, 0);
                self.release();
                self.phase = Phase::Ended;
                JobStep::Done(Finished {
                    reason: reason::OK,
                    facts: Some(facts),
                })
            }
            // Only code of a computer: another kind of device.
            Some(index) if CODE_FILES.get(index).copied() != Some(CODE) => {
                self.end(reason::NOT_HERE)
            }
            Some(_) => {
                self.phase = Phase::Header;
                JobStep::More
            }
        }
    }

    fn facts(
        found: &Found,
        manifest: &Manifest,
        signed: Option<(&[u8; 32], &[u8; 32])>,
        flags: u32,
    ) -> PluginFacts {
        PluginFacts {
            id: manifest.id,
            version: manifest.version,
            bytes: found.bytes,
            signer: Text::new(""),
            key: signed.map(|(key, _)| *key),
            sha256: signed.map(|(_, sha)| *sha),
            flags: if found.code.is_none() {
                flags | FLAG_DATA_PLUGIN
            } else {
                flags
            },
        }
    }

    #[inline(never)]
    fn header(&mut self, env: &JobEnv<'_>) -> JobStep {
        let Some(path) = self.code_path() else {
            return self.end(reason::OTHER);
        };
        self.code = path;
        let path = self.code.as_str();
        let storage = env.storage;
        let size = match storage.size(path) {
            Ok(size) => size,
            Err(why) => return self.end(card_reason(why)),
        };
        if let Err(why) = read_exact(storage, path, 0, &mut self.buf[..HEADER_LEN]) {
            // A file shorter than a header is not a plugin file.
            return self.end(if size < HEADER_LEN as u64 { reason::DAMAGED } else { why });
        }
        let header = match Header5::read(&self.buf[..HEADER_LEN], size, &env.host.loader) {
            Ok(header) => header,
            Err(why) => return self.end(header_reason(why)),
        };
        self.size = size;
        // Room for the code beside the others, before the file is read whole.
        let manifest = self.manifest;
        let id = manifest.as_ref().map_or("", |m| m.id.as_str());
        if !room_fits(storage, &env.host.loader, &header, id, &mut self.buf) {
            return self.end(reason::NO_ROOM);
        }
        let sha = header.digest();
        if header.signed {
            let mut trailer = [0_u8; SIGNATURE_LEN];
            if let Err(why) = read_exact(storage, path, header.file_len(), &mut trailer) {
                return self.end(why);
            }
            let Some(signature) = Signature::read(&trailer) else {
                return self.end(reason::DAMAGED);
            };
            if u64::from(signature.signed_len) != header.file_len() {
                return self.end(reason::DAMAGED);
            }
            let Ok(mut check) = signature.check() else {
                return self.end(reason::DAMAGED);
            };
            check.absorb(&self.buf[..HEADER_LEN]);
            self.check = Some(check);
            self.signature = Some(signature);
        }
        self.sha = Some(sha);
        self.at = HEADER_LEN as u64;
        self.header = Some(header);
        self.phase = Phase::Body;
        JobStep::More
    }

    #[inline(never)]
    fn body(&mut self, env: &JobEnv<'_>) -> JobStep {
        let Some(header) = self.header else {
            return self.end(reason::OTHER);
        };
        let end = header.file_len();
        let len = (end - self.at).min(self.buf.len() as u64) as usize;
        if let Err(why) = read_exact(env.storage, &self.code, self.at, &mut self.buf[..len]) {
            return self.end(why);
        }
        if let Some(sha) = self.sha.as_mut() {
            sha.update(&self.buf[..len]);
        }
        if let Some(check) = self.check.as_mut() {
            check.absorb(&self.buf[..len]);
        }
        self.at += len as u64;
        if self.at >= end {
            self.phase = Phase::Verify;
        }
        JobStep::More
    }

    /// The digest, then the signature: the latter is the step's expensive
    /// part and has a frame of its own.
    fn verify(&mut self) -> JobStep {
        let (Some(header), Some(sha)) = (self.header, self.sha.take()) else {
            return self.end(reason::OTHER);
        };
        if !header.holds(sha) {
            return self.end(reason::DAMAGED);
        }
        self.digest = Some(header.sha256);
        if let Some(check) = self.check.take()
            && !signature_holds(check)
        {
            return self.end(reason::DAMAGED);
        }
        self.phase = Phase::Metadata;
        JobStep::More
    }

    #[inline(never)]
    fn metadata(&mut self, env: &JobEnv<'_>) -> JobStep {
        let Some(header) = self.header else {
            return self.end(reason::OTHER);
        };
        let len = header.meta_len as usize;
        if len > metadata::MAX_BYTES || len > self.buf.len() {
            return self.end(reason::DAMAGED);
        }
        let at = HEADER_LEN as u64 + u64::from(header.meta_at);
        if let Err(why) = read_exact(env.storage, &self.code, at, &mut self.buf[..len]) {
            return self.end(why);
        }
        match metadata::check(&self.buf[..len], &env.host.offered) {
            Ok(checked) => {
                if checked.abi_major != header.abi_major || checked.needs_minor != header.needs_minor
                {
                    return self.end(reason::DAMAGED);
                }
            }
            Err(metadata::Refusal::Malformed) => return self.end(reason::DAMAGED),
            Err(metadata::Refusal::Incompatible) => return self.end(reason::OTHER_CHIP),
            // A service or feature this device lacks: a newer PaperPaw might.
            Err(_) => return self.end(reason::NEEDS_NEWER),
        }
        self.phase = Phase::Trust;
        JobStep::More
    }

    fn trust(&mut self, env: &JobEnv<'_>) -> JobStep {
        let (Some(found), Some(manifest)) = (self.found, self.manifest) else {
            return self.end(reason::OTHER);
        };
        let (Some(signature), Some(digest)) = (self.signature, self.digest) else {
            // Code with no author's signature does not install.
            return self.end(reason::UNSIGNED);
        };
        let standing = match plugin_trust::standing(env.storage, manifest.id.as_str(), &mut self.buf)
        {
            Ok(standing) => standing,
            Err(plugin_trust::Unreadable) => return self.end(reason::CARD),
        };
        let mut flags = 0;
        match standing.pinned {
            Some(pinned) if pinned == signature.key => flags |= FLAG_PINNED_HERE,
            Some(_) => flags |= FLAG_PINNED_OTHER,
            None => {}
        }
        let mut facts = Self::facts(&found, &manifest, Some((&signature.key, &digest)), flags);
        facts.signer = plugin_trust::signer_name(env.host.signers, &signature.key);
        // A revoked key never installs; one revoked as another key's is none
        // of this plugin's.
        let reason = if flags & FLAG_PINNED_HERE != 0 && standing.revoked {
            reason::REVOKED
        } else {
            reason::OK
        };
        self.release();
        self.phase = Phase::Ended;
        JobStep::Done(Finished {
            reason,
            facts: Some(facts),
        })
    }
}

/// Whether the signature holds for everything absorbed: the heavy step, in
/// a frame of its own.
#[inline(never)]
fn signature_holds(check: SignatureCheck) -> bool {
    check.finish().is_ok()
}

impl PluginJob for PluginCheck {
    fn owner(&self) -> u64 {
        self.owner
    }

    fn request(&self) -> u32 {
        self.request
    }

    fn kind(&self) -> u32 {
        KIND_CHECK
    }

    fn step(&mut self, env: &JobEnv<'_>) -> JobStep {
        if let Some(why) = self.stopped {
            return self.end(why);
        }
        match self.phase {
            Phase::Start => self.start(),
            Phase::Walk => self.walk(env),
            Phase::Manifest => self.manifest(env),
            Phase::Header => self.header(env),
            Phase::Body => self.body(env),
            Phase::Verify => self.verify(),
            Phase::Metadata => self.metadata(env),
            Phase::Trust => self.trust(env),
            Phase::Ended => JobStep::Done(Finished::refused(reason::OTHER)),
        }
    }

    fn abort(&mut self, why: u8) {
        self.stopped = Some(why);
        self.release();
    }
}

/// `buf` filled from `path` at `at`.
///
/// # Errors
/// 1 for no memory, 16 for a card error or a file that ends early.
fn read_exact(storage: &dyn Storage, path: &str, mut at: u64, buf: &mut [u8]) -> Result<(), u8> {
    let mut done = 0;
    while done < buf.len() {
        match storage.read_at(path, at, &mut buf[done..]) {
            Ok(0) => return Err(reason::CARD),
            Ok(n) => {
                done += n;
                at += n as u64;
            }
            Err(why) => return Err(card_reason(why)),
        }
    }
    Ok(())
}

fn card_reason(why: StorageError) -> u8 {
    match why {
        StorageError::OutOfMemory => reason::OTHER,
        StorageError::NotFound => reason::DAMAGED,
        _ => reason::CARD,
    }
}

/// The reason a header the loader refuses ends a check with.
fn header_reason(why: Refusal) -> u8 {
    match why {
        Refusal::OlderFormat | Refusal::OlderInterface => reason::OLDER_BUILD,
        Refusal::NewerFormat
        | Refusal::NewerInterface
        | Refusal::NeedsNewerMinor
        | Refusal::Reserved => reason::NEEDS_NEWER,
        Refusal::UnknownChip | Refusal::OtherChip | Refusal::NoProfile => reason::OTHER_CHIP,
        _ => reason::DAMAGED,
    }
}

/// Whether the code fits in the room for code beside the plugins placed at
/// this start, in place of the id's own version (an update). Where the shell
/// says no room, the code is taken to fit: the start checks again.
fn room_fits(
    storage: &dyn Storage,
    loader: &paperpaw_native::header5::Host,
    header: &Header5,
    id: &str,
    buf: &mut [u8],
) -> bool {
    let Some(room) = native::code_room(storage) else {
        return true;
    };
    let need = room.needs(u64::from(header.image_len), header.align);
    let own = own_need(storage, loader, &room, id, buf);
    let taken = room.used.saturating_sub(own.unwrap_or(0));
    let placed = room.placed.saturating_sub(u32::from(own.is_some()));
    room.fits(taken, placed, need)
}

/// The room the code of the installed plugin `id` takes, if it has some
/// this device reads.
fn own_need(
    storage: &dyn Storage,
    loader: &paperpaw_native::header5::Host,
    room: &native::CodeRoom,
    id: &str,
    buf: &mut [u8],
) -> Option<u64> {
    let mut path = String::new();
    path.try_reserve_exact(paperpaw_sdk::builtin::PLUGIN_DIR.len() + id.len() + 2 + CODE.len())
        .ok()?;
    path.push_str(paperpaw_sdk::builtin::PLUGIN_DIR);
    path.push('/');
    path.push_str(id);
    path.push('/');
    path.push_str(CODE);
    let size = storage.size(&path).ok()?;
    read_exact(storage, &path, 0, buf.get_mut(..HEADER_LEN)?).ok()?;
    let header = Header5::read(&buf[..HEADER_LEN], size, loader).ok()?;
    Some(room.needs(u64::from(header.image_len), header.align))
}

#[cfg(test)]
#[path = "plugin_check_tests.rs"]
mod tests;

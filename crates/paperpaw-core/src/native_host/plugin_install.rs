//! `install_plugin` (`docs/abi/device.md` 5.2, 5.4): the install of a plugin
//! source folder as a job of the queue, in bounded steps.
//!
//! 1. **Probe:** the source's `plugin.ini` (id, name, version); `core` and the
//!    plugins built in are taken (15); an installed plugin of the id needs the
//!    replace flag (15), and one of the other kind (code or data) is none to
//!    replace (15).
//! 2. **Copy:** the source into the stage `plugins/.<id>.part`, outside every
//!    root a plugin reaches, a piece of 4 KiB a step, the folder listed in
//!    pieces of 16 entries (so at most 16 names are held at once). A source
//!    changed after its copy has no effect: everything below reads the stage.
//! 3. **Check:** the stage is checked again, whatever a check said
//!    ([`PluginCheck`], the loader's own refusals), and its manifest must still
//!    name the id.
//! 4. **Rules:** the trust rules of 5.2 ([`plugin_rules::decide`]) on the stage's
//!    key and digest and the evidence the plugin sent: reasons 5, 6, 7, 18.
//! 5. **Version:** an older or the same version needs the allow-older flag (14).
//! 6. **Ask:** the host asks the user ([`PluginQuestion`]); a plugin holding the
//!    trusted-installer grant skips only the prompt, never for a sideload or a
//!    version that declares a `uses.*` feature the installed one did not.
//!    Declined is reason 2, dismissed 3; the stage is removed.
//! 7. **Swap:** the origin is marked into the stage, the journal written and
//!    the stage swapped in ([`plugin_swap`]); the pins are taken there, never
//!    before.
//!
//! The stage is removed on every end before the journal. A job aborted while
//! it holds the card leaves the stage for the next start's cleanup (B2c-3);
//! after the journal, the next start finishes the swap.
use alloc::{boxed::Box, string::String, vec::Vec};
use paperpaw_native::{
    header5::{HEADER_LEN, Header5},
    metadata,
};
use paperpaw_platform::{Storage, StorageError, firmware::Text};
use paperpaw_sdk::{
    Version,
    install::marks::{self, From, Marks},
    native::CODE_FILES,
};

use super::files_service::same_fold;
use super::firmware_install::BuiltIns;
use super::plugin_check::PluginCheck;
use super::plugin_folder::{self, Manifest, Walk, Walked};
use super::plugin_job::{
    Answer, DONE_AT_NEXT_START, Finished, ID_MAX, JobEnv, JobStep, KIND_INSTALL, PluginJob,
    PluginQuestion, Relation, USE_KEY_MAX, USES_SHOWN, Uses, io_buffer, reason,
};
use super::plugin_rules::{Decision, Evidence, Pins, Staged, decide};
use super::plugin_swap::{self, Swap, SwapStep, io, present};
use super::plugin_trust::{self, Inquiry, Standing};
use super::shared::try_box;

/// `install_plugin.flags` bit 0: replace an installed plugin of the id.
pub(crate) const FLAG_REPLACE: u32 = 1;
/// Bit 1: an older (or the same) version is allowed.
pub(crate) const FLAG_OLDER: u32 = 2;
/// Bit 2: from a catalog, a store's install (code or data): the version is
/// marked as the store's, for its updates (`reg_plugin.flags` bit 5). It
/// grants no trust and changes no check.
pub(crate) const FLAG_CATALOG: u32 = 4;
/// The bits `install_plugin.flags` defines; another is `ERR_INVALID`.
pub(crate) const FLAGS: u32 = FLAG_REPLACE | FLAG_OLDER | FLAG_CATALOG;

const CODE: &str = "code.ppn";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    Start,
    CleanStage,
    List,
    Copy,
    Check,
    Standing,
    Rules,
    Ask,
    Waiting,
    Marks,
    Journal,
    Swapping,
    Ended,
}

/// The file being copied.
struct Copying {
    src: String,
    dst: String,
    size: u64,
    at: u64,
}

/// What stands installed under the id.
#[derive(Clone, Copy)]
pub(super) struct Installed {
    pub(super) version: Option<Version>,
    pub(super) code: bool,
}

/// An install in the queue.
pub(crate) struct PluginInstall {
    owner: u64,
    request: u32,
    flags: u32,
    // The big values are boxed: the job is built and moved by value, and a
    // frame on the X4 holds 2 KiB.
    evidence: Box<Evidence>,
    phase: Phase,
    source: String,
    id: Text<ID_MAX>,
    stage: String,
    walk: Option<Walk>,
    listed: bool,
    pending: Vec<(String, u64)>,
    next: usize,
    file: Option<Copying>,
    buf: Vec<u8>,
    check: Option<Box<PluginCheck>>,
    facts: Option<super::plugin_job::PluginFacts>,
    installed: Option<Installed>,
    manifest: Option<Manifest>,
    pins: Option<Box<Pins>>,
    question: Option<Box<PluginQuestion>>,
    inquiry: Inquiry,
    standing: Option<Standing>,
    /// The name of the staged code file as it was copied (a FAT keeps the
    /// case it was stored in).
    code_name: Text<16>,
    answer: Option<Answer>,
    swap: Option<Box<Swap>>,
    stopped: Option<u8>,
}

impl PluginInstall {
    /// An install of the plugin folder at `source` (a card path the caller
    /// resolved from its root), with the evidence that came with the request.
    ///
    /// # Errors
    /// `None` for no memory.
    pub(crate) fn new(
        owner: u64,
        request: u32,
        source: &str,
        flags: u32,
        evidence: Evidence,
    ) -> Option<Self> {
        let mut path = String::new();
        path.try_reserve_exact(source.len()).ok()?;
        path.push_str(source.trim_end_matches('/'));
        Some(Self {
            owner,
            request,
            flags,
            evidence: try_box(evidence)?,
            phase: Phase::Start,
            source: path,
            id: Text::new(""),
            stage: String::new(),
            walk: None,
            listed: false,
            pending: Vec::new(),
            next: 0,
            file: None,
            buf: Vec::new(),
            check: None,
            facts: None,
            installed: None,
            manifest: None,
            pins: None,
            question: None,
            inquiry: Inquiry::new(),
            standing: None,
            code_name: Text::new(CODE),
            answer: None,
            swap: None,
            stopped: None,
        })
    }

    /// Ends the job: the stage goes unless the journal is written (then the
    /// swap, or the next start, finishes it).
    #[inline(never)]
    fn end(&mut self, storage: &dyn Storage, why: u8) -> JobStep {
        if matches!(
            self.phase,
            Phase::Start
                | Phase::CleanStage
                | Phase::List
                | Phase::Copy
                | Phase::Check
                | Phase::Rules
                | Phase::Ask
                | Phase::Waiting
                | Phase::Marks
                | Phase::Journal
        ) && !self.stage.is_empty()
        {
            let _ = storage.remove_dir_all(&self.stage);
        }
        self.release();
        self.phase = Phase::Ended;
        JobStep::Done(Finished::refused(why))
    }

    fn release(&mut self) {
        self.buf = Vec::new();
        self.walk = None;
        self.pending = Vec::new();
        self.file = None;
        self.check = None;
    }

    #[inline(never)]
    fn start(&mut self, env: &JobEnv<'_>) -> JobStep {
        let storage = env.storage;
        let folder = self.source.rsplit('/').next().unwrap_or("");
        let Some(mut buf) = io_buffer() else {
            return self.end(storage, reason::OTHER);
        };
        let manifest = match plugin_folder::read_manifest(storage, &self.source, folder, &mut buf) {
            Ok(manifest) => manifest,
            Err(why) => return self.end(storage, why),
        };
        let id = manifest.id.as_str();
        // A removed plugin whose secrets the host could not forget: its id is
        // not free until it has (`plugin_unforgotten`).
        if id == "core"
            || env.built_ins.index_of(id).is_some()
            || super::plugin_unforgotten::has(storage, id)
        {
            return self.end(storage, reason::ID_TAKEN);
        }
        let (Some(stage), Some(target)) =
            (plugin_swap::stage_path(id), plugin_swap::target_path(id))
        else {
            return self.end(storage, reason::OTHER);
        };
        let installed = match present(storage, &target) {
            Ok(false) => None,
            Ok(true) => Some(installed_of(storage, &target, id, &mut buf)),
            Err(why) => return self.end(storage, why),
        };
        // An installed plugin of the id is replaced only when asked to.
        if installed.is_some() && self.flags & FLAG_REPLACE == 0 {
            return self.end(storage, reason::ID_TAKEN);
        }
        // A folder of this source that is the plugin's own is nothing to install.
        if paperpaw_sdk::files::same_path(storage, &self.source, &target) {
            return self.end(storage, reason::ID_TAKEN);
        }
        self.id = manifest.id;
        self.installed = installed;
        self.stage = stage;
        self.buf = buf;
        self.phase = Phase::CleanStage;
        JobStep::More
    }

    /// A stage left by an earlier cut goes; the new one is made.
    #[inline(never)]
    fn clean_stage(&mut self, storage: &dyn Storage) -> JobStep {
        match present(storage, &self.stage) {
            Ok(true) if storage.remove_dir_all(&self.stage).is_err() => {
                return self.end(storage, reason::CARD);
            }
            Err(why) => return self.end(storage, why),
            _ => {}
        }
        if let Err(error) = storage.create_dir(&self.stage) {
            return self.end(storage, io(error));
        }
        let Some(walk) = Walk::copying(&self.source) else {
            return self.end(storage, reason::OTHER);
        };
        self.walk = Some(walk);
        self.phase = Phase::List;
        JobStep::More
    }

    #[inline(never)]
    fn list(&mut self, storage: &dyn Storage) -> JobStep {
        let Some(walk) = self.walk.as_mut() else {
            return self.end(storage, reason::OTHER);
        };
        let walked = walk.step(storage);
        let files = walk.take_files();
        match walked {
            Walked::Refused(why) => return self.end(storage, why),
            Walked::Done(_) => self.listed = true,
            Walked::More => {}
        }
        self.pending = files;
        self.next = 0;
        self.phase = Phase::Copy;
        JobStep::More
    }

    /// One piece of one file.
    #[inline(never)]
    fn copy(&mut self, storage: &dyn Storage) -> JobStep {
        if self.file.is_none() {
            let Some((rel, size)) = self
                .pending
                .get(self.next)
                .map(|(rel, size)| (rel.as_str(), *size))
            else {
                self.phase = if self.listed {
                    Phase::Check
                } else {
                    Phase::List
                };
                return JobStep::More;
            };
            let (src, dst) = match (join(&self.source, rel), join(&self.stage, rel)) {
                (Some(src), Some(dst)) => (src, dst),
                _ => return self.end(storage, reason::OTHER),
            };
            self.next += 1;
            if !rel.contains('/') && same_fold(rel, CODE) {
                self.code_name = Text::new(rel);
            }
            if let Some((dir, _)) = dst.rsplit_once('/')
                && let Err(error) = storage.create_dir(dir)
            {
                return self.end(storage, io(error));
            }
            self.file = Some(Copying {
                src,
                dst,
                size,
                at: 0,
            });
        }
        let Some(file) = self.file.as_mut() else {
            return JobStep::More;
        };
        let len = (file.size - file.at).min(self.buf.len() as u64) as usize;
        if let Err(why) = read_all(storage, &file.src, file.at, &mut self.buf[..len]) {
            return self.end(storage, why);
        }
        let wrote = if file.at == 0 {
            storage.write(&file.dst, &self.buf[..len])
        } else {
            storage.append(&file.dst, &self.buf[..len])
        };
        if let Err(error) = wrote {
            return self.end(storage, io(error));
        }
        file.at += len as u64;
        if file.at >= file.size {
            // What lies in the stage is what was listed.
            let same = storage.size(&file.dst).is_ok_and(|size| size == file.size);
            self.file = None;
            if !same {
                return self.end(storage, reason::CHANGED);
            }
        }
        JobStep::More
    }

    /// The stage checked again; its buffer is the check's meanwhile.
    #[inline(never)]
    fn check(&mut self, env: &JobEnv<'_>) -> JobStep {
        if self.check.is_none() {
            // The copy is done: its buffer, walk and list are let go before
            // the check takes its own.
            self.buf = Vec::new();
            self.walk = None;
            self.pending = Vec::new();
            let Some(check) =
                PluginCheck::staged_box(self.owner, self.request, &self.stage, self.id.as_str())
            else {
                return self.end(env.storage, reason::OTHER);
            };
            self.check = Some(check);
        }
        let Some(check) = self.check.as_mut() else {
            return JobStep::More;
        };
        match check.step(env) {
            JobStep::Done(done) => {
                self.check = None;
                // A stage that does not fit ends as an install would; a
                // revoked key is told with its facts, and ends the same.
                if done.reason != reason::OK {
                    return self.end(env.storage, done.reason);
                }
                self.facts = done.facts;
                // The pins and revocations are read with the job's buffer.
                let Some(buf) = io_buffer() else {
                    return self.end(env.storage, reason::OTHER);
                };
                self.buf = buf;
                self.phase = Phase::Standing;
                JobStep::More
            }
            _ => JobStep::More,
        }
    }

    /// The pins and revocations of the id, a step at a time.
    #[inline(never)]
    fn standing(&mut self, storage: &dyn Storage) -> JobStep {
        match self.inquiry.step(storage, self.id.as_str(), &mut self.buf) {
            Ok(Some(standing)) => {
                self.standing = Some(standing);
                self.phase = Phase::Rules;
                JobStep::More
            }
            Ok(None) => JobStep::More,
            Err(plugin_trust::Unreadable) => self.end(storage, reason::CARD),
        }
    }

    /// The trust rules, the version gate and the question.
    #[inline(never)]
    fn rules(&mut self, env: &JobEnv<'_>) -> JobStep {
        let storage = env.storage;
        let Some(facts) = self.facts else {
            return self.end(storage, reason::OTHER);
        };
        let Some(standing) = self.standing else {
            return self.end(storage, reason::OTHER);
        };
        // The manifest of the stage is what installs: it names the id still.
        let manifest = match plugin_folder::read_manifest(
            storage,
            &self.stage,
            self.id.as_str(),
            &mut self.buf,
        ) {
            Ok(manifest) if manifest.id.as_str() == self.id.as_str() => manifest,
            Ok(_) => return self.end(storage, reason::CHANGED),
            Err(why) => return self.end(storage, why),
        };
        let code = facts.key.zip(facts.sha256);
        let staged = Staged {
            id: self.id.as_str(),
            code,
            standing,
        };
        let (sideload, pins) = match self.decided(&staged) {
            Ok(decided) => decided,
            Err(why) => return self.end(storage, why),
        };
        // A plugin of the other kind is none to replace.
        if self
            .installed
            .is_some_and(|installed| installed.code != code.is_some())
        {
            return self.end(storage, reason::ID_TAKEN);
        }
        let relation = match self.installed.and_then(|installed| installed.version) {
            Some(old) if manifest.parsed < old => Relation::Older,
            Some(old) if manifest.parsed == old => Relation::Same,
            _ => Relation::Newer,
        };
        if relation != Relation::Newer && self.flags & FLAG_OLDER == 0 {
            return self.end(storage, reason::OLDER_NOT_ALLOWED);
        }
        // What the staged code declares: a stage that cannot be read does not
        // show "no features" (the grant would then skip the question).
        let uses = match self.uses(env, code.is_some()) {
            Ok(uses) => uses,
            Err(why) => return self.end(storage, why),
        };
        let new_uses = code.is_some() && self.installed.is_some() && self.new_uses(env, &uses);
        self.manifest = Some(manifest);
        self.pins = Some(pins);
        // Nothing after the rules reads through the buffer: it is not held
        // while the question waits, nor through the marks and the swap.
        self.buf = Vec::new();
        // The grant skips only the prompt.
        let skip = env.granted && !sideload && !new_uses && self.installed.is_some();
        if skip {
            self.phase = Phase::Marks;
            JobStep::More
        } else {
            let Some(question) = self.boxed_question(&manifest, &facts, sideload, relation, &uses)
            else {
                return self.end(storage, reason::OTHER);
            };
            self.question = Some(question);
            self.phase = Phase::Ask;
            JobStep::More
        }
    }

    /// What the rules decide for `staged`, the pins already in their box (the
    /// pins are big: they are neither kept on the frame of `rules` nor copied
    /// from it to the box). A box that cannot be had is reason `OTHER`.
    #[inline(never)]
    fn decided(&self, staged: &Staged<'_>) -> Result<(bool, Box<Pins>), u8> {
        match decide(staged, &self.evidence) {
            Decision::Install { sideload, pins } => try_box(pins)
                .map(|pins| (sideload, pins))
                .ok_or(reason::OTHER),
            Decision::Refused(why) => Err(why),
        }
    }

    /// The question for the user, in a box of its own made here (its 700 bytes
    /// are not built on the frame of `rules`, nor copied to the box from it).
    #[inline(never)]
    fn boxed_question(
        &self,
        manifest: &Manifest,
        facts: &super::plugin_job::PluginFacts,
        sideload: bool,
        relation: Relation,
        uses: &Uses,
    ) -> Option<Box<PluginQuestion>> {
        try_box(PluginQuestion {
            owner: self.owner,
            request: self.request,
            kind: KIND_INSTALL,
            id: self.id,
            name: manifest.name,
            version: manifest.version,
            signer: facts.signer,
            key: facts.key,
            sideload,
            relation,
            uses: *uses,
        })
    }

    /// What the staged code declares, as shown (empty for a data plugin).
    ///
    /// # Errors
    /// A reason where the code of the stage cannot be read.
    // Its own frame: `Uses` is 400 bytes and several are alive in `rules`.
    #[inline(never)]
    fn uses(&self, env: &JobEnv<'_>, code: bool) -> Result<Uses, u8> {
        if !code {
            return Ok(no_uses());
        }
        let path = join(&self.stage, self.code_name.as_str()).ok_or(reason::OTHER)?;
        uses_of(env, &path)
    }

    /// Whether the staged version declares a feature the installed one does
    /// not (or the installed one's cannot be read).
    #[inline(never)]
    fn new_uses(&self, env: &JobEnv<'_>, staged: &Uses) -> bool {
        let mut path = String::new();
        if path
            .try_reserve_exact(
                paperpaw_sdk::builtin::PLUGIN_DIR.len() + 2 + self.id.as_str().len() + CODE.len(),
            )
            .is_err()
        {
            return true;
        }
        path.push_str(paperpaw_sdk::builtin::PLUGIN_DIR);
        path.push('/');
        path.push_str(self.id.as_str());
        path.push('/');
        path.push_str(CODE);
        let Ok(installed) = uses_of(env, &path) else {
            return true;
        };
        staged.more
            || staged.keys[..usize::from(staged.count)]
                .iter()
                .any(|key| !installed.keys[..usize::from(installed.count)].contains(key))
    }

    #[inline(never)]
    fn marks(&mut self, storage: &dyn Storage) -> JobStep {
        let (Some(manifest), Some(_)) = (self.manifest, self.pins.as_ref()) else {
            return self.end(storage, reason::OTHER);
        };
        // A catalog's install keeps its store: it offers its updates.
        let from = if self.flags & FLAG_CATALOG != 0 {
            From::Store(manifest.parsed)
        } else {
            From::Local
        };
        if marks::write(storage, &self.stage, &Marks::new(from)).is_err() {
            return self.end(storage, reason::CARD);
        }
        self.phase = Phase::Journal;
        JobStep::More
    }

    #[inline(never)]
    fn journal(&mut self, storage: &dyn Storage, built_ins: &dyn BuiltIns) -> JobStep {
        // Before the install commits: a device that kept no record of the
        // store's installs takes its plugins for the store's, once.
        if let Some(found) = built_ins.found() {
            super::store_record::adopt(storage, found);
        }
        let Some(pins) = self.pins.as_deref() else {
            return self.end(storage, reason::OTHER);
        };
        let Some(target) = plugin_swap::target_path(self.id.as_str()) else {
            return self.end(storage, reason::OTHER);
        };
        let had_old = match present(storage, &target) {
            Ok(had) => had,
            Err(why) => return self.end(storage, why),
        };
        // A folder that appeared (or went) while the user decided is not what
        // was confirmed.
        if had_old != self.installed.is_some() {
            return self.end(storage, reason::CHANGED);
        }
        // Built before the journal: a refusal for memory leaves none.
        let Some(swap) = Swap::live_box(self.id, had_old, pins) else {
            return self.end(storage, reason::OTHER);
        };
        if let Err(why) = plugin_swap::begin(storage, swap.entry()) {
            return self.end(storage, why);
        }
        // From the journal on, the stage is the swap's.
        self.phase = Phase::Swapping;
        self.release();
        self.swap = Some(swap);
        JobStep::More
    }

    #[inline(never)]
    fn swapping(&mut self, storage: &dyn Storage) -> JobStep {
        let Some(swap) = self.swap.as_mut() else {
            return self.end(storage, reason::OTHER);
        };
        match swap.step(storage) {
            SwapStep::More => JobStep::More,
            SwapStep::Done { rolled_back: false } => {
                let code = self.facts.is_some_and(|facts| facts.key.is_some());
                self.phase = Phase::Ended;
                JobStep::Done(Finished {
                    reason: reason::OK,
                    facts: None,
                    flags: if code { DONE_AT_NEXT_START } else { 0 },
                    changed: Some(self.id),
                    needs_draft: 0,
                })
            }
            SwapStep::Done { rolled_back: true } => self.end(storage, reason::OTHER),
            SwapStep::Failed(why) => self.end(storage, why),
            SwapStep::Stuck => self.end(storage, reason::OTHER),
        }
    }
}

/// No feature declared.
fn no_uses() -> Uses {
    Uses {
        keys: [Text::new(""); USES_SHOWN],
        count: 0,
        more: false,
    }
}

pub(super) fn installed_of(
    storage: &dyn Storage,
    target: &str,
    id: &str,
    buf: &mut [u8],
) -> Installed {
    let version = plugin_folder::read_manifest(storage, target, id, buf)
        .ok()
        .map(|manifest| manifest.parsed);
    let mut code = String::new();
    let have = code
        .try_reserve_exact(target.len() + 1 + CODE.len())
        .is_ok()
        && {
            code.push_str(target);
            code.push('/');
            code.push_str(CODE);
            storage.exists(&code)
        };
    // Code of another kind than `code.ppn` is still code.
    let other = CODE_FILES.iter().skip(1).any(|name| {
        let mut path = String::new();
        path.try_reserve_exact(target.len() + 1 + name.len())
            .is_ok()
            && {
                path.push_str(target);
                path.push('/');
                path.push_str(name);
                storage.exists(&path)
            }
    });
    Installed {
        version,
        code: have || other,
    }
}

fn join(dir: &str, rel: &str) -> Option<String> {
    let mut path = String::new();
    path.try_reserve_exact(dir.len() + 1 + rel.len()).ok()?;
    path.push_str(dir);
    path.push('/');
    path.push_str(rel);
    Some(path)
}

fn read_all(storage: &dyn Storage, path: &str, mut at: u64, buf: &mut [u8]) -> Result<(), u8> {
    let mut done = 0;
    while done < buf.len() {
        match storage.read_at(path, at, &mut buf[done..]) {
            Ok(0) | Err(StorageError::NotFound) => return Err(reason::CARD),
            Ok(n) => {
                done += n;
                at += n as u64;
            }
            Err(error) => return Err(io(error)),
        }
    }
    Ok(())
}

/// The `uses.*` features the format-5 code at `path` asks for, as shown.
///
/// # Errors
/// A reason: 1 for no memory, 16 for a card error, 4 where the file is none
/// this device reads.
#[inline(never)]
fn uses_of(env: &JobEnv<'_>, path: &str) -> Result<Uses, u8> {
    let storage = env.storage;
    let size = storage.size(path).map_err(io)?;
    let mut head = [0_u8; HEADER_LEN];
    read_all(storage, path, 0, &mut head)?;
    let header = Header5::read(&head, size, &env.host.loader).map_err(|_| reason::DAMAGED)?;
    let mut record = Vec::new();
    record
        .try_reserve_exact(header.meta_len as usize)
        .map_err(|_| reason::OTHER)?;
    record.resize(header.meta_len as usize, 0);
    read_all(
        storage,
        path,
        HEADER_LEN as u64 + u64::from(header.meta_at),
        &mut record,
    )?;
    let checked = metadata::check(&record, &env.host.offered).map_err(|_| reason::DAMAGED)?;
    let mut uses = no_uses();
    for feature in checked.requested_features() {
        if usize::from(uses.count) < USES_SHOWN {
            uses.keys[usize::from(uses.count)] = Text::<USE_KEY_MAX>::new(feature.key);
            uses.count += 1;
        } else {
            uses.more = true;
        }
    }
    Ok(uses)
}

impl PluginJob for PluginInstall {
    fn owner(&self) -> u64 {
        self.owner
    }

    fn request(&self) -> u32 {
        self.request
    }

    fn kind(&self) -> u32 {
        KIND_INSTALL
    }

    #[inline(never)]
    fn step(&mut self, env: &JobEnv<'_>) -> JobStep {
        if let Some(why) = self.stopped {
            return self.end(env.storage, why);
        }
        let storage = env.storage;
        match self.phase {
            Phase::Start => self.start(env),
            Phase::CleanStage => self.clean_stage(storage),
            Phase::List => self.list(storage),
            Phase::Copy => self.copy(storage),
            Phase::Check => self.check(env),
            Phase::Standing => self.standing(storage),
            Phase::Rules => self.rules(env),
            Phase::Ask => match self.question.take() {
                Some(question) => {
                    self.phase = Phase::Waiting;
                    JobStep::Ask(*question)
                }
                None => self.end(storage, reason::OTHER),
            },
            Phase::Waiting => match self.answer {
                None => JobStep::Wait,
                Some(Answer::Yes) => {
                    self.phase = Phase::Marks;
                    JobStep::More
                }
                Some(Answer::No) => self.end(storage, reason::DECLINED),
                // An install offers two answers; the removal's second one
                // is never a yes to it.
                Some(Answer::Dismissed | Answer::YesAndData) => {
                    self.end(storage, reason::DISMISSED)
                }
            },
            Phase::Marks => self.marks(storage),
            Phase::Journal => self.journal(storage, env.built_ins),
            Phase::Swapping => self.swapping(storage),
            Phase::Ended => JobStep::Done(Finished::refused(reason::OTHER)),
        }
    }

    fn answer(&mut self, answer: Answer) {
        if self.phase == Phase::Waiting {
            self.answer = Some(answer);
        }
    }

    fn must_finish(&self) -> bool {
        // From the journal on the card holds half of a change: the swap is
        // stepped on to its end, whatever stops, and tells its real outcome.
        self.phase == Phase::Swapping
    }

    fn keep_without_sender(&self) -> bool {
        // The user said yes (or the grant did): the plugin that asked going
        // away does not stop what was confirmed.
        matches!(self.phase, Phase::Marks | Phase::Journal | Phase::Swapping)
            || (self.phase == Phase::Waiting && self.answer == Some(Answer::Yes))
    }

    fn abort(&mut self, why: u8) {
        if self.must_finish() {
            return;
        }
        self.stopped = Some(why);
        self.release();
    }
}

#[cfg(test)]
#[path = "plugin_install_tests.rs"]
pub(in crate::native_host) mod tests;

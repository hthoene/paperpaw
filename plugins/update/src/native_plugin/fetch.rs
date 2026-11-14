//! The code plugins the update server names builds of, fetched once the update
//! is downloaded: each file into the data folder's `staging/<id>/` with `core.net`
//! (checked against its size and digest), then the host is asked to install the
//! folder over the plugin installed (`command.install_plugin`: it checks it and
//! asks the reader, decision 132). One plugin at a time. A plugin that fails
//! here is fetched or got from the store later; it never stops the update.
use super::{COMMANDS, NativeUpdate, online::FETCH};
use crate::native_online::Build;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Storage, StorageError,
    digest::{sha1::Sha1, sha256::Sha256},
    install::{Digest, Package},
    native::{
        Error, Host,
        commands::{CommandSender, InstallRoot},
        device::{DeviceOutcome, DeviceRequest, InstallKind},
        events::InstallDone,
        formatting::Arg,
        net::{FileRoot, HttpDone, Net, REPLY, REQUEST_INPUT, Reason, Request, RequestId},
        storage::DataStorage,
        ui::Notify,
    },
};
use paperpaw_ui_kit::{
    Messages,
    mem::{buffer, push},
    text::{concat, copy},
};

/// The caller's own number of the install request of the first build; the next
/// build's is one more.
const INSTALL_REQUEST: u32 = 100;
/// Bytes of a file read at a time to check it.
const PIECE: usize = 4096;
/// Where the files wait for the host to install them.
const STAGING: &str = "staging";

/// The builds being fetched, and how far.
#[derive(Debug)]
pub struct Fetch {
    pub version: String,
    builds: Vec<Build>,
    /// The build and its file being downloaded.
    build: usize,
    file: usize,
    request: Option<RequestId>,
    /// The host was asked to install the build (its question is open).
    installing: bool,
    done: usize,
    failed: usize,
}

/// Where the file `path` of the plugin `id` is kept while it is fetched.
fn staged(id: &str, path: &str) -> Result<String, Error> {
    concat(&[STAGING, "/", id, "/", path])
}

/// The folder the host installs.
fn folder(id: &str) -> Result<String, Error> {
    concat(&[STAGING, "/", id])
}

/// Whether the file `path` has the size and the digest the server said, read in
/// pieces, never whole. A file that cannot be read is no match.
///
/// # Errors
/// `NoMemory`.
pub fn verify(
    storage: &dyn Storage,
    path: &str,
    size: u64,
    digest: &Digest,
) -> Result<bool, Error> {
    match storage.size(path) {
        Ok(found) if found == size => {}
        Ok(_) => return Ok(false),
        Err(error) => return fallible(error),
    }
    let mut piece = buffer(PIECE)?;
    let (mut sha1, mut sha256) = (Sha1::new(), Sha256::new());
    let mut at = 0u64;
    loop {
        let n = match storage.read_at(path, at, &mut piece) {
            Ok(n) => n,
            Err(error) => return fallible(error),
        };
        if n == 0 {
            break;
        }
        let bytes = piece.get(..n).unwrap_or(&[]);
        match digest {
            Digest::Sha1(_) => sha1.update(bytes),
            _ => sha256.update(bytes),
        }
        at += n as u64;
    }
    let (got, want) = match digest {
        Digest::Sha1(hex) => (paperpaw_sdk::digest::hex::encode(&sha1.finish()), hex),
        Digest::Sha256(hex) => (sha256.hex(), hex),
        _ => return Ok(false),
    };
    Ok(at == size && got.eq_ignore_ascii_case(want))
}

/// A refused allocation is the caller's; any other failure is a file that does
/// not check out.
fn fallible(error: StorageError) -> Result<bool, Error> {
    match error {
        StorageError::Other(0x6e6d) => Err(Error::NoMemory),
        _ => Ok(false),
    }
}

/// Removes what was staged for `package` (what is not there is no error) and
/// the folders it was in, the deepest first.
fn clear(storage: &dyn Storage, package: &Package) {
    let mut folders: Vec<String> = Vec::new();
    for file in &package.files {
        let Ok(path) = staged(&package.plugin, &file.path) else {
            continue;
        };
        storage.remove(&path).ok();
        let mut rest = path.as_str();
        while let Some(cut) = rest.rfind('/') {
            rest = rest.get(..cut).unwrap_or("");
            if !folders.iter().any(|f| f == rest)
                && let Ok(copy) = copy(rest)
            {
                push(&mut folders, copy).ok();
            }
        }
    }
    paperpaw_sdk::sort::sort_unstable_by_key(&mut folders, |f| core::cmp::Reverse(f.len()));
    for folder in folders {
        storage.remove(&folder).ok();
    }
}

/// What a build's next step came to.
enum Step {
    /// A request or the host's question is open.
    Waiting,
    /// No build is left.
    Ended,
}

impl Fetch {
    /// Asks for the next file of the build at hand, or for the install of the
    /// build whose files are all there, or says there is no build left.
    fn step(&mut self, host: &Host<'_>, net: &Net) -> Result<Step, Error> {
        let Some(build) = self.builds.get(self.build) else {
            return Ok(Step::Ended);
        };
        let Some(file) = build.package.files.get(self.file) else {
            let request = INSTALL_REQUEST.saturating_add(u32::try_from(self.build).unwrap_or(0));
            let mut commands = buffer(COMMANDS)?;
            CommandSender::new(host, &mut commands).install_plugin(
                request,
                InstallRoot::Data,
                &folder(&build.package.plugin)?,
                true,
                false,
            )?;
            self.installing = true;
            return Ok(Step::Waiting);
        };
        let path = staged(&build.package.plugin, &file.path)?;
        let request = Request::get(FETCH, &file.url)?
            .save_to(FileRoot::Data, &path)?
            .max_file(file.size)?;
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        self.request = Some(net.request(host, &request, &mut input, &mut out)?);
        Ok(Step::Waiting)
    }

    /// The build at hand goes: what was staged is removed, it counts as failed.
    fn fail(&mut self, host: &Host<'_>) {
        if let Some(build) = self.builds.get(self.build) {
            clear(&DataStorage::new(host), &build.package);
        }
        self.failed += 1;
        self.next();
    }

    /// On to the next build.
    fn next(&mut self) {
        self.build += 1;
        self.file = 0;
        self.installing = false;
    }

    /// The install number of the build at hand.
    fn install_request(&self) -> u32 {
        INSTALL_REQUEST.saturating_add(u32::try_from(self.build).unwrap_or(0))
    }
}

impl NativeUpdate {
    /// Of `builds`, those of code plugins installed here: the others have nothing
    /// to be replaced (a plugin built in now that the update leaves out comes
    /// back as a file only where a host way to put it in place for the next
    /// firmware exists, `PORTING-7.md`).
    pub(super) fn wanted(
        &mut self,
        host: &Host<'_>,
        builds: &[Build],
    ) -> Result<Vec<Build>, Error> {
        if builds.is_empty() {
            return Ok(Vec::new());
        }
        let mut ids: Vec<String> = Vec::new();
        self.code_plugins(host, |id, _| push(&mut ids, copy(id)?))?;
        let mut wanted = Vec::new();
        for build in builds.iter().filter(|b| ids.contains(&b.package.plugin)) {
            push(&mut wanted, build.clone())?;
        }
        Ok(wanted)
    }

    /// Starts fetching the `builds` for the update `version`.
    pub(super) fn fetch_begin(
        &mut self,
        host: &Host<'_>,
        version: &str,
        builds: Vec<Build>,
    ) -> Result<(), Error> {
        let fetch = Fetch {
            version: copy(version)?,
            builds,
            build: 0,
            file: 0,
            request: None,
            installing: false,
            done: 0,
            failed: 0,
        };
        self.fetch_drive(host, fetch)?;
        self.show_downloading(host)
    }

    /// Goes on until a request or the host's question is open, or no build is
    /// left. A build that cannot go on counts as failed.
    fn fetch_drive(&mut self, host: &Host<'_>, mut fetch: Fetch) -> Result<(), Error> {
        let net = self.online.net(host)?;
        loop {
            match fetch.step(host, net) {
                Ok(Step::Waiting) => {
                    self.online.fetch = Some(paperpaw_ui_kit::mem::boxed(fetch)?);
                    return Ok(());
                }
                Ok(Step::Ended) => return self.fetch_end(host, &fetch),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => fetch.fail(host),
            }
        }
    }

    /// A file arrived, or did not: checked, and on to the next or to the install.
    pub(super) fn fetched(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        let Some(mut fetch) = self.online.fetch.take() else {
            return Ok(());
        };
        if fetch.request != Some(done.id()) {
            self.online.fetch = Some(fetch);
            return Ok(());
        }
        fetch.request = None;
        let arrived = done.reason() == Reason::Ok && done.succeeded() && done.saved();
        let kept = match fetch.builds.get(fetch.build) {
            Some(build) if arrived => match build.package.files.get(fetch.file) {
                Some(file) => verify(
                    &DataStorage::new(host),
                    &staged(&build.package.plugin, &file.path)?,
                    file.size,
                    &file.digest,
                )?,
                None => false,
            },
            _ => false,
        };
        if kept {
            fetch.file += 1;
        } else {
            fetch.fail(host);
        }
        self.fetch_drive(host, *fetch)
    }

    /// The end of an install this flow asked for: on to the next build.
    #[inline(never)]
    pub(super) fn fetch_install_done(
        &mut self,
        host: &Host<'_>,
        done: &InstallDone<'_>,
    ) -> Result<(), Error> {
        let Some(mut fetch) = self.online.fetch.take() else {
            return Ok(());
        };
        let own = done.kind() == InstallKind::Plugin
            && fetch.installing
            && done.request().map(DeviceRequest::get) == Some(fetch.install_request());
        if !own {
            self.online.fetch = Some(fetch);
            return Ok(());
        }
        if done.status().is_ok() && done.outcome() == DeviceOutcome::Ok {
            if let Some(build) = fetch.builds.get(fetch.build) {
                clear(&DataStorage::new(host), &build.package);
            }
            fetch.done += 1;
            fetch.next();
        } else {
            fetch.fail(host);
        }
        self.fetch_drive(host, *fetch)
    }

    /// Every build is fetched or failed: the update is ready, and what did not
    /// come is said.
    fn fetch_end(&mut self, host: &Host<'_>, fetch: &Fetch) -> Result<(), Error> {
        if fetch.failed == 0 {
            return self.downloaded(host, &fetch.version);
        }
        let mut messages = Messages::new()?;
        let count = i64::try_from(fetch.failed).unwrap_or(i64::MAX);
        let text = messages.format(host, "update.missing", &[Arg::Count(count)])?;
        let mut commands = buffer(COMMANDS)?;
        match CommandSender::new(host, &mut commands).notify(Notify::new(&text)?) {
            Ok(()) | Err(Error::State) => {}
            Err(error) => return Err(error),
        }
        self.refresh(host)
    }
}

//! The production [`NetHost`] and [`NetTransport`]: core's adapter over the
//! device's storage and the two small traits the shell implements
//! ([`ShellHttp`]: the HTTP client and server; [`ShellNetwork`]: the clock, the
//! network and the power state).
//!
//! What core derives itself, from the storage and the path rules of
//! `files.md` section 3: the access to a root ([`NetHost::root_access`]), the
//! size of a file to send, whether two names are one file, the free-space
//! reserve, the paths a saving request holds (in the device's one
//! [`PathsInUse`] table, which `core.files` asks too), the staged file of a
//! saved answer (written by the shell under a name core chose, renamed over the
//! target by core when the answer is whole, removed otherwise) and the
//! clean-up of a plugin's `.received/`. The shell gets storage paths already
//! joined, so it needs no notion of plugins or roots.
//!
//! # Views of the kernel
//! The plugin id of an owner, its event subscriptions, the work run of the
//! running callback and the screen instance that is showing come from the
//! kernel's [`OwnerDirectory`]. An owner it does not know (stopped, or not
//! admitted) is refused (`ERR_STATE`): no request or server is admitted
//! without a subscription, and no root-0 path is joined.
use super::{
    Decision, NetHost, NetProfile, NetTable, NetTransport, PowerEnd, Refused, Reply, Start,
    args::{self, ROLE_SAVE},
    reason,
};
use crate::native_host::{
    host_services::Caller,
    net_policy::NetPolicy,
    net_pool::NetPool,
    owner_directory::OwnerDirectory,
    owner_paths::{self, OwnerPaths, RECEIVED, owned},
    paths_in_use::PathsInUse,
    save_files::SaveFiles,
    shared::Shared,
    shell_wiring::{
        HttpDone, HttpHeaders, HttpJob, HttpReason, ServedDecision, ServedReply, ServerReason,
        ShellHttp, ShellNetwork,
    },
    storage::{self, has_room},
};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY, ERR_NOT_FOUND};
use paperpaw_platform::Storage;

/// The longest address of a server (`event.server.address`).
const ADDRESS_MAX: usize = 64;

/// A saving request: where its answer goes and where it is staged.
struct Staged {
    id: u32,
    owner: u64,
    target: String,
    staged: String,
    /// The staged file was renamed over the target (a report to retry must not
    /// rename again).
    renamed: bool,
}

/// The device's `core.net` host.
pub(crate) struct NetDevice {
    table: NetTable,
    storage: Rc<dyn Storage>,
    http: Rc<dyn ShellHttp>,
    network: Rc<dyn ShellNetwork>,
    profile: NetProfile,
    saves: SaveFiles,
    staged: RefCell<Vec<Staged>>,
    /// The owners: the plugin ids kept for their cleanup, and the kernel's
    /// view of them (subscriptions, the screen that shows, the work run).
    paths: OwnerPaths,
    /// What the user allows (`net_policy`).
    policy: Rc<NetPolicy>,
    /// The transport of the cable's server (port 0), where the profile has
    /// the cable (`cable_link`). Held in the device's own block: making the
    /// device stays one allocation, the one that may be refused.
    cable: Option<super::cable_link::CableLink>,
}

/// The staged name of a saved answer's target (`.pp-staged`, core's one name).
fn staged_name(target: &str) -> Result<String, i32> {
    crate::native_host::staged_name::staged_path(target).ok_or(ERR_NO_MEMORY)
}

impl NetDevice {
    /// The host over `storage`, `http` and `network`, charging `pool` and
    /// holding its saves in `in_use`.
    #[allow(clippy::too_many_arguments, reason = "one device, built in one place")]
    pub(crate) fn new(
        storage: Rc<dyn Storage>,
        http: Rc<dyn ShellHttp>,
        network: Rc<dyn ShellNetwork>,
        profile: NetProfile,
        pool: Rc<NetPool>,
        in_use: Rc<PathsInUse>,
        owners: Shared<OwnerDirectory>,
        policy: Rc<NetPolicy>,
    ) -> Self {
        Self {
            policy,
            cable: None,
            table: NetTable::new(pool),
            saves: SaveFiles::new(storage.clone(), in_use),
            storage,
            http,
            network,
            profile,
            staged: RefCell::new(Vec::new()),
            paths: OwnerPaths::new(owners),
        }
    }

    /// The device with the cable's server served over `link` (`cable_link`).
    #[must_use]
    pub(crate) fn with_cable(mut self, link: super::cable_link::CableLink) -> Self {
        self.cable = Some(link);
        self
    }

    /// The card the service saves to and receives bodies on.
    pub(crate) fn storage(&self) -> &dyn Storage {
        &*self.storage
    }

    /// The cable's link, where the profile has the cable.
    pub(crate) fn cable(&self) -> Option<&super::cable_link::CableLink> {
        self.cable.as_ref()
    }

    /// The cable's request `request`, if the cable carries it.
    fn on_cable(&self, request: u32) -> Option<&super::cable_link::CableLink> {
        self.cable.as_ref().filter(|link| link.carries(request))
    }

    fn remove_best_effort(&self, joined: &str) {
        // A file left behind is replaced by the next save to its path.
        self.storage.remove(joined).unwrap_or_default();
    }

    fn keep(&self, entry: Staged) -> Result<(), i32> {
        let mut staged = self.staged.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        staged.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        staged.push(entry);
        Ok(())
    }

    /// Forgets the staged file of request `id` and removes it.
    fn drop_staged(&self, id: u32) {
        let entry = self.staged.try_borrow_mut().ok().and_then(|mut staged| {
            let at = staged.iter().position(|s| s.id == id)?;
            Some(staged.remove(at))
        });
        if let Some(entry) = entry {
            // Not a staged file that is the only copy of an answer.
            self.saves.discard(entry.owner, &entry.target);
        }
    }

    /// The transport reports request `id` ended: a whole saved answer is
    /// renamed over its target first (the plugin reads it in the event that
    /// follows), anything else removes the staged file.
    ///
    /// # Errors
    /// What the service answers (`ERR_BUSY`: report again).
    pub(crate) fn report_finished(&self, id: u32, mut done: HttpDone<'_>) -> Result<(), i32> {
        let whole =
            done.reason == HttpReason::Ok && done.saved && (200..300).contains(&done.status);
        if whole {
            match self.promote(id) {
                Ok(()) => {}
                Err(ERR_BUSY) => return Err(ERR_BUSY),
                Err(_) => {
                    // The answer is lost with the file: told as an I/O failure.
                    done.saved = false;
                    done.reason = HttpReason::Io;
                }
            }
        } else {
            done.saved = false;
        }
        let result = super::finished(self, id, done.into_finished());
        // `ERR_BUSY` keeps the entry for the retry, which must not rename again.
        if result != Err(ERR_BUSY) {
            self.drop_staged(id);
        }
        result
    }

    /// Renames the staged file of request `id` over its target, once.
    fn promote(&self, id: u32) -> Result<(), i32> {
        let mut staged = self.staged.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let entry = staged
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or(ERR_NOT_FOUND)?;
        if entry.renamed {
            return Ok(());
        }
        self.saves
            .commit(entry.owner, &entry.target, &entry.staged)
            .map_err(storage::status)?;
        entry.renamed = true;
        Ok(())
    }

    /// A request head arrived on `server`: the decision, with the storage path
    /// the body goes to (`.received/<server>/<request>` of the plugin).
    pub(crate) fn served_head(
        &self,
        server: u32,
        method: &str,
        target: &str,
        content_length: Option<u64>,
        chunked: bool,
        head_len: u32,
    ) -> Result<ServedDecision, i32> {
        use core::fmt::Write;
        let decision = super::served_head(
            self,
            server,
            method,
            target,
            content_length,
            chunked,
            head_len,
        )?;
        let (request, token) = match decision {
            Decision::Reject(status) => return Ok(ServedDecision::Reject(status)),
            Decision::Accept { request, token } => (request, token),
            _ => return Ok(ServedDecision::Busy),
        };
        let path = (|| {
            let owner = super::server_owner(self, server).ok_or(ERR_NOT_FOUND)?;
            let mut rel = String::new();
            rel.try_reserve_exact(32).map_err(|_| ERR_NO_MEMORY)?;
            write!(rel, "{RECEIVED}/{token}/{request}").map_err(|_| ERR_NO_MEMORY)?;
            self.paths.join_owned(owner, 0, &rel)
        })();
        match path {
            Ok(path) => Ok(ServedDecision::Accept { request, path }),
            Err(_) => {
                // The request cannot be received here: it ends, the plugin is
                // told nothing.
                super::served_gone(self, request)?;
                Ok(ServedDecision::Reject(503))
            }
        }
    }
}

impl NetHost for NetDevice {
    fn table(&self) -> &NetTable {
        &self.table
    }
    fn transport(&self) -> &dyn NetTransport {
        self
    }
    fn profile(&self) -> NetProfile {
        self.profile
    }
    fn now_ms(&self) -> u64 {
        self.network.now_ms()
    }

    fn root_access(&self, who: &Caller<'_>, root: u32, path: &str, write: bool) -> Result<(), i32> {
        owner_paths::access(who, root, path, write)
    }

    fn file_len(&self, who: &Caller<'_>, root: u32, path: &str) -> Result<u64, i32> {
        let joined = OwnerPaths::join_for(who, root, path)?;
        self.storage.size(&joined).map_err(|_| ERR_NOT_FOUND)
    }

    fn same_file(&self, _root: u32, a: &str, b: &str) -> bool {
        // By spelling, with case ignored where the storage ignores it. Short
        // (8.3) aliases of FAT are not resolved here: a path with `~` is
        // taken for the same file (the safe side for a replace).
        if a.contains('~') || b.contains('~') {
            return true;
        }
        if self.storage.ignores_case() {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    }

    fn claim_save(&self, owner: u64, root: u32, path: &str) -> Result<(), i32> {
        let target = self.paths.join_owned(owner, root, path)?;
        let staged = staged_name(&target)?;
        // Another writer's path is not saved over; one's own, again, is.
        self.saves.claim(owner, root, path, &target, &staged, false)
    }

    fn release_save(&self, owner: u64, root: u32, path: &str) {
        self.saves.release_at(owner, root, path);
    }

    fn subscribed(&self, owner: u64, tag: u32) -> bool {
        self.paths.owners().subscribed(owner, tag)
    }

    fn power_end(&self, owner: u64, background: bool) -> Option<PowerEnd> {
        let mut end = None;
        match self
            .paths
            .plugin_of(owner, &mut |id| end = self.policy.end_for(id, background))
        {
            Ok(()) => end,
            // A plugin whose id cannot be had (not known, or the table in use)
            // is not let out: the answer is the safe one.
            Err(_) => Some(PowerEnd {
                reason: reason::NETWORK_OFF,
                later: false,
            }),
        }
    }

    fn resource_opened(&self, run: u64) -> Result<(), i32> {
        self.paths.owners().resource_opened(run)
    }
    fn resource_closed(&self, run: u64) {
        self.paths
            .owners()
            .resource_closed(run, self.network.now_ms());
    }

    fn network_up(&self) -> bool {
        self.network.network_up()
    }
    fn network_demand(&self, wanted: bool) {
        self.network.network_demand(wanted);
    }

    fn current_run(&self, owner: u64) -> Option<u64> {
        self.paths.owners().current_run(owner)
    }
    fn showing_screen(&self, owner: u64) -> Option<u64> {
        self.paths.owners().showing_screen(owner)
    }
    fn topmost_screen(&self, owner: u64) -> Option<u64> {
        self.paths.owners().topmost_screen(owner)
    }
    fn run_known(&self) -> bool {
        self.paths.owners().run_known()
    }

    fn room_for(&self, extra: u64) -> bool {
        has_room(&*self.storage, extra)
    }

    fn clear_received(&self, owner: u64, server: Option<u32>) {
        self.clear_folder(owner, server);
        if server.is_none() {
            // The owner's last cleanup (it started or stopped): a staged file
            // kept after a failed rename is tried again.
            self.saves.recover(None);
            self.paths.forget(owner);
        }
    }

    fn remove_received(&self, owner: u64, server: u32, request: u32) {
        self.remove_one(owner, server, request);
    }
}

impl NetDevice {
    fn clear_folder(&self, owner: u64, server: Option<u32>) {
        use core::fmt::Write;
        // A plugin that has no id cannot have received anything.
        let mut path = String::new();
        let made = self.paths.plugin_of(owner, &mut |id| {
            if path
                .try_reserve_exact(storage::PREFIX.len() + id.len() + 32)
                .is_ok()
            {
                path.push_str(storage::PREFIX);
                path.push_str(id);
                path.push('/');
                path.push_str(RECEIVED);
            }
        });
        if made.is_err() || path.is_empty() {
            return;
        }
        if let Some(server) = server
            && write!(path, "/{server}").is_err()
        {
            return;
        }
        // A folder that is not there is already clear.
        self.storage.remove_dir_all(&path).unwrap_or_default();
    }

    fn remove_one(&self, owner: u64, server: u32, request: u32) {
        use core::fmt::Write;
        let mut path = String::new();
        let made = self.paths.plugin_of(owner, &mut |id| {
            if path
                .try_reserve_exact(storage::PREFIX.len() + id.len() + 48)
                .is_ok()
            {
                path.push_str(storage::PREFIX);
                path.push_str(id);
            }
        });
        if made.is_err() || path.is_empty() {
            return;
        }
        if write!(path, "/{RECEIVED}/{server}/{request}").is_ok() {
            self.remove_best_effort(&path);
        }
    }
}

/// A job for the shell: the request's own fields and the storage paths of its
/// files, already joined.
fn job<'a>(
    id: u32,
    start: &'a Start<'a>,
    send: Option<&'a str>,
    save: Option<&'a str>,
) -> HttpJob<'a> {
    let request = start.request;
    HttpJob::new(
        id,
        request.head.method,
        request.head.url,
        HttpHeaders::new(&request.headers[..request.header_count]),
        request.body,
        send,
        save,
        start.max_body,
        start.max_file,
        start.timeout_ms,
    )
}

fn why(reason: ServerReason) -> Refused {
    match reason {
        ServerReason::NoNetwork => reason::NO_NETWORK,
        ServerReason::Offline => reason::OFFLINE,
        ServerReason::Saving => reason::SAVING,
        ServerReason::NetworkOff => reason::NETWORK_OFF,
        _ => reason::REFUSED,
    }
}

impl NetTransport for NetDevice {
    fn start(&self, id: u32, start: &Start<'_>) -> Result<(), i32> {
        let owner = start.owner;
        let send = start
            .request
            .send
            .map(|f| self.paths.join_owned(owner, f.root, f.path))
            .transpose()?;
        let target = start
            .request
            .save
            .filter(|f| f.role == ROLE_SAVE)
            .map(|f| self.paths.join_owned(owner, f.root, f.path))
            .transpose()?;
        let staged = target.as_deref().map(staged_name).transpose()?;
        if let (Some(target), Some(staged)) = (&target, &staged) {
            self.keep(Staged {
                id,
                owner,
                target: owned(target, 0)?,
                staged: owned(staged, 0)?,
                renamed: false,
            })?;
            // The staged file starts empty: the shell appends, so bytes a
            // power cut left at that name must not be in front of the answer.
            if let Err(error) = self.saves.begin(owner, target, staged) {
                self.drop_staged(id);
                return Err(storage::status(error));
            }
        }
        let started = self
            .http
            .start(&job(id, start, send.as_deref(), staged.as_deref()));
        if started.is_err() {
            self.drop_staged(id);
        }
        started
    }

    fn abort(&self, id: u32) {
        self.http.abort(id);
        self.drop_staged(id);
    }

    fn listen(&self, server: u32, port: u32, address: &mut String) -> Result<(), Refused> {
        if port == 0 {
            // The cable is the kernel's: the shell only passes its lines. Its
            // address is empty and `started` comes from the kernel's pump.
            return match &self.cable {
                Some(link) if link.listen(server) => Ok(()),
                _ => Err(reason::REFUSED),
            };
        }
        let mut buf = [0u8; ADDRESS_MAX];
        let length = self.http.listen(server, port, &mut buf).map_err(why)?;
        // The shell is listening now: a refusal from here on stops it.
        let kept = (|| {
            let text = buf
                .get(..length)
                .and_then(|b| core::str::from_utf8(b).ok())
                .ok_or(reason::REFUSED)?;
            super::gate().map_err(|_| reason::REFUSED)?;
            address
                .try_reserve_exact(text.len())
                .map_err(|_| reason::REFUSED)?;
            address.push_str(text);
            Ok(())
        })();
        if kept.is_err() {
            self.http.unlisten(server);
        }
        kept
    }

    fn unlisten(&self, server: u32) {
        if self
            .cable
            .as_ref()
            .is_some_and(|link| link.unlisten(server))
        {
            return;
        }
        self.http.unlisten(server);
    }

    fn reply(&self, request: u32, reply: &Reply<'_>) -> Result<(), i32> {
        let owner = reply.owner;
        let response: &args::Response<'_> = reply.response;
        if let Some(link) = self.on_cable(request) {
            return link.answer(
                request,
                response.head.status,
                response.head.content_type,
                response.body,
                response.send.is_some(),
            );
        }
        let send = response
            .send
            .map(|f| self.paths.join_owned(owner, f.root, f.path))
            .transpose()?;
        self.http.reply(
            request,
            &ServedReply::new(
                response.head.status,
                response.head.content_type,
                HttpHeaders::new(&response.headers[..response.header_count]),
                response.body,
                send.as_deref(),
                reply.file_len,
            ),
        )
    }

    fn close_served(&self, request: u32, status: u32) {
        match self.on_cable(request) {
            Some(link) => link.close(request, status),
            None => self.http.close_served(request, status),
        }
    }
}

#[cfg(test)]
#[path = "device_tests.rs"]
mod tests;

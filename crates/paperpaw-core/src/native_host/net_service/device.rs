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
    net_pool::NetPool,
    owner_directory::OwnerDirectory,
    paths_in_use::PathsInUse,
    shared::Shared,
    shell_wiring::{
        HttpDone, HttpHeaders, HttpJob, HttpReason, NetEnd, ServedDecision, ServedReply,
        ServerReason, ShellHttp, ShellNetwork,
    },
    storage::{self, has_room, relative_path},
};
use alloc::{rc::Rc, string::String, vec::Vec};
use core::cell::RefCell;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_UNSUPPORTED,
};
use paperpaw_platform::Storage;

/// Longest joined path (as `storage.rs`).
const MAX_JOINED: usize = 640;
/// Root 1 (the user's files) and 2 (the plugin's install folder).
const CARD: u32 = 1;
const BUNDLE: u32 = 2;
/// The folders of root 1 that belong to the kernel and the plugins.
const PROTECTED: [&str; 2] = ["plugins", ".paperpaw"];
/// The host's folder in root 0.
const RECEIVED: &str = ".received";
/// The name a saved answer is written under until it is whole.
pub(crate) const STAGED: &str = ".pp-staged";
/// The longest address of a server (`event.server.address`).
const ADDRESS_MAX: usize = 64;

/// A saving request: where its answer goes and where it is staged.
struct Staged {
    id: u32,
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
    in_use: Rc<PathsInUse>,
    staged: RefCell<Vec<Staged>>,
    /// The plugin ids of the owners that used the network, kept from their
    /// first use to their last cleanup: a stopped owner is gone from the
    /// registry before its saves and `.received/` are cleaned up.
    plugins: RefCell<Vec<(u64, String)>>,
    /// The kernel's view of the owners: plugin ids, subscriptions, the screen
    /// that shows, the work run.
    owners: Shared<OwnerDirectory>,
}

fn owned(text: &str, extra: usize) -> Result<String, i32> {
    super::gate()?;
    let mut copy = String::new();
    copy.try_reserve_exact(text.len() + extra)
        .map_err(|_| ERR_NO_MEMORY)?;
    copy.push_str(text);
    Ok(copy)
}

fn staged_name(target: &str) -> Result<String, i32> {
    let mut name = owned(target, STAGED.len())?;
    name.push_str(STAGED);
    Ok(name)
}

impl NetDevice {
    /// The host over `storage`, `http` and `network`, charging `pool` and
    /// holding its saves in `in_use`.
    pub(crate) fn new(
        storage: Rc<dyn Storage>,
        http: Rc<dyn ShellHttp>,
        network: Rc<dyn ShellNetwork>,
        profile: NetProfile,
        pool: Rc<NetPool>,
        in_use: Rc<PathsInUse>,
        owners: Shared<OwnerDirectory>,
    ) -> Self {
        Self {
            table: NetTable::new(pool),
            storage,
            http,
            network,
            profile,
            in_use,
            staged: RefCell::new(Vec::new()),
            plugins: RefCell::new(Vec::new()),
            owners,
        }
    }

    /// Hands the id of the plugin that owns `owner` to `visit`: the id kept
    /// from an earlier use (a stopped owner is gone from the directory before
    /// its cleanup), else the directory's. `ERR_STATE` when neither knows it.
    fn plugin_of(&self, owner: u64, visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        {
            let known = self.plugins.try_borrow().map_err(|_| ERR_BUSY)?;
            if let Some((_, id)) = known.iter().find(|(o, _)| *o == owner) {
                visit(id);
                return Ok(());
            }
        }
        let mut found = false;
        self.owners.with_id(owner, &mut |id| {
            found = true;
            self.remember(owner, id);
            visit(id);
        })?;
        debug_assert!(found);
        Ok(())
    }

    /// Keeps the id of `owner`'s plugin for its cleanup after it stopped. When
    /// there is no room, the id is not kept (the cleanup then needs the view).
    fn remember(&self, owner: u64, id: &str) {
        let Ok(mut known) = self.plugins.try_borrow_mut() else {
            return;
        };
        if known.try_reserve(1).is_err() {
            return;
        }
        if let Ok(copy) = owned(id, 0) {
            known.push((owner, copy));
        }
    }

    /// The owner's last cleanup is done: its id is let go.
    fn forget(&self, owner: u64) {
        if let Ok(mut known) = self.plugins.try_borrow_mut() {
            known.retain(|(o, _)| *o != owner);
        }
    }

    /// `path` of `root` for `plugin`, joined for the storage.
    fn join(plugin: Option<&str>, root: u32, path: &str) -> Result<String, i32> {
        let (kind, id) = match (root, plugin) {
            (CARD, _) => ("", None),
            (0, Some(id)) => (storage::PREFIX, Some(id)),
            (BUNDLE, Some(id)) => ("plugins/", Some(id)),
            _ => return Err(ERR_UNSUPPORTED),
        };
        let id_len = id.map_or(0, |id| id.len() + usize::from(!path.is_empty()));
        let len = kind.len() + id_len + path.len();
        if len > MAX_JOINED {
            return Err(ERR_INVALID);
        }
        super::gate()?;
        let mut joined = String::new();
        joined
            .try_reserve_exact(len + STAGED.len())
            .map_err(|_| ERR_NO_MEMORY)?;
        joined.push_str(kind);
        if let Some(id) = id {
            joined.push_str(id);
            if !path.is_empty() {
                joined.push('/');
            }
        }
        joined.push_str(path);
        Ok(joined)
    }

    /// `path` of `root` for the plugin of the call.
    fn join_for(who: &Caller<'_>, root: u32, path: &str) -> Result<String, i32> {
        Self::join(Some(who.plugin), root, path)
    }

    /// `path` of `root` for the plugin that owns `owner`, outside any call.
    fn join_owned(&self, owner: u64, root: u32, path: &str) -> Result<String, i32> {
        if root == CARD {
            return Self::join(None, root, path);
        }
        let mut joined = Err(ERR_UNSUPPORTED);
        self.plugin_of(owner, &mut |id| {
            joined = Self::join(Some(id), root, path);
        })?;
        joined
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
            self.remove_best_effort(&entry.staged);
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
        self.storage
            .rename(&entry.staged, &entry.target)
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
            self.join_owned(owner, 0, &rel)
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
        match root {
            0 => {}
            CARD => who.features.feature("uses.storage")?,
            BUNDLE if write => return Err(ERR_DENIED),
            BUNDLE => {}
            _ => return Err(ERR_UNSUPPORTED),
        }
        if !path.is_empty() {
            relative_path(path.as_bytes())?;
        }
        let first = path.split('/').next().unwrap_or_default();
        if root == CARD && PROTECTED.iter().any(|p| first.eq_ignore_ascii_case(p)) {
            return Err(ERR_INVALID);
        }
        // `.received` is the host's in root 0: a plugin may read what is
        // there, never write into it (`network.md` 5.2).
        if root == 0 && write && first.eq_ignore_ascii_case(RECEIVED) {
            return Err(ERR_DENIED);
        }
        // The staged name is core's, as `.part` was: no path of the plugin's
        // may end in it.
        if write
            && path
                .as_bytes()
                .last_chunk::<{ STAGED.len() }>()
                .is_some_and(|tail| tail.eq_ignore_ascii_case(STAGED.as_bytes()))
        {
            return Err(ERR_INVALID);
        }
        Ok(())
    }

    fn file_len(&self, who: &Caller<'_>, root: u32, path: &str) -> Result<u64, i32> {
        let joined = Self::join_for(who, root, path)?;
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
        let target = self.join_owned(owner, root, path)?;
        let staged = staged_name(&target)?;
        let ignore_case = self.storage.ignores_case();
        // Another writer's path is not saved over; one's own, again, is.
        if self.in_use.holds_except(&target, ignore_case, Some(owner))
            || self.in_use.holds_except(&staged, ignore_case, Some(owner))
        {
            return Err(ERR_BUSY);
        }
        self.in_use.claim(owner, &target)?;
        if let Err(error) = self.in_use.claim(owner, &staged) {
            self.in_use.release(owner, &target);
            return Err(error);
        }
        Ok(())
    }

    fn release_save(&self, owner: u64, root: u32, path: &str) {
        let Ok(target) = self.join_owned(owner, root, path) else {
            return;
        };
        if !self.in_use.release(owner, &target) {
            return;
        }
        if let Ok(staged) = staged_name(&target) {
            self.in_use.release(owner, &staged);
            self.remove_best_effort(&staged);
        }
    }

    fn subscribed(&self, owner: u64, tag: u32) -> bool {
        self.owners.subscribed(owner, tag)
    }

    fn power_end(&self, owner: u64, background: bool) -> Option<PowerEnd> {
        let mut end = None;
        self.plugin_of(owner, &mut |id| {
            end = self.network.power_end(id, background);
        })
        .ok()?;
        end.map(|e| PowerEnd {
            reason: match e.end {
                NetEnd::Offline => reason::OFFLINE,
                NetEnd::Saving => reason::SAVING,
                _ => reason::NETWORK_OFF,
            },
            later: e.later,
        })
    }

    fn network_up(&self) -> bool {
        self.network.network_up()
    }
    fn network_demand(&self, wanted: bool) {
        self.network.network_demand(wanted);
    }

    fn current_run(&self, owner: u64) -> Option<u64> {
        self.owners.current_run(owner)
    }
    fn showing_screen(&self, owner: u64) -> Option<u64> {
        self.owners.showing_screen(owner)
    }

    fn room_for(&self, extra: u64) -> bool {
        has_room(&*self.storage, extra)
    }

    fn clear_received(&self, owner: u64, server: Option<u32>) {
        self.clear_folder(owner, server);
        if server.is_none() {
            // The owner's last cleanup (it started or stopped).
            self.forget(owner);
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
        let made = self.plugin_of(owner, &mut |id| {
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
        let made = self.plugin_of(owner, &mut |id| {
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
            .map(|f| self.join_owned(owner, f.root, f.path))
            .transpose()?;
        let target = start
            .request
            .save
            .filter(|f| f.role == ROLE_SAVE)
            .map(|f| self.join_owned(owner, f.root, f.path))
            .transpose()?;
        let staged = target.as_deref().map(staged_name).transpose()?;
        if let (Some(target), Some(staged)) = (&target, &staged) {
            self.keep(Staged {
                id,
                target: owned(target, 0)?,
                staged: owned(staged, 0)?,
                renamed: false,
            })?;
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
        let mut buf = [0u8; ADDRESS_MAX];
        let length = self.http.listen(server, port, &mut buf).map_err(why)?;
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
    }

    fn unlisten(&self, server: u32) {
        self.http.unlisten(server);
    }

    fn reply(&self, request: u32, reply: &Reply<'_>) -> Result<(), i32> {
        let owner = reply.owner;
        let response: &args::Response<'_> = reply.response;
        let send = response
            .send
            .map(|f| self.join_owned(owner, f.root, f.path))
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
        self.http.close_served(request, status);
    }
}

#[cfg(test)]
#[path = "device_tests.rs"]
mod tests;

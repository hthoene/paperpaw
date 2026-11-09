#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! `core.net` over a fake device and a fake transport with failure
//! injection: admission, the queue, the pool, the events, the servers.
extern crate std;
use super::*;
use crate::native_host::{
    admitted_features::AdmittedFeatures, host_services::HostServices, net_pool::NetPool,
};
use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use std::{collections::BTreeMap, string::ToString};

#[path = "tests/admission.rs"]
mod admission;
#[path = "tests/deadline.rs"]
mod deadline;
#[path = "tests/pump_bytes.rs"]
mod pump_bytes;
#[path = "tests/requests.rs"]
mod requests;
#[path = "tests/serving.rs"]
mod serving_tests;
#[path = "tests/tls.rs"]
mod tls_tests;

std::thread_local! {
    /// Armed with `n`, the `n`th allocation gate of the test fails.
    pub(crate) static FAIL_NTH: Cell<u32> = const { Cell::new(0) };
}

pub(crate) const OWNER: u64 = 7;
pub(crate) const OTHER: u64 = 8;

/// The fake device and its transport. Everything the service asks of either
/// is logged, everything it is told is switchable.
pub(crate) struct Fake {
    pub(crate) table: NetTable,
    pub(crate) profile: Cell<NetProfile>,
    pub(crate) clock: Cell<u64>,
    pub(crate) network: Cell<bool>,
    pub(crate) power: Cell<Option<PowerEnd>>,
    pub(crate) subscribed: Cell<bool>,
    pub(crate) screen: Cell<Option<u64>>,
    /// The plugin's topmost showing screen, for a call from no screen.
    pub(crate) topmost: Cell<Option<u64>>,
    /// Whether the work scheduler can be asked.
    pub(crate) run_known: Cell<bool>,
    pub(crate) run: Cell<Option<u64>>,
    /// The work run's resources: told open, told closed, refused to open.
    pub(crate) run_opened: RefCell<Vec<u64>>,
    pub(crate) run_closed: RefCell<Vec<u64>>,
    pub(crate) run_open_fails: Cell<Option<i32>>,
    pub(crate) free: Cell<u64>,
    /// The plugins have `uses.storage` (root 1).
    pub(crate) storage: Cell<bool>,
    /// (root, path) -> size of files that exist.
    pub(crate) files: RefCell<BTreeMap<(u32, std::string::String), u64>>,
    pub(crate) claims: RefCell<Vec<(u64, u32, std::string::String)>>,
    pub(crate) released: RefCell<Vec<(u64, u32, std::string::String)>>,
    pub(crate) started: RefCell<Vec<u32>>,
    pub(crate) aborted: RefCell<Vec<u32>>,
    pub(crate) start_fails: Cell<Option<i32>>,
    pub(crate) demand: RefCell<Vec<bool>>,
    pub(crate) listens: RefCell<Vec<(u32, u32)>>,
    pub(crate) listen_result: RefCell<Option<Result<&'static str, u32>>>,
    pub(crate) unlistened: RefCell<Vec<u32>>,
    pub(crate) replies: RefCell<Vec<(u32, u32, Option<u64>)>>,
    pub(crate) reply_fails: Cell<Option<i32>>,
    pub(crate) claim_fails: Cell<Option<i32>>,
    pub(crate) closed: RefCell<Vec<(u32, u32)>>,
    pub(crate) cleared: RefCell<Vec<(u64, Option<u32>)>>,
    pub(crate) removed: RefCell<Vec<(u64, u32, u32)>>,
}

impl Fake {
    pub(crate) fn new(profile: NetProfile) -> Self {
        Self::with_pool(
            profile,
            Rc::new(NetPool::with_tls_sessions(profile.pool, 4)),
        )
    }
    /// A device whose pool is shared with someone else (`core.sockets`).
    pub(crate) fn with_pool(profile: NetProfile, pool: Rc<NetPool>) -> Self {
        Self {
            table: NetTable::new(pool),
            profile: Cell::new(profile),
            clock: Cell::new(1_000),
            network: Cell::new(true),
            power: Cell::new(None),
            subscribed: Cell::new(true),
            screen: Cell::new(Some(5)),
            topmost: Cell::new(None),
            run_known: Cell::new(true),
            run: Cell::new(None),
            run_opened: RefCell::new(Vec::new()),
            run_closed: RefCell::new(Vec::new()),
            run_open_fails: Cell::new(None),
            free: Cell::new(u64::MAX),
            storage: Cell::new(false),
            files: RefCell::new(BTreeMap::new()),
            claims: RefCell::new(Vec::new()),
            released: RefCell::new(Vec::new()),
            started: RefCell::new(Vec::new()),
            aborted: RefCell::new(Vec::new()),
            start_fails: Cell::new(None),
            demand: RefCell::new(Vec::new()),
            listens: RefCell::new(Vec::new()),
            listen_result: RefCell::new(None),
            unlistened: RefCell::new(Vec::new()),
            replies: RefCell::new(Vec::new()),
            reply_fails: Cell::new(None),
            claim_fails: Cell::new(None),
            closed: RefCell::new(Vec::new()),
            cleared: RefCell::new(Vec::new()),
            removed: RefCell::new(Vec::new()),
        }
    }
    pub(crate) fn x4() -> Self {
        Self::new(NetProfile::X4)
    }
    /// The X4's profile with the servers of the X4 Pro (two at once), for the
    /// tests that run several.
    pub(crate) fn servers(count: u32) -> Self {
        Self::new(NetProfile {
            servers: count,
            ..NetProfile::X4
        })
    }
    pub(crate) fn advance(&self, ms: u64) {
        self.clock.set(self.clock.get() + ms);
    }
    pub(crate) fn pool_used(&self) -> u32 {
        self.table.pool().used()
    }
}

impl NetTransport for Fake {
    fn start(&self, id: u32, _: &Start<'_>) -> Result<(), i32> {
        if let Some(error) = self.start_fails.get() {
            return Err(error);
        }
        self.started.borrow_mut().push(id);
        Ok(())
    }
    fn abort(&self, id: u32) {
        self.aborted.borrow_mut().push(id);
    }
    fn listen(
        &self,
        server: u32,
        port: u32,
        address: &mut std::string::String,
    ) -> Result<(), Refused> {
        self.listens.borrow_mut().push((server, port));
        match self
            .listen_result
            .borrow()
            .unwrap_or(Ok("192.168.1.20:8080"))
        {
            Ok(text) => {
                address.push_str(text);
                Ok(())
            }
            Err(reason) => Err(reason),
        }
    }
    fn unlisten(&self, server: u32) {
        self.unlistened.borrow_mut().push(server);
    }
    fn reply(&self, request: u32, reply: &Reply<'_>) -> Result<(), i32> {
        if let Some(error) = self.reply_fails.get() {
            return Err(error);
        }
        self.replies
            .borrow_mut()
            .push((request, reply.response.head.status, reply.file_len));
        Ok(())
    }
    fn close_served(&self, request: u32, status: u32) {
        self.closed.borrow_mut().push((request, status));
    }
}

impl NetHost for Fake {
    fn table(&self) -> &NetTable {
        &self.table
    }
    fn transport(&self) -> &dyn NetTransport {
        self
    }
    fn profile(&self) -> NetProfile {
        self.profile.get()
    }
    fn now_ms(&self) -> u64 {
        self.clock.get()
    }
    fn root_access(&self, _: &Caller<'_>, root: u32, path: &str, write: bool) -> Result<(), i32> {
        // Root 1 needs `uses.storage`: the fake gives it to nobody, and keeps
        // `plugins` out; root 2 is read-only.
        match (root, write) {
            (1, _) if !self.storage.get() => Err(paperpaw_native::host_glue::ERR_DENIED),
            (1, _) => Ok(()),
            (2, true) => Err(paperpaw_native::host_glue::ERR_DENIED),
            _ if path.starts_with(".received") => Err(paperpaw_native::host_glue::ERR_DENIED),
            _ => Ok(()),
        }
    }
    fn file_len(&self, _: &Caller<'_>, root: u32, path: &str) -> Result<u64, i32> {
        self.files
            .borrow()
            .get(&(root, path.to_string()))
            .copied()
            .ok_or(paperpaw_native::host_glue::ERR_NOT_FOUND)
    }
    fn same_file(&self, _: u32, a: &str, b: &str) -> bool {
        a.eq_ignore_ascii_case(b)
    }
    fn claim_save(&self, owner: u64, root: u32, path: &str) -> Result<(), i32> {
        if let Some(error) = self.claim_fails.get() {
            return Err(error);
        }
        self.claims
            .borrow_mut()
            .push((owner, root, path.to_string()));
        Ok(())
    }
    fn release_save(&self, owner: u64, root: u32, path: &str) {
        let mut claims = self.claims.borrow_mut();
        if let Some(at) = claims
            .iter()
            .position(|c| c.0 == owner && c.1 == root && c.2 == path)
        {
            claims.remove(at);
        }
        self.released
            .borrow_mut()
            .push((owner, root, path.to_string()));
    }
    fn subscribed(&self, _: u64, _: u32) -> bool {
        self.subscribed.get()
    }
    fn power_end(&self, _: u64, _: bool) -> Option<PowerEnd> {
        self.power.get()
    }
    fn network_up(&self) -> bool {
        self.network.get()
    }
    fn network_demand(&self, wanted: bool) {
        self.demand.borrow_mut().push(wanted);
    }
    fn current_run(&self, _: u64) -> Option<u64> {
        self.run.get()
    }
    fn resource_opened(&self, run: u64) -> Result<(), i32> {
        if let Some(error) = self.run_open_fails.get() {
            return Err(error);
        }
        self.run_opened.borrow_mut().push(run);
        Ok(())
    }
    fn resource_closed(&self, run: u64) {
        self.run_closed.borrow_mut().push(run);
    }
    fn showing_screen(&self, _: u64) -> Option<u64> {
        self.screen.get()
    }
    fn topmost_screen(&self, _: u64) -> Option<u64> {
        self.topmost.get()
    }
    fn run_known(&self) -> bool {
        self.run_known.get()
    }
    fn room_for(&self, extra: u64) -> bool {
        extra <= self.free.get()
    }
    fn clear_received(&self, owner: u64, server: Option<u32>) {
        self.cleared.borrow_mut().push((owner, server));
    }
    fn remove_received(&self, owner: u64, server: u32, request: u32) {
        self.removed.borrow_mut().push((owner, server, request));
    }
}

pub(crate) fn caller<'a>(owner: u64, hosts: &'a HostServices) -> Caller<'a> {
    Caller {
        owner,
        plugin: "plugin",
        features: AdmittedFeatures::NONE,
        hosts,
    }
}

/// A counted input batch.
pub(crate) fn batch(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = std::vec![0; 4096];
    let used = wire::records_encode_counted(records, 20, 4096, &mut out).unwrap();
    out.truncate(used);
    out
}

pub(crate) fn head(token: u32, url: &'static str) -> wire::ServiceNetRequest<'static> {
    wire::ServiceNetRequest {
        token,
        method: "GET",
        url,
        flags: 0,
        timeout_ms: 0,
        max_body: 0,
        max_file: 0,
    }
}

pub(crate) fn get(token: u32) -> Vec<u8> {
    batch(&[Record::ServiceNetRequest(head(
        token,
        "https://example.org/a",
    ))])
}

pub(crate) fn save_to(token: u32, root: u32, path: &'static str) -> Vec<u8> {
    batch(&[
        Record::ServiceNetRequest(head(token, "https://example.org/a")),
        Record::ServiceNetFile(wire::ServiceNetFile {
            role: args::ROLE_SAVE,
            root,
            path,
        }),
    ])
}

pub(crate) fn big(token: u32, max_body: u32) -> Vec<u8> {
    let mut h = head(token, "https://example.org/big");
    h.max_body = max_body;
    batch(&[Record::ServiceNetRequest(h)])
}

/// `request` as the plugin `owner` calls it.
pub(crate) fn request(fake: &Fake, owner: u64, input: &[u8]) -> Result<u32, i32> {
    let hosts = HostServices::new();
    table::request(fake, &caller(owner, &hosts), input)
}

/// What a plugin is told, decoded.
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) enum Event {
    Done {
        id: u32,
        token: u32,
        status: u32,
        reason: u32,
        flags: u32,
        body_len: u32,
    },
    Progress {
        id: u32,
        received: u64,
        total: u64,
        flags: u32,
    },
    Server {
        token: u32,
        state: u32,
        reason: u32,
        address: std::string::String,
    },
    Served {
        server: u32,
        request: u32,
        method: std::string::String,
        target: std::string::String,
        flags: u32,
        body_len: u64,
    },
}

/// The next event of `owner`, if any.
pub(crate) fn next(fake: &dyn NetHost, owner: u64) -> Option<Event> {
    let mut out = std::vec![0; 2048];
    let used = next_event(fake, owner, &mut out).unwrap()?;
    // What the plugin's callback gets: the SDK frames it as a batch of one
    // (the kernel's `service_event` reads its tag the same way), and decodes
    // the kinds it has (99, 100; it has none yet for 101 and 102).
    let incoming = paperpaw_sdk::native::events::Incoming::new(&out[..used]).unwrap();
    if incoming.is::<paperpaw_sdk::native::events::kinds::HttpDone>()
        || incoming.is::<paperpaw_sdk::native::events::kinds::HttpProgress>()
    {
        assert!(incoming.decode().is_ok(), "the plugin decodes the event");
    }
    Some(
        match crate::native_host::codec::decode_any_single(&out[..used]).unwrap() {
            Record::EventHttpDone(e) => Event::Done {
                id: e.id,
                token: e.token,
                status: e.status,
                reason: e.reason,
                flags: e.flags,
                body_len: e.body_len,
            },
            Record::EventHttpProgress(e) => Event::Progress {
                id: e.id,
                received: e.received,
                total: e.total,
                flags: e.flags,
            },
            Record::EventServer(e) => Event::Server {
                token: e.server,
                state: e.state,
                reason: e.reason,
                address: e.address.to_string(),
            },
            Record::EventServedRequest(e) => Event::Served {
                server: e.server,
                request: e.request,
                method: e.method.to_string(),
                target: e.target.to_string(),
                flags: e.flags,
                body_len: e.body_len,
            },
            other => panic!("{other:?}"),
        },
    )
}

/// Every event of `owner`, then the callback returns.
pub(crate) fn drain(fake: &dyn NetHost, owner: u64) -> Vec<Event> {
    let mut all = Vec::new();
    while let Some(event) = next(fake, owner) {
        all.push(event);
    }
    event_delivered(fake, owner).unwrap();
    all
}

/// The transport reports a finished answer.
pub(crate) fn finish(fake: &Fake, id: u32, status: u32, body: &[u8]) {
    finished(
        fake,
        id,
        Finished {
            status,
            reason: reason::OK,
            saved: false,
            truncated: false,
            etag: "",
            content_type: "text/plain",
            final_url: "https://example.org/a",
            body: body.to_vec(),
        },
    )
    .unwrap();
}

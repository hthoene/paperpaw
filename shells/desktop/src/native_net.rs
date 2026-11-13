//! `core.net` for the linked native plugins on the computer: HTTP requests
//! through the emulator's own client (`http`: the system's `curl` on a thread
//! of its own, or the route a test or the workshop set), reported back to core
//! from the device's own loop ([`NativeNet::pump`], called by
//! `Device::poll_http`), never from inside a call.
//!
//! What core does is core's (limits, the policy, the pool, staged files,
//! events); this moves bytes. A download to a file is written here in 4 KiB
//! appends to the staged path core made (core renames it over the target); an
//! upload from a file is read whole (a computer has the memory; the request
//! still ends when its bytes stop coming). Serving (`listen`) is not backed on
//! the desktop yet: a plugin that asks to serve is refused (`Refused`). Sockets
//! are `native_sock`'s.
//!
//! **Redirects** are followed here by `paperpaw_http::follow`, the rules of
//! `network.md` 3.1 that the other shells keep (at most 10; never from
//! `https` to `http`; 303, and 301 or 302 after a `POST`, go on as a `GET`
//! without a body; no body and only the 12 harmless headers to another
//! origin), and `curl` is told not to follow any (no `-L`: it would send key
//! headers on, and a body, and follow down to `http`). A redirect from `http`
//! to `https` is told to core (`redirected_to_https`) and the request goes on
//! when core starts it again. `timeout_ms` of a request in memory counts from
//! its first start.
//!
//! **The network on demand:** core tells the network manager plugin of a
//! wanted network (`event.network_wanted`) and the shell hears of it
//! ([`NativeNet::wanted`]). No manager plugin is linked on the desktop, so
//! `Device::poll_native_net` brings the emulated Wi-Fi up for it, as
//! `--wifi online` does, on a profile where PaperPaw manages Wi-Fi.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use paperpaw_core::native_host::shell_wiring::{
    HttpDone, HttpJob, HttpReason, NetReports, ServedReply, ServerReason, ShellHttp, ShellNetwork,
};
use paperpaw_core::sdk::HttpError;
use paperpaw_core::{HttpCall, HttpResponse};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_IO, ERR_NOT_FOUND, ERR_UNSUPPORTED};
use paperpaw_platform::Storage;

use crate::http::Http;
use crate::linked::DeviceView;

/// The bytes of one append of a download to its file.
const CHUNK: usize = 4096;

/// Redirects followed at most (`network.md` 3.1).
const MAX_REDIRECTS: u32 = 10;

/// What a request needs when its answer comes.
struct Pending {
    save_path: Option<String>,
    max_file: u64,
    /// The request as it goes now: changed by each redirect followed.
    call: HttpCall,
    /// Redirects followed so far.
    hops: u32,
    /// When it first started (`timeout_ms` of a request in memory counts
    /// from there, redirects and all).
    started: Instant,
    /// The first call's `timeout_ms`.
    timeout_ms: u32,
    /// Redirected to `https` and told to core: it goes on when core starts
    /// it again.
    parked: bool,
}

impl Pending {
    /// The time left to a request in memory (a file's is by its bytes
    /// moving, as for each hop), at least 1 ms; `None` where it is spent.
    fn time_left(&self) -> Option<u32> {
        if self.save_path.is_some() || self.call.body_from.is_some() {
            return Some(self.timeout_ms);
        }
        let spent = u32::try_from(self.started.elapsed().as_millis()).unwrap_or(u32::MAX);
        self.timeout_ms.checked_sub(spent).filter(|left| *left > 0)
    }
}

/// The shell side of `core.net` (see the module).
pub struct NativeNet {
    http: RefCell<Http>,
    storage: Rc<dyn Storage>,
    view: Rc<DeviceView>,
    pending: RefCell<HashMap<u32, Pending>>,
    demand: Cell<bool>,
    /// A network was up at the last pump.
    was_up: Cell<bool>,
    /// Redirects to `https` core was too busy to take.
    upgrades: RefCell<Vec<u32>>,
}

impl std::fmt::Debug for NativeNet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeNet")
            .field("in_flight", &self.in_flight())
            .finish_non_exhaustive()
    }
}

impl NativeNet {
    /// The transport over `storage` (the card), reading the clock and network
    /// from `view`.
    #[must_use]
    pub fn new(storage: Rc<dyn Storage>, view: Rc<DeviceView>) -> Self {
        Self {
            http: RefCell::new(Http::new()),
            storage,
            view,
            pending: RefCell::new(HashMap::new()),
            demand: Cell::new(false),
            was_up: Cell::new(false),
            upgrades: RefCell::new(Vec::new()),
        }
    }

    /// Requests started and not answered yet.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.pending.borrow().len()
    }

    /// Whether a network is wanted now (a request or server is open).
    #[must_use]
    pub fn wanted(&self) -> bool {
        self.demand.get()
    }

    /// Hands core the answers that have come (waiting up to `wait` for the
    /// first), then lets it take its turn (`tick`). `true` when core was
    /// told anything: the kernel must then tick, as its pass delivers the
    /// events (the X4's shell does the same, `host_net::Bridge::report`).
    pub fn pump(&self, reports: &NetReports, wait: Option<std::time::Duration>) -> bool {
        // A network gone since the last pump ends every request.
        let up = ShellNetwork::network_up(self);
        let lost = self.was_up.replace(up) && !up;
        if lost {
            let _ = reports.network_lost();
        }
        let told = self.pump_with(
            wait,
            &|id, done| {
                // A request core no longer knows is answered `ERR_NOT_FOUND`: no one to tell.
                let _ = reports.finished(id, done);
            },
            &|id| reports.redirected_to_https(id),
        );
        let _ = reports.tick();
        lost || told
    }

    /// [`NativeNet::pump`] with what core is told through `finish` (a
    /// request's end) and `to_https` (`redirected_to_https`); `true` when
    /// either was called.
    fn pump_with(
        &self,
        wait: Option<std::time::Duration>,
        finish: &dyn Fn(u32, HttpDone<'_>),
        to_https: &dyn Fn(u32) -> Result<(), i32>,
    ) -> bool {
        let told = Cell::new(false);
        let finish = |id, done: HttpDone<'_>| {
            told.set(true);
            finish(id, done);
        };
        let to_https = |id| {
            told.set(true);
            to_https(id)
        };
        let finish: &dyn Fn(u32, HttpDone<'_>) = &finish;
        let to_https: &dyn Fn(u32) -> Result<(), i32> = &to_https;
        // Redirects core was too busy to take: again.
        for id in std::mem::take(&mut *self.upgrades.borrow_mut()) {
            self.upgrade(id, finish, to_https);
        }
        loop {
            let response = {
                let mut http = self.http.borrow_mut();
                if http.in_flight == 0 {
                    break;
                }
                http.next(wait)
            };
            let Some(response) = response else { break };
            // A request aborted since has no one to tell.
            let Some(mut pending) = self.pending.borrow_mut().remove(&response.id) else {
                continue;
            };
            let id = response.id;
            match followed(&mut pending, response) {
                Outcome::Done(response) => {
                    finish(id, done_of(&response, &pending, &*self.storage));
                }
                Outcome::Send => {
                    self.resume(&mut pending);
                    self.pending.borrow_mut().insert(id, pending);
                }
                Outcome::Upgrade => {
                    // Back in the table, parked, before core is told: core
                    // may start the request again within its report.
                    self.pending.borrow_mut().insert(id, pending);
                    self.upgrade(id, finish, to_https);
                }
            }
        }
        told.get()
    }

    /// Tells core that request `id` was redirected to `https` and is parked
    /// (no entry is borrowed: core starts it again from inside the report
    /// where the session is free).
    fn upgrade(
        &self,
        id: u32,
        finish: &dyn Fn(u32, HttpDone<'_>),
        to_https: &dyn Fn(u32) -> Result<(), i32>,
    ) {
        match to_https(id) {
            Ok(()) => {}
            Err(ERR_BUSY) => self.upgrades.borrow_mut().push(id),
            // Cancelled meanwhile: no one to tell.
            Err(ERR_NOT_FOUND) => {
                self.pending.borrow_mut().remove(&id);
            }
            Err(_) => {
                let Some(pending) = self.pending.borrow_mut().remove(&id) else {
                    return;
                };
                let failed =
                    HttpResponse::failed(id, HttpError::Io, "the redirect to https was not taken");
                finish(id, done_of(&failed, &pending, &*self.storage));
            }
        }
    }

    /// A request core starts: new, or one parked for `https` going on as it
    /// was parked (its address, method, body and headers as redirected).
    fn begin(&self, mut call: HttpCall, save_path: Option<&str>, max_file: u64) -> Result<(), i32> {
        let id = call.id;
        if let Some(pending) = self.pending.borrow_mut().get_mut(&id) {
            if !std::mem::take(&mut pending.parked) {
                return Err(ERR_INVALID);
            }
            self.resume(pending);
            return Ok(());
        }
        call.load_body(&*self.storage).map_err(|_| ERR_IO)?;
        let mut pending = Pending {
            save_path: save_path.map(str::to_owned),
            max_file,
            timeout_ms: call.timeout_ms,
            call,
            hops: 0,
            started: Instant::now(),
            parked: false,
        };
        self.resume(&mut pending);
        self.pending.borrow_mut().insert(id, pending);
        Ok(())
    }

    /// Sends the request on as it is now; where its time is spent, it is
    /// answered `Timeout` at the next pump.
    fn resume(&self, pending: &mut Pending) {
        let mut http = self.http.borrow_mut();
        let Some(left) = pending.time_left() else {
            let failed =
                HttpResponse::failed(pending.call.id, HttpError::Timeout, "no answer in time");
            http.answer_now(failed);
            return;
        };
        pending.call.timeout_ms = left;
        http.start_unfollowed(pending.call.clone());
    }
}

impl ShellHttp for NativeNet {
    fn start(&self, job: &HttpJob<'_>) -> Result<(), i32> {
        let call = HttpCall {
            id: job.id,
            method: job.method.to_owned(),
            url: job.url.to_owned(),
            headers: job
                .headers
                .clone()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect(),
            body: job.body.map(<[u8]>::to_vec),
            body_from: job.send_path.map(str::to_owned),
            max_response: usize::try_from(if job.save_path.is_some() {
                job.max_file
            } else {
                u64::from(job.max_body)
            })
            .unwrap_or(usize::MAX),
            timeout_ms: job.timeout_ms,
            // The redirects are followed here, by their `Location`.
            want_headers: vec![String::from("location")],
            save_to: job.save_path.map(str::to_owned),
            progress: false,
        };
        self.begin(call, job.save_path, job.max_file)
    }

    fn abort(&self, id: u32) {
        // Its answer, when it comes, is passed over (`pump`).
        self.pending.borrow_mut().remove(&id);
    }

    fn listen(&self, _server: u32, _port: u32, _address: &mut [u8]) -> Result<usize, ServerReason> {
        Err(ServerReason::Refused)
    }

    fn unlisten(&self, _server: u32) {}

    fn reply(&self, _request: u32, _reply: &ServedReply<'_>) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }

    fn close_served(&self, _request: u32, _status: u32) {}
}

impl ShellNetwork for NativeNet {
    fn now_ms(&self) -> u64 {
        use paperpaw_core::native_host::shell_wiring::WorkDevice;
        self.view.now_ms()
    }

    fn network_up(&self) -> bool {
        use paperpaw_core::native_host::shell_wiring::WorkDevice;
        self.view.network_up()
    }

    fn network_demand(&self, wanted: bool) {
        self.demand.set(wanted);
    }
}

/// What a request's answer comes to.
enum Outcome {
    /// The answer is the request's.
    Done(HttpResponse),
    /// A redirect was followed: the request goes on as it is now.
    Send,
    /// Redirected to `https` and parked (`parked`): core is told, and starts
    /// it again.
    Upgrade,
}

/// Follows the redirect `response` is, if it is one (`network.md` 3.1, by
/// `paperpaw_http::follow`): `pending.call` becomes the request that goes
/// on.
fn followed(pending: &mut Pending, response: HttpResponse) -> Outcome {
    let id = response.id;
    let failed = |error, why: &str| Outcome::Done(HttpResponse::failed(id, error, why));
    let location = response
        .headers
        .iter()
        .find(|(name, _)| name == "location")
        .map(|(_, value)| value.as_str())
        .filter(|location| !location.is_empty());
    let redirect =
        response.error.is_none() && matches!(response.status, 301 | 302 | 303 | 307 | 308);
    let (true, Some(location)) = (redirect, location) else {
        let mut response = response;
        if pending.hops > 0 && response.error.is_none() {
            response.url = Some(pending.call.url.clone());
        }
        return Outcome::Done(response);
    };
    if pending.hops >= MAX_REDIRECTS {
        return failed(HttpError::Refused, "too many redirects");
    }
    let call = &mut pending.call;
    let step = match paperpaw_http::follow(
        &call.url,
        response.status,
        location,
        &call.method,
        call.body.is_some(),
    ) {
        Ok(step) => step,
        Err(refusal) => return failed(HttpError::Refused, refusal.reason()),
    };
    let https = |url: &str| {
        url.get(..8)
            .is_some_and(|p| p.eq_ignore_ascii_case("https://"))
    };
    let upgrade = !https(&call.url) && https(&step.url);
    if step.to_get {
        call.method = String::from("GET");
        call.body = None;
        call.body_from = None;
    }
    step.keep(&mut call.headers);
    call.url = step.url;
    pending.hops += 1;
    if !upgrade {
        return Outcome::Send;
    }
    // The plain connection is closed (curl's ended with the answer); core
    // starts the request again when the session is its.
    pending.parked = true;
    Outcome::Upgrade
}

/// Core's reason for a failed request.
fn reason_of(error: Option<&HttpError>) -> HttpReason {
    match error {
        None => HttpReason::Ok,
        Some(HttpError::NoNetwork) => HttpReason::NoNetwork,
        Some(HttpError::Dns) => HttpReason::Dns,
        Some(HttpError::Tls) => HttpReason::Tls,
        Some(HttpError::Timeout) => HttpReason::Timeout,
        Some(HttpError::Refused) => HttpReason::Refused,
        Some(_) => HttpReason::Io,
    }
}

/// What core is told of an answer. A 2xx answer to a download is appended to
/// its staged file here (`saved`); an error's body is not the file.
fn done_of<'a>(
    response: &'a HttpResponse,
    pending: &Pending,
    storage: &dyn Storage,
) -> HttpDone<'a> {
    let reason = reason_of(response.error.as_ref());
    // An answer longer than the file may be was cut short by the client.
    if pending.save_path.is_some() && crate::http::too_large(response) {
        return HttpDone::new(0, HttpReason::TooLarge);
    }
    if reason != HttpReason::Ok {
        return HttpDone::new(0, reason);
    }
    let mut done = HttpDone::new(u32::from(response.status), HttpReason::Ok);
    done.etag = response.etag.as_deref().unwrap_or("");
    done.content_type = response.content_type.as_deref().unwrap_or("");
    done.final_url = response.url.as_deref().unwrap_or("");
    done.truncated = response.truncated;
    let success = (200..300).contains(&response.status);
    match (&pending.save_path, success) {
        (Some(path), true) => {
            if response.body.len() as u64 > pending.max_file {
                return HttpDone::new(0, HttpReason::TooLarge);
            }
            for piece in response.body.chunks(CHUNK) {
                if storage.append(path, piece).is_err() {
                    return HttpDone::new(0, HttpReason::Io);
                }
            }
            done.saved = true;
        }
        _ => done.body.clone_from(&response.body),
    }
    done
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    fn pending(save_path: Option<&str>, max_file: u64) -> Pending {
        flight("GET", "https://a.example/x", &[], None, save_path, max_file)
    }

    fn flight(
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
        save_path: Option<&str>,
        max_file: u64,
    ) -> Pending {
        Pending {
            save_path: save_path.map(String::from),
            max_file,
            call: HttpCall {
                id: 1,
                method: method.into(),
                url: url.into(),
                headers: headers
                    .iter()
                    .map(|(n, v)| (String::from(*n), String::from(*v)))
                    .collect(),
                body: body.map(<[u8]>::to_vec),
                body_from: None,
                max_response: 1000,
                timeout_ms: 30_000,
                want_headers: vec![String::from("location")],
                save_to: save_path.map(String::from),
                progress: false,
            },
            hops: 0,
            started: Instant::now(),
            timeout_ms: 30_000,
            parked: false,
        }
    }

    fn redirect(status: u16, location: &str) -> HttpResponse {
        HttpResponse {
            id: 1,
            status,
            headers: vec![(String::from("location"), location.into())],
            ..HttpResponse::default()
        }
    }

    fn header_names(pending: &Pending) -> Vec<&str> {
        pending
            .call
            .headers
            .iter()
            .map(|(n, _)| n.as_str())
            .collect()
    }

    fn answer(status: u16, body: &[u8]) -> HttpResponse {
        HttpResponse {
            id: 1,
            status,
            body: body.to_vec(),
            etag: Some(String::from("\"e\"")),
            content_type: Some(String::from("text/plain")),
            ..HttpResponse::default()
        }
    }

    #[test]
    fn a_memory_answer_carries_its_body_and_its_headers() {
        let storage = MemoryStorage::new();
        let pending = pending(None, 0);
        let response = answer(200, b"hello");
        let done = done_of(&response, &pending, &storage);
        assert_eq!((done.status, done.reason), (200, HttpReason::Ok));
        assert_eq!(done.body, b"hello");
        assert_eq!((done.etag, done.content_type), ("\"e\"", "text/plain"));
        assert!(!done.saved);
    }

    #[test]
    fn a_download_is_appended_to_its_staged_file_in_pieces_and_not_in_the_body() {
        let storage = MemoryStorage::new();
        storage.write("a.pp-staged", b"").unwrap();
        let pending = pending(Some("a.pp-staged"), 100_000);
        let big = vec![7u8; CHUNK * 2 + 10];
        let response = answer(200, &big);
        let done = done_of(&response, &pending, &storage);
        assert!(done.saved && done.body.is_empty());
        assert_eq!(storage.size("a.pp-staged").unwrap(), big.len() as u64);
    }

    #[test]
    fn an_error_answer_or_a_too_large_file_does_not_touch_the_staged_file() {
        let storage = MemoryStorage::new();
        storage.write("a.pp-staged", b"").unwrap();
        let pending = pending(Some("a.pp-staged"), 4);
        // 404: the body is the answer's, the file stays empty.
        let response = answer(404, b"nope");
        let done = done_of(&response, &pending, &storage);
        assert!(!done.saved && done.body == b"nope");
        // Over the file limit.
        let response = answer(200, b"too long");
        let done = done_of(&response, &pending, &storage);
        assert_eq!((done.status, done.reason), (0, HttpReason::TooLarge));
        assert_eq!(storage.size("a.pp-staged").unwrap(), 0);
    }

    #[test]
    fn failures_keep_their_reason() {
        let storage = MemoryStorage::new();
        let pending = pending(None, 0);
        for (error, reason) in [
            (HttpError::Dns, HttpReason::Dns),
            (HttpError::Tls, HttpReason::Tls),
            (HttpError::Timeout, HttpReason::Timeout),
            (HttpError::NoNetwork, HttpReason::NoNetwork),
            (HttpError::Refused, HttpReason::Refused),
        ] {
            let response = HttpResponse::failed(1, error, "why");
            let done = done_of(&response, &pending, &storage);
            assert_eq!((done.status, done.reason), (0, reason));
        }
    }
    const KEYED: [(&str, &str); 5] = [
        ("Accept", "text/html"),
        ("X-Api-Key", "k"),
        ("Authorization", "Bearer t"),
        ("Cookie", "s=1"),
        ("Content-Type", "text/plain"),
    ];

    #[test]
    fn a_downgrade_to_plain_http_is_refused() {
        let mut p = flight("GET", "https://a.example/x", &KEYED, None, None, 0);
        let Outcome::Done(done) = followed(&mut p, redirect(302, "http://a.example/y")) else {
            panic!("not refused");
        };
        assert_eq!(done.error, Some(HttpError::Refused));
        assert_eq!(p.call.url, "https://a.example/x", "nothing was sent on");
    }

    #[test]
    fn another_origin_gets_only_the_harmless_headers_and_the_same_one_all() {
        let mut p = flight("GET", "https://a.example/x", &KEYED, None, None, 0);
        assert!(matches!(
            followed(&mut p, redirect(302, "https://b.example/y")),
            Outcome::Send
        ));
        assert_eq!(header_names(&p), ["Accept", "Content-Type"]);
        assert_eq!(p.call.url, "https://b.example/y");
        let mut p = flight("GET", "https://a.example/x", &KEYED, None, None, 0);
        assert!(matches!(
            followed(&mut p, redirect(302, "/z")),
            Outcome::Send
        ));
        assert_eq!(header_names(&p).len(), 5);
        assert_eq!(p.call.url, "https://a.example/z");
    }

    #[test]
    fn a_body_does_not_go_to_another_origin_and_a_303_makes_a_get() {
        let mut p = flight(
            "POST",
            "https://a.example/x",
            &KEYED,
            Some(b"secret"),
            None,
            0,
        );
        let Outcome::Done(done) = followed(&mut p, redirect(307, "https://b.example/y")) else {
            panic!("not refused");
        };
        assert_eq!(done.error, Some(HttpError::Refused));
        // The same origin keeps method and body on a 307.
        let mut p = flight(
            "POST",
            "https://a.example/x",
            &KEYED,
            Some(b"secret"),
            None,
            0,
        );
        assert!(matches!(
            followed(&mut p, redirect(307, "/y")),
            Outcome::Send
        ));
        assert_eq!(
            (p.call.method.as_str(), p.call.body.as_deref()),
            ("POST", Some(&b"secret"[..]))
        );
        // A 303 goes on as a GET without a body, even to another origin.
        let mut p = flight(
            "POST",
            "https://a.example/x",
            &KEYED,
            Some(b"secret"),
            None,
            0,
        );
        assert!(matches!(
            followed(&mut p, redirect(303, "https://b.example/y")),
            Outcome::Send
        ));
        assert_eq!(
            (p.call.method.as_str(), p.call.body.as_deref()),
            ("GET", None)
        );
        assert_eq!(header_names(&p), ["Accept", "Content-Type"]);
    }

    #[test]
    fn eleven_redirects_end_refused_and_ten_are_followed() {
        let mut p = flight("GET", "https://a.example/0", &[], None, None, 0);
        for n in 1..=10 {
            let to = format!("/{n}");
            assert!(matches!(
                followed(&mut p, redirect(302, &to)),
                Outcome::Send
            ));
        }
        let Outcome::Done(done) = followed(&mut p, redirect(302, "/11")) else {
            panic!("followed an eleventh");
        };
        assert_eq!(done.error, Some(HttpError::Refused));
        assert_eq!(p.call.url, "https://a.example/10");
    }

    #[test]
    fn a_redirect_to_https_parks_the_request_with_all_its_headers() {
        let mut p = flight("GET", "http://a.example/x", &KEYED, None, None, 0);
        let outcome = followed(&mut p, redirect(301, "https://a.example/x"));
        assert!(matches!(outcome, Outcome::Upgrade));
        assert_eq!(
            (p.parked, p.call.url.as_str()),
            (true, "https://a.example/x")
        );
        // Same host, the schemes' own ports: the same server, all headers.
        assert_eq!(header_names(&p).len(), 5);
    }

    /// A route that answers an `http` address with a redirect to its
    /// `https` one and that one with `ok`, and keeps what it was asked.
    struct Upgrading(Rc<RefCell<Vec<HttpCall>>>);

    impl crate::http::Route for Upgrading {
        fn start(&mut self, call: HttpCall) -> Option<HttpResponse> {
            let id = call.id;
            let plain = call.url.starts_with("http://");
            self.0.borrow_mut().push(call);
            Some(if plain {
                HttpResponse {
                    headers: vec![(
                        String::from("location"),
                        String::from("https://a.example/x"),
                    )],
                    ..HttpResponse {
                        id,
                        status: 307,
                        ..HttpResponse::default()
                    }
                }
            } else {
                answer_to(id, 200, b"ok")
            })
        }

        fn later(&mut self) -> Option<HttpResponse> {
            None
        }
    }

    fn answer_to(id: u32, status: u16, body: &[u8]) -> HttpResponse {
        HttpResponse {
            id,
            status,
            body: body.to_vec(),
            ..HttpResponse::default()
        }
    }

    /// Core answers a redirect report by starting the request again within
    /// it: the plain request is not sent a second time, the `https` one goes
    /// out once, and its answer is the one id's.
    #[test]
    fn a_redirect_to_https_that_core_restarts_at_once_is_one_request() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        crate::http::set_route(Some(Box::new(Upgrading(seen.clone()))));
        let net = NativeNet::new(Rc::new(MemoryStorage::new()), Rc::default());
        let first = flight(
            "POST",
            "http://a.example/x",
            &KEYED,
            Some(b"secret"),
            None,
            0,
        )
        .call;
        net.begin(first, None, 0).unwrap();
        let ended = RefCell::new(Vec::new());
        let told = Cell::new(0);
        net.pump_with(
            None,
            &|id, done| ended.borrow_mut().push((id, done.status, done.reason)),
            &|id| {
                told.set(told.get() + 1);
                // Core starts the parked request again from inside the report.
                let again = flight("POST", "http://a.example/x", &[], None, None, 0).call;
                assert_eq!(id, again.id);
                net.begin(again, None, 0)
            },
        );
        crate::http::set_route(None);
        let seen = seen.borrow();
        let urls: Vec<&str> = seen.iter().map(|call| call.url.as_str()).collect();
        assert_eq!(urls, ["http://a.example/x", "https://a.example/x"]);
        // The redirect kept method, body and headers (the same server).
        assert_eq!(seen[1].method, "POST");
        assert_eq!(seen[1].body.as_deref(), Some(&b"secret"[..]));
        assert_eq!(seen[1].headers.len(), 5);
        assert_eq!(told.get(), 1);
        assert_eq!(*ended.borrow(), [(1, 200, HttpReason::Ok)]);
        assert_eq!(net.in_flight(), 0);
    }

    /// The pump says whether core was told anything: the device then ticks
    /// the kernel, whose pass tells the plugin (#2795).
    #[test]
    fn the_pump_says_whether_core_was_told_anything() {
        crate::http::set_route(Some(Box::new(Upgrading(Rc::default()))));
        let net = NativeNet::new(Rc::new(MemoryStorage::new()), Rc::default());
        let nothing = |_, _: HttpDone<'_>| {};
        assert!(
            !net.pump_with(None, &nothing, &|_| Ok(())),
            "nothing in flight"
        );
        let call = flight("GET", "https://a.example/x", &[], None, None, 0).call;
        net.begin(call, None, 0).unwrap();
        assert!(net.pump_with(None, &nothing, &|_| Ok(())), "an answer");
        assert!(!net.pump_with(None, &nothing, &|_| Ok(())), "told once");
        crate::http::set_route(None);
    }

    #[test]
    fn a_busy_report_is_tried_again_and_a_refused_one_ends_the_request() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        crate::http::set_route(Some(Box::new(Upgrading(seen.clone()))));
        let net = NativeNet::new(Rc::new(MemoryStorage::new()), Rc::default());
        let first = flight("GET", "http://a.example/x", &[], None, None, 0).call;
        net.begin(first, None, 0).unwrap();
        let ended = RefCell::new(Vec::new());
        let finish =
            |id, done: HttpDone<'_>| ended.borrow_mut().push((id, done.status, done.reason));
        // Busy: parked, tried again at the next pump, which starts it and
        // takes its answer.
        net.pump_with(None, &finish, &|_| Err(ERR_BUSY));
        assert!(ended.borrow().is_empty() && net.in_flight() == 1);
        net.pump_with(None, &finish, &|id| {
            let again = flight("GET", "http://a.example/x", &[], None, None, 0).call;
            assert_eq!(id, again.id);
            net.begin(again, None, 0)
        });
        assert_eq!(*ended.borrow(), [(1, 200, HttpReason::Ok)]);
        // Refused: the request ends `Io` and leaves nothing parked.
        ended.borrow_mut().clear();
        let first = flight("GET", "http://a.example/x", &[], None, None, 0).call;
        net.begin(first, None, 0).unwrap();
        net.pump_with(None, &finish, &|_| Err(ERR_IO));
        assert_eq!(*ended.borrow(), [(1, 0, HttpReason::Io)]);
        assert_eq!(net.in_flight(), 0);
        // Cancelled meanwhile: nothing to tell, nothing parked.
        ended.borrow_mut().clear();
        let first = flight("GET", "http://a.example/x", &[], None, None, 0).call;
        net.begin(first, None, 0).unwrap();
        net.pump_with(None, &finish, &|_| Err(ERR_NOT_FOUND));
        assert!(ended.borrow().is_empty());
        assert_eq!(net.in_flight(), 0);
        crate::http::set_route(None);
    }

    #[test]
    fn the_final_address_and_an_ordinary_answer_are_kept() {
        let mut p = flight("GET", "https://a.example/0", &[], None, None, 0);
        assert!(matches!(
            followed(&mut p, redirect(302, "/1")),
            Outcome::Send
        ));
        let Outcome::Done(done) = followed(&mut p, answer(200, b"ok")) else {
            panic!("not done");
        };
        assert_eq!(done.url.as_deref(), Some("https://a.example/1"));
        // A 302 with no Location is an answer like any other.
        let mut p = flight("GET", "https://a.example/0", &[], None, None, 0);
        let Outcome::Done(done) = followed(&mut p, answer(302, b"")) else {
            panic!("not done");
        };
        assert_eq!((done.status, done.url), (302, None));
    }

    #[test]
    fn an_oversized_download_is_too_large_and_not_refused() {
        let storage = MemoryStorage::new();
        let p = pending(Some("a.pp-staged"), 4);
        let cut = HttpResponse::failed(1, HttpError::Refused, "the answer is larger than 4 bytes");
        let done = done_of(&cut, &p, &storage);
        assert_eq!((done.status, done.reason), (0, HttpReason::TooLarge));
        // Another refusal stays a refusal.
        let other = HttpResponse::failed(1, HttpError::Refused, "too many redirects");
        assert_eq!(done_of(&other, &p, &storage).reason, HttpReason::Refused);
    }

    #[test]
    fn a_request_in_memory_runs_out_of_time_over_its_redirects() {
        let mut p = pending(None, 0);
        p.timeout_ms = 1;
        p.started = Instant::now() - std::time::Duration::from_millis(50);
        assert_eq!(p.time_left(), None);
        let mut p = pending(Some("a"), 0);
        p.timeout_ms = 1;
        p.started = Instant::now() - std::time::Duration::from_millis(50);
        assert_eq!(
            p.time_left(),
            Some(1),
            "a file's time is by its bytes moving"
        );
    }
}

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
//! the desktop yet: a plugin that asks to serve is refused (`Refused`), and
//! `uses.network` for sockets is not offered with it.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use paperpaw_core::native_host::shell_wiring::{
    HttpDone, HttpJob, HttpReason, NetReports, ServedReply, ServerReason, ShellHttp, ShellNetwork,
};
use paperpaw_core::sdk::HttpError;
use paperpaw_core::{HttpCall, HttpResponse};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_IO, ERR_UNSUPPORTED};
use paperpaw_platform::Storage;

use crate::http::Http;
use crate::linked::DeviceView;

/// The bytes of one append of a download to its file.
const CHUNK: usize = 4096;

/// What a request needs when its answer comes.
struct Pending {
    save_path: Option<String>,
    max_file: u64,
}

/// The shell side of `core.net` (see the module).
pub struct NativeNet {
    http: RefCell<Http>,
    storage: Rc<dyn Storage>,
    view: Rc<DeviceView>,
    pending: RefCell<HashMap<u32, Pending>>,
    demand: Cell<bool>,
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
    /// first), then lets it take its turn (`tick`).
    pub fn pump(&self, reports: &NetReports, wait: Option<std::time::Duration>) {
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
            let Some(pending) = self.pending.borrow_mut().remove(&response.id) else {
                continue;
            };
            // A request core no longer knows is answered `ERR_NOT_FOUND`: no one to tell.
            let _ = reports.finished(response.id, done_of(&response, &pending, &*self.storage));
        }
        let _ = reports.tick();
    }
}

impl ShellHttp for NativeNet {
    fn start(&self, job: &HttpJob<'_>) -> Result<(), i32> {
        let mut call = HttpCall {
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
            want_headers: Vec::new(),
            save_to: job.save_path.map(str::to_owned),
            progress: false,
        };
        call.load_body(&*self.storage).map_err(|_| ERR_IO)?;
        {
            let mut pending = self.pending.borrow_mut();
            if pending.contains_key(&job.id) {
                return Err(ERR_INVALID);
            }
            pending.insert(
                job.id,
                Pending {
                    save_path: job.save_path.map(str::to_owned),
                    max_file: job.max_file,
                },
            );
        }
        self.http.borrow_mut().start(call);
        Ok(())
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
        let pending = Pending {
            save_path: None,
            max_file: 0,
        };
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
        let pending = Pending {
            save_path: Some(String::from("a.pp-staged")),
            max_file: 100_000,
        };
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
        let pending = Pending {
            save_path: Some(String::from("a.pp-staged")),
            max_file: 4,
        };
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
        let pending = Pending {
            save_path: None,
            max_file: 0,
        };
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
}

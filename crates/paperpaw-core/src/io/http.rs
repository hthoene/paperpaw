//! HTTP requests that plugins ask for and shells make ([`Io::Http`],
//! answered with `Kernel::http_response`).
//!
//! [`Io::Http`]: super::Io::Http

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::Ask;
use paperpaw_platform::{Instant, Storage, StorageError};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::http_params;
use paperpaw_sdk::{HttpError, HttpOptions, HttpProgress, HttpReply, HttpRequest, Id, Params};

/// Longest response body passed on to plugins.
pub const MAX_RESPONSE: usize = 1 << 20;
/// Longest response body passed on to plugins on small devices, unless a
/// request asks for more ([`HttpOptions::max_body`]).
pub const MAX_RESPONSE_SMALL: usize = 64 << 10;
/// How long a request may take in all unless it says
/// ([`HttpOptions::timeout`]).
pub const TIMEOUT_MS: u32 = 30_000;
/// How long a download to a file ([`HttpRequest::save_to`]) or an
/// upload from one may go without a byte, unless it says: nothing limits
/// it in all, as a large file on a slow network takes long, and slow is
/// fine where breaking off is not.
pub const DOWNLOAD_TIMEOUT_MS: u32 = 60_000;
/// Longest response body saved to a file, and longest file sent.
pub const MAX_FILE: usize = 64 << 20;
/// A request's progress is passed on once it came this much further, in
/// thousandths of the whole ([`HttpRequest::progress`]), ...
pub const PROGRESS_STEP: u16 = 50;
/// ... or this long after it was last, if more came: a screen redraws
/// its bar no more often (on e-ink each redraw costs).
pub const PROGRESS_MS: u64 = 2000;

/// A request for the shell to make, e.g. on a background thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpCall {
    /// Passed back with the response.
    pub id: u32,
    /// `GET`, `PUT`, `POST`, …
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// A file in storage to send as the body instead of `body` (e.g. a
    /// database to upload): the shell reads it, in parts where it can, so
    /// it never needs to fit in memory.
    pub body_from: Option<String>,
    /// The longest response body wanted: shells cut longer ones at
    /// exactly this many bytes, and fail one for a file
    /// ([`save_to`](Self::save_to)) instead, so that no file is saved cut
    /// short as if whole.
    pub max_response: usize,
    /// How long it may take in all, the connection and TLS counted; the
    /// shell answers with status 0 after that. For a download to a file
    /// ([`save_to`](Self::save_to)) or an upload from one
    /// ([`body_from`](Self::body_from)): how long it may go without a
    /// byte, however long it takes in all.
    pub timeout_ms: u32,
    /// Headers of the answer to pass back ([`HttpResponse::headers`]),
    /// by name in any case.
    pub want_headers: Vec<String>,
    /// The file in storage the body is for, if it is one (e.g. a
    /// download). A shell may write it there itself as it comes, for one
    /// too large for memory, and say so with [`HttpResponse::saved`];
    /// otherwise the kernel writes the body it gets. Only a successful
    /// answer's (2xx) body is the file: an error's is its reason, and the
    /// file stays as it was. A later call saving to the same file (in
    /// any case) replaces this one: a shell that writes the file itself
    /// drops this one's transfer then; its answer is passed over.
    pub save_to: Option<String>,
    /// The plugin wants to hear how much of the body came: the shell says
    /// so as it comes ([`Kernel::http_progress`](crate::Kernel::http_progress)),
    /// as often as it likes (the kernel thins it out).
    pub progress: bool,
}

impl HttpCall {
    /// Reads [`body_from`](Self::body_from) into `body`, for shells with
    /// memory to spare (others send the file in parts). `body_from` stays,
    /// saying the body is a file's (limited as [`timeout_ms`](Self::timeout_ms)
    /// says).
    pub fn load_body(&mut self, storage: &dyn Storage) -> Result<(), StorageError> {
        if let Some(path) = self.body_from.clone() {
            let size = storage.size(&path)?;
            let mut body = alloc::vec![0; usize::try_from(size).unwrap_or(usize::MAX)];
            let mut at = 0;
            while at < body.len() {
                let n = storage.read_at(&path, at as u64, &mut body[at..])?;
                if n == 0 {
                    break;
                }
                at += n;
            }
            body.truncate(at);
            self.body = Some(body);
        }
        Ok(())
    }
}

/// What came back. `status` 0 means the request failed (no network, no
/// such host, timeout): `error` says why, where the shell knows, and
/// `body` holds a short reason.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HttpResponse {
    pub id: u32,
    pub status: u16,
    pub body: Vec<u8>,
    /// The `ETag` header, if any.
    pub etag: Option<String>,
    /// The `Content-Type` header, if any.
    pub content_type: Option<String>,
    /// Where the answer came from after redirects, if elsewhere.
    pub url: Option<String>,
    /// The body was written to [`HttpCall::save_to`] by the shell (and
    /// is not in `body`).
    pub saved: bool,
    /// The headers asked for ([`HttpCall::want_headers`]) that the answer
    /// had, their names in lower case.
    pub headers: Vec<(String, String)>,
    /// The body was cut to [`HttpCall::max_response`].
    pub truncated: bool,
    /// Why it failed (status 0).
    pub error: Option<HttpError>,
}

impl HttpResponse {
    /// The answer of a request that failed (status 0): `error`, and
    /// `reason` to say more.
    pub fn failed(id: u32, error: HttpError, reason: &str) -> Self {
        Self {
            id,
            status: 0,
            body: reason.as_bytes().to_vec(),
            error: Some(error),
            ..Self::default()
        }
    }
}

/// A request made and not answered yet.
#[derive(Debug)]
struct Awaiting {
    id: u32,
    reply: Id,
    context: Params,
    save_to: Option<String>,
    /// The largest file it may save ([`HttpOptions::max_file`]): checked
    /// again here, as a shell may not cut.
    max_file: usize,
    /// Made while the kernel handled the reader's input.
    reader: bool,
    /// The event its progress comes as, and how far it was when last.
    progress: Option<Progress>,
}

/// A request's progress, as last passed on: when, how many bytes, and in
/// thousandths of the whole where known.
#[derive(Debug)]
struct Progress {
    event: Id,
    shown: Option<(Instant, u64, Option<u16>)>,
}

/// `params` without the event a request's progress comes as, and that
/// event.
fn take_progress(params: Params) -> (Params, Option<Id>) {
    let Some(event) = params.get(http_params::PROGRESS) else {
        return (params, None);
    };
    let event = Id::parse(event).ok();
    let mut kept = Params::new();
    for (key, value) in params.iter().filter(|(k, _)| *k != http_params::PROGRESS) {
        kept.set(String::from(key), value);
    }
    (kept, event)
}

/// Plugins' requests: those for the shell, and those it has not answered.
#[derive(Debug)]
pub(crate) struct Http {
    calls: Vec<HttpCall>,
    awaiting: Vec<Awaiting>,
    next: u32,
    /// The longest body kept in memory unless a request says.
    max_response: usize,
}

impl Default for Http {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            awaiting: Vec::new(),
            next: 0,
            max_response: MAX_RESPONSE,
        }
    }
}

impl Http {
    /// Keeps bodies to what the device's memory allows by default.
    pub fn for_memory(memory: paperpaw_platform::MemoryClass) -> Self {
        let max_response = match memory {
            paperpaw_platform::MemoryClass::Large => MAX_RESPONSE,
            // Small, and any class to come: the frugal side.
            _ => MAX_RESPONSE_SMALL,
        };
        Self {
            max_response,
            ..Self::default()
        }
    }

    /// Queues a plugin's request for the shell, or fails it at once
    /// (without a network, or a file to send that is not there). One
    /// that saves to a file another request still saves to replaces that
    /// one, which fails at once.
    pub fn request(
        &mut self,
        request: HttpRequest,
        network: bool,
        reader: bool,
        storage: &dyn Storage,
        out: &mut Vec<Ask>,
    ) {
        let mut request = request.into_parts();
        let (params, progress) = take_progress(request.params);
        request.params = params;
        if !network {
            return reply(
                request.reply,
                HttpReply::failed(HttpError::NoNetwork, "no network", request.params),
                out,
            );
        }
        // A file to send must be there; the shell reads it.
        if let Some(path) = &request.body_from
            && let Err(e) = storage.size(path)
        {
            let failed = HttpReply::failed(HttpError::Io, format!("{e}"), request.params);
            return reply(request.reply, failed, out);
        }
        // Two answers in one file (and in one partial file of the shell)
        // would mix: the newer request replaces the older one, whose
        // answer is passed over when it comes (shells drop its transfer).
        if let Some(path) = &request.save_to
            && let Some(at) = self.awaiting.iter().position(|a| {
                a.save_to
                    .as_deref()
                    .is_some_and(|other| other.eq_ignore_ascii_case(path))
            })
        {
            let older = self.awaiting.remove(at);
            self.calls.retain(|call| call.id != older.id);
            let reason = "replaced by a newer request";
            let failed = HttpReply::failed(HttpError::Replaced, reason, older.context);
            reply(older.reply, failed, out);
        }
        let HttpOptions {
            timeout,
            want_headers,
            max_body,
            max_file,
            ..
        } = request.options;
        let body = match request.body_from {
            Some(_) => None,
            None => request.body.map(String::into_bytes),
        };
        self.next = self.next.wrapping_add(1);
        let id = self.next;
        let max_response = if request.save_to.is_some() {
            max_file.map_or(MAX_FILE, |cap| {
                usize::try_from(cap).unwrap_or(usize::MAX).min(MAX_FILE)
            })
        } else {
            max_body.unwrap_or(self.max_response).min(MAX_RESPONSE)
        };
        let default = if request.save_to.is_some() || request.body_from.is_some() {
            DOWNLOAD_TIMEOUT_MS
        } else {
            TIMEOUT_MS
        };
        let timeout_ms = timeout.map_or(default, |t| {
            u32::try_from(t.as_millis()).unwrap_or(u32::MAX)
        });
        self.calls.push(HttpCall {
            id,
            method: request.method,
            url: request.url,
            headers: request.headers,
            body,
            body_from: request.body_from,
            max_response,
            timeout_ms,
            want_headers,
            save_to: request.save_to.clone(),
            progress: progress.is_some(),
        });
        self.awaiting.push(Awaiting {
            id,
            reply: request.reply,
            context: request.params,
            save_to: request.save_to,
            max_file: max_response,
            reader,
            progress: progress.map(|event| Progress { event, shown: None }),
        });
    }

    /// How much of the body of request `id` came (`received` of `total`,
    /// where known), at `now`: the plugin's progress event, where it asked
    /// for one, the first time, then once it came [`PROGRESS_STEP`] further
    /// or [`PROGRESS_MS`] passed with more come. Nothing for a request
    /// answered or taken back.
    pub fn progress(
        &mut self,
        id: u32,
        received: u64,
        total: Option<u64>,
        now: Instant,
        out: &mut Vec<Ask>,
    ) {
        let Some(awaiting) = self.awaiting.iter_mut().find(|a| a.id == id) else {
            return;
        };
        let Some(progress) = awaiting.progress.as_mut() else {
            return;
        };
        let shown = HttpProgress::new(received, total, awaiting.context.clone());
        let permille = shown.permille();
        let due = match progress.shown {
            None => true,
            Some((at, before, was)) => {
                let further = match (permille, was) {
                    (Some(permille), Some(was)) => permille >= was.saturating_add(PROGRESS_STEP),
                    _ => false,
                };
                received > before && (further || now.since(at) >= PROGRESS_MS)
            }
        };
        if due {
            progress.shown = Some((now, received, permille));
            out.push(Ask::Emit(progress.event.clone(), shown.into_params()));
        }
    }

    /// Turns a request's answer into the plugin's event; a successful
    /// body for a file is written there. The body moves on without a copy
    /// (it may be most of the memory left), to the plugin's handler too
    /// (see [`EventData::into_params`]).
    pub fn answer(&mut self, response: HttpResponse, storage: &dyn Storage, out: &mut Vec<Ask>) {
        let Some(at) = self.awaiting.iter().position(|a| a.id == response.id) else {
            return;
        };
        let awaiting = self.awaiting.remove(at);
        let mut status = response.status;
        let mut error = response.error;
        let mut truncated = response.truncated;
        let body = match awaiting.save_to {
            // Answers that are files: written where asked.
            Some(path) if (200..300).contains(&status) => {
                let saved_size = response
                    .saved
                    .then(|| storage.size(&path).ok())
                    .flatten()
                    .map_or(0, |size| usize::try_from(size).unwrap_or(usize::MAX));
                if response.body.len().max(saved_size) > awaiting.max_file {
                    // Larger than the plugin allowed: not kept (a shell
                    // that wrote it itself had it replace the old file).
                    if response.saved {
                        let _ = storage.remove(&path);
                    }
                    status = 0;
                    error = Some(HttpError::Refused);
                    format!("larger than {} bytes", awaiting.max_file).into_bytes()
                } else if response.saved {
                    Vec::new()
                } else if let Err(e) = storage.write(&path, &response.body) {
                    status = 0;
                    error = Some(HttpError::Io);
                    format!("{e}").into_bytes()
                } else {
                    Vec::new()
                }
            }
            // A failure's reason, or an error's body (not the file).
            Some(_) => {
                let mut body = response.body;
                if body.len() > MAX_RESPONSE {
                    body.truncate(MAX_RESPONSE);
                    truncated = true;
                }
                body
            }
            None => response.body,
        };
        let mut answer = HttpReply::new(status, body);
        answer.truncated = truncated;
        answer.error = error;
        answer.etag = response.etag;
        answer.content_type = response.content_type;
        answer.final_url = response.url;
        answer.context = awaiting.context;
        answer.headers = response.headers;
        out.push(Ask::Emit(awaiting.reply, answer.into_params()));
    }

    /// Takes back the requests whose reply is `reply`, each answered with
    /// the refusal `Cancelled`; an answer the shell brings later is passed
    /// over.
    pub fn cancel(&mut self, reply: &Id, out: &mut Vec<Ask>) {
        let cancelled: Vec<Awaiting> = {
            let (gone, kept) = core::mem::take(&mut self.awaiting)
                .into_iter()
                .partition(|a| a.reply == *reply);
            self.awaiting = kept;
            gone
        };
        for awaiting in cancelled {
            self.calls.retain(|call| call.id != awaiting.id);
            let refusal = paperpaw_sdk::work::Refusal::new(
                paperpaw_sdk::work::Reason::Cancelled,
                paperpaw_sdk::work::Retry::Never,
            );
            let answer = HttpReply::refused(refusal, awaiting.context);
            out.push(Ask::Emit(awaiting.reply, answer.into_params()));
        }
    }

    /// Whether a request made for the reader is not answered yet.
    pub fn reader_waits(&self) -> bool {
        self.awaiting.iter().any(|a| a.reader)
    }

    /// Whether request `id` was made for the reader.
    pub fn of_reader(&self, id: u32) -> bool {
        self.awaiting.iter().any(|a| a.id == id && a.reader)
    }

    /// The requests for the shell.
    pub fn take(&mut self) -> Vec<HttpCall> {
        core::mem::take(&mut self.calls)
    }

    /// Whether requests wait for the shell (for a network).
    pub fn waiting(&self) -> bool {
        !self.calls.is_empty()
    }

    /// Whether requests are not answered yet.
    pub fn busy(&self) -> bool {
        !self.awaiting.is_empty()
    }

    /// Fails the requests still waiting for the shell, for `reason`.
    pub fn fail_waiting(&mut self, reason: &str, storage: &dyn Storage, out: &mut Vec<Ask>) {
        for call in self.take() {
            let failed = HttpResponse::failed(call.id, HttpError::NoNetwork, reason);
            self.answer(failed, storage, out);
        }
    }
}

fn reply(event: Id, answer: HttpReply, out: &mut Vec<Ask>) {
    out.push(Ask::Emit(event, answer.into_params()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::{MemoryStorage, read_file};

    const REPLY: Id = Id::new_static("test.reply");

    fn get(url: &str, save_to: Option<&str>) -> HttpRequest {
        let request = HttpRequest::new("GET", url, REPLY);
        match save_to {
            Some(path) => request.save_to(path),
            None => request,
        }
    }

    /// The reply, as its hearer gets it.
    fn reply_of(out: &[Ask]) -> HttpReply {
        let Some(Ask::Emit(_, params)) = out.last() else {
            panic!("no reply");
        };
        params.carried::<HttpReply>().cloned().unwrap()
    }

    /// The reply's status and body.
    fn replied(out: &[Ask]) -> (u16, String) {
        let reply = reply_of(out);
        (reply.status, reply.text().into_owned())
    }

    fn answer(status: u16, body: &[u8]) -> HttpResponse {
        HttpResponse {
            id: 1,
            status,
            body: body.to_vec(),
            ..HttpResponse::default()
        }
    }

    #[test]
    fn options_reach_the_shell_and_headers_come_back() {
        let storage = MemoryStorage::new();
        let mut out = Vec::new();
        let mut http = Http::for_memory(paperpaw_platform::MemoryClass::Small);
        let plain = get("https://h.org/a", None);
        let download = get("https://h.org/b", Some("b.epub"));
        let asked = get("https://h.org/c", None)
            .timeout(core::time::Duration::from_secs(5))
            .want_headers(&["Last-Modified"])
            .max_body(2048);
        for request in [plain, download, asked] {
            http.request(request, true, false, &storage, &mut out);
        }
        let calls = http.take();
        let limits: Vec<_> = calls
            .iter()
            .map(|c| (c.timeout_ms, c.max_response))
            .collect();
        assert_eq!(
            limits,
            [
                (TIMEOUT_MS, MAX_RESPONSE_SMALL),
                (DOWNLOAD_TIMEOUT_MS, MAX_FILE),
                (5000, 2048)
            ]
        );
        assert_eq!(calls[2].want_headers, ["Last-Modified"]);
        let response = HttpResponse {
            id: calls[2].id,
            status: 200,
            headers: alloc::vec![(String::from("last-modified"), String::from("Tue"))],
            ..HttpResponse::default()
        };
        http.answer(response, &storage, &mut out);
        assert_eq!(reply_of(&out).header("Last-Modified"), Some("Tue"));
    }

    /// A download's own cap reaches the shell (never above `MAX_FILE`),
    /// and an answer past it is refused by the kernel too: nothing written
    /// where the kernel writes, and a file the shell wrote itself removed.
    #[test]
    fn a_download_is_capped_where_the_plugin_says() {
        let storage = MemoryStorage::new().with("old.epub", "old");
        let mut out = Vec::new();
        let mut http = Http::default();
        let capped = get("https://h.org/a", Some("a.epub")).max_file(4);
        let huge = get("https://h.org/b", Some("b.epub")).max_file(u64::MAX);
        http.request(capped, true, false, &storage, &mut out);
        http.request(huge, true, false, &storage, &mut out);
        let calls = http.take();
        assert_eq!(calls[0].max_response, 4);
        assert_eq!(calls[1].max_response, MAX_FILE);
        // Written by the kernel: too long, refused, nothing saved.
        let mut response = answer(200, b"0123456789");
        response.id = calls[0].id;
        http.answer(response, &storage, &mut out);
        let reply = reply_of(&out);
        assert_eq!(reply.error, Some(HttpError::Refused));
        assert!(storage.size("a.epub").is_err());
        // Exactly the cap is fine.
        let again = get("https://h.org/a", Some("a.epub")).max_file(4);
        http.request(again, true, false, &storage, &mut out);
        let id = http.take()[0].id;
        let mut response = answer(200, b"0123");
        response.id = id;
        http.answer(response, &storage, &mut out);
        assert_eq!(reply_of(&out).error, None);
        assert_eq!(read_file(&storage, "a.epub", 64).unwrap(), b"0123");
        // Written by a shell that did not cut: removed, refused.
        let shell = get("https://h.org/c", Some("old.epub")).max_file(2);
        http.request(shell, true, false, &storage, &mut out);
        let id = http.take()[0].id;
        let response = HttpResponse {
            id,
            status: 200,
            saved: true,
            ..HttpResponse::default()
        };
        http.answer(response, &storage, &mut out);
        assert_eq!(reply_of(&out).error, Some(HttpError::Refused));
        assert!(storage.size("old.epub").is_err());
    }

    const PROGRESS: paperpaw_sdk::event::Event<HttpProgress> =
        paperpaw_sdk::event::Event::new("test.progress");

    /// The progress passed on: bytes received and thousandths, each time.
    fn progress_of(out: &[Ask]) -> Vec<(u64, Option<u16>)> {
        out.iter()
            .filter_map(|c| match c {
                Ask::Emit(event, params) if *event == *PROGRESS.id() => {
                    HttpProgress::from_params(params).map(|p| (p.received, p.permille()))
                }
                _ => None,
            })
            .collect()
    }

    /// A download's progress, as the shell says it: passed on the first
    /// time, then 5 % further on or 2 seconds later with more come, never
    /// after the answer; without a size, every 2 seconds. The request's
    /// own parameters come with it and with the reply, the event not.
    #[test]
    fn progress_is_passed_on_thinned_and_ends_with_the_answer() {
        let storage = MemoryStorage::new();
        let mut out = Vec::new();
        let mut http = Http::default();
        let request = get("https://h.org/book.epub", Some("book.epub"))
            .progress(&PROGRESS)
            .with(Params::new().with("book", "b"));
        http.request(request, true, false, &storage, &mut out);
        let plain = get("https://h.org/feed", None);
        http.request(plain, true, false, &storage, &mut out);
        let calls = http.take();
        assert!(calls[0].progress && !calls[1].progress);
        let (book, feed) = (calls[0].id, calls[1].id);
        let said = [
            (0, 0),
            (100, 10),
            (200, 49),
            (300, 50),
            (400, 60),
            (2400, 61),
            (5000, 61),
        ];
        for (at, received) in said {
            http.progress(book, received, Some(1000), Instant(at), &mut out);
        }
        // Asked for nothing: nothing heard.
        http.progress(feed, 10, Some(100), Instant(0), &mut out);
        assert_eq!(
            progress_of(&out),
            [(0, Some(0)), (50, Some(50)), (61, Some(61))]
        );
        let Some(Ask::Emit(_, params)) = out.last() else {
            panic!("no progress");
        };
        let heard = HttpProgress::from_params(params).unwrap();
        assert_eq!(heard.context.get("book"), Some("b"));
        assert_eq!(heard.context.get(http_params::PROGRESS), None);
        out.clear();
        let mut done = answer(200, b"PK");
        done.id = book;
        http.answer(done, &storage, &mut out);
        assert_eq!(reply_of(&out).context.get(http_params::PROGRESS), None);
        http.progress(book, 1000, Some(1000), Instant(9000), &mut out);
        assert!(progress_of(&out).is_empty());
        // Without a size: once, then every 2 seconds.
        let unsized_ = get("https://h.org/other.epub", Some("other.epub")).progress(&PROGRESS);
        http.request(unsized_, true, false, &storage, &mut out);
        let other = http.take()[0].id;
        for (at, received) in [(0, 100), (1000, 200), (2100, 300)] {
            http.progress(other, received, None, Instant(at), &mut out);
        }
        assert_eq!(progress_of(&out), [(100, None), (300, None)]);
    }

    #[test]
    fn errors_never_overwrite_the_file() {
        let storage = MemoryStorage::new().with("book.epub", "old");
        let mut out = Vec::new();
        for (status, body) in [(404, &b"not here"[..]), (304, b""), (500, b"oops")] {
            let mut http = Http::default();
            http.request(
                get("https://h.org/b", Some("book.epub")),
                true,
                false,
                &storage,
                &mut out,
            );
            http.take();
            http.answer(answer(status, body), &storage, &mut out);
            assert_eq!(
                replied(&out),
                (status, String::from_utf8_lossy(body).into())
            );
            assert_eq!(
                read_file(&storage, "book.epub", 64).unwrap(),
                b"old",
                "{status}"
            );
        }
        let mut http = Http::default();
        http.request(
            get("https://h.org/b", Some("book.epub")),
            true,
            false,
            &storage,
            &mut out,
        );
        http.take();
        http.answer(answer(200, b"new"), &storage, &mut out);
        assert_eq!(replied(&out), (200, String::new()));
        assert_eq!(read_file(&storage, "book.epub", 64).unwrap(), b"new");
    }

    #[test]
    fn a_newer_request_for_a_file_replaces_the_older() {
        let storage = MemoryStorage::new();
        let mut out = Vec::new();
        let mut http = Http::default();
        http.request(
            get("http://h.org/1", Some("dl.tmp")),
            true,
            false,
            &storage,
            &mut out,
        );
        assert_eq!(http.take().len(), 1);
        http.request(
            get("http://h.org/2", Some("DL.tmp")),
            true,
            false,
            &storage,
            &mut out,
        );
        assert_eq!(
            replied(&out),
            (0, String::from("replaced by a newer request"))
        );
        let calls = http.take();
        assert_eq!(calls.len(), 1);
        // The older one's answer comes late: passed over, the file not
        // written.
        out.clear();
        http.answer(answer(200, b"old"), &storage, &mut out);
        assert!(out.is_empty());
        assert!(!storage.exists("dl.tmp"));
        http.answer(
            HttpResponse {
                id: calls[0].id,
                ..answer(200, b"new")
            },
            &storage,
            &mut out,
        );
        assert_eq!(read_file(&storage, "DL.tmp", 64).unwrap(), b"new");
        // Not yet passed to the shell: only the newer one goes.
        http.request(
            get("http://h.org/3", Some("a")),
            true,
            false,
            &storage,
            &mut out,
        );
        http.request(
            get("http://h.org/4", Some("a")),
            true,
            false,
            &storage,
            &mut out,
        );
        let calls = http.take();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].url, "http://h.org/4");
    }

    #[test]
    fn bodies_pass_as_text() {
        let storage = MemoryStorage::new();
        let mut out = Vec::new();
        let mut http = Http::default();
        http.request(get("http://h.org/", None), true, false, &storage, &mut out);
        http.take();
        http.answer(answer(200, b"caf\xc3\xa9 \xff"), &storage, &mut out);
        assert_eq!(replied(&out), (200, String::from("café \u{fffd}")));
    }
}

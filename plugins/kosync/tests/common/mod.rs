//! Deterministic interface-7 services; no sockets or real accounts.
#![allow(dead_code, reason = "shared by the native acceptance cases")]
use paperpaw_core::native_host::{environment::SettingValueRef, registrations::*};
use paperpaw_plugin_kosync::{NATIVE_ENTRY, declarations as tokens};
use paperpaw_scripted_host::{Callback, Dialogs, Plugin, ScriptedHost, event_batch, row};
use std::{cell::RefCell, rc::Rc};

pub const FINGERPRINT: &str = "d41d8cd98f00b204e9800998ecf8427e";
pub const TEST_KEY: &str = "test-only-key";

pub struct Request {
    pub id: u32,
    pub token: u32,
    pub method: String,
    pub url: String,
    pub body: Vec<u8>,
    pub authenticated: bool,
}

#[derive(Default)]
pub struct Server {
    pub requests: Vec<Request>,
    pub body: Vec<u8>,
    pub limit: Option<usize>,
    pub key: Option<String>,
    pub keep_calls: usize,
    pub remove_calls: usize,
}

pub struct World {
    pub host: ScriptedHost,
    pub dialogs: Dialogs,
    pub server: Rc<RefCell<Server>>,
    pub library_ops: Rc<RefCell<Vec<u32>>>,
}

impl World {
    pub fn new(signed_in: bool) -> Self {
        let mut host = ScriptedHost::new();
        for key in ["kosync.user", "kosync.server"] {
            host.settings_mut()
                .define(paperpaw_sdk::SettingSpec::text(
                    key,
                    key,
                    "",
                    paperpaw_sdk::Id::new_static("kosync.group"),
                ))
                .unwrap();
        }
        host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
            .unwrap();
        host.set_setting(
            "kosync.user",
            SettingValueRef::Text(if signed_in { "test-reader" } else { "" }),
        );
        host.set_setting("kosync.auto", SettingValueRef::Bool(true));
        host.set_setting(
            "kosync.server",
            SettingValueRef::Text("https://fake.invalid/kosync"),
        );
        let dialogs = host.dialogs();
        let server = Rc::new(RefCell::new(Server {
            key: signed_in.then(|| TEST_KEY.to_owned()),
            ..Server::default()
        }));
        let state = server.clone();
        host.service("core.secrets", 1, 0, move |op, input| {
            let rows = records_decode(input).map_err(|_| -2)?;
            let mut server = state.borrow_mut();
            match op {
                1 => {
                    let key = server.key.as_deref().ok_or(-10)?;
                    row(&Record::ServiceSecValue(ServiceSecValue {
                        flags: 0,
                        label: "test",
                        value: key,
                    }))
                }
                3 => {
                    let Some(Record::ServiceSecKeep(write)) = rows.iter().next() else {
                        return Err(-2);
                    };
                    server.key = Some(write.value.to_owned());
                    server.keep_calls += 1;
                    Ok(Vec::new())
                }
                4 => {
                    server.key = None;
                    server.remove_calls += 1;
                    Ok(Vec::new())
                }
                5 => Err(-1),
                _ => Err(-1),
            }
        });
        for op in [1, 3, 4] {
            host.reply_bound("core.secrets", op, 1280);
        }
        let state = server.clone();
        host.service("core.net", 1, 0, move |op, input| {
            let rows = records_decode(input).map_err(|_| -2)?;
            let mut server = state.borrow_mut();
            match op {
                1 => {
                    let Some(Record::ServiceNetRequest(request)) = rows.iter().next() else { return Err(-2); };
                    let id = u32::try_from(server.requests.len() + 1).map_err(|_| -2)?;
                    let body = rows.iter().find_map(|r| match r { Record::ServiceNetBody(body) => Some(body.data.to_vec()), _ => None }).unwrap_or_default();
                    let authenticated = rows.iter().any(|r| matches!(r, Record::ServiceNetHeader(h) if h.name == "x-auth-key" && !h.value.is_empty()));
                    server.requests.push(Request { id, token: request.token, method: request.method.to_owned(), url: request.url.to_owned(), body, authenticated });
                    row(&Record::ServiceNetStarted(ServiceNetStarted { id }))
                }
                4 => {
                    let Some(Record::ServiceNetRead(read)) = rows.iter().next() else { return Err(-2); };
                    let start = read.offset as usize;
                    let end = server.limit.unwrap_or(server.body.len()).min(server.body.len()).min(start.saturating_add(3840));
                    let part = server.body.get(start..end).unwrap_or_default();
                    row(&Record::ServiceNetBytes(ServiceNetBytes { data: part }))
                }
                _ => Err(-1),
            }
        });
        let library_ops = Rc::new(RefCell::new(Vec::new()));
        let calls = library_ops.clone();
        host.service("core.library", 1, 0, move |op, input| {
            calls.borrow_mut().push(op);
            if op != 25 { return Err(-1); }
            let rows = records_decode(input).map_err(|_| -2)?;
            if !matches!(rows.iter().next(), Some(Record::ServiceLibRef(book)) if !book.path.is_empty()) { return Err(-2); }
            row(&Record::ServiceLibKey(ServiceLibKey { key: "0123456789abcdef", fingerprint: FINGERPRINT, size: 0 }))
        });
        Self {
            host,
            dialogs,
            server,
            library_ops,
        }
    }

    pub fn load(&self) -> Plugin<'_> {
        self.host.load(NATIVE_ENTRY.entry()).unwrap()
    }

    pub fn opened(&self, plugin: &Plugin<'_>, path: &str, ppm: u32) -> Callback {
        plugin
            .event(
                tokens::BOOK_OPENED,
                &event_batch(&Record::EventBookOpened(EventBookOpened {
                    now_ms: 0,
                    section: 0,
                    offset: 10,
                    progress_ppm: ppm,
                    path,
                    title: "Test",
                    chapter: "",
                    reason: 0,
                    page: None,
                    pages: None,
                    insert: None,
                    chapter_left: None,
                }))
                .unwrap(),
            )
            .unwrap()
    }

    pub fn closed(&self, plugin: &Plugin<'_>, path: &str, ppm: u32, now_ms: u64) -> Callback {
        plugin
            .event(
                tokens::BOOK_CLOSED,
                &event_batch(&Record::EventBookClosed(EventBookClosed {
                    now_ms,
                    section: 0,
                    offset: 10,
                    progress_ppm: ppm,
                    path,
                    title: "Test",
                    chapter: "",
                    reason: 0,
                    page: None,
                    pages: None,
                    insert: None,
                    chapter_left: None,
                }))
                .unwrap(),
            )
            .unwrap()
    }

    pub fn complete(
        &self,
        plugin: &Plugin<'_>,
        request: usize,
        status: u32,
        body: &[u8],
    ) -> Callback {
        self.server.borrow_mut().body = body.to_vec();
        let server = self.server.borrow();
        let request = &server.requests[request];
        let done = event_batch(&Record::EventHttpDone(EventHttpDone {
            now_ms: 100,
            id: request.id,
            token: request.token,
            status,
            reason: 0,
            flags: 1,
            body_len: u32::try_from(body.len()).unwrap(),
        }))
        .unwrap();
        drop(server);
        plugin.event(tokens::HTTP_DONE, &done).unwrap()
    }

    pub fn account(&self, plugin: &Plugin<'_>) -> Callback {
        let opened = plugin
            .event(
                tokens::ACCOUNT,
                &event_batch(&Record::EventScreenOpened(EventScreenOpened {
                    now_ms: 0,
                    instance: 1,
                    screen: "kosync.account",
                    slot: "",
                    flags: 0,
                    handover: 0,
                    params: 0,
                    text_bytes: 0,
                }))
                .unwrap(),
            )
            .unwrap();
        assert_eq!(opened.status, 0);
        plugin
            .event(
                tokens::ACCOUNT,
                &event_batch(&Record::EventScreenShown(EventScreenShown {
                    now_ms: 0,
                    screen: "kosync.account",
                    width: 480,
                    height: 800,
                    instance: Some(1),
                    last_input: 0,
                }))
                .unwrap(),
            )
            .unwrap()
    }

    pub fn action(&self, plugin: &Plugin<'_>, action: &str) -> Callback {
        plugin
            .input(
                tokens::ACCOUNT,
                &event_batch(&Record::InputAction(InputAction { now_ms: 0, action })).unwrap(),
            )
            .unwrap()
    }
}

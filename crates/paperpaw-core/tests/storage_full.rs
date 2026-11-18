//! A full card: what the kernel writes for plugins and for itself
//! (settings, secrets, request bodies, files read from sockets, downloads)
//! fails with a warning or a failure the plugin hears, never a panic, and
//! what was on the card before stays as it was.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
// Tests panic on purpose when something is wrong.

mod common;

use common::AddDriver;

use std::cell::Cell;
use std::rc::Rc;

use common::{DEVICE, Harness, REPLY};
use paperpaw_core::Ask;
use paperpaw_core::sdk::builtin::CONFIG_FILE;
use paperpaw_core::sdk::secrets::{FILE as SECRETS, SecretChange};
use paperpaw_core::sdk::{
    DirEntry, HttpRequest, Id, MemoryStorage, SocketRead, Storage, StorageError,
};
use paperpaw_core::{HttpResponse, Io, Kernel, ServerCall, ServerEvent, SocketCall, SocketEvent};
use paperpaw_platform::Instant;

/// A card that fills up when told to: then nothing more is written.
#[derive(Clone, Default)]
struct Card {
    files: Rc<MemoryStorage>,
    full: Rc<Cell<bool>>,
}

impl Card {
    fn fill(&self) {
        self.full.set(true);
    }

    fn check(&self) -> Result<(), StorageError> {
        if self.full.get() {
            Err(StorageError::Full)
        } else {
            Ok(())
        }
    }

    fn text(&self, path: &str) -> String {
        String::from_utf8(self.files.get(path).unwrap_or_default()).unwrap()
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.files.read_at(path, offset, buf)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.files.size(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.check()?;
        self.files.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.check()?;
        self.files.append(path, data)
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.files.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.files.remove(path)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.check()?;
        self.files.rename(from, to)
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.check()?;
        self.files.create_dir(path)
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.files.remove_dir_all(path)
    }
}

fn kernel(harness: &Harness, card: &Card) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .driver(harness.plugin())
        .storage(card.clone())
        .network(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    common::render(&mut kernel);
    harness.take();
    kernel
}

#[test]
fn a_setting_changed_on_a_full_card_applies_and_keeps_the_file() {
    let harness = Harness::new();
    let card = Card::default();
    let mut kernel = kernel(&harness, &card);
    kernel
        .set_setting(Instant(10), "core.ui-size", "12")
        .unwrap();
    let before = card.text(CONFIG_FILE);
    card.fill();
    kernel
        .set_setting(Instant(20), "core.ui-size", "14")
        .unwrap();
    harness.run(
        &mut kernel,
        30,
        [
            Ask::ResetSetting(Id::new_static("core.ui-size")),
            Ask::SetMenu(Id::new_static("core.main"), Some(vec![])),
            Ask::bind(
                String::from("global"),
                String::from("button.up"),
                Some(Id::new_static("core.menu")),
            ),
        ],
    );
    assert_eq!(card.text(CONFIG_FILE), before);
    let warnings = kernel.take_warnings();
    assert!(warnings.len() >= 3, "{warnings:#?}");
    assert!(
        warnings.iter().all(|w| w.message.contains(CONFIG_FILE)),
        "{warnings:#?}"
    );
}

#[test]
fn a_secret_kept_on_a_full_card_is_reported_and_kept_for_now() {
    let harness = Harness::new();
    let card = Card::default();
    let mut kernel = kernel(&harness, &card);
    card.fill();
    kernel.set_secret(SecretChange::keep("drv.key", "Key", "value"));
    assert_eq!(kernel.secrets().get("drv.key"), Some("value"));
    assert!(!card.files.exists(SECRETS));
    let warnings = kernel.take_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
}

#[test]
fn request_bodies_that_do_not_fit_are_refused_and_not_passed_on() {
    let harness = Harness::new();
    let card = Card::default();
    let mut kernel = kernel(&harness, &card);
    harness.run(
        &mut kernel,
        10,
        [Ask::serve(Id::new_static("drv.server"), 8080, REPLY)],
    );
    let number = kernel
        .take_io()
        .into_iter()
        .find_map(|io| match io {
            Io::Server(ServerCall::Listen { server, .. }) => Some(server),
            _ => None,
        })
        .unwrap();
    let request = |request| ServerEvent::Request {
        server: number,
        request,
        method: String::from("PUT"),
        target: String::from("/up"),
        body: true,
    };
    let body = |request, last| ServerEvent::Body {
        request,
        data: b"data".to_vec(),
        last,
    };
    // Full halfway through a body; full before another begins.
    kernel.server_event(Instant(20), request(1));
    kernel.server_event(Instant(21), body(1, false));
    card.fill();
    kernel.server_event(Instant(22), body(1, false));
    kernel.server_event(Instant(23), body(1, true));
    kernel.server_event(Instant(24), request(2));
    kernel.server_event(Instant(25), body(2, true));
    let refused: Vec<(u32, u16)> = kernel
        .take_io()
        .into_iter()
        .filter_map(|io| match io {
            Io::Server(ServerCall::Respond {
                request, status, ..
            }) => Some((request, status)),
            _ => None,
        })
        .collect();
    assert_eq!(refused, [(1, 507), (2, 507)]);
    assert!(
        harness
            .lines("event")
            .iter()
            .all(|l| !l.contains("kind=request")),
        "{:?}",
        harness.log.borrow()
    );
    assert!(!card.files.exists(".paperpaw/tmp/request-1"));
    assert!(kernel.serving());
}

#[test]
fn a_file_read_from_a_socket_onto_a_full_card_fails_and_closes() {
    let harness = Harness::new();
    let card = Card::default();
    let mut kernel = kernel(&harness, &card);
    let socket = Id::new_static("drv.socket");
    harness.run(
        &mut kernel,
        10,
        [Ask::connect(
            socket.clone(),
            String::from("calibre.local"),
            9090,
            REPLY,
        )],
    );
    let number = kernel
        .take_io()
        .into_iter()
        .find_map(|io| match io {
            Io::Socket(SocketCall::Connect { socket, .. }) => Some(socket),
            _ => None,
        })
        .unwrap();
    kernel.socket_event(Instant(20), SocketEvent::Connected { socket: number });
    let read = || {
        Ask::socket_read(
            socket.clone(),
            SocketRead::File {
                path: String::from("books/new.epub"),
                len: 100,
            },
        )
    };
    harness.run(&mut kernel, 21, [read()]);
    kernel.socket_event(
        Instant(22),
        SocketEvent::Data {
            socket: number,
            data: vec![1; 40],
        },
    );
    card.fill();
    kernel.socket_event(
        Instant(23),
        SocketEvent::Data {
            socket: number,
            data: vec![2; 60],
        },
    );
    let told = harness.lines("event drv.reply");
    assert!(told.last().unwrap().contains("kind=closed"), "{told:?}");
    assert!(!kernel.sockets_open());
    // A read onto a full card does not even begin.
    harness.take();
    harness.run(
        &mut kernel,
        30,
        [Ask::connect(
            socket.clone(),
            String::from("calibre.local"),
            9090,
            REPLY,
        )],
    );
    harness.run(&mut kernel, 31, [read()]);
    let told = harness.lines("event drv.reply");
    assert!(told.last().unwrap().contains("kind=failed"), "{told:?}");
}

#[test]
fn a_download_onto_a_full_card_fails_and_keeps_the_old_file() {
    let harness = Harness::new();
    let card = Card::default();
    let mut kernel = kernel(&harness, &card);
    card.files.write("books/a.epub", b"old").unwrap();
    harness.run(
        &mut kernel,
        10,
        [Ask::http(
            HttpRequest::get("http://example.org/a.epub", REPLY).save_to("books/a.epub"),
        )],
    );
    let id = kernel
        .take_io()
        .into_iter()
        .find_map(|io| match io {
            Io::Http(call) => Some(call.id),
            _ => None,
        })
        .unwrap();
    card.fill();
    kernel.http_response(
        Instant(20),
        HttpResponse {
            id,
            status: 200,
            body: b"new".to_vec(),
            ..HttpResponse::default()
        },
    );
    let told = harness.lines("event drv.reply");
    assert_eq!(told.len(), 1);
    assert!(told[0].contains("status=0"), "{told:?}");
    assert_eq!(card.text("books/a.epub"), "old");
}

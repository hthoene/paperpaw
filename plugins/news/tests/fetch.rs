//! Fetching feeds against answers as servers and shells give them: each
//! feed's page has its own articles, even when the list of feeds changes
//! during a fetch, feeds share a name or two fetches are under way at
//! once; an answer that is no feed keeps the page there is.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::{ButtonState, InputEvent, Rotation};
use paperpaw_plugin_news::News;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, PixelFormat,
    Storage, StorageError,
};

const DEVICE: DeviceInfo = DeviceInfo {
    id: "test",
    name: "Test device",
    display: DisplayInfo {
        width: 200,
        height: 300,
        dpi: 100,
        format: PixelFormat::Gray4,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &[],
    },
    memory: MemoryClass::Large,
};

/// A storage the test looks into while the kernel has it.
#[derive(Clone)]
struct Shared(Rc<MemoryStorage>);

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}

/// The library at the storage's top.
const CONFIG: &str = "[core]\nlibrary-folder =\n[keymap.global]\nbutton.fetch = news.fetch\n";
const FEEDS: &str = ".paperpaw/plugins/news/feeds.txt";
const FOLDER: &str = "News";
const ALPHA: &str = "https://alpha.example/rss";
const BETA: &str = "https://beta.example/rss";
const GAMMA: &str = "https://gamma.example/rss";

/// A feed with one article, `item`.
fn rss(title: &str, item: &str) -> String {
    format!(
        "<rss><channel><title>{title}</title><item><title>{item}</title>\
         <description>All about {item}.</description></item></channel></rss>"
    )
}

/// The feed each address answers with, its article named after it.
fn feed_of(url: &str) -> String {
    let name = url
        .trim_start_matches("https://")
        .split('.')
        .next()
        .unwrap();
    rss(name, &format!("{name} news"))
}

fn kernel(feeds: &str) -> (Kernel, Shared) {
    let storage = Shared(Rc::new(MemoryStorage::new().with(FEEDS, feeds)));
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .user_config(CONFIG)
        .network(true)
        .plugin(News)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    (kernel, storage)
}

fn fetch(kernel: &mut Kernel, at: u64) -> Vec<HttpCall> {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        kernel.handle(
            Instant(at),
            InputEvent::Button {
                name: "fetch",
                state,
            },
        );
    }
    requests(kernel)
}

/// The requests the kernel asks the shell to make, once it has shown
/// what the call drew (as a shell does: until then, the kernel may keep
/// what it read, and would miss the test changing a file).
fn requests(kernel: &mut Kernel) -> Vec<HttpCall> {
    kernel.render(&mut |_, _| {});
    let io = kernel.take_io().into_iter();
    io.filter_map(|io| match io {
        Io::Http(call) => Some(call),
        _ => None,
    })
    .collect()
}

fn answer(kernel: &mut Kernel, call: &HttpCall, status: u16, body: &str) -> Vec<HttpCall> {
    kernel.http_response(
        Instant(10),
        HttpResponse {
            id: call.id,
            status,
            body: body.as_bytes().to_vec(),
            ..HttpResponse::default()
        },
    );
    requests(kernel)
}

/// Answers `calls` as a shell does that writes each answer to its file
/// as it comes, all of them before the first reply; then the requests
/// that follow, the same way, until there are none.
fn serve(kernel: &mut Kernel, storage: &Shared, mut calls: Vec<HttpCall>) {
    while !calls.is_empty() {
        for call in &calls {
            let path = call.save_to.as_deref().unwrap();
            storage.write(path, feed_of(&call.url).as_bytes()).unwrap();
        }
        let mut next = Vec::new();
        for call in &calls {
            kernel.http_response(
                Instant(10),
                HttpResponse {
                    id: call.id,
                    status: 200,
                    saved: true,
                    ..HttpResponse::default()
                },
            );
            next.extend(requests(kernel));
        }
        calls = next;
    }
}

/// The pages in the news folder: their names and texts.
fn pages(storage: &Shared) -> Vec<(String, String)> {
    let mut names = Vec::new();
    let _ = storage.list(FOLDER, &mut |entry| {
        if !entry.is_dir {
            names.push(entry.name.to_string());
        }
    });
    names
        .into_iter()
        .map(|name| {
            let data = storage.0.get(&format!("{FOLDER}/{name}")).unwrap();
            (name, String::from_utf8(data).unwrap())
        })
        .collect()
}

/// The text of the page whose name starts with `name`.
fn page(storage: &Shared, name: &str) -> String {
    let pages = pages(storage);
    let found = pages.iter().filter(|(n, _)| n.starts_with(name));
    let texts: Vec<&String> = found.map(|(_, text)| text).collect();
    assert_eq!(texts.len(), 1, "{name}: {pages:?}");
    texts[0].clone()
}

/// A feed removed while the feeds are fetched: its answer goes nowhere,
/// and the feeds after it are fetched each into its own page.
#[test]
fn a_feed_removed_during_a_fetch_moves_no_answer() {
    let (mut kernel, storage) = kernel(&format!("{ALPHA}\tAlpha\n{BETA}\tBeta\n{GAMMA}\tGamma\n"));
    let calls = fetch(&mut kernel, 1);
    assert_eq!(calls[0].url, ALPHA);
    storage
        .write(FEEDS, format!("{BETA}\tBeta\n{GAMMA}\tGamma\n").as_bytes())
        .unwrap();
    let calls = answer(&mut kernel, &calls[0], 200, &feed_of(ALPHA));
    assert_eq!(calls[0].url, BETA);
    let calls = answer(&mut kernel, &calls[0], 200, &feed_of(BETA));
    assert_eq!(calls[0].url, GAMMA);
    let calls = answer(&mut kernel, &calls[0], 200, &feed_of(GAMMA));
    assert!(calls.is_empty());
    let all = pages(&storage);
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(page(&storage, "Beta").contains("beta news"), "{all:?}");
    assert!(page(&storage, "Gamma").contains("gamma news"), "{all:?}");
    assert!(
        !all.iter().any(|(_, text)| text.contains("alpha")),
        "{all:?}"
    );
    let feeds = String::from_utf8(storage.0.get(FEEDS).unwrap()).unwrap();
    assert_eq!(feeds, format!("{BETA}\tBeta\n{GAMMA}\tGamma\n"));
}

/// Feeds of one name keep a page each; the page an earlier version kept
/// under that name goes once both have theirs.
#[test]
fn feeds_of_one_name_keep_a_page_each() {
    let (mut kernel, storage) = kernel(&format!("{ALPHA}\tDaily\n{BETA}\tDaily\n"));
    storage
        .write(
            &format!("{FOLDER}/Daily.html"),
            b"<!DOCTYPE html>
<html>
<head>
<meta charset=\"utf-8\"/>
<title>Daily</title>",
        )
        .unwrap();
    let calls = fetch(&mut kernel, 1);
    serve(&mut kernel, &storage, calls);
    let all = pages(&storage);
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(
        all.iter().all(|(name, _)| name.starts_with("Daily (")),
        "{all:?}"
    );
    assert!(
        all.iter().any(|(_, text)| text.contains("alpha news")),
        "{all:?}"
    );
    assert!(
        all.iter().any(|(_, text)| text.contains("beta news")),
        "{all:?}"
    );
}

/// Two fetches under way at once (the second started while the first
/// still runs) do not swap their feeds' answers.
#[test]
fn fetches_under_way_at_once_keep_their_answers() {
    let (mut kernel, storage) = kernel(&format!("{ALPHA}\tAlpha\n{BETA}\tBeta\n"));
    let first = fetch(&mut kernel, 1);
    let first = answer(&mut kernel, &first[0], 200, &feed_of(ALPHA));
    assert_eq!(first[0].url, BETA);
    let second = fetch(&mut kernel, 2);
    assert_eq!(second[0].url, ALPHA);
    assert_ne!(first[0].save_to, second[0].save_to);
    serve(&mut kernel, &storage, [first, second].concat());
    assert!(page(&storage, "Alpha").contains("alpha news"));
    assert!(page(&storage, "Beta").contains("beta news"));
}

/// An answer that is no feed (a Wi-Fi's sign-in page) keeps the page
/// there is.
#[test]
fn an_answer_that_is_no_feed_keeps_the_page() {
    let (mut kernel, storage) = kernel(&format!("{ALPHA}\tAlpha\n"));
    let calls = fetch(&mut kernel, 1);
    assert!(answer(&mut kernel, &calls[0], 200, &feed_of(ALPHA)).is_empty());
    let before = page(&storage, "Alpha");
    assert!(before.contains("alpha news"));
    let portal = "<!DOCTYPE html><html><head><title>Hotel Wi-Fi</title></head>\
                  <body><form><input name=\"room\"/></form></body></html>";
    let calls = fetch(&mut kernel, 2);
    assert!(answer(&mut kernel, &calls[0], 200, portal).is_empty());
    assert_eq!(page(&storage, "Alpha"), before);
    assert_eq!(pages(&storage).len(), 1);
    assert!(!storage.exists(".paperpaw/plugins/news/page.tmp"));
}

/// A fetched feed as a reader gets it: a long body in CDATA (as
/// WordPress writes it) whole instead of the summary, and an Atom date
/// short in the language.
#[test]
fn a_fetched_feed_keeps_its_long_text_and_shows_short_dates() {
    let (mut kernel, storage) = kernel(&format!(
        "{ALPHA}	Alpha
"
    ));
    let calls = fetch(&mut kernel, 1);
    let sentence = "The quick brown rabbit jumps over the lazy dog. ";
    let body = format!("<p>{}</p>", sentence.repeat(300));
    assert!(body.len() > 14_000);
    let atom = format!(
        "<feed xmlns=\"http://www.w3.org/2005/Atom\"><title>Atom</title><entry>         <title>Long</title><updated>2026-10-24T14:32:44-04:00</updated>         <summary>Only the summary</summary>         <content type=\"html\"><![CDATA[{body}]]></content></entry></feed>"
    );
    assert!(answer(&mut kernel, &calls[0], 200, &atom).is_empty());
    let page = page(&storage, "Alpha");
    assert!(page.contains("<p><i>Sat 24 Oct 2026</i></p>"), "{page}");
    assert!(!page.contains("Only the summary"));
    assert_eq!(page.matches("lazy dog.").count(), 300);
}

//! The cable over a fake host: the permission, the offer's limits, what is taken
//! as offered, the install that waits for the host, the books, and refused
//! allocations.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use crate::fail_alloc::{exempt, sweep};
use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use paperpaw_sdk::digest::sha256;

/// Files of the data folder, for reading an offer.
struct Bodies<'a>(&'a [(String, Vec<u8>)]);

impl Volume for Bodies<'_> {
    fn size(&self, name: &str) -> Result<u64, Error> {
        let found = self.0.iter().find(|(path, _)| path == name);
        found
            .map(|(_, bytes)| bytes.len() as u64)
            .ok_or(Error::NotFound)
    }
    fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        let (_, bytes) = self
            .0
            .iter()
            .find(|(path, _)| path == name)
            .ok_or(Error::NotFound)?;
        let from = usize::try_from(at).unwrap().min(bytes.len());
        let n = out.len().min(bytes.len() - from);
        out[..n].copy_from_slice(&bytes[from..from + n]);
        Ok(n)
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    fn rename(&self, _: &str, _: &str) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    fn remove(&self, _: &str) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    fn sync(&self) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
}

/// A host that keeps what was staged, imported and asked. Its own allocations
/// are not the code's: a refused allocation sweep leaves them alone.
struct Fake {
    bodies: Vec<(String, Vec<u8>)>,
    free: Option<u64>,
    wifi: Vec<String>,
    asked: Vec<(u32, Offer)>,
    ask_error: Option<Error>,
    forgotten: Vec<String>,
    /// Staged files: plugin, path, bytes.
    staged: Vec<(String, String, Vec<u8>)>,
    known: Known,
    installs: Vec<(u32, String, bool)>,
    install_error: Option<Error>,
    imported: Vec<(String, Vec<u8>)>,
    import_error: Option<Error>,
    skips: Vec<usize>,
}

impl Fake {
    fn new() -> Self {
        Self {
            bodies: Vec::new(),
            free: Some(10_000_000),
            wifi: Vec::new(),
            asked: Vec::new(),
            ask_error: None,
            forgotten: Vec::new(),
            staged: Vec::new(),
            known: Known::Absent,
            installs: Vec::new(),
            install_error: None,
            imported: Vec::new(),
            import_error: None,
            skips: Vec::new(),
        }
    }

    fn body(&mut self, path: &str) -> Vec<u8> {
        let at = self
            .bodies
            .iter()
            .position(|(name, _)| name == path)
            .unwrap();
        self.bodies.remove(at).1
    }
}

impl CableHost for Fake {
    fn hello(&mut self) -> Result<String, Error> {
        Ok(exempt(|| "{\"protocol\":1}".to_string()))
    }
    fn listing(&mut self, skip: usize) -> Result<String, Error> {
        exempt(|| self.skips.push(skip));
        Ok(exempt(|| "{\"plugins\":[]}".to_string()))
    }
    fn read_offer(&mut self, body: &str) -> Result<Option<Offer>, Error> {
        read_offer(&Bodies(&self.bodies), body)
    }
    fn free_space(&mut self) -> Result<Option<u64>, Error> {
        Ok(self.free)
    }
    fn wifi_is_sent(&mut self, id: &str) -> bool {
        self.wifi.iter().any(|sent| sent == id)
    }
    fn ask(&mut self, offer: &Offer) -> Result<u32, Error> {
        if let Some(error) = self.ask_error {
            return Err(error);
        }
        let request = 100 + self.asked.len() as u32;
        exempt(|| self.asked.push((request, offer.clone())));
        Ok(request)
    }
    fn forget(&mut self, id: &str) {
        exempt(|| {
            self.forgotten.push(id.to_string());
            self.staged.retain(|(plugin, ..)| plugin != id);
        });
    }
    fn stage(
        &mut self,
        id: &str,
        path: &str,
        body: &str,
        sha256: &[u8; 32],
    ) -> Result<Staged, Error> {
        let bytes = self.body(body);
        if sha256::digest(&bytes) != *sha256 {
            return Ok(Staged::Damaged);
        }
        exempt(|| {
            self.staged
                .retain(|(plugin, file, _)| !(plugin == id && file == path));
            self.staged.push((id.to_string(), path.to_string(), bytes));
        });
        Ok(Staged::Taken)
    }
    fn staged_files(&mut self, id: &str, limit: usize) -> Result<Vec<String>, Error> {
        Ok(exempt(|| {
            self.staged
                .iter()
                .filter(|(plugin, ..)| plugin == id)
                .map(|(_, path, _)| path.clone())
                .take(limit)
                .collect()
        }))
    }
    fn staged_matches(
        &mut self,
        id: &str,
        path: &str,
        size: u64,
        sha256: &[u8; 32],
    ) -> Result<bool, Error> {
        Ok(self.staged.iter().any(|(plugin, file, bytes)| {
            plugin == id
                && file == path
                && bytes.len() as u64 == size
                && sha256::digest(bytes) == *sha256
        }))
    }
    fn known(&mut self, _: &str) -> Result<Known, Error> {
        Ok(self.known)
    }
    fn install(&mut self, request: u32, id: &str, update: bool) -> Result<(), Error> {
        exempt(|| self.installs.push((request, id.to_string(), update)));
        self.install_error.map_or(Ok(()), Err)
    }
    fn import(&mut self, body: &str, name: &str, sha256: &[u8; 32]) -> Result<Imported, Error> {
        if let Some(error) = self.import_error {
            return Err(error);
        }
        let bytes = self.body(body);
        if sha256::digest(&bytes) != *sha256 {
            return Ok(Imported::Damaged);
        }
        let taken = self
            .imported
            .iter()
            .filter(|(path, _)| path.contains(name))
            .count();
        let path = exempt(|| {
            let path = if taken == 0 {
                format!("books/{name}")
            } else {
                format!("books/{name} ({})", taken + 1)
            };
            self.imported.push((path.clone(), bytes));
            path
        });
        Ok(Imported::Taken(path))
    }
}

/// A computer at the cable, and the device's side of it.
struct Rig {
    cable: Cable,
    host: Fake,
    next: u32,
}

impl Rig {
    fn new() -> Self {
        Self {
            cable: Cable::new(),
            host: Fake::new(),
            next: 1,
        }
    }

    /// One request; the answer's status and body, or `None` where it comes later.
    fn ask(&mut self, method: &str, target: &str, body: Option<&[u8]>) -> Option<(u16, String)> {
        let number = self.next;
        self.next += 1;
        let path = format!(".received/3/{number}");
        if let Some(bytes) = body {
            self.host.bodies.push((path.clone(), bytes.to_vec()));
        }
        let call = Call {
            number,
            method,
            target,
            body: body.map(|_| path.as_str()),
            body_len: body.map_or(0, |bytes| bytes.len() as u64),
        };
        match self.cable.handle(&mut self.host, &call).unwrap() {
            Answer::Now(reply) => Some((
                reply.status,
                String::from_utf8(reply.body.bytes().to_vec()).unwrap(),
            )),
            Answer::Later => None,
        }
    }

    fn status(&mut self, method: &str, target: &str, body: Option<&[u8]>) -> u16 {
        self.ask(method, target, body).unwrap().0
    }

    fn offer(&mut self, text: &str) -> u16 {
        self.status("POST", "/api/offer", Some(text.as_bytes()))
    }

    /// The reader allows the question asked last.
    fn allow(&mut self) {
        let request = self.host.asked.last().unwrap().0;
        assert!(self.cable.answered(&mut self.host, request, true));
    }

    fn put(&mut self, id: &str, path: &str, data: &[u8]) -> u16 {
        let target = format!("/api/plugin/{id}/{path}?sha256={}", sha256::hex(data));
        self.status("PUT", &target, Some(data))
    }
}

fn hex(data: &[u8]) -> String {
    sha256::hex(data)
}

/// The offer's text: a plugin with its files, and books.
/// A file or a book: its name and its bytes.
type Named<'a> = (&'a str, &'a [u8]);

fn offer_text(plugin: Option<(&str, &[Named<'_>])>, books: &[Named<'_>]) -> String {
    let mut text = String::new();
    if let Some((id, files)) = plugin {
        text.push_str(&format!("plugin {id}\n"));
        for (path, data) in files {
            text.push_str(&format!("{path} {} {}\n", data.len(), hex(data)));
        }
    }
    for (name, data) in books {
        text.push_str(&format!("book {name} {} {}\n", data.len(), hex(data)));
    }
    text
}

const INI: &[u8] = b"[plugin]\nid = airy\nname = Airy\nversion = 1.0.0\n";
const DE: &[u8] = b"[meta]\n";

fn plugin_offer() -> String {
    offer_text(
        Some(("airy", &[("plugin.ini", INI), ("i18n/de.ini", DE)])),
        &[],
    )
}

/// A rig with the plugin `airy` offered and allowed.
fn allowed_plugin() -> Rig {
    let mut rig = Rig::new();
    assert_eq!(rig.offer(&plugin_offer()), 202);
    rig.allow();
    rig
}

#[test]
fn hello_and_the_plugins_need_no_permission() {
    let mut rig = Rig::new();
    assert_eq!(rig.ask("GET", "/api/hello", None).unwrap().0, 200);
    assert_eq!(rig.ask("GET", "/api/plugins", None).unwrap().0, 200);
    rig.ask("GET", "/api/plugins?skip=12&x=1", None).unwrap();
    assert_eq!(rig.host.skips, [0, 12]);
    assert_eq!(
        rig.ask("GET", "/api/offer", None).unwrap().1,
        "{\"state\":\"none\"}"
    );
    assert_eq!(rig.cable.permission(), Permission::None);
}

#[test]
fn nothing_is_taken_before_the_reader_allows_it() {
    let mut rig = Rig::new();
    for (method, target) in [
        ("PUT", "/api/plugin/airy/plugin.ini"),
        ("POST", "/api/plugin/airy"),
        ("PUT", "/api/book/a.epub"),
        ("GET", "/api/anything"),
        ("DELETE", "/api/plugin/airy"),
    ] {
        let (status, body) = rig.ask(method, target, Some(b"x")).unwrap();
        assert_eq!(status, 403, "{method} {target}");
        assert!(body.contains("not allowed on the device"), "{body}");
    }
    // Asked about is not allowed yet; refused is not either.
    assert_eq!(rig.offer(&plugin_offer()), 202);
    assert_eq!(rig.put("airy", "plugin.ini", INI), 403);
    let request = rig.host.asked[0].0;
    assert!(rig.cable.answered(&mut rig.host, request, false));
    assert_eq!(rig.put("airy", "plugin.ini", INI), 403);
    assert!(rig.host.staged.is_empty());
}

#[test]
fn the_reader_is_asked_and_the_answer_decides() {
    let mut rig = Rig::new();
    let (status, body) = rig
        .ask("POST", "/api/offer", Some(plugin_offer().as_bytes()))
        .unwrap();
    assert_eq!((status, body.as_str()), (202, "{\"state\":\"asking\"}"));
    assert_eq!(rig.cable.permission(), Permission::Asking);
    assert_eq!(rig.host.asked.len(), 1);
    assert_eq!(
        rig.ask("GET", "/api/offer", None).unwrap().1,
        "{\"state\":\"asking\"}"
    );
    assert!(rig.cable.waiting());
    rig.allow();
    assert_eq!(
        rig.ask("GET", "/api/offer", None).unwrap().1,
        "{\"state\":\"allowed\"}"
    );
    assert!(!rig.cable.waiting());
    // A refusal (the dialog's no, or closed without an answer) leaves nothing.
    assert_eq!(rig.offer(&plugin_offer()), 202);
    let request = rig.host.asked.last().unwrap().0;
    assert!(rig.cable.answered(&mut rig.host, request, false));
    assert_eq!(
        rig.ask("GET", "/api/offer", None).unwrap().1,
        "{\"state\":\"refused\"}"
    );
    assert_eq!(rig.put("airy", "plugin.ini", INI), 403);
}

#[test]
fn an_answer_counts_for_its_offer_only() {
    let mut rig = Rig::new();
    assert_eq!(rig.offer(&plugin_offer()), 202);
    let first = rig.host.asked[0].0;
    // Another offer while the reader is asked is not taken.
    assert_eq!(rig.offer(&plugin_offer()), 409);
    assert_eq!(rig.host.asked.len(), 1);
    // An answer to another question is not this one's.
    assert!(!rig.cable.answered(&mut rig.host, first + 1, true));
    assert_eq!(rig.cable.permission(), Permission::Asking);
    assert!(rig.cable.answered(&mut rig.host, first, false));
    // A new offer is asked anew; the old answer cannot allow it.
    assert_eq!(rig.offer(&plugin_offer()), 202);
    let second = rig.host.asked[1].0;
    assert_ne!(first, second);
    assert!(!rig.cable.answered(&mut rig.host, first, true));
    assert_eq!(rig.cable.permission(), Permission::Asking);
    // Nor does an answer count when nothing is asked.
    assert!(rig.cable.answered(&mut rig.host, second, true));
    assert!(!rig.cable.answered(&mut rig.host, second, false));
    assert_eq!(rig.cable.permission(), Permission::Allowed);
}

#[test]
fn a_new_offer_ends_the_one_before_and_what_was_sent_for_it() {
    let mut rig = allowed_plugin();
    assert_eq!(rig.put("airy", "plugin.ini", INI), 200);
    assert_eq!(rig.host.staged.len(), 1);
    let other = offer_text(None, &[("a.epub", b"book")]);
    assert_eq!(rig.offer(&other), 202);
    assert!(rig.host.staged.is_empty());
    assert_eq!(rig.cable.permission(), Permission::Asking);
    // Not allowed any more for the plugin.
    rig.allow();
    assert_eq!(rig.put("airy", "plugin.ini", INI), 403);
}

#[test]
fn offers_that_are_not_offers_are_refused() {
    let big = "x".repeat(400);
    let long = format!("book {big}.epub 1 {}\n", hex(b"a"));
    let mut lots = String::from("plugin airy\n");
    for n in 0..65 {
        lots.push_str(&format!("f{n}.txt 1 {}\n", hex(b"a")));
    }
    let mut books = String::new();
    for n in 0..65 {
        books.push_str(&format!("book b{n}.epub 1 {}\n", hex(b"a")));
    }
    let cases = [
        String::from(""),
        String::from("\n\n"),
        String::from("plugin airy\n"),
        String::from("plugin ../x\nplugin.ini 1 00\n"),
        format!("plugin airy\n../a.txt 1 {}\n", hex(b"a")),
        format!("plugin airy\n/a.txt 1 {}\n", hex(b"a")),
        format!("book .hidden.epub 1 {}\n", hex(b"a")),
        format!("book a/b.epub 1 {}\n", hex(b"a")),
        String::from("book a.epub x y\n"),
        String::from("stray words\n"),
        format!("a.txt 1 {}\n", hex(b"a")),
        long,
        lots,
        books,
    ];
    for text in cases {
        let mut rig = Rig::new();
        assert_eq!(rig.offer(&text), 400, "{text:.60}");
        assert!(rig.host.asked.is_empty());
        assert_eq!(rig.cable.permission(), Permission::None);
    }
    // No body at all.
    let mut rig = Rig::new();
    assert_eq!(rig.status("POST", "/api/offer", None), 400);
}

#[test]
fn an_offer_is_read_a_line_at_a_time_up_to_the_most_lines() {
    let sha = hex(b"a");
    // Exactly the most lines: a plugin with 64 files, and 64 books.
    let mut text = String::from("plugin airy\r\n");
    for n in 0..crate::native_transfer::MAX_FILES - 65 {
        text.push_str(&format!("book book number {n}.epub 1 {sha}\r\n"));
    }
    for n in 0..64 {
        text.push_str(&format!("dir/file{n}.txt 1 {sha}\r\n"));
    }
    let bodies = vec![(String::from("offer"), text.clone().into_bytes())];
    let offer = read_offer(&Bodies(&bodies), "offer").unwrap().unwrap();
    assert_eq!(offer.plugin.as_ref().unwrap().1.len(), 64);
    assert_eq!(offer.books.len(), 64);
    assert_eq!(offer.named.len(), crate::native_transfer::NAMED);
    // One line more is not an offer.
    text.push_str(&format!("book more.epub 1 {sha}\n"));
    let bodies = vec![(String::from("offer"), text.into_bytes())];
    assert_eq!(read_offer(&Bodies(&bodies), "offer").unwrap(), None);
    // A file that is not there, or too long to be an offer.
    assert_eq!(read_offer(&Bodies(&bodies), "missing").unwrap(), None);
    let huge = vec![(String::from("offer"), vec![b'\n'; MAX_OFFER as usize + 1])];
    assert_eq!(read_offer(&Bodies(&huge), "offer").unwrap(), None);
    // The same offer as `Offer::parse` reads it.
    let plain = offer_text(
        Some(("airy", &[("plugin.ini", INI)])),
        &[("a b.epub", b"x")],
    );
    let bodies = vec![(String::from("offer"), plain.clone().into_bytes())];
    assert_eq!(
        read_offer(&Bodies(&bodies), "offer").unwrap(),
        Offer::parse(&plain).unwrap()
    );
}

#[test]
fn an_offer_needs_room_and_is_not_made_over_wifi() {
    let mut rig = Rig::new();
    rig.host.free = Some(10);
    assert_eq!(rig.offer(&plugin_offer()), 507);
    assert!(rig.host.asked.is_empty());
    // Room that is not known does not refuse.
    rig.host.free = None;
    assert_eq!(rig.offer(&plugin_offer()), 202);
    let request = rig.host.asked[0].0;
    rig.cable.answered(&mut rig.host, request, false);
    // Sizes that cannot be added are not room.
    let sha = hex(b"a");
    let text = format!("book a.epub 18446744073709551615 {sha}\nbook b.epub 2 {sha}\n");
    assert_eq!(rig.offer(&text), 507);
    // The plugin the Wi-Fi transfer is sent.
    rig.host.wifi.push(String::from("airy"));
    assert_eq!(rig.offer(&plugin_offer()), 409);
    assert_eq!(rig.cable.permission(), Permission::None);
    // No question can be asked now: nothing is allowed, nothing is kept.
    rig.host.wifi.clear();
    rig.host.ask_error = Some(Error::Full);
    assert_eq!(rig.offer(&plugin_offer()), 503);
    assert_eq!(rig.cable.permission(), Permission::None);
    assert!(!rig.cable.waiting());
}

#[test]
fn only_files_as_offered_come_in() {
    let mut rig = allowed_plugin();
    assert_eq!(rig.put("airy", "plugin.ini", INI), 200);
    assert_eq!(
        rig.cable.said(),
        Some(&Said::Receiving(String::from("airy")))
    );
    assert_eq!(rig.put("airy", "i18n/de.ini", DE), 200);
    assert_eq!(rig.host.staged.len(), 2);
    // A file not offered, another plugin's, another size or digest.
    assert_eq!(rig.put("airy", "code.ppn", b"x"), 403);
    assert_eq!(rig.put("other", "plugin.ini", INI), 403);
    assert_eq!(
        rig.put(
            "airy",
            "plugin.ini",
            b"[plugin]\nid = airy\nname = Evil\nversion = 1.0.0\n"
        ),
        422
    );
    let target = format!("/api/plugin/airy/plugin.ini?sha256={}", hex(b"other"));
    assert_eq!(rig.status("PUT", &target, Some(INI)), 422);
    // The body is not what its digest says (the size is): damaged.
    let mut same_size = INI.to_vec();
    same_size[0] = b'{';
    let target = format!("/api/plugin/airy/plugin.ini?sha256={}", hex(INI));
    let (status, body) = rig.ask("PUT", &target, Some(&same_size)).unwrap();
    assert_eq!(status, 422);
    assert!(body.contains("\"damaged\""), "{body}");
    // No body, a bare folder, and a digest left out (the offer's counts).
    assert_eq!(rig.status("PUT", "/api/plugin/airy/plugin.ini", None), 403);
    assert_eq!(rig.status("PUT", "/api/plugin/airy", Some(INI)), 404);
    assert_eq!(
        rig.status("PUT", "/api/plugin/airy/plugin.ini", Some(INI)),
        200
    );
    assert_eq!(rig.status("GET", "/api/plugin/airy", None), 404);
}

#[test]
fn a_plugin_is_installed_in_one_step_when_the_host_ends_it() {
    let mut rig = allowed_plugin();
    rig.put("airy", "plugin.ini", INI);
    rig.put("airy", "i18n/de.ini", DE);
    rig.host.known = Known::Installed;
    // The host is asked, and the answer waits.
    assert_eq!(rig.ask("POST", "/api/plugin/airy", None), None);
    let (request, id, update) = rig.host.installs[0].clone();
    assert_eq!((id.as_str(), update), ("airy", true));
    assert!(request >= FIRST_INSTALL);
    assert!(rig.cable.waiting());
    assert_eq!(
        rig.cable.said(),
        Some(&Said::Installing(String::from("airy")))
    );
    // One at a time.
    assert_eq!(rig.status("POST", "/api/plugin/airy", None), 409);
    // A new offer would forget what is being installed: busy too, and the
    // install still ends as asked.
    assert_eq!(rig.offer(&plugin_offer()), 409);
    assert!(rig.cable.waiting());
    // The end of another request is not this one's.
    let other = rig
        .cable
        .install_done(&mut rig.host, request + 1, Ok(false))
        .unwrap();
    assert_eq!(other, None);
    let (number, reply) = rig
        .cable
        .install_done(&mut rig.host, request, Ok(true))
        .unwrap()
        .unwrap();
    assert_eq!(number, 4);
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.body.bytes(),
        b"{\"state\":\"installed\",\"restart\":true}"
    );
    assert_eq!(
        rig.cable.said(),
        Some(&Said::Installed {
            id: String::from("airy"),
            restart: true
        })
    );
    // What is left of it goes, and it is offered once.
    assert!(rig.host.staged.is_empty());
    assert_eq!(rig.status("POST", "/api/plugin/airy", None), 403);
    assert!(!rig.cable.waiting());
}

#[test]
fn a_plugin_the_host_refuses_is_told_why() {
    let mut rig = allowed_plugin();
    rig.put("airy", "plugin.ini", INI);
    rig.put("airy", "i18n/de.ini", DE);
    assert_eq!(rig.ask("POST", "/api/plugin/airy", None), None);
    assert!(!rig.host.installs[0].2);
    let request = rig.host.installs[0].0;
    let (_, reply) = rig
        .cable
        .install_done(&mut rig.host, request, Err("not approved"))
        .unwrap()
        .unwrap();
    assert_eq!(reply.status, 422);
    assert_eq!(
        reply.body.bytes(),
        b"{\"state\":\"refused\",\"why\":\"not approved\"}"
    );
    assert_eq!(
        rig.cable.said(),
        Some(&Said::NotInstalled(String::from("airy")))
    );
    assert!(rig.host.staged.is_empty());
}

#[test]
fn the_host_may_refuse_the_install_command_itself() {
    for (error, status, why) in [
        (Error::Denied, 422, "not allowed"),
        (Error::Unsupported, 422, "not supported"),
        (Error::Io, 422, "refused"),
        (Error::Busy, 409, "busy"),
        (Error::Full, 409, "busy"),
    ] {
        let mut rig = allowed_plugin();
        rig.put("airy", "plugin.ini", INI);
        rig.put("airy", "i18n/de.ini", DE);
        rig.host.install_error = Some(error);
        let (got, body) = rig.ask("POST", "/api/plugin/airy", None).unwrap();
        assert_eq!(got, status, "{error:?}");
        assert!(body.contains(why), "{error:?}: {body}");
        assert!(!rig.cable.waiting());
        if status == 409 {
            // The files stay for another try.
            assert_eq!(rig.host.staged.len(), 2);
        } else {
            assert!(rig.host.staged.is_empty());
        }
    }
    // A plugin built into the firmware is never installed over.
    let mut rig = allowed_plugin();
    rig.put("airy", "plugin.ini", INI);
    rig.put("airy", "i18n/de.ini", DE);
    rig.host.known = Known::BuiltIn;
    let (status, body) = rig.ask("POST", "/api/plugin/airy", None).unwrap();
    assert_eq!(status, 422);
    assert!(body.contains("built in"), "{body}");
    assert!(rig.host.installs.is_empty());
}

#[test]
fn only_files_as_offered_are_installed() {
    // Not all of it came.
    let mut rig = allowed_plugin();
    rig.put("airy", "plugin.ini", INI);
    assert_eq!(rig.status("POST", "/api/plugin/airy", None), 422);
    assert!(rig.host.installs.is_empty());
    // Something else is staged beside it.
    rig.put("airy", "i18n/de.ini", DE);
    rig.host
        .staged
        .push((String::from("airy"), String::from("extra.txt"), vec![1]));
    assert_eq!(rig.status("POST", "/api/plugin/airy", None), 422);
    // A staged file changed after it came in.
    rig.host.staged.pop();
    rig.host.staged[0].2 = b"changed!".to_vec();
    assert_eq!(rig.status("POST", "/api/plugin/airy", None), 422);
    assert!(rig.host.installs.is_empty());
    // Another plugin than the offered one.
    assert_eq!(rig.status("POST", "/api/plugin/other", None), 403);
    // An offer of books only has no plugin to install.
    let mut rig = Rig::new();
    assert_eq!(rig.offer(&offer_text(None, &[("a.epub", b"a")])), 202);
    rig.allow();
    assert_eq!(rig.status("POST", "/api/plugin/airy", None), 403);
}

#[test]
fn staged_files_are_compared_with_the_offer_whole() {
    let files = [
        (String::from("a.txt"), 1, sha256::digest(b"a")),
        (String::from("d/b.txt"), 1, sha256::digest(b"b")),
    ];
    let mut host = Fake::new();
    let stage = |host: &mut Fake, path: &str, data: &[u8]| {
        host.staged
            .push((String::from("p"), path.to_string(), data.to_vec()));
    };
    stage(&mut host, "a.txt", b"a");
    assert!(!staged_as_offered(&mut host, "p", &files).unwrap());
    stage(&mut host, "d/b.txt", b"b");
    assert!(staged_as_offered(&mut host, "p", &files).unwrap());
    // Another plugin's files are not counted.
    host.staged
        .push((String::from("q"), String::from("z"), vec![]));
    assert!(staged_as_offered(&mut host, "p", &files).unwrap());
    // More than offered, the same number but another name, other content.
    stage(&mut host, "c.txt", b"c");
    assert!(!staged_as_offered(&mut host, "p", &files).unwrap());
    host.staged.pop();
    host.staged[1].1 = String::from("d/c.txt");
    assert!(!staged_as_offered(&mut host, "p", &files).unwrap());
    host.staged[1].1 = String::from("d/b.txt");
    host.staged[1].2 = b"B".to_vec();
    assert!(!staged_as_offered(&mut host, "p", &files).unwrap());
    // Nothing offered, nothing staged: whole, but nothing to install.
    assert!(staged_as_offered(&mut host, "none", &[]).unwrap());
}

#[test]
fn books_come_in_under_a_free_name_once_each() {
    let mut rig = Rig::new();
    let text = offer_text(None, &[("My Book.epub", b"one"), ("b.epub", b"two")]);
    assert_eq!(rig.offer(&text), 202);
    rig.allow();
    let target = format!("/api/book/My%20Book.epub?sha256={}", hex(b"one"));
    let (status, body) = rig.ask("PUT", &target, Some(b"one")).unwrap();
    assert_eq!(
        (status, body.as_str()),
        (200, "{\"path\":\"books/My Book.epub\"}")
    );
    assert_eq!(
        rig.cable.said(),
        Some(&Said::Received(String::from("My Book.epub")))
    );
    // Offered once, taken once.
    assert_eq!(rig.status("PUT", &target, Some(b"one")), 403);
    assert_eq!(rig.host.imported.len(), 1);
    // Not offered, not as offered, damaged.
    assert_eq!(rig.status("PUT", "/api/book/c.epub", Some(b"x")), 403);
    assert_eq!(rig.status("PUT", "/api/book/b.epub", None), 403);
    assert_eq!(rig.status("PUT", "/api/book/b.epub", Some(b"three")), 422);
    let wrong = format!("/api/book/b.epub?sha256={}", hex(b"one"));
    assert_eq!(rig.status("PUT", &wrong, Some(b"two")), 422);
    let damaged = format!("/api/book/b.epub?sha256={}", hex(b"two"));
    assert_eq!(rig.status("PUT", &damaged, Some(b"TWO")), 422);
    assert_eq!(rig.status("PUT", "/api/book/b.epub", Some(b"two")), 200);
    assert_eq!(rig.host.imported.len(), 2);
}

#[test]
fn what_the_library_refuses_is_told() {
    for (error, status) in [
        (Error::Full, 507),
        (Error::Busy, 503),
        (Error::Unsupported, 501),
        (Error::Denied, 403),
        (Error::Invalid, 422),
        (Error::Io, 500),
    ] {
        let mut rig = Rig::new();
        assert_eq!(rig.offer(&offer_text(None, &[("a.epub", b"a")])), 202);
        rig.allow();
        rig.host.import_error = Some(error);
        assert_eq!(
            rig.status("PUT", "/api/book/a.epub", Some(b"a")),
            status,
            "{error:?}"
        );
        // Not taken: it may be sent again.
        rig.host.import_error = None;
        assert_eq!(rig.status("PUT", "/api/book/a.epub", Some(b"a")), 200);
    }
}

#[test]
fn what_is_not_a_request_of_the_cable_is_not_found_once_allowed() {
    let mut rig = allowed_plugin();
    for (method, target) in [
        ("GET", "/api/file?path=a"),
        ("DELETE", "/api/plugin/airy"),
        ("GET", "/api/plugin/airy"),
        ("POST", "/api/book/a.epub"),
        ("PUT", "/api/offer"),
        ("GET", "/"),
    ] {
        assert_eq!(
            rig.status(method, target, Some(b"x")),
            404,
            "{method} {target}"
        );
    }
}

#[test]
fn closing_the_screen_ends_the_permission_and_what_was_sent() {
    let mut rig = allowed_plugin();
    rig.put("airy", "plugin.ini", INI);
    rig.cable.close(&mut rig.host);
    assert_eq!(rig.cable.permission(), Permission::None);
    assert!(rig.host.staged.is_empty());
    assert_eq!(rig.put("airy", "plugin.ini", INI), 403);
    // An install the host asks about when the screen closes is let go too.
    let mut rig = allowed_plugin();
    rig.put("airy", "plugin.ini", INI);
    rig.put("airy", "i18n/de.ini", DE);
    assert_eq!(rig.ask("POST", "/api/plugin/airy", None), None);
    let request = rig.host.installs[0].0;
    rig.cable.close(&mut rig.host);
    assert!(!rig.cable.waiting());
    let ended = rig
        .cable
        .install_done(&mut rig.host, request, Ok(false))
        .unwrap();
    assert_eq!(ended, None);
}

#[test]
fn the_digest_a_request_names_is_checked_when_it_names_one() {
    let sha = sha256::digest(b"a");
    assert!(same_digest("", &sha));
    assert!(same_digest("x=1", &sha));
    assert!(same_digest(&format!("sha256={}", hex(b"a")), &sha));
    assert!(same_digest(
        &format!("sha256={}", hex(b"a").to_uppercase()),
        &sha
    ));
    assert!(same_digest(&format!("k=1&sha256={}", hex(b"a")), &sha));
    assert!(!same_digest(&format!("sha256={}", hex(b"b")), &sha));
    assert!(!same_digest("sha256=zz", &sha));
    assert!(!same_digest("sha256=", &sha));
}

/// The plugin flow, each allocation in turn refused: it ends in `NoMemory` or
/// well, never in an abort.
#[test]
fn a_refused_allocation_fails_the_request_not_the_plugin() {
    let total = sweep(|| {
        let (mut cable, mut host) = exempt(|| (Cable::new(), Fake::new()));
        let mut number = 0;
        let mut send = |cable: &mut Cable,
                        host: &mut Fake,
                        method: &str,
                        target: &str,
                        body: Option<&[u8]>|
         -> Result<Answer, Error> {
            number += 1;
            let path = exempt(|| {
                let path = format!(".received/3/{number}");
                if let Some(bytes) = body {
                    host.bodies.push((path.clone(), bytes.to_vec()));
                }
                path
            });
            let call = Call {
                number,
                method,
                target,
                body: body.map(|_| path.as_str()),
                body_len: body.map_or(0, |bytes| bytes.len() as u64),
            };
            cable.handle(host, &call)
        };
        let text = exempt(plugin_offer);
        send(
            &mut cable,
            &mut host,
            "POST",
            "/api/offer",
            Some(text.as_bytes()),
        )?;
        let request = host.asked[0].0;
        cable.answered(&mut host, request, true);
        for (path, data) in [("plugin.ini", INI), ("i18n/de.ini", DE)] {
            let target = exempt(|| format!("/api/plugin/airy/{path}?sha256={}", hex(data)));
            send(&mut cable, &mut host, "PUT", &target, Some(data))?;
        }
        let first = send(&mut cable, &mut host, "POST", "/api/plugin/airy", None)?;
        assert_eq!(first, Answer::Later);
        let request = host.installs[0].0;
        let ended = cable.install_done(&mut host, request, Ok(false))?;
        assert_eq!(ended.unwrap().1.status, 200);
        Ok(())
    });
    assert!(total > 10, "{total}");
}

#[test]
fn a_refused_allocation_fails_a_book_or_an_offer_not_the_plugin() {
    sweep(|| {
        let (mut cable, mut host) = exempt(|| (Cable::new(), Fake::new()));
        let text = exempt(|| offer_text(None, &[("My Book.epub", b"one"), ("b.epub", b"two")]));
        exempt(|| host.bodies.push((String::from("offer"), text.into_bytes())));
        let call = Call {
            number: 1,
            method: "POST",
            target: "/api/offer",
            body: Some("offer"),
            body_len: 0,
        };
        cable.handle(&mut host, &call)?;
        let request = host.asked[0].0;
        cable.answered(&mut host, request, true);
        exempt(|| host.bodies.push((String::from("book"), b"one".to_vec())));
        let target = exempt(|| format!("/api/book/My%20Book.epub?sha256={}", hex(b"one")));
        let call = Call {
            number: 2,
            method: "PUT",
            target: &target,
            body: Some("book"),
            body_len: 3,
        };
        let Answer::Now(reply) = cable.handle(&mut host, &call)? else {
            panic!("a book is answered at once");
        };
        assert_eq!(reply.status, 200);
        cable.close(&mut host);
        Ok(())
    });
    // The offer's file alone, and the staged files' check.
    let text = offer_text(
        Some(("airy", &[("plugin.ini", INI)])),
        &[("a b.epub", b"x")],
    );
    let bodies = vec![(String::from("offer"), text.into_bytes())];
    sweep(|| Ok(read_offer(&Bodies(&bodies), "offer")?.unwrap()));
    let files = [(String::from("a.txt"), 1, sha256::digest(b"a"))];
    sweep(|| {
        let mut host = exempt(Fake::new);
        exempt(|| {
            host.staged
                .push((String::from("p"), String::from("a.txt"), b"a".to_vec()))
        });
        assert!(staged_as_offered(&mut host, "p", &files)?);
        Ok(())
    });
}

//! Plugins sent from the computer, over a fake host.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use alloc::string::ToString;
use alloc::vec;
use paperpaw_sdk::Instant;
use paperpaw_sdk::digest::sha256;

/// A host that keeps what was staged and what it was asked.
pub(crate) struct Fake {
    known: Known,
    /// The staged files by plugin and path, with what they hold.
    pub(crate) staged: Vec<(String, String, Vec<u8>)>,
    /// The bodies of the data folder.
    bodies: Vec<(String, Vec<u8>)>,
    pub(crate) discarded: Vec<String>,
    pub(crate) cleared: Vec<String>,
    /// What the host answers to an install command.
    install: Result<(), Error>,
    pub(crate) asked: Vec<(u32, String, bool)>,
    pub(crate) listings: u32,
    storage_full: bool,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            known: Known::Absent,
            staged: Vec::new(),
            bodies: Vec::new(),
            discarded: Vec::new(),
            cleared: Vec::new(),
            install: Ok(()),
            asked: Vec::new(),
            listings: 0,
            storage_full: false,
        }
    }
}

impl Plugins for Fake {
    fn known(&mut self, _: &str) -> Result<Known, Error> {
        Ok(self.known)
    }
    fn write_listing(&mut self) -> Result<(), Error> {
        self.listings += 1;
        Ok(())
    }
    fn stage(
        &mut self,
        id: &str,
        path: &str,
        body: &str,
        sha256: &[u8; 32],
    ) -> Result<Staged, Error> {
        let at = self.bodies.iter().position(|(name, _)| name == body);
        let Some(at) = at else {
            return Ok(Staged::Storage);
        };
        let (_, bytes) = self.bodies.remove(at);
        if paperpaw_sdk::digest::sha256::digest(&bytes) != *sha256 {
            return Ok(Staged::Damaged);
        }
        if self.storage_full {
            return Ok(Staged::Storage);
        }
        self.staged
            .retain(|(plugin, file, _)| !(plugin == id && file == path));
        self.staged.push((id.to_string(), path.to_string(), bytes));
        Ok(Staged::Taken)
    }
    fn discard(&mut self, path: &str) {
        self.discarded.push(path.to_string());
        self.bodies.retain(|(name, _)| name != path);
    }
    fn clear(&mut self, id: &str) {
        self.cleared.push(id.to_string());
        self.staged.retain(|(plugin, ..)| plugin != id);
    }
    fn install(&mut self, request: u32, id: &str, update: bool) -> Result<(), Error> {
        self.asked.push((request, id.to_string(), update));
        self.install
    }
}

fn hex(bytes: &[u8]) -> String {
    sha256::hex(bytes)
}

fn request<'a>(method: &'a str, target: &'a str, body: Option<&'a str>) -> Request<'a> {
    Request {
        method,
        target,
        body,
        body_len: 0,
        at: Instant(0),
    }
}

/// Runs one request with the path and query split as `native_web` does.
fn run(
    installs: &mut Installs,
    host: &mut Fake,
    method: &str,
    target: &str,
    body: Option<(&str, &[u8])>,
) -> Answer {
    if let Some((name, bytes)) = body {
        host.bodies.push((name.to_string(), bytes.to_vec()));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let asked = request(method, target, body.map(|(name, _)| name));
    installs.answer(host, &asked, path, query).unwrap()
}

fn text(answer: &Answer) -> &str {
    match &answer.body {
        Body::Owned(text) => text,
        Body::Text(text) => text,
        other => panic!("{other:?}"),
    }
}

fn put(installs: &mut Installs, host: &mut Fake, path: &str, bytes: &[u8]) -> Answer {
    let target = alloc::format!("/api/plugin/dark/{path}?sha256={}", hex(bytes));
    run(
        installs,
        host,
        "PUT",
        &target,
        Some((".received/1/7", bytes)),
    )
}

#[test]
fn the_list_of_the_plugins_is_a_file_the_host_writes() {
    let (mut installs, mut host) = (Installs::default(), Fake::default());
    let answer = run(&mut installs, &mut host, "GET", "/api/plugins?k=x", None);
    assert_eq!(
        (answer.status, answer.body),
        (200, Body::Data(LISTING_FILE))
    );
    assert_eq!(host.listings, 1);
}

#[test]
fn a_file_is_staged_only_as_its_sha256_says() {
    let (mut installs, mut host) = (Installs::default(), Fake::default());
    let answer = put(
        &mut installs,
        &mut host,
        "plugin.ini",
        b"[plugin]\nid = dark\n",
    );
    assert_eq!((answer.status, text(&answer)), (200, "{\"ok\":true}"));
    assert_eq!(host.staged.len(), 1);
    // Changed on the way: refused, and what was sent before stays.
    let wrong = alloc::format!(
        "/api/plugin/dark/plugin.ini?sha256={}",
        hex(b"what was sent")
    );
    let answer = run(
        &mut installs,
        &mut host,
        "PUT",
        &wrong,
        Some((".received/1/8", b"changed")),
    );
    assert_eq!(answer.status, 422);
    assert_eq!(text(&answer), "{\"state\":\"refused\",\"why\":\"damaged\"}");
    assert_eq!(host.staged[0].2, b"[plugin]\nid = dark\n");
    // The body is gone either way.
    assert!(host.bodies.is_empty());
    assert!(host.discarded.contains(&".received/1/8".to_string()));
}

#[test]
fn names_that_leave_the_folder_or_have_no_digest_are_refused() {
    let (mut installs, mut host) = (Installs::default(), Fake::default());
    for path in ["../escape", ".hidden", "a//b", "a b"] {
        let answer = put(&mut installs, &mut host, path, b"x");
        assert_eq!(answer.status, 422, "{path}");
    }
    let answer = run(
        &mut installs,
        &mut host,
        "PUT",
        "/api/plugin/dark/a.ini",
        Some((".received/1/9", b"x")),
    );
    assert_eq!(text(&answer), "{\"why\":\"sha256\"}");
    assert_eq!(answer.status, 400);
    let answer = run(&mut installs, &mut host, "GET", "/api/plugin/Dark", None);
    assert_eq!(answer.status, 400);
    assert!(host.staged.is_empty());
}

#[test]
fn a_plugin_has_at_most_so_many_files() {
    let (mut installs, mut host) = (Installs::default(), Fake::default());
    for n in 0..MAX_FILES {
        let path = alloc::format!("f{n}.txt");
        assert_eq!(put(&mut installs, &mut host, &path, b"x").status, 200);
    }
    assert_eq!(put(&mut installs, &mut host, "more.txt", b"x").status, 422);
    // A file sent again replaces its own.
    assert_eq!(put(&mut installs, &mut host, "f0.txt", b"x").status, 200);
}

#[test]
fn a_full_card_is_a_refusal_of_the_file() {
    let (mut installs, mut host) = (Installs::default(), Fake::default());
    host.storage_full = true;
    let answer = put(&mut installs, &mut host, "a.ini", b"x");
    assert_eq!(text(&answer), "{\"state\":\"refused\",\"why\":\"storage\"}");
}

#[test]
fn the_install_asks_the_host_and_waits_for_the_reader() {
    let (mut installs, mut host) = (Installs::default(), Fake::default());
    put(&mut installs, &mut host, "plugin.ini", b"x");
    let answer = run(&mut installs, &mut host, "POST", "/api/plugin/dark", None);
    assert_eq!(
        (answer.status, text(&answer)),
        (202, "{\"state\":\"asking\"}")
    );
    let (request, id, update) = host.asked[0].clone();
    assert_eq!((id.as_str(), update), ("dark", false));
    // Asked again while it shows: the same answer, no second question.
    let answer = run(&mut installs, &mut host, "POST", "/api/plugin/dark", None);
    assert_eq!(answer.status, 202);
    assert_eq!(host.asked.len(), 1);
    let answer = run(&mut installs, &mut host, "GET", "/api/plugin/dark", None);
    assert_eq!(text(&answer), "{\"state\":\"asking\"}");
    // Files cannot be put or dropped while the host copies them.
    assert_eq!(put(&mut installs, &mut host, "b.ini", b"y").status, 409);
    let answer = run(&mut installs, &mut host, "DELETE", "/api/plugin/dark", None);
    assert_eq!(answer.status, 409);
    // The reader said yes.
    let ended = installs.ended(&mut host, request, Ok(false)).unwrap();
    assert_eq!((ended.id.as_str(), ended.result), ("dark", Ok(false)));
    let answer = run(&mut installs, &mut host, "GET", "/api/plugin/dark", None);
    assert_eq!(
        (answer.status, text(&answer)),
        (200, "{\"state\":\"installed\",\"restart\":false}")
    );
    assert!(host.staged.is_empty());
    // An end nobody asked for is not this plugin's.
    assert_eq!(installs.ended(&mut host, request, Ok(false)), None);
}

#[test]
fn code_runs_after_a_restart_and_an_update_says_so() {
    let (mut installs, mut host) = (Installs::default(), Fake::default());
    host.known = Known::Installed;
    put(&mut installs, &mut host, "plugin.ini", b"x");
    run(&mut installs, &mut host, "POST", "/api/plugin/dark", None);
    let (request, _, update) = host.asked[0].clone();
    assert!(update);
    installs.ended(&mut host, request, Ok(true)).unwrap();
    let answer = run(&mut installs, &mut host, "GET", "/api/plugin/dark", None);
    assert_eq!(text(&answer), "{\"state\":\"installed\",\"restart\":true}");
}

#[test]
fn a_refusal_by_the_reader_or_the_host_is_the_state() {
    let (mut installs, mut host) = (Installs::default(), Fake::default());
    put(&mut installs, &mut host, "plugin.ini", b"x");
    run(&mut installs, &mut host, "POST", "/api/plugin/dark", None);
    let (request, ..) = host.asked[0].clone();
    installs
        .ended(
            &mut host,
            request,
            Err(why(Err(Error::Denied), DeviceOutcome::Declined)),
        )
        .unwrap();
    let answer = run(&mut installs, &mut host, "GET", "/api/plugin/dark", None);
    assert_eq!(answer.status, 422);
    assert_eq!(
        text(&answer),
        "{\"state\":\"refused\",\"why\":\"not approved\"}"
    );
    // Denied and Unsupported at the command are the answer too.
    for (error, said) in [
        (Error::Denied, "not allowed"),
        (Error::Unsupported, "not supported"),
    ] {
        let (mut installs, mut host) = (Installs::default(), Fake::default());
        host.install = Err(error);
        put(&mut installs, &mut host, "plugin.ini", b"x");
        let answer = run(&mut installs, &mut host, "POST", "/api/plugin/dark", None);
        assert_eq!(answer.status, 422);
        assert!(text(&answer).contains(said), "{}", text(&answer));
        assert!(host.staged.is_empty());
    }
}

#[test]
fn nothing_staged_and_a_built_in_plugin_are_not_asked_about() {
    let (mut installs, mut host) = (Installs::default(), Fake::default());
    let answer = run(&mut installs, &mut host, "POST", "/api/plugin/dark", None);
    assert_eq!(
        text(&answer),
        "{\"state\":\"refused\",\"why\":\"not the plugin\"}"
    );
    host.known = Known::BuiltIn;
    put(&mut installs, &mut host, "plugin.ini", b"x");
    let answer = run(&mut installs, &mut host, "POST", "/api/plugin/dark", None);
    assert_eq!(
        text(&answer),
        "{\"state\":\"refused\",\"why\":\"built in\"}"
    );
    assert!(host.asked.is_empty());
}

#[test]
fn a_busy_host_leaves_the_files_for_another_try() {
    let (mut installs, mut host) = (Installs::default(), Fake::default());
    host.install = Err(Error::Full);
    put(&mut installs, &mut host, "plugin.ini", b"x");
    let answer = run(&mut installs, &mut host, "POST", "/api/plugin/dark", None);
    assert_eq!(answer.status, 409);
    assert_eq!(host.staged.len(), 1);
    host.install = Ok(());
    let answer = run(&mut installs, &mut host, "POST", "/api/plugin/dark", None);
    assert_eq!(answer.status, 202);
}

#[test]
fn only_one_plugin_is_asked_about_at_a_time() {
    let (mut installs, mut host) = (Installs::default(), Fake::default());
    put(&mut installs, &mut host, "plugin.ini", b"x");
    run(&mut installs, &mut host, "POST", "/api/plugin/dark", None);
    let target = alloc::format!("/api/plugin/other/plugin.ini?sha256={}", hex(b"y"));
    run(
        &mut installs,
        &mut host,
        "PUT",
        &target,
        Some((".received/1/3", b"y")),
    );
    let answer = run(&mut installs, &mut host, "POST", "/api/plugin/other", None);
    assert_eq!(answer.status, 409);
    assert_eq!(host.asked.len(), 1);
}

#[test]
fn delete_drops_the_files_and_the_state() {
    let (mut installs, mut host) = (Installs::default(), Fake::default());
    put(&mut installs, &mut host, "plugin.ini", b"x");
    let answer = run(&mut installs, &mut host, "DELETE", "/api/plugin/dark", None);
    assert_eq!(answer.status, 200);
    assert!(host.staged.is_empty());
    let answer = run(&mut installs, &mut host, "GET", "/api/plugin/dark", None);
    assert_eq!(text(&answer), "{\"state\":\"none\"}");
    let answer = run(&mut installs, &mut host, "PATCH", "/api/plugin/dark", None);
    assert_eq!(answer.status, 404);
}

#[test]
fn at_most_a_few_plugins_are_kept_and_idle_ones_make_room() {
    let (mut installs, mut host) = (Installs::default(), Fake::default());
    for id in ["a", "b", "c", "d", "e", "f"] {
        let target = alloc::format!("/api/plugin/{id}/x.ini?sha256={}", hex(b"y"));
        let answer = run(
            &mut installs,
            &mut host,
            "PUT",
            &target,
            Some((".received/1/3", b"y")),
        );
        assert_eq!(answer.status, 200, "{id}");
    }
    assert!(installs.entries.len() <= MOST_PLUGINS);
}

#[test]
fn the_screen_closing_forgets_what_is_not_asked_about() {
    let (mut installs, mut host) = (Installs::default(), Fake::default());
    put(&mut installs, &mut host, "plugin.ini", b"x");
    installs.forget_idle();
    assert!(!installs.is_asking("dark"));
    assert!(installs.entries.is_empty());
}

#[test]
fn the_end_the_host_sent_becomes_a_word() {
    assert_eq!(why(Ok(()), DeviceOutcome::Declined), "not approved");
    assert_eq!(
        why(Err(Error::Denied), DeviceOutcome::Dismissed),
        "not approved"
    );
    assert_eq!(why(Err(Error::Invalid), DeviceOutcome::Damaged), "damaged");
    assert_eq!(
        why(Err(Error::Denied), DeviceOutcome::Unsigned),
        "not trusted"
    );
    assert_eq!(
        why(Err(Error::Unsupported), DeviceOutcome::OlderInterface),
        "built for another PaperPaw"
    );
    assert_eq!(why(Err(Error::NoSpace), DeviceOutcome::NoRoom), "no room");
    assert_eq!(why(Err(Error::Denied), DeviceOutcome::IdTaken), "built in");
    assert_eq!(why(Err(Error::Denied), DeviceOutcome::Ok), "not allowed");
    assert_eq!(why(Err(Error::Io), DeviceOutcome::Other), "refused");
    let _ = vec![0u8; 0];
}

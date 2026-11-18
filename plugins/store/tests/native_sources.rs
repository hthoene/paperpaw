//! Community plugin sources (#214): the sources kept, trust on first use, one
//! source owning a plugin, PaperPaw's own key and library, the size of a
//! source's list; against a scripted host.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use paperpaw_plugin_store::native_device::Facts;
use paperpaw_plugin_store::native_ed25519;
use paperpaw_plugin_store::native_index::{self, MAX_LIST_FILE};
use paperpaw_plugin_store::native_item::FIRST_PARTY_KEY;
use paperpaw_plugin_store::native_load::{
    Done, Ended, LIST_PART, LIST_TOKEN, Loader, SIGNATURE_TOKEN, Trouble, Wire,
};
use paperpaw_plugin_store::native_model::{OFFICIAL_LIST, Revoked};
use paperpaw_plugin_store::native_page::{Row, of_source};
use paperpaw_plugin_store::native_sources::{
    Added, Gate, MAX_SIGNATURE_FILE, MAX_SOURCE_LIST, MAX_SOURCES, Origins, Pinned, SIGNATURE_PART,
    SOURCES, Signed, SourceFiles, Sources, Trust, Verdict, address, foreign, gate, is_official,
    judge, message, name, names_code, signature_url,
};
use paperpaw_sdk::digest::{hex, sha256};
use paperpaw_sdk::native::{
    Error,
    net::{Reason, RequestId},
    signature::{public_key, sign_message},
};
use paperpaw_sdk::{MemoryStorage, Storage};

const SOURCE: &str = "https://src.example/plugins.json";
const SEED: [u8; 32] = [3; 32];
const OTHER_SEED: [u8; 32] = [4; 32];

const LIST: &str = r#"{"format": 2, "plugins": [{"id": "plugin-a", "name": "A", "summary": "Does a well.", "group": "tools",
 "kind": "data", "author": "X", "licence": "MIT", "added": "2024-01-02",
 "latest": {"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 30, "date": "2024-01-02"}}]}"#;

/// A source's signature file for `list`, signed with `seed`.
fn signature_file(list: &[u8], seed: &[u8; 32]) -> String {
    let signature = sign_message(&message(&sha256::digest(list)), seed);
    format!(
        "{}\n{}\n",
        hex::encode(&public_key(seed)),
        hex::encode(&signature)
    )
}

struct Script {
    storage: MemoryStorage,
    trust: Trust,
    /// What was asked for: the address, the file, the token, the most bytes.
    asked: Vec<(String, String, u32, u64)>,
    pinned: Vec<[u8; 32]>,
}

impl Script {
    fn new(trust: Trust) -> Self {
        Self {
            storage: MemoryStorage::new(),
            trust,
            asked: Vec::new(),
            pinned: Vec::new(),
        }
    }
}

impl Wire for Script {
    fn storage(&self) -> &dyn Storage {
        &self.storage
    }
    fn facts(&mut self) -> Result<Option<Facts>, Error> {
        let official = self.trust == Trust::Official;
        Ok(Some(
            Facts::new("x4", "code.ppn", "en", official, Vec::new()).unwrap(),
        ))
    }
    fn catalog(&mut self) -> Result<(String, bool), Error> {
        Ok((SOURCE.into(), false))
    }
    fn get(&mut self, url: &str, file: &str, token: u32, max: u64) -> Result<RequestId, Error> {
        self.asked.push((url.into(), file.into(), token, max));
        Ok(RequestId::new(7).unwrap())
    }
    fn cancel(&mut self, _token: u32) {}
    fn final_url(&mut self, _id: RequestId) -> Option<String> {
        None
    }
    fn small_memory(&self) -> bool {
        true
    }
    fn revoke(&mut self, _revoked: &[Revoked]) -> Result<(), Error> {
        Ok(())
    }
    fn trust(&mut self) -> Result<Trust, Error> {
        Ok(self.trust)
    }
    fn pin(&mut self, key: &[u8; 32]) -> Result<(), Error> {
        if self.trust != Trust::FirstUse || *key == FIRST_PARTY_KEY {
            return Err(Error::Denied);
        }
        self.pinned.push(*key);
        self.trust = Trust::Pinned(*key);
        Ok(())
    }
}

fn done(token: u32, reason: Reason, status: u32) -> Done {
    Done::new(RequestId::new(7).unwrap(), token, reason, status)
}

/// Fetches `list` with the signature file `signature` (`None`: the source has
/// none, 404) and says what the fetch came to.
fn fetch(
    wire: &mut Script,
    loader: &mut Loader,
    list: &[u8],
    signature: Option<&str>,
) -> (Ended, Trouble) {
    loader.start_list(wire).unwrap();
    wire.storage.write(LIST_PART, list).unwrap();
    let ended = loader
        .ended(wire, &done(LIST_TOKEN, Reason::Ok, 200))
        .unwrap();
    if ended != Ended::Ignored {
        return (ended, loader.trouble());
    }
    let status = match signature {
        Some(text) => {
            wire.storage.write(SIGNATURE_PART, text.as_bytes()).unwrap();
            200
        }
        None => 404,
    };
    let ended = loader
        .ended(wire, &done(SIGNATURE_TOKEN, Reason::Ok, status))
        .unwrap();
    (ended, loader.trouble())
}

#[test]
fn the_first_list_of_a_source_pins_its_key() {
    let mut wire = Script::new(Trust::FirstUse);
    let mut loader = Loader::default();
    let signature = signature_file(LIST.as_bytes(), &SEED);
    let (ended, trouble) = fetch(&mut wire, &mut loader, LIST.as_bytes(), Some(&signature));
    assert_eq!((ended, trouble), (Ended::ListChanged, Trouble::None));
    assert_eq!(wire.pinned, [public_key(&SEED)]);
    assert_eq!(loader.index().unwrap().len(), 1);
    // The list is asked for with the source's limit, then its signature
    // beside it.
    let (url, file, token, max) = wire.asked[0].clone();
    assert!(url.starts_with(SOURCE), "{url}");
    assert_eq!(
        (file.as_str(), token, max),
        (LIST_PART, LIST_TOKEN, MAX_SOURCE_LIST)
    );
    let (url, file, token, max) = wire.asked[1].clone();
    assert!(
        url.starts_with("https://src.example/plugins.json.sig?device=x4"),
        "{url}"
    );
    assert_eq!(
        (file.as_str(), token, max),
        (SIGNATURE_PART, SIGNATURE_TOKEN, MAX_SIGNATURE_FILE)
    );
    assert!(!wire.storage.exists(SIGNATURE_PART));
    // The same key again reads the next list and pins nothing anew.
    let (ended, _) = fetch(&mut wire, &mut loader, LIST.as_bytes(), Some(&signature));
    assert_eq!(ended, Ended::ListChanged);
    assert_eq!(wire.pinned.len(), 1);
}

#[test]
fn a_list_signed_by_another_key_is_refused() {
    let mut wire = Script::new(Trust::FirstUse);
    let mut loader = Loader::default();
    let first = signature_file(LIST.as_bytes(), &SEED);
    fetch(&mut wire, &mut loader, LIST.as_bytes(), Some(&first));
    assert!(loader.index().is_some());
    // The source's server now sends a list another key signed, rightly so by
    // that key: it is not read, and the list read before does not serve.
    let other = signature_file(LIST.as_bytes(), &OTHER_SEED);
    let (ended, trouble) = fetch(&mut wire, &mut loader, LIST.as_bytes(), Some(&other));
    assert_eq!(
        (ended, trouble),
        (Ended::ListUnchanged, Trouble::KeyChanged)
    );
    assert!(loader.index().is_none());
    assert!(!wire.storage.exists(native_index::HEAD));
    assert!(!wire.storage.exists(LIST_PART));
    // The key kept is the first one still.
    assert_eq!(wire.trust, Trust::Pinned(public_key(&SEED)));
    assert_eq!(wire.pinned.len(), 1);
    assert_eq!(
        judge(
            Trust::Pinned(public_key(&SEED)),
            Signed::read(&other).as_ref(),
            &sha256::digest(LIST.as_bytes())
        ),
        Verdict::KeyChanged
    );
}

#[test]
fn a_list_without_a_signature_that_holds_is_refused() {
    let signature = signature_file(LIST.as_bytes(), &SEED);
    let changed = LIST.replace("plugin-a", "plugin-b");
    for (list, signature) in [
        // No signature file.
        (LIST, None),
        // Not a signature file.
        (LIST, Some("<html>not found</html>")),
        // The list was changed after it was signed.
        (changed.as_str(), Some(signature.as_str())),
    ] {
        for trust in [Trust::FirstUse, Trust::Pinned(public_key(&SEED))] {
            let mut wire = Script::new(trust);
            let mut loader = Loader::default();
            let (ended, trouble) = fetch(&mut wire, &mut loader, list.as_bytes(), signature);
            assert_eq!((ended, trouble), (Ended::ListUnchanged, Trouble::Unsigned));
            assert!(loader.index().is_none());
            assert!(wire.pinned.is_empty(), "nothing is pinned");
        }
    }
    // No answer at all for the signature is no connection, not a refusal.
    let mut wire = Script::new(Trust::Pinned(public_key(&SEED)));
    let mut loader = Loader::default();
    loader.start_list(&mut wire).unwrap();
    wire.storage.write(LIST_PART, LIST.as_bytes()).unwrap();
    loader
        .ended(&mut wire, &done(LIST_TOKEN, Reason::Ok, 200))
        .unwrap();
    loader
        .ended(&mut wire, &done(SIGNATURE_TOKEN, Reason::NoNetwork, 0))
        .unwrap();
    assert_eq!(loader.trouble(), Trouble::Offline);
}

/// A store that is a plugin file (the X4's, and the X4 Pro's and X4 Classic's
/// on the S3) checks a source's signature with its own Ed25519, whose frames
/// fit a file (#320): a store built without `linked`, as this one is by
/// default, judges a list with it.
#[test]
fn a_store_packed_as_a_file_reads_a_signed_list_and_refuses_a_forged_one() {
    let key = public_key(&SEED);
    let signature = signature_file(LIST.as_bytes(), &SEED);
    let signed = Signed::read(&signature).unwrap();
    let digest = sha256::digest(LIST.as_bytes());
    assert!(native_ed25519::verify(
        &key,
        &message(&digest),
        &signed.signature
    ));
    let mut wire = Script::new(Trust::FirstUse);
    let mut loader = Loader::default();
    let (ended, trouble) = fetch(&mut wire, &mut loader, LIST.as_bytes(), Some(&signature));
    assert_eq!((ended, trouble), (Ended::ListChanged, Trouble::None));
    assert_eq!(loader.index().unwrap().len(), 1);
    assert_eq!(wire.pinned, [key]);

    // Forged: another list under the signature, a signature with one bit
    // changed, and one by another key that names the source's key.
    let forged = LIST.replace("plugin-a", "plugin-b");
    let forged_digest = sha256::digest(forged.as_bytes());
    assert!(!native_ed25519::verify(
        &key,
        &message(&forged_digest),
        &signed.signature
    ));
    let mut bent = signed;
    bent.signature[40] ^= 1;
    assert!(!native_ed25519::verify(
        &key,
        &message(&digest),
        &bent.signature
    ));
    let others = Signed {
        key,
        signature: sign_message(&message(&digest), &OTHER_SEED),
    };
    let as_file = |signed: &Signed| {
        format!(
            "{}\n{}\n",
            hex::encode(&signed.key),
            hex::encode(&signed.signature)
        )
    };
    for (list, signature) in [
        (forged.as_str(), signature.clone()),
        (LIST, as_file(&bent)),
        (LIST, as_file(&others)),
    ] {
        for trust in [Trust::FirstUse, Trust::Pinned(key)] {
            assert_eq!(
                judge(
                    trust,
                    Signed::read(&signature).as_ref(),
                    &sha256::digest(list.as_bytes())
                ),
                Verdict::Unsigned
            );
            let mut wire = Script::new(trust);
            let mut loader = Loader::default();
            let (ended, trouble) = fetch(&mut wire, &mut loader, list.as_bytes(), Some(&signature));
            assert_eq!((ended, trouble), (Ended::ListUnchanged, Trouble::Unsigned));
            assert!(loader.index().is_none());
            assert!(wire.pinned.is_empty());
        }
    }
}

#[test]
fn an_oversize_list_is_refused() {
    let mut wire = Script::new(Trust::Pinned(public_key(&SEED)));
    let mut loader = Loader::default();
    // One byte over the limit, rightly signed: not read, and its signature
    // is not even asked for.
    let mut list = LIST.as_bytes().to_vec();
    list.resize(usize::try_from(MAX_SOURCE_LIST).unwrap() + 1, b' ');
    let signature = signature_file(&list, &SEED);
    let (ended, trouble) = fetch(&mut wire, &mut loader, &list, Some(&signature));
    assert_eq!((ended, trouble), (Ended::ListUnchanged, Trouble::TooLarge));
    assert_eq!(wire.asked.len(), 1, "only the list was asked for");
    assert_eq!(wire.asked[0].3, MAX_SOURCE_LIST);
    assert!(loader.index().is_none());
    assert!(!wire.storage.exists(LIST_PART));
    // The host stopping the answer at the limit says the same.
    let mut loader = Loader::default();
    loader.start_list(&mut wire).unwrap();
    loader
        .ended(&mut wire, &done(LIST_TOKEN, Reason::TooLarge, 200))
        .unwrap();
    assert_eq!(loader.trouble(), Trouble::TooLarge);
    // A list at the limit is read.
    list.truncate(usize::try_from(MAX_SOURCE_LIST).unwrap());
    let signature = signature_file(&list, &SEED);
    let (ended, _) = fetch(&mut wire, &mut loader, &list, Some(&signature));
    assert_eq!(ended, Ended::ListChanged);
    // PaperPaw's own library keeps its own limit and asks for no signature.
    let mut wire = Script::new(Trust::Official);
    let mut loader = Loader::default();
    let (ended, _) = fetch(&mut wire, &mut loader, LIST.as_bytes(), None);
    assert_eq!(ended, Ended::ListChanged);
    assert_eq!(wire.asked.len(), 1);
    assert_eq!(wire.asked[0].3, MAX_LIST_FILE);
}

#[test]
fn the_official_key_and_library_cannot_be_replaced() {
    let official_key = hex::encode(&FIRST_PARTY_KEY);
    let other_key = hex::encode(&public_key(&OTHER_SEED));
    // A sources file never makes PaperPaw's library a source with a key of
    // its own, nor PaperPaw's key the key of another source.
    let text = format!(
        "{OFFICIAL_LIST} {other_key}\n{OFFICIAL_LIST}?x=1\nhttps://evil.example/plugins.json {official_key}\n{SOURCE}\n"
    );
    let mut sources = Sources::parse(&text).unwrap();
    assert_eq!(sources.all().len(), 1);
    assert_eq!(sources.all()[0].url, SOURCE);
    assert!(is_official(
        "https://PaperPaw.org/api/v1/plugins.json?community=1"
    ));
    assert_eq!(sources.add(OFFICIAL_LIST).unwrap(), Added::Official);
    assert_eq!(
        sources
            .add("https://paperpaw.org/api/v1/plugins.json?device=x4")
            .unwrap(),
        Added::Official
    );
    // PaperPaw's key is pinned for no source, and a key pinned stays.
    assert_eq!(sources.pin(SOURCE, &FIRST_PARTY_KEY), Pinned::Official);
    assert_eq!(sources.all()[0].key, None);
    assert_eq!(sources.pin(SOURCE, &public_key(&SEED)), Pinned::Now);
    assert_eq!(sources.pin(SOURCE, &public_key(&OTHER_SEED)), Pinned::Other);
    assert_eq!(sources.pin(SOURCE, &public_key(&SEED)), Pinned::Already);
    assert_eq!(sources.all()[0].key, Some(public_key(&SEED)));
    // A list that names PaperPaw's key as a community source's is not read,
    // whatever it is signed with, and pins nothing.
    let signature = format!(
        "{official_key}\n{}\n",
        hex::encode(&sign_message(
            &message(&sha256::digest(LIST.as_bytes())),
            &SEED
        ))
    );
    assert_eq!(
        judge(
            Trust::FirstUse,
            Signed::read(&signature).as_ref(),
            &sha256::digest(LIST.as_bytes())
        ),
        Verdict::Official
    );
    let mut wire = Script::new(Trust::FirstUse);
    let mut loader = Loader::default();
    let (ended, trouble) = fetch(&mut wire, &mut loader, LIST.as_bytes(), Some(&signature));
    assert_eq!((ended, trouble), (Ended::ListUnchanged, Trouble::Unsigned));
    assert!(wire.pinned.is_empty());
    assert!(loader.index().is_none());
    // A plugin of PaperPaw's library is not updated from a community source.
    assert!(foreign(None, Some(true), Trust::Pinned(public_key(&SEED))));
}

#[test]
fn an_update_from_a_foreign_source_is_refused() {
    let ours = public_key(&SEED);
    let theirs = public_key(&OTHER_SEED);
    let storage = MemoryStorage::new();
    let mut origins = Origins::read(&storage).unwrap();
    assert!(origins.set("plugin-a", Some(&ours)).unwrap());
    assert!(!origins.set("plugin-a", Some(&ours)).unwrap());
    origins.keep(&storage).unwrap();
    let origins = Origins::read(&storage).unwrap();
    assert_eq!(origins.of("plugin-a"), Some(&ours));
    let recorded = origins.of("plugin-a");
    // Installed by the store from our source: only our source updates it.
    assert!(!foreign(recorded, Some(true), Trust::Pinned(ours)));
    assert!(foreign(recorded, Some(true), Trust::Pinned(theirs)));
    assert!(foreign(recorded, Some(true), Trust::Official));
    assert!(foreign(recorded, Some(true), Trust::FirstUse));
    // A plugin the store installed and no source is kept for is PaperPaw's
    // library's.
    assert!(!foreign(None, Some(true), Trust::Official));
    assert!(foreign(None, Some(true), Trust::Pinned(theirs)));
    // Not installed, or put in place by the reader: any source may.
    assert!(!foreign(recorded, None, Trust::Pinned(theirs)));
    assert!(!foreign(recorded, Some(false), Trust::Pinned(theirs)));
    // The install says no, and the page offers neither install nor update.
    assert_eq!(
        gate(Trust::Pinned(theirs), true, Some(&theirs), true),
        Gate::Foreign
    );
    assert_eq!(gate(Trust::Official, false, None, true), Gate::Foreign);
    assert_eq!(
        of_source(vec![Row::Update, Row::Remove, Row::How], true),
        [Row::Remove, Row::How]
    );
    assert_eq!(
        of_source(vec![Row::UseStore, Row::Remove], true),
        [Row::Remove]
    );
    assert_eq!(
        of_source(vec![Row::Update, Row::Remove], false),
        [Row::Update, Row::Remove]
    );
    // PaperPaw's library takes a plugin back only once it is no source's.
    let mut origins = origins;
    assert!(origins.set("plugin-a", None).unwrap());
    origins.keep(&storage).unwrap();
    assert!(!storage.exists("origins.txt"), "no plugin, no file");
}

#[test]
fn a_code_plugin_installs_only_with_the_sources_key() {
    let ours = public_key(&SEED);
    let theirs = public_key(&OTHER_SEED);
    assert_eq!(
        gate(Trust::Pinned(ours), true, Some(&ours), false),
        Gate::Open
    );
    assert_eq!(
        gate(Trust::Pinned(ours), true, Some(&theirs), false),
        Gate::OtherKey
    );
    assert_eq!(gate(Trust::Pinned(ours), true, None, false), Gate::OtherKey);
    // A data plugin has no code to sign.
    assert_eq!(gate(Trust::Pinned(ours), false, None, false), Gate::Open);
    // Nothing installs from a source whose key is not kept yet.
    assert_eq!(gate(Trust::FirstUse, false, None, false), Gate::NoKey);
    // PaperPaw's library: by the plugin's author, as before.
    assert_eq!(
        gate(Trust::Official, true, Some(&theirs), false),
        Gate::Open
    );
    // Code among the files of a page that says it is a data plugin's.
    assert!(names_code(["plugin.ini", "Code.PPN"]));
    assert!(names_code(["code.so"]));
    assert!(!names_code([
        "plugin.ini",
        "data/code.ppn.txt",
        "i18n/en.ini"
    ]));
}

#[test]
fn the_sources_file_is_read_as_written_by_hand_and_kept() {
    let key = hex::encode(&public_key(&SEED));
    let text = format!(
        "\u{feff}  https://a.example/plugins.json  \r\n# my sources\r\n\r\nhttp://plain.example/plugins.json\r\nhttps://b.example/p/plugins.json {key}\r\nhttps://a.example/plugins.json\r\nhttps://c.example/plugins.json nokey\r\nhttps://d.example/../plugins.json\r\nftp://e.example\r\n"
    );
    let sources = Sources::parse(&text).unwrap();
    let urls: Vec<&str> = sources.all().iter().map(|s| s.url.as_str()).collect();
    // The file begins with a byte-order mark and has a remark; plain http,
    // a second naming, a key that is none and a path with `..` are left out.
    assert_eq!(
        urls,
        [
            "https://a.example/plugins.json",
            "https://b.example/p/plugins.json"
        ]
    );
    assert_eq!(sources.all()[0].key, None);
    assert_eq!(sources.all()[1].key, Some(public_key(&SEED)));
    let storage = MemoryStorage::new();
    sources.keep(&storage).unwrap();
    assert_eq!(Sources::read(&storage).unwrap(), sources);
    // A cut between taking the old file away and moving the new one in
    // leaves the part: it is read.
    let kept = storage.get(SOURCES).unwrap();
    storage.remove(SOURCES).unwrap();
    storage.write("sources.part", &kept).unwrap();
    assert_eq!(Sources::read(&storage).unwrap(), sources);
    // Adding and removing; no more than the most sources.
    let mut sources = Sources::default();
    for n in 0..MAX_SOURCES {
        let url = format!("https://s{n}.example/plugins.json");
        assert_eq!(sources.add(&url).unwrap(), Added::New);
        assert_eq!(sources.add(&url).unwrap(), Added::Known);
    }
    assert_eq!(
        sources.add("https://more.example/plugins.json").unwrap(),
        Added::Full
    );
    assert_eq!(sources.add("not an address").unwrap(), Added::Invalid);
    assert!(sources.remove("https://s0.example/plugins.json"));
    assert!(!sources.remove("https://s0.example/plugins.json"));
    assert_eq!(sources.all().len(), MAX_SOURCES - 1);
    // No sources, no file.
    Sources::default().keep(&storage).unwrap();
    assert!(!storage.exists(SOURCES));
}

#[test]
fn addresses_and_names() {
    assert_eq!(
        address(" https://a.example/x.json "),
        Some("https://a.example/x.json")
    );
    assert_eq!(address("https:///x.json"), None);
    assert_eq!(address("https://a.example/a b"), None);
    assert_eq!(address("https://a.example/ä"), None);
    assert_eq!(
        address(&format!("https://a.example/{}", "x".repeat(200))),
        None
    );
    assert_eq!(
        name("https://a.example/p/plugins.json?key=1"),
        "a.example/p/plugins.json"
    );
    assert_eq!(name(&format!("https://{}", "x".repeat(80))).len(), 40);
    assert_eq!(
        signature_url("https://a.example/plugins.json?device=x4&lang=en").unwrap(),
        "https://a.example/plugins.json.sig?device=x4&lang=en"
    );
    assert_eq!(
        signature_url("https://a.example/plugins.json").unwrap(),
        "https://a.example/plugins.json.sig"
    );
}

#[test]
fn a_community_sources_list_never_writes_over_paperpaws() {
    let files = SourceFiles::new(MemoryStorage::new(), true);
    let storage = files.inner();
    storage.write(native_index::HEAD, b"official").unwrap();
    storage.write("queue.txt", b"queue").unwrap();
    files.write(native_index::HEAD, b"community").unwrap();
    files.write("index.txt.part", b"lines").unwrap();
    files.rename("index.txt.part", native_index::INDEX).unwrap();
    // The list fetched is where the host saves it, whichever source it is.
    files.write(LIST_PART, b"list").unwrap();
    assert_eq!(storage.get(native_index::HEAD).unwrap(), b"official");
    assert_eq!(storage.get("source-index.head").unwrap(), b"community");
    assert_eq!(storage.get("source-index.txt").unwrap(), b"lines");
    assert!(storage.exists(LIST_PART));
    assert_eq!(files.size(native_index::HEAD).unwrap(), 9);
    files.remove(native_index::HEAD).unwrap();
    assert!(!files.exists(native_index::HEAD));
    assert!(storage.exists(native_index::HEAD));
    // Every other file is the same one.
    let mut queue = [0; 5];
    assert_eq!(files.read_at("queue.txt", 0, &mut queue).unwrap(), 5);
    assert!(files.read_at(SOURCES, 0, &mut queue).is_err());
    // PaperPaw's library reads its own files.
    let official = SourceFiles::new(MemoryStorage::new(), false);
    official.write(native_index::HEAD, b"official").unwrap();
    assert_eq!(
        official.inner().get(native_index::HEAD).unwrap(),
        b"official"
    );
}

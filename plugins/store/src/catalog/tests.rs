use super::*;
use paperpaw_sdk::MemoryStorage;

const SHA1: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const KEY: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const FILES: [&str; 4] = ["i.txt", "i.bin", "i.head", "i.sections"];
/// The fingerprint of the X4's PaperPaw in these tests: of the plugin
/// interface of this SDK, as builds of this device are listed by it.
const FINGERPRINT: Fingerprint = Fingerprint::of(paperpaw_sdk::native::INTERFACE, 0x4567_89ab_cdef);
/// The X4 running a PaperPaw that loads code plugins of this build.
const X4: Device<'static> = Device {
    name: "x4",
    fingerprint: Some(FINGERPRINT),
    code_file: "code.ppn",
    plugins: None,
    sdk: paperpaw_sdk::SDK_VERSION,
    official: true,
    provides: None,
};

fn plugin(id: &str, group: &str, extra: &str) -> String {
    format!(
        r#"{{"id": "{id}", "name": "Name {id}", "summary": "Does {id}.", "group": "{group}",
  "kind": "data", "author": "A", "licence": "MIT", {extra}
  "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 30}}}}"#
    )
}

/// A build for `device` and `abi`.
/// The fingerprint of [`FINGERPRINT`], as a store's build names it.
fn abi() -> String {
    format!("{:016x}", FINGERPRINT.0)
}

fn build_of(device: &str, abi: &str) -> String {
    format!(
        r#"{{"device": "{device}", "abi": "{abi}", "url": "/b/{device}.ppn", "size": 70000, "sha256": "{SHA256}"}}"#
    )
}

/// A code plugin with `builds`.
fn code(id: &str, builds: &[String]) -> String {
    plugin(id, "reading", &format!(r#""author_key": "{KEY}","#))
        .replace(r#""kind": "data""#, r#""kind": "code""#)
        .replace(
            r#""size": 30}"#,
            &format!(r#""size": 30, "builds": [{}]}}"#, builds.join(",")),
        )
}

fn list(plugins: &[String]) -> MemoryStorage {
    let text = format!(r#"{{"format": 2, "plugins": [{}]}}"#, plugins.join(","));
    MemoryStorage::new().with("p.json", text)
}

/// Plugin `n` of the index built in `storage`.
fn entry(storage: &MemoryStorage, n: usize) -> Entry {
    let offset = number_at(storage, "i.bin", n).unwrap();
    Entry::parse(&line_at(storage, "i.txt", offset).unwrap()).unwrap()
}

/// The plugins of the list for `device` through the index, in its
/// order, and its runs of groups.
fn entries_for(storage: &MemoryStorage, device: &Device<'_>) -> (Vec<Entry>, Vec<(usize, usize)>) {
    assert!(build(storage, "p.json", FILES, device));
    let index = Index::open(storage, "i.head").unwrap();
    let entries = (0..index.len()).map(|n| entry(storage, n)).collect();
    (entries, index.runs)
}

fn entries(storage: &MemoryStorage) -> (Vec<Entry>, Vec<(usize, usize)>) {
    entries_for(storage, &X4)
}

fn ids(entries: &[Entry]) -> Vec<&str> {
    entries.iter().map(|e| e.id.as_str()).collect()
}

#[test]
fn only_what_fits_is_listed_in_its_order() {
    let later = format!(r#""formats": {}"#, paperpaw_sdk::DATA_FORMATS + 1);
    let storage = list(&[
        plugin("plain", "reading", ""),
        plugin("built", "reading", r#""built_in": true,"#),
        plugin("later", "look", "").replace(r#""formats": 1"#, &later),
        plugin("next-sdk", "look", "").replace(r#""sdk": "0.4""#, r#""sdk": "0.9""#),
        plugin("Bad_Id", "look", ""),
        plugin("phone", "look", r#""devices": ["android"],"#),
        plugin("theme", "look", r#""devices": ["x4", "x4-pro"],"#),
        plugin("odd", "elsewhere", ""),
    ]);
    let (entries, runs) = entries(&storage);
    assert_eq!(ids(&entries), ["plain", "theme", "odd"]);
    // Groups as they come, an unknown one under `system`, the last.
    assert_eq!(runs, [(0, 1), (5, 1), (12, 1)]);
    assert_eq!(entries[0].summary, "Does plain.");
    assert_eq!((entries[0].size, entries[0].code), (30, false));
}

/// PaperPaw's mark is kept with the index, and taken only where the
/// plugin's key is PaperPaw's own: never from the list's word alone.
#[test]
fn only_paperpaws_own_key_makes_a_plugin_official() {
    let fits = build_of("x4", &abi());
    let ours = "148ccf3ebc30506b355c6b5cb2baf87d9265606cc6a81f085bb3b9740a95aebd";
    let official =
        |plugin: String| plugin.replace(r#""author": "A","#, r#""author": "A", "official": true,"#);
    let storage = list(&[
        official(code("own", core::slice::from_ref(&fits)).replace(KEY, ours)),
        official(code("forged", core::slice::from_ref(&fits))),
        official(plugin("data", "reading", "")),
        code("unsaid", core::slice::from_ref(&fits)).replace(KEY, ours),
    ]);
    let (entries, _) = entries(&storage);
    let marked: Vec<(&str, bool)> = entries
        .iter()
        .map(|e| (e.id.as_str(), e.official))
        .collect();
    assert_eq!(
        marked,
        [
            ("own", true),
            ("forged", false),
            ("data", true),
            ("unsaid", false)
        ]
    );
    assert!(entries[0].code);
    // From another list, with PaperPaw's key and its word: no mark.
    let elsewhere = Device {
        official: false,
        ..X4
    };
    let forged = list(&[
        official(code("own", core::slice::from_ref(&fits)).replace(KEY, ours)),
        official(plugin(
            "data",
            "reading",
            &format!(r#""author_key": "{ours}","#),
        )),
    ]);
    let (entries, _) = entries_for(&forged, &elsewhere);
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|e| !e.official), "{entries:?}");
    // Lines an index kept before the mark read as they did.
    let old = "0\t30\t1\t0\t0\tclock\t1.0.0\t0.4\tA\tMIT\tClock\tTells the time.";
    let entry = Entry::parse(old).unwrap();
    assert_eq!((entry.code, entry.official), (true, false));
    assert_eq!(entry.summary, "Tells the time.");
}

#[test]
fn code_plugins_are_listed_where_a_build_fits() {
    let fits = build_of("x4", &abi());
    let storage = list(&[
        code("clock", &[build_of("x4-pro", &abi()), fits.clone()]),
        code("other-build", &[build_of("x4", "fedcba9876543210")]),
        code("pro-only", &[build_of("x4-pro", &abi())]),
        code("no-builds", &[]),
    ]);
    let (entries, _) = entries(&storage);
    assert_eq!(ids(&entries), ["clock"]);
    // Its code counted in, and what it is made for.
    let clock = &entries[0];
    assert_eq!(
        (clock.code, clock.size, clock.sdk.as_str()),
        (true, 70_030, "0.4")
    );
    // A PaperPaw that loads no code plugins lists none.
    let without = Device {
        fingerprint: None,
        code_file: "",
        ..X4
    };
    let storage = list(&[code("clock", &[fits])]);
    assert!(entries_for(&storage, &without).0.is_empty());
}

#[test]
fn many_plugins_take_no_memory_each() {
    let plugins: Vec<String> = (0..500)
        .map(|n| plugin(&format!("p{n}"), "look", ""))
        .collect();
    let storage = list(&plugins);
    let (_, runs) = entries(&storage);
    assert_eq!(runs, [(5, 500)]);
    assert_eq!(entry(&storage, 499).id, "p499");
}

#[test]
fn other_lists_are_not_read() {
    let storage = MemoryStorage::new().with("p.json", r#"{"format": 1, "plugins": []}"#);
    assert!(!build(&storage, "p.json", FILES, &X4));
    assert!(!build(&storage, "missing.json", FILES, &X4));
    assert!(!storage.exists("i.txt"));
    assert!(Index::open(&storage, "i.head").is_none());
}

fn page(files: &str) -> MemoryStorage {
    page_of(files, "data", "")
}

fn page_of(files: &str, kind: &str, more: &str) -> MemoryStorage {
    page_with(files, kind, more, &build_of("x4", &abi()))
}

/// A page whose code plugin has the build `build`.
fn page_with(files: &str, kind: &str, more: &str, build: &str) -> MemoryStorage {
    let text = format!(
        r#"{{"format": 2, "plugin": {{"id": "airy", "name": "Airy", "summary": "Roomy.",
  "group": "look", "kind": "{kind}", "author": "A", "licence": "MIT", {more}
  "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1, "size": 20, "files": [{files}]{}}}}}}}"#,
        if kind == "code" {
            format!(r#", "builds": [{build}]"#)
        } else {
            String::new()
        }
    );
    MemoryStorage::new().with("a.json", text)
}

fn file(path: &str) -> String {
    format!(
        r#"{{"path": "{path}", "url": "/api/v1/plugins/airy/1.0.0/{path}", "size": 10, "sha1": "{SHA1}"}}"#
    )
}

const BASE: &str = "https://paperpaw.org/api/v1/plugins/airy.json?device=x4&lang=en";

#[test]
fn a_plugins_files_come_with_its_page() {
    let storage = page(&format!("{},{}", file("i18n/de.ini"), file("plugin.ini")));
    let plugin = read_plugin(&storage, "a.json", &X4, BASE).unwrap();
    let (entry, package) = (&plugin.entry, &plugin.package);
    assert_eq!((entry.id.as_str(), entry.name.as_str()), ("airy", "Airy"));
    assert_eq!(package.plugin, "airy");
    let urls: Vec<&str> = package.files.iter().map(|f| f.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://paperpaw.org/api/v1/plugins/airy/1.0.0/i18n/de.ini",
            "https://paperpaw.org/api/v1/plugins/airy/1.0.0/plugin.ini"
        ]
    );
    assert_eq!(plugin.author_key, None);
    assert!(matches!(&package.files[0].digest, Digest::Sha1(hex) if hex == SHA1));
    // Given with its SHA-256 too, a file is checked by that.
    let both = file("plugin.ini").replace(
        &format!(r#""sha1": "{SHA1}""#),
        &format!(r#""sha1": "{SHA1}", "sha256": "{SHA256}""#),
    );
    let plugin = read_plugin(&page(&both), "a.json", &X4, BASE).unwrap();
    assert_eq!(
        plugin.package.files[0].digest,
        Digest::Sha256(SHA256.into())
    );
}

#[test]
fn a_code_plugins_page_brings_its_code_and_its_authors_key() {
    let author = format!(r#""author_key": "{KEY}","#);
    let storage = page_of(&file("plugin.ini"), "code", &author);
    let plugin = read_plugin(&storage, "a.json", &X4, BASE).unwrap();
    let code = plugin.package.files.last().unwrap();
    assert_eq!(code.path, "code.ppn");
    assert_eq!(code.url, "https://paperpaw.org/b/x4.ppn");
    assert_eq!(code.digest, Digest::Sha256(SHA256.into()));
    assert_eq!(plugin.author_key, Some([0x11; 32]));
    assert!(plugin.entry.code);
    // Without the author's key, a code plugin is not offered.
    let storage = page_of(&file("plugin.ini"), "code", "");
    assert!(read_plugin(&storage, "a.json", &X4, BASE).is_none());
}

#[test]
fn unsafe_files_are_refused() {
    for bad in ["../evil.ini", "/abs.ini", "a/../../b", ".hidden/x"] {
        let storage = page(&format!("{},{}", file("plugin.ini"), file(bad)));
        let read = read_plugin(&storage, "a.json", &X4, "https://x/");
        assert!(read.is_none(), "{bad}");
    }
    let short = file("plugin.ini").replace(SHA1, "abc");
    assert!(read_plugin(&page(&short), "a.json", &X4, "https://x/").is_none());
    // No plugin.ini.
    assert!(read_plugin(&page(&file("theme.ini")), "a.json", &X4, "https://x/").is_none());
    // More files than a package may have.
    let many: Vec<String> = (0..=MAX_FILES)
        .map(|n| file(&format!("f{n}.ini")))
        .chain([file("plugin.ini")])
        .collect();
    assert!(read_plugin(&page(&many.join(",")), "a.json", &X4, "https://x/").is_none());
}

#[test]
fn addresses_name_the_device_and_the_language() {
    let list = "https://paperpaw.org/api/v1/plugins.json";
    assert_eq!(
        list_url(list, "x4-pro", "de", false, None),
        "https://paperpaw.org/api/v1/plugins.json?device=x4-pro&lang=de"
    );
    assert_eq!(
        list_url("https://x/p.json?key=1", "x4", "en", false, None),
        "https://x/p.json?key=1&device=x4&lang=en"
    );
    assert_eq!(
        plugin_url(list, "x4", "en", "airy", false, None),
        "https://paperpaw.org/api/v1/plugins/airy.json?device=x4&lang=en"
    );
    // Community plugins only where the reader asked for them.
    assert_eq!(
        list_url(list, "x4", "en", true, None),
        "https://paperpaw.org/api/v1/plugins.json?device=x4&lang=en&community=1"
    );
    assert_eq!(
        plugin_url(list, "x4", "en", "airy", true, None),
        "https://paperpaw.org/api/v1/plugins/airy.json?device=x4&lang=en&community=1"
    );
    // A library's key goes with its pages.
    assert_eq!(
        plugin_url(
            "https://x/lib/p.json?key=1#top",
            "x4",
            "en",
            "airy",
            false,
            None
        ),
        "https://x/lib/plugins/airy.json?key=1&device=x4&lang=en"
    );
}

#[test]
fn addresses_name_the_build_the_device_runs() {
    // The library gives a device the builds of its own PaperPaw only where
    // it is told which that is (#148): 16 lower-case hex digits.
    let list = "https://paperpaw.org/api/v1/plugins.json";
    let abi = Some(Fingerprint(0x0006_0003_2e94_0609));
    assert_eq!(
        list_url(list, "x4-pro", "de", false, abi),
        "https://paperpaw.org/api/v1/plugins.json?device=x4-pro&lang=de&abi=000600032e940609"
    );
    assert_eq!(
        plugin_url(list, "x4", "en", "airy", true, abi),
        "https://paperpaw.org/api/v1/plugins/airy.json?device=x4&lang=en&community=1&abi=000600032e940609"
    );
}

#[test]
fn searches_find_names_and_summaries() {
    let storage = list(&[
        plugin("airy", "look", ""),
        plugin("dark", "look", "").replace("Does dark.", "Night colours for AIRY rooms."),
        plugin("keys", "system", ""),
    ]);
    let index = take(&storage, "p.json", &X4).unwrap();
    assert!(!storage.exists("p.json"));
    assert_eq!(index.search(&storage, " Airy "), 2);
    assert_eq!((hit(&storage, 0), hit(&storage, 1)), (Some(0), Some(1)));
    assert_eq!(hit(&storage, 2), None);
    assert_eq!(index.entry(&storage, 2).unwrap().id, "keys");
    assert_eq!(index.search(&storage, "nothing"), 0);
    // A list fetched anew: the hits of the one before go.
    index.search(&storage, "keys");
    let storage = storage.with("p.json", r#"{"format": 2, "plugins": []}"#);
    let index = take(&storage, "p.json", &X4).unwrap();
    assert_eq!(index.len(), 0);
    assert_eq!(hit(&storage, 0), None);
}

#[test]
fn dates_are_read_as_numbers() {
    assert_eq!(date("2026-10-07"), 20261007);
    assert_eq!(date("2026-10-07T12:00:00Z"), 20261007);
    assert_eq!(date("2024"), 20240101);
    assert_eq!(date("soon"), 0);
    assert_eq!(date(""), 0);
}

#[test]
fn a_page_tells_its_source_author_and_screenshots() {
    let more = r#""source": "https://example.org/airy", "author_since": "2024-03-02",
  "author_plugins": 3, "screenshots": [
{"url": "s/1.png", "width": 360, "height": 600, "size": 2000},
{"url": "s/2.webp", "width": 360, "height": 600, "size": 2000},
{"url": "s/3.JPG?v=2", "width": 360, "height": 600, "size": 2000},
{"url": "s/4.png", "width": 4000, "height": 600, "size": 2000},
{"url": "http://example.org/5.jpeg", "width": 10, "height": 10, "size": 20},
{"url": "s/6.png", "width": 10, "height": 10, "size": 20}],"#;
    let storage = page_of(&file("plugin.ini"), "data", more);
    let plugin = read_plugin(&storage, "a.json", &X4, BASE).unwrap();
    let more = &plugin.more;
    assert_eq!(more.source, "https://example.org/airy");
    assert_eq!((more.author_since, more.author_plugins), (20240302, 3));
    // Data plugins carry no build to check.
    assert_eq!(more.verified, "");
    // Only what PaperPaw decodes, not too large, four at most; their
    // addresses relative to the page's.
    let shots: Vec<&str> = more.shots.iter().map(|s| s.url.as_str()).collect();
    assert_eq!(
        shots,
        [
            "https://paperpaw.org/api/v1/plugins/s/1.png",
            "https://paperpaw.org/api/v1/plugins/s/3.JPG?v=2",
            "http://example.org/5.jpeg",
            "https://paperpaw.org/api/v1/plugins/s/6.png"
        ]
    );
    assert_eq!((more.heap, more.image_len), (None, None));
}

#[test]
fn a_code_plugins_page_tells_whether_its_build_was_checked() {
    let author = format!(r#""author_key": "{KEY}", "source": "https://example.org/clock","#);
    let with = |latest: &str, build: &str| {
        let storage = page_of(&file("plugin.ini"), "code", &author);
        let text = paperpaw_sdk::read_file(&storage, "a.json", 1 << 16).unwrap();
        let text = String::from_utf8(text)
            .unwrap()
            .replace(r#""latest": {"#, &format!(r#""latest": {{{latest}"#))
            .replace(r#""device": "x4","#, &format!(r#""device": "x4", {build}"#));
        let storage = MemoryStorage::new().with("a.json", text);
        read_plugin(&storage, "a.json", &X4, BASE).unwrap().more
    };
    let more = with(
        r#""verified": true, "heap": 40000,
  "source": {"url": "https://example.org/clock/src/tag/v1.0.0"},"#,
        r#""image_len": 9000, "rebuilt": "matches","#,
    );
    assert_eq!(more.verified, "verified");
    assert_eq!(more.source, "https://example.org/clock/src/tag/v1.0.0");
    assert_eq!((more.heap, more.image_len), (Some(40000), Some(9000)));
    let more = with(r#""verified": false,"#, r#""rebuilt": "differs","#);
    assert_eq!(more.verified, "failed");
    assert_eq!(more.source, "https://example.org/clock");
    let more = with(r#""verified": false,"#, r#""rebuilt": "none","#);
    assert_eq!(more.verified, "unverified");
}

#[test]
fn plugins_are_picked_in_the_order_asked() {
    let dated = |id: &str, added: &str, date: &str| {
        plugin(id, "look", "")
            .replace(r#""kind""#, &format!(r#""added": "{added}", "kind""#))
            .replace(r#""size": 30"#, &format!(r#""size": 30, "date": "{date}""#))
    };
    let storage = list(&[
        dated("a", "2025-01-01", "2026-03-01"),
        dated("b", "2026-01-01", "2026-01-01"),
        dated("c", "2025-06-01", "2026-09-01"),
        plugin("d", "system", ""),
    ]);
    let index = take(&storage, "p.json", &X4).unwrap();
    let picked = |order: Order, most: usize| {
        let count = index.pick(
            &storage,
            |_, e| (e.group == group("look")).then_some(u32::from(e.id == "c")),
            order,
            most,
        );
        (0..count)
            .map(|n| index.entry(&storage, hit(&storage, n).unwrap()).unwrap().id)
            .collect::<Vec<_>>()
    };
    assert_eq!(picked(Order::Name, usize::MAX), ["a", "b", "c"]);
    assert_eq!(picked(Order::Name, 2), ["a", "b"]);
    assert_eq!(picked(Order::Added, usize::MAX), ["b", "c", "a"]);
    assert_eq!(picked(Order::Added, 1), ["b"]);
    assert_eq!(picked(Order::Updated, usize::MAX), ["c", "a", "b"]);
    assert_eq!(picked(Order::Rank, usize::MAX), ["c", "a", "b"]);
    // Read through the index in one pass, every plugin.
    let mut ids = Vec::new();
    index.scan(&storage, &mut |n, e| ids.push((n, e.id)));
    assert_eq!(ids.len(), 4);
    assert_eq!(ids[3], (3, String::from("d")));
}

#[test]
fn long_lists_are_sorted_in_runs_on_the_storage() {
    // More plugins than a run holds, their dates in no order, many of
    // one day.
    let count = 3 * RUN + 77;
    let day = |i: usize| 20250101 + ((i * 7919) % 400) as u32;
    let plugins: Vec<String> = (0..count)
        .map(|i| {
            let date = day(i).to_string();
            let date = format!("{}-{}-{}", &date[..4], &date[4..6], &date[6..]);
            plugin(&format!("p{i:05}"), "look", "")
                .replace(r#""kind""#, &format!(r#""added": "{date}", "kind""#))
        })
        .collect();
    let storage = list(&plugins);
    let index = take(&storage, "p.json", &X4).unwrap();
    let mut expected: Vec<(u64, u32)> = (0..count).map(|i| (u64::from(day(i)), i as u32)).collect();
    expected.sort_by(|a, b| order_of(*a, *b));
    for most in [usize::MAX, RUN + 1, 30] {
        let picked = index.pick(&storage, |_, _| Some(0), Order::Added, most);
        let want: Vec<u32> = expected.iter().take(most).map(|(_, n)| *n).collect();
        assert_eq!(picked, want.len());
        let got: Vec<u32> = (0..picked)
            .map(|h| hit(&storage, h).unwrap() as u32)
            .collect();
        assert_eq!(got, want, "{most}");
    }
    // The runs are gone again.
    assert!(!storage.exists(&file("sort.bin")));
}

#[test]
fn keys_are_read_from_hex() {
    assert_eq!(key(KEY), Some([0x11; 32]));
    assert_eq!(key("11"), None);
    assert_eq!(key(&"zz".repeat(32)), None);
}

#[test]
fn the_groups_are_the_plugin_stores() {
    // The site's rules (tools/publish/store-rules.json): the same groups,
    // in its order.
    let rules = paperpaw_publish::Rules::built_in();
    assert_eq!(GROUPS.as_slice(), rules.groups.as_slice());
}

#[test]
fn names_sort_from_a_to_z_by_their_letters() {
    let mut names = [
        "Zebra", "äpfel", "Apples", "2048", "Être", "eule", "Ωmega", "a b",
    ];
    names.sort_by_key(|name| name_key(name));
    assert_eq!(
        names,
        [
            "2048", "a b", "äpfel", "Apples", "Être", "eule", "Zebra", "Ωmega"
        ]
    );
    // Case and accents left aside, signs passed over.
    assert_eq!(name_key("ABC"), name_key("abc"));
    assert_eq!(name_key("Ab-c"), name_key("abc"));
    assert_eq!(name_key("Äb"), name_key("ab"));
    // A name of one letter: where that letter's names begin.
    assert!(name_key("m") <= name_key("Maps") && name_key("Maps") < name_key("n"));
}

#[test]
fn a_fair_list_says_so_and_keeps_its_picks() {
    let storage = MemoryStorage::new().with(
        "p.json",
        format!(
            r#"{{"format": 2, "plugins": [{}, {}],
              "sections": {{"recommended": ["b", "a"], "new": [], "trending": ["a"],
                "updated": [], "discover": ["../x"]}}}}"#,
            plugin("a", "look", ""),
            plugin("b", "look", "")
        ),
    );
    let index = take(&storage, "p.json", &X4).unwrap();
    assert!(index.fair());
    assert_eq!(index.orders()[0], Order::Fair);
    assert_eq!(index.order("added"), Order::Added);
    assert_eq!(index.order("nonsense"), Order::Fair);
    assert_eq!(index.section(&storage, 0), ["b", "a"]);
    assert_eq!(index.section_len(0), 2);
    assert!(index.section(&storage, 1).is_empty());
    assert_eq!(index.section(&storage, 2), ["a"]);
    // No plugin's id: left out.
    assert_eq!(index.section_len(4), 0);
    // A list without picks is by name, as before.
    let storage = list(&[plugin("a", "look", "")]);
    let index = take(&storage, "p.json", &X4).unwrap();
    assert!(!index.fair());
    assert_eq!(index.order("fair"), Order::Name);
}

/// A plugin on the device as the kernel lists it.
fn on_device(plugins: &mut Plugins, id: &'static str, kind: PluginKind, enabled: bool) {
    let manifest = paperpaw_sdk::Manifest::new(
        paperpaw_sdk::Id::new_static(id),
        "Name",
        Version::new(1, 0, 0),
    );
    plugins.push(paperpaw_sdk::PluginInfo::new(
        paperpaw_sdk::ManifestRef::try_new(manifest, None).unwrap(),
        if enabled {
            paperpaw_sdk::PluginState::Running
        } else {
            paperpaw_sdk::PluginState::Off
        },
        enabled,
        paperpaw_sdk::RequiredFor::default(),
        kind,
        None,
        None,
    ));
}

#[test]
fn a_built_in_plugin_switched_off_is_listed_to_be_switched_on() {
    let mut plugins = Plugins::new();
    on_device(&mut plugins, "off-data", PluginKind::BuiltIn, false);
    on_device(&mut plugins, "off-code", PluginKind::BuiltIn, false);
    on_device(&mut plugins, "on", PluginKind::BuiltIn, true);
    on_device(&mut plugins, "core", PluginKind::BuiltIn, false);
    let device = Device {
        plugins: Some(&plugins),
        ..X4
    };
    let storage = list(&[
        plugin("off-data", "tools", ""),
        // Code in the library, with no file for this device (Android runs it
        // built in): still offered to be switched on.
        plugin("off-code", "tools", "").replace(r#""kind": "data""#, r#""kind": "code""#),
        // On already: nothing to offer.
        plugin("on", "tools", ""),
        // The library says it is built into every PaperPaw: never listed.
        plugin("core", "tools", r#""built_in": true,"#),
        plugin("new", "tools", ""),
    ]);
    let (entries, _) = entries_for(&storage, &device);
    let mut listed = ids(&entries);
    listed.sort_unstable();
    // `on` takes the place of nothing here: a built-in plugin on is passed
    // over, as before.
    assert_eq!(listed, ["new", "off-code", "off-data"]);
    assert!(switched_off(&plugins, "off-data"));
    assert!(!switched_off(&plugins, "on"));
    assert!(!switched_off(&plugins, "new"));
}

/// A build with what it says it needs of the firmware.
fn needing(abi: &str, needs: &str) -> String {
    build_of("x4", abi).replace(r#""size": 70000"#, &format!(r#""size": 70000, {needs}"#))
}

/// The X4 with a firmware whose table is 400 bytes, with families 3 and 2.
fn with_table() -> Device<'static> {
    Device {
        provides: Some(Provides {
            minor: Some(2),
            api: 400,
            families: [3, 2, 0, 0, 0, 0, 0, 0],
            oldest: [2, 1, 0, 0, 0, 0, 0, 0],
            checks_families: true,
        }),
        ..X4
    }
}

#[test]
fn a_build_that_needs_a_newer_paperpaw_is_listed_as_such_and_not_installed() {
    let abi = &abi();
    let storage = list(&[
        code(
            "plain",
            &[needing(
                abi,
                r#""interface": 5, "needs_api": 400, "families": [3, 2, 0, 0, 0, 0, 0, 0]"#,
            )],
        ),
        code(
            "too-new-family",
            &[needing(abi, r#""families": [4, 0, 0, 0, 0, 0, 0, 0]"#)],
        ),
        code("too-new-table", &[needing(abi, r#""needs_api": 404"#)]),
        code(
            "too-old",
            &[needing(abi, r#""families": [1, 0, 0, 0, 0, 0, 0, 0]"#)],
        ),
        // An older library says none of it: as before.
        code("unsaid", &[build_of("x4", abi)]),
        // Odd: not eight numbers, or not numbers; the rest is still read.
        code("odd-short", &[needing(abi, r#""families": [9, 9]"#)]),
        code(
            "odd-words",
            &[needing(
                abi,
                r#""families": [9, "x", 0, 0, 0, 0, 0, 0], "interface": "five""#,
            )],
        ),
        code(
            "odd-long",
            &[needing(abi, r#""families": [9, 0, 0, 0, 0, 0, 0, 0, 0]"#)],
        ),
    ]);
    let (entries, _) = entries_for(&storage, &with_table());
    assert_eq!(
        ids(&entries),
        [
            "plain",
            "too-new-family",
            "too-new-table",
            "unsaid",
            "odd-short",
            "odd-words",
            "odd-long"
        ],
        "an older build is no build for this PaperPaw, as another fingerprint is"
    );
    let by = |id: &str| entries.iter().find(|e| e.id == id).unwrap();
    for id in ["too-new-family", "too-new-table"] {
        let e = by(id);
        assert!(e.needs_newer && e.code && e.size == 30, "{id}: {e:?}");
    }
    for id in ["plain", "unsaid", "odd-long", "odd-short", "odd-words"] {
        let e = by(id);
        assert!(!e.needs_newer && e.size == 70_030, "{id}: {e:?}");
    }
    // The index keeps the mark, and reads an index without it as before.
    let line = by("too-new-table").line();
    assert!(Entry::parse(&line).unwrap().needs_newer);
    let mut old = by("plain").clone();
    old.needs_newer = false;
    assert!(!Entry::parse(&old.line()).unwrap().needs_newer);
    // Without a table to compare with, every build fits.
    let (all, _) = entries_for(&storage, &X4);
    assert!(all.iter().all(|e| !e.needs_newer));
}

#[test]
fn a_page_of_a_plugin_made_for_a_newer_paperpaw_has_no_code_to_install() {
    let author = format!(r#""author_key": "{KEY}","#);
    let build = needing(&abi(), r#""needs_api": 500"#);
    let storage = page_with(&file("plugin.ini"), "code", &author, &build);
    let plugin = read_plugin(&storage, "a.json", &with_table(), BASE).unwrap();
    assert!(plugin.entry.needs_newer && plugin.entry.code);
    assert!(plugin.package.files.iter().all(|f| f.path != "code.ppn"));
    // The same build for a firmware with the room: code to install.
    let roomy = Device {
        provides: Some(Provides {
            minor: Some(2),
            api: 500,
            families: [3, 2, 0, 0, 0, 0, 0, 0],
            oldest: [2, 1, 0, 0, 0, 0, 0, 0],
            checks_families: true,
        }),
        ..X4
    };
    let plugin = read_plugin(&storage, "a.json", &roomy, BASE).unwrap();
    assert!(!plugin.entry.needs_newer);
    assert!(plugin.package.files.iter().any(|f| f.path == "code.ppn"));
}

#[test]
fn a_group_this_store_does_not_know_is_system() {
    // A later library may add a fourteenth group (#148): its plugins are
    // listed under `system`, the last, and the groups are all different.
    let system = GROUPS.len() - 1;
    for name in ["fourteenth", "", "LANGUAGES", "languages "] {
        assert_eq!(group(name), system, "{name:?}");
    }
    assert_eq!(group("languages"), GROUPS.len() - 2);
    for (n, name) in GROUPS.iter().enumerate() {
        assert_eq!(group(name), n);
    }
    let storage = list(&[plugin("future", "fourteenth", "")]);
    let (entries, runs) = entries(&storage);
    assert_eq!(ids(&entries), ["future"]);
    assert_eq!(runs, [(system, 1)]);
}

#[test]
fn a_plugin_the_library_has_no_build_of_for_this_paperpaw_is_listed_waiting() {
    // Asked with `abi`, the library keeps such a plugin in the list with
    // `waits` and no builds (#148): the device says so, and offers nothing.
    let waiting = |id: &str, waits: &str| {
        code(id, &[]).replacen(
            r#""kind": "code""#,
            &format!(r#""kind": "code", "waits": "{waits}""#),
            1,
        )
    };
    let storage = list(&[
        waiting("by-author", "author"),
        waiting("by-update", "update"),
        code("plain", &[build_of("x4", &abi())]),
        // No builds and no word on it: not a plugin for this device.
        code("silent", &[]),
    ]);
    let (entries, _) = entries(&storage);
    assert_eq!(ids(&entries), ["by-author", "by-update", "plain"]);
    let by = |id: &str| entries.iter().find(|e| e.id == id).unwrap();
    let author = by("by-author");
    assert!(author.waits_author && author.unbuilt() && !author.needs_newer);
    let update = by("by-update");
    assert!(update.needs_newer && update.unbuilt() && !update.waits_author);
    assert!(!by("plain").unbuilt());
    // The index keeps the mark.
    assert!(Entry::parse(&author.line()).unwrap().waits_author);
}

#[test]
fn a_build_for_this_paperpaw_repairs_an_installed_one_built_for_an_older_one() {
    // The owner's case (#148): a plugin installed from the 0.5 store is of
    // the same version as the build the library now gives this PaperPaw,
    // and the plugin manager says "update it in the store".
    let storage = list(&[
        code("stats", &[build_of("x4", &abi())]),
        code("waits", &[]).replacen(
            r#""kind": "code""#,
            r#""kind": "code", "waits": "author""#,
            1,
        ),
    ]);
    let (entries, _) = entries(&storage);
    let (stats, waits) = (&entries[0], &entries[1]);
    let installed = |problem| {
        let manifest = paperpaw_sdk::Manifest::new(
            paperpaw_sdk::Id::new_static("stats"),
            "Name",
            Version::new(1, 0, 0),
        );
        let mut info = paperpaw_sdk::PluginInfo::new(
            paperpaw_sdk::ManifestRef::try_new(manifest, None).unwrap(),
            paperpaw_sdk::PluginState::Failed,
            true,
            paperpaw_sdk::RequiredFor::default(),
            PluginKind::Code,
            None,
            None,
        );
        info.problem = problem;
        info
    };
    use paperpaw_sdk::PluginProblem as Problem;
    for problem in [
        Problem::OlderInterface { built_for: 4 },
        Problem::OtherBuild,
    ] {
        assert!(
            stats.repairs(Some(&installed(Some(problem)))),
            "{problem:?}"
        );
    }
    // One that runs, one that crashed, one that is not there: nothing to
    // repair.
    assert!(!stats.repairs(Some(&installed(None))));
    assert!(!stats.repairs(Some(&installed(Some(Problem::Crashed)))));
    assert!(!stats.repairs(None));
    // Nothing to offer where the library has no build for this PaperPaw.
    assert!(!waits.repairs(Some(&installed(Some(Problem::OtherBuild)))));
}

/// Format 5 names a build's `abi` as `<major>.<needs_minor>`: the device is
/// the target, the major is 7 (below: built for an older PaperPaw, above:
/// needs a newer one) and the minor at most the host's (here 2); no draft.
#[test]
fn a_dotted_abi_is_judged_by_major_and_minor() {
    let storage = list(&[
        code("fits", &[build_of("x4", "7.2")]),
        code("fits-zero", &[build_of("x4", "7.0")]),
        code("newer-minor", &[build_of("x4", "7.3")]),
        code("newer-major", &[build_of("x4", "8.0")]),
        code("older-major", &[build_of("x4", "6.9")]),
        code("other-device", &[build_of("x4-pro", "7.1")]),
        code("odd-words", &[build_of("x4", "7.x")]),
        code("odd-three", &[build_of("x4", "7.1.2")]),
        code("odd-empty", &[build_of("x4", "7.")]),
        code("odd-zeros", &[build_of("x4", "07.1")]),
        code("odd-wide", &[build_of("x4", "7.65536")]),
        // The old form still works beside it.
        code("old-form", &[build_of("x4", &abi())]),
    ]);
    let (entries, _) = entries_for(&storage, &with_table());
    // A plugin none of whose builds is for this device and PaperPaw is not
    // listed (an older one is not offered, as another fingerprint is not).
    assert_eq!(
        ids(&entries),
        [
            "fits",
            "fits-zero",
            "newer-minor",
            "newer-major",
            "old-form"
        ]
    );
    let by = |id: &str| entries.iter().find(|e| e.id == id).unwrap();
    for id in ["fits", "fits-zero", "old-form"] {
        assert!(by(id).code && !by(id).needs_newer, "{id}: {:?}", by(id));
    }
    for id in ["newer-minor", "newer-major"] {
        assert!(by(id).code && by(id).needs_newer, "{id}: {:?}", by(id));
    }
    // A host whose minor is not known yet (`Provides::minor`) skips the
    // minor check.
    let unknown = Device {
        provides: with_table().provides.map(|p| Provides { minor: None, ..p }),
        ..X4
    };
    let (entries, _) = entries_for(&storage, &unknown);
    assert!(
        entries
            .iter()
            .all(|e| !e.needs_newer || e.id == "newer-major")
    );
}

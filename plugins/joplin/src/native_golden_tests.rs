//! What the native Joplin modules (`native_joplin`, `native_kept`,
//! `native_note`, `native_locks`) decide, write and read, on the inputs the
//! interface-6 parity tests used, against transcripts kept in
//! `testdata/golden_joplin.v1`, `golden_kept.v1`, `golden_read.v1` and
//! `golden_note.v1`.
//!
//! The transcripts were generated from interface-6 commit 57ff02a17 (the
//! interface-6 `item`, `kept`, `locks`, `markdown` and `service` modules) by
//! a one-off test that ran the very same corpora and line formats through
//! those modules and wrote the files; that test was removed with the
//! interface-6 code. Every line is `section TAB case TAB result`, a result
//! being what interface 6 gave (a long text by its length and an FNV-1a
//! hash). A line that changes is a change of what is written to a sync
//! target or decided about it, so it is reviewed, not regenerated from the
//! native code.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use crate::native_joplin::{self as joplin, Service};
use crate::native_kept::{self as kept, Sent};
use crate::native_locks;
use crate::native_note;
use crate::test_volume::Mem;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const JOPLIN: &str = include_str!("../testdata/golden_joplin.v1");
const KEPT: &str = include_str!("../testdata/golden_kept.v1");
const READ: &str = include_str!("../testdata/golden_read.v1");
const NOTE: &str = include_str!("../testdata/golden_note.v1");

pub(crate) const PROPS: &str = "id: 0123456789abcdef0123456789abcdef\nparent_id: fedcba9876543210fedcba9876543210\ncreated_time: 2026-01-02T03:04:05.000Z\nupdated_time: 2026-02-03T04:05:06.000Z\nuser_created_time: 2026-01-02T03:04:05.000Z\norder: 5\ndeleted_time: 0\nencryption_applied: 0\ntype_: 1";

const FIRST: &str = "# Alice\n\nLewis Carroll\n\n## Highlights\n\n> Curiouser!\n>\n> 12 %\n\n";

/// As Joplin 3.7 wrote it to a sync target.
const JOPLIN_NOTE: &str = "Hello\n\nLine one\n\n> quote\n\n\
    id: 1ae79351dd744c2794eebb334b1629a7\n\
    parent_id: 7db793874c624138b3b4520ae5cdf20c\n\
    created_time: 2026-10-19T22:24:10.090Z\n\
    updated_time: 2026-10-19T22:24:11.708Z\n\
    is_conflict: 0\nauthor: \nsource_url: \n\
    user_created_time: 2026-10-19T22:24:10.090Z\n\
    user_updated_time: 2026-10-19T22:24:10.090Z\n\
    order: 1792448650089\n\
    encryption_cipher_text: \nencryption_applied: 0\n\
    deleted_time: 0\nis_locked: 0\ntype_: 1";

/// 2026-10-20 08:00:00 UTC.
pub(crate) const NOW: i64 = 1_792_483_200_000;

/// A text on one line: escaped, a long one by its length and a hash.
pub(crate) fn q(bytes: &[u8]) -> String {
    if bytes.len() <= 1024 {
        return format!("\"{}\"", bytes.escape_ascii());
    }
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    });
    format!("<{} bytes {hash:016x}>", bytes.len())
}

/// An item's properties as one result.
pub(crate) struct Fields<'a> {
    pub(crate) kind: u8,
    pub(crate) parent: &'a str,
    pub(crate) created: &'a str,
    pub(crate) updated: i64,
    pub(crate) user_created: &'a str,
    pub(crate) order: i64,
    pub(crate) deleted: bool,
    pub(crate) encrypted: bool,
    /// Left out where only the properties were compared.
    pub(crate) text_end: Option<u64>,
}

impl Fields<'_> {
    pub(crate) fn show(&self) -> String {
        format!(
            "{} {} {} {} {} {} {} {} {:?}",
            self.kind,
            q(self.parent.as_bytes()),
            q(self.created.as_bytes()),
            self.updated,
            q(self.user_created.as_bytes()),
            self.order,
            self.deleted,
            self.encrypted,
            self.text_end,
        )
    }
}

/// How much of an item's file the native reader is given.
#[derive(Debug, Clone, Copy)]
pub(crate) enum View {
    Whole,
    /// What the caller reads: the last `TAIL` bytes.
    Tail,
    /// The last so many bytes.
    Have(usize),
}

/// The operations the transcripts are made of: interface 6's in the
/// one-off generator, the native ones here.
pub(crate) trait Backend {
    fn consts(&self) -> String;
    fn service(&self, i: usize) -> String;
    fn url(&self, i: usize, base: &str, name: &str) -> String;
    fn locks_url(&self, i: usize, base: &str) -> String;
    fn of_key(&self, i: usize) -> String;
    fn new_id(&self, seed: &str, replaces: Option<&str>) -> String;
    fn is_id(&self, text: &str) -> bool;
    fn parse_props(&self, text: &str) -> String;
    fn notebook(&self, id: &str, title: &str, now: &str) -> String;
    fn note_props(&self, id: &str, parent: &str, author: &str, order: i64) -> String;
    fn parse_head(&self, text: &str) -> String;
    fn kept_files(&self) -> Vec<(String, String)>;
    fn item_props(&self, file: &[u8], view: View) -> String;
    fn head_of(&self, file: &[u8]) -> String;
    fn note_head(&self, text: &[u8]) -> String;
    fn write_note(&self, text: &[u8], title: &str) -> String;
    fn read_back(&self, notes: &[u8]) -> String;
    fn upgrading(&self, i: usize, text: &str) -> bool;
}

fn line(out: &mut String, section: &str, case: &str, result: &str) {
    out.push_str(&format!("{section}\t{case}\t{result}\n"));
}

pub(crate) fn a_note(title: &str) -> joplin::Note<'_> {
    joplin::Note {
        id: "1ae79351dd744c2794eebb334b1629a7",
        parent: "7db793874c624138b3b4520ae5cdf20c",
        title,
        author: "Lewis\nCarroll",
        created: "2026-10-19T22:24:10.090Z",
        user_created: "2026-10-19T22:24:09.000Z",
        updated: "2026-10-19T22:24:11.708Z",
        order: 1_792_448_650_090,
    }
}

/// The titles a note is written with.
pub(crate) const TITLES: [&str; 3] = ["Alice", "  A \n long\ttitle ", ""];

/// Notes files of every shape the body copy has a rule for.
pub(crate) fn notes() -> Vec<String> {
    let mut out: Vec<String> = [
        FIRST,
        "# Bob\n\n",
        "",
        "# Only a title",
        "\n\n  \t# T\n\ntext\n\n\n",
        "#not a title\nbody\n",
        "## Heading first\n\nbody\n",
        "no title at all\n\nmore\r\n\r\n",
        "# T\r\n\r\nA\r\n## H\r\n> a\r\n",
        "# T\n\n#\n",
        "# T\n\n# \n\nx",
        "# ü\nÄ\n## H\n> ö ä\n> 7 %\nÜ\n\n",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    // Blank runs across the pieces the body is read in.
    for at in [1000, 1020, 1023, 1024, 1025, 2047] {
        out.push(format!(
            "# T\n\n{}{}x{}\n\n\n",
            "y".repeat(at),
            "\n".repeat(30),
            "z".repeat(40)
        ));
    }
    out.push(format!("# Long\n\n{}\n", "word ".repeat(5000)));
    out
}

fn listing(responses: &[(&str, &str)]) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\">\
         <d:response><d:href>/dav/Joplin/locks/</d:href>\
         <d:propstat><d:prop><d:getlastmodified>Tue, 20 Oct 2026 07:59:00 GMT\
         </d:getlastmodified></d:prop></d:propstat></d:response>",
    );
    for (name, modified) in responses {
        xml.push_str(&format!(
            "<d:response><d:href>/dav/Joplin/locks/{name}</d:href><d:propstat><d:prop>\
             <d:getlastmodified>{modified}</d:getlastmodified></d:prop></d:propstat>\
             </d:response>"
        ));
    }
    xml.push_str("</d:multistatus>");
    xml
}

fn server_list(locks: &[(u8, i64)]) -> String {
    let items: Vec<String> = locks
        .iter()
        .map(|(kind, ago)| {
            format!(
                "{{\"id\":\"x\",\"type\":{kind},\"clientType\":1,\"clientId\":\"c\",\
                 \"updatedTime\":{}}}",
                NOW - ago
            )
        })
        .collect();
    format!("{{\"items\":[{}],\"has_more\":false}}", items.join(","))
}

/// Listings of a sync target's locks, by service (0 Cloud, 1 Server,
/// 2 WebDAV).
pub(crate) fn lock_cases() -> Vec<(usize, String)> {
    let fresh = "Tue, 20 Oct 2026 07:58:30 GMT";
    let old = "Tue, 20 Oct 2026 07:56:30 GMT";
    let long = "x".repeat(64 * 1024);
    let mut out = vec![(2, listing(&[("2_1_abc.json", fresh)]))];
    for (name, modified) in [
        ("2_1_abc.json", old),
        ("1_1_abc.json", fresh),
        ("2_1_abc.txt", fresh),
    ] {
        out.push((2, listing(&[(name, modified)])));
    }
    out.push((2, listing(&[("2_1_ABC.JSON", fresh)])));
    out.push((2, listing(&[])));
    out.push((2, String::new()));
    out.push((
        2,
        listing(&[("1_2_m.json", fresh), ("2_3_cli.json", fresh)]),
    ));
    // A huge name and date do not hide the lock that follows.
    out.push((
        2,
        listing(&[(long.as_str(), long.as_str()), ("2_1_abc.json", fresh)]),
    ));
    out.push((1, server_list(&[(2, 30_000)])));
    out.push((1, server_list(&[(2, native_locks::TTL + 1)])));
    out.push((1, server_list(&[(1, 30_000)])));
    out.push((1, server_list(&[(1, 30_000), (2, 60_000)])));
    out.push((1, server_list(&[])));
    out.push((1, "not json".into()));
    out.push((0, "{}".into()));
    out
}

/// Files of an item at the end of which its properties are read, and the
/// pieces of each the native reader is given.
pub(crate) fn item_cases() -> Vec<(Vec<u8>, Vec<View>)> {
    let bodies = [
        String::new(),
        String::from("Title\n\nBody"),
        format!("Title\n\n{}", "text ünï\n\nmore ".repeat(400)),
        "x".repeat(2048 - PROPS.len() - 2),
        "x".repeat(2048 - PROPS.len() - 1),
        "y".repeat(5000),
    ];
    let mut out = Vec::new();
    for body in &bodies {
        // With and without a trailing line break, and with a stray byte.
        for tail in ["", "\n", "\r\n\r\n"] {
            let file = format!("{body}\n\n{PROPS}{tail}");
            out.push((file.into_bytes(), vec![View::Whole, View::Tail]));
        }
    }
    let mut bytes: Vec<u8> = b"T\n\n".to_vec();
    bytes.extend_from_slice(PROPS.as_bytes());
    bytes.insert(10, 0xff);
    out.push((bytes, vec![View::Whole]));
    // Properties beyond the end read are not seen.
    let far = format!("{PROPS}\n\n{}", "z".repeat(3000));
    out.push((far.into_bytes(), vec![View::Whole]));
    // A big file of which the caller read less than the bytes that count.
    let file = format!("{}\n\nBody text\n\n{PROPS}", "y".repeat(5000));
    let views = [PROPS.len() + 20, 700, 1500, joplin::TAIL, 3000, file.len()]
        .map(View::Have)
        .to_vec();
    out.push((file.into_bytes(), views));
    let small = format!("T\n\n{PROPS}");
    out.push((small.into_bytes(), vec![View::Whole]));
    out
}

/// Notes files read for their title and author.
pub(crate) fn head_files() -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = [
        String::new(),
        String::from("# A Book\nJane Author\n\n## Highlight\n"),
        String::from("\n\n# Late title\n\nAuthor\n"),
        format!("# {}\nAuthor\n", "T".repeat(1500)),
        format!("# Title\n{}Author far down\n", "\n".repeat(1100)),
        String::from("no heading\n"),
    ]
    .into_iter()
    .map(String::into_bytes)
    .collect();
    out.push(b"# T\xffitle\nAu\xc3thor\n".to_vec());
    out
}

fn joplin_services(b: &dyn Backend, out: &mut String) {
    line(out, "const", "-", &b.consts());
    for i in 0..3 {
        line(out, "service", &i.to_string(), &b.service(i));
        for base in ["https://j.example", "http://h:8080/x", ""] {
            for name in ["info.json", "a b.md", ""] {
                let case = format!("{i} {base:?} {name:?}");
                line(out, "url", &case, &b.url(i, base, name));
            }
            let case = format!("{i} {base:?}");
            line(out, "locks_url", &case, &b.locks_url(i, base));
        }
        line(out, "of_key", &i.to_string(), &b.of_key(i));
    }
}

fn joplin_ids(b: &dyn Backend, out: &mut String) {
    for (seed, replaces) in [
        ("a book 1", None),
        ("a book 1", Some("abc")),
        ("", None),
        ("ü", Some("")),
    ] {
        let case = format!("{seed:?} {replaces:?}");
        line(out, "new_id", &case, &b.new_id(seed, replaces));
    }
    for text in [
        "",
        "info",
        &"g".repeat(32),
        &"a".repeat(31),
        &"A".repeat(32),
        &"0".repeat(33),
    ] {
        line(
            out,
            "is_id",
            &q(text.as_bytes()),
            &b.is_id(text).to_string(),
        );
    }
}

/// The texts whose properties are read.
pub(crate) fn props_texts() -> Vec<String> {
    let trashed = JOPLIN_NOTE.replace("deleted_time: 0", "deleted_time: 1792448650090");
    let encrypted = JOPLIN_NOTE.replace("encryption_applied: 0", "encryption_applied: 1");
    vec![
        JOPLIN_NOTE.into(),
        trashed,
        encrypted,
        "".into(),
        "type_: 2".into(),
        "no props at all".into(),
        "title\n\nbody\n\nparent_id: x\nupdated_time: bad\norder: z\ntype_: q".into(),
        "a: b\n\nupdated_time: 2026-10-19T22:24:11.708Z\n\n".into(),
        "x\n\nkey without colon\nparent_id : spaced \n deleted_time:  \ntype_ : 1\n".into(),
    ]
}

fn joplin_items(b: &dyn Backend, out: &mut String) {
    for (i, text) in props_texts().iter().enumerate() {
        line(out, "parse_props", &i.to_string(), &b.parse_props(text));
    }
    let now = "2026-10-19T22:24:10.090Z";
    for title in ["Books", " Alice's\n Adventures ", "", "ünï\t😀", "  "] {
        let id = "7db793874c624138b3b4520ae5cdf20c";
        line(
            out,
            "notebook",
            &q(title.as_bytes()),
            &b.notebook(id, title, now),
        );
    }
    let (id, parent) = (
        "1ae79351dd744c2794eebb334b1629a7",
        "7db793874c624138b3b4520ae5cdf20c",
    );
    for (author, order) in [
        ("", 0),
        ("Lewis Carroll", 1_792_448_650_089),
        ("a\nb\\nc\r\\r\\\\n", -5),
        ("ü", i64::MIN),
    ] {
        let case = format!("{} {order}", q(author.as_bytes()));
        line(out, "note", &case, &b.note_props(id, parent, author, order));
    }
    for text in [
        "", "a\nb", "a\\nb", "\\n\\r", "\\\\n", "\\", "\r\n", "ü\\nü", "\\\\\\n",
    ] {
        let result = b.note_props("i", "p", text, 0);
        line(out, "escape", &q(text.as_bytes()), &result);
    }
    for (i, text) in head_texts().iter().enumerate() {
        line(out, "parse_head", &i.to_string(), &b.parse_head(text));
    }
}

/// Texts whose title and author are told apart.
pub(crate) fn head_texts() -> [&'static str; 7] {
    [
        "# Alice's Adventures\n\nLewis Carroll\n\n## Highlights\n\n> x\n",
        "# Title\n\n## Bookmarks\n",
        "",
        "Author first\n# Title\nAuthor\n",
        "# T1\n# T2\nA\nB\n",
        "  # Indented\n  Author  \n",
        "## Only a heading\n# After\nA\n",
    ]
}

/// What the decisions of `native_joplin` come to, as `golden_joplin.v1`.
pub(crate) fn joplin_transcript(b: &dyn Backend) -> String {
    let mut out = String::new();
    joplin_services(b, &mut out);
    joplin_ids(b, &mut out);
    joplin_items(b, &mut out);
    out
}

/// What the lists the plugin keeps come to, as `golden_kept.v1`.
pub(crate) fn kept_transcript(b: &dyn Backend) -> String {
    let mut out = String::new();
    for (case, result) in b.kept_files() {
        line(&mut out, "kept", &case, &result);
    }
    out
}

/// What is read of an item's end and of a notes file's start, as
/// `golden_read.v1`.
pub(crate) fn read_transcript(b: &dyn Backend) -> String {
    let mut out = String::new();
    for (i, (file, views)) in item_cases().iter().enumerate() {
        for view in views {
            let case = format!("{i} {} {view:?}", file.len());
            line(&mut out, "item_props", &case, &b.item_props(file, *view));
        }
    }
    for (i, file) in head_files().iter().enumerate() {
        line(&mut out, "head_of", &i.to_string(), &b.head_of(file));
    }
    out
}

/// What the note made from a notes file and the locks come to, as
/// `golden_note.v1`.
pub(crate) fn note_transcript(b: &dyn Backend) -> String {
    let mut out = String::new();
    for (i, text) in notes().iter().enumerate() {
        for (t, title) in TITLES.iter().enumerate() {
            let result = b.write_note(text.as_bytes(), title);
            line(&mut out, "write_note", &format!("{i} {t}"), &result);
        }
        line(
            &mut out,
            "head",
            &i.to_string(),
            &b.note_head(text.as_bytes()),
        );
    }
    line(&mut out, "read_back", "-", &b.read_back(FIRST.as_bytes()));
    for (i, (service, text)) in lock_cases().iter().enumerate() {
        let case = format!("{i} {service}");
        line(
            &mut out,
            "upgrading",
            &case,
            &b.upgrading(*service, text).to_string(),
        );
    }
    out
}

/// The native modules as a [`Backend`].
struct Native;

fn native_fields(p: &joplin::Props, with_end: bool) -> String {
    Fields {
        kind: p.kind,
        parent: &p.parent,
        created: &p.created,
        updated: p.updated,
        user_created: &p.user_created,
        order: p.order,
        deleted: p.deleted,
        encrypted: p.encrypted,
        text_end: with_end.then_some(p.text_end),
    }
    .show()
}

fn native_head(h: &joplin::Head) -> String {
    format!("{} | {}", q(h.title.as_bytes()), q(h.author.as_bytes()))
}

fn sent(n: Option<Sent>) -> Option<(String, String)> {
    n.map(|n| (n.id, n.text))
}

impl Native {
    /// The steps a plugin's lists go through.
    fn keep_steps(new: &Mem) {
        let note = |id: &str, text: &str| Sent {
            id: id.to_string(),
            text: text.to_string(),
        };
        for (path, n) in [
            ("Notes/a.md", note("1", "t1")),
            ("Notes/b.md", note("2", "t2")),
            ("Notes/a.md", note("1", "t3")),
        ] {
            kept::keep_sent(new, path, &n).unwrap();
            kept::keep_sending(new, path, &n).unwrap();
        }
        kept::remove_sending(new, "Notes/b.md").unwrap();
        for path in ["Notes/a.md", "Notes/b.md", "Notes/a.md", "Notes/c.md"] {
            kept::add_waiting(new, path).unwrap();
        }
        kept::remove_waiting(new, "Notes/b.md").unwrap();
        kept::keep_notebook(new, "Books", "abc").unwrap();
    }
}

impl Backend for Native {
    fn consts(&self) -> String {
        format!(
            "{:?} {} {} {} {:?}",
            joplin::CLOUD,
            joplin::SYNC_VERSION,
            joplin::NOTE,
            joplin::NOTEBOOK,
            Service::KEYS
        )
    }

    fn service(&self, i: usize) -> String {
        let s = Service::ALL[i];
        format!(
            "{} {} {} {}",
            s.key(),
            s.next().key(),
            s.has_address(),
            s.signs_in()
        )
    }

    fn url(&self, i: usize, base: &str, name: &str) -> String {
        q(Service::ALL[i].url(base, name).unwrap().as_bytes())
    }

    fn locks_url(&self, i: usize, base: &str) -> String {
        q(Service::ALL[i].locks_url(base).unwrap().as_bytes())
    }

    fn of_key(&self, i: usize) -> String {
        Service::of_key(Service::ALL[i].key()).key().to_string()
    }

    fn new_id(&self, seed: &str, replaces: Option<&str>) -> String {
        let id = joplin::new_id(seed, replaces).unwrap();
        format!("{id} {}", joplin::file_name(&id).unwrap())
    }

    fn is_id(&self, text: &str) -> bool {
        joplin::is_id(text)
    }

    fn parse_props(&self, text: &str) -> String {
        native_fields(&joplin::parse_props(text).unwrap(), false)
    }

    fn notebook(&self, id: &str, title: &str, now: &str) -> String {
        q(joplin::notebook(id, title, now).unwrap().as_bytes())
    }

    fn note_props(&self, id: &str, parent: &str, author: &str, order: i64) -> String {
        let note = joplin::Note {
            id,
            parent,
            title: "T",
            author,
            created: "c",
            user_created: "u",
            updated: "x",
            order,
        };
        q(note.props().unwrap().as_bytes())
    }

    fn parse_head(&self, text: &str) -> String {
        native_head(&joplin::parse_head(text).unwrap())
    }

    fn kept_files(&self) -> Vec<(String, String)> {
        let new = Mem::default();
        Self::keep_steps(&new);
        let mut out = Vec::new();
        for name in [kept::SENT, kept::SENDING, kept::WAITING, kept::NOTEBOOK] {
            out.push((format!("file {name}"), q(&new.get(name).unwrap())));
        }
        out.push(("copy".into(), new.get("sent.txt.new").is_some().to_string()));
        let (a, x, b) = ("Notes/a.md", "Notes/x.md", "Notes/b.md");
        out.push((
            "sent a".into(),
            format!("{:?}", sent(kept::sent(&new, a).unwrap())),
        ));
        out.push((
            "sent x".into(),
            format!("{:?}", sent(kept::sent(&new, x).unwrap())),
        ));
        let sending = sent(kept::sending(&new, b).unwrap());
        out.push(("sending b".into(), format!("{sending:?}")));
        out.push((
            "waiting".into(),
            format!("{:?}", kept::waiting(&new).unwrap()),
        ));
        out.push((
            "notebook".into(),
            format!("{:?}", kept::notebook(&new).unwrap()),
        ));
        out
    }

    fn item_props(&self, file: &[u8], view: View) -> String {
        let size = file.len() as u64;
        let from = match view {
            View::Whole => 0,
            View::Tail => file.len().saturating_sub(joplin::TAIL),
            View::Have(n) => file.len() - n,
        };
        native_fields(&joplin::item_props(size, &file[from..]).unwrap(), true)
    }

    fn head_of(&self, file: &[u8]) -> String {
        native_head(&joplin::head_of(file).unwrap())
    }

    fn note_head(&self, text: &[u8]) -> String {
        let card = Mem::with("n.md", text);
        native_head(&native_note::head(&card, "n.md").unwrap())
    }

    fn write_note(&self, text: &[u8], title: &str) -> String {
        let note = a_note(title);
        let (card, data) = (Mem::with("from.md", text), Mem::default());
        let hash = native_note::write_note(&note, &card, "from.md", &data, "to.md").unwrap();
        format!("{hash} {}", q(&data.get("to.md").unwrap()))
    }

    fn read_back(&self, notes: &[u8]) -> String {
        // The note written from the first of the notes files, read back.
        let note = a_note("Alice");
        let (card, written) = (Mem::with("from.md", notes), Mem::default());
        let hash = native_note::write_note(&note, &card, "from.md", &written, "to.md").unwrap();
        let props = native_note::read_props(&written, "to.md").unwrap();
        let again = native_note::text_hash(&written, "to.md", props.text_end).unwrap();
        assert_eq!(again, hash);
        format!("{} {again}", native_fields(&props, true))
    }

    fn upgrading(&self, i: usize, text: &str) -> bool {
        let volume = Mem::with("l", text.as_bytes());
        native_locks::upgrading(Service::ALL[i], &volume, "l", NOW).unwrap()
    }
}

/// `got` line by line against the transcript kept.
fn same_as_golden(name: &str, got: &str, want: &str) {
    for (i, (got, want)) in got.lines().zip(want.lines()).enumerate() {
        assert_eq!(got, want, "{name} line {i}");
    }
    assert_eq!(got.lines().count(), want.lines().count(), "{name}");
}

#[test]
fn the_decisions_and_items_are_those_interface_6_gave() {
    same_as_golden("golden_joplin.v1", &joplin_transcript(&Native), JOPLIN);
}

#[test]
fn the_lists_kept_are_those_interface_6_wrote() {
    same_as_golden("golden_kept.v1", &kept_transcript(&Native), KEPT);
}

#[test]
fn what_is_read_of_a_file_is_what_interface_6_read() {
    same_as_golden("golden_read.v1", &read_transcript(&Native), READ);
}

#[test]
fn the_note_and_the_locks_are_those_interface_6_gave() {
    same_as_golden("golden_note.v1", &note_transcript(&Native), NOTE);
}

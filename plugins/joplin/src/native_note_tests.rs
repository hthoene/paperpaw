//! The note made from a notes file (`native_note`), the reads of the files of
//! a sync target and the locks (`native_locks`) against interface 6
//! (`item`, `markdown`, `locks`): the same bytes, hashes and decisions on the
//! same input, in any piece, and refusing instead of aborting when memory
//! runs short.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use crate::fail_alloc::sweep;
use crate::native_joplin::{self as joplin, Service};
use crate::native_locks;
use crate::native_note::{self as native, Body};
use crate::test_volume::Mem;
use crate::{item, locks, markdown, service};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::files::Volume;

const FIRST: &str = "# Alice\n\nLewis Carroll\n\n## Highlights\n\n> Curiouser!\n>\n> 12 %\n\n";

/// Notes files of every shape the body copy has a rule for.
fn notes() -> Vec<String> {
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
        let text = format!(
            "# T\n\n{}{}x{}\n\n\n",
            "y".repeat(at),
            "\n".repeat(30),
            "z".repeat(40)
        );
        out.push(text);
    }
    out.push(format!("# Long\n\n{}\n", "word ".repeat(5000)));
    out
}

fn old_note<'a>(n: &'a joplin::Note<'a>) -> item::Note<'a> {
    item::Note {
        id: n.id,
        parent: n.parent,
        title: n.title,
        author: n.author,
        created: n.created,
        user_created: n.user_created,
        updated: n.updated,
        order: n.order,
    }
}

fn a_note(title: &str) -> joplin::Note<'_> {
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

#[test]
fn a_note_is_written_as_interface_6_writes_it() {
    for text in notes() {
        for title in ["Alice", "  A \n long\ttitle ", ""] {
            let note = a_note(title);
            let old = MemoryStorage::new().with("from.md", text.as_str());
            let old_hash = old_note(&note).write(&old, "from.md", "to.md").unwrap();
            let (card, data) = (Mem::with("from.md", text.as_bytes()), Mem::default());
            let hash = native::write_note(&note, &card, "from.md", &data, "to.md").unwrap();
            assert_eq!(hash, old_hash, "{text:.40?}");
            assert_eq!(
                data.get("to.md").unwrap(),
                old.get("to.md").unwrap(),
                "{text:.40?}"
            );
            assert!(*data.longest.borrow() <= paperpaw_sdk::native::storage::MAX_DATA);
        }
    }
}

#[test]
fn a_note_is_read_back_by_its_properties_and_its_hash() {
    let note = a_note("Alice");
    let (card, data) = (Mem::with("from.md", FIRST.as_bytes()), Mem::default());
    let hash = native::write_note(&note, &card, "from.md", &data, "to.md").unwrap();
    let props = native::read_props(&data, "to.md").unwrap();
    let old = MemoryStorage::new().with("to.md", data.get("to.md").unwrap());
    let old_props = item::read_props(&old, "to.md").unwrap();
    assert_eq!(
        (
            props.kind,
            props.parent.as_str(),
            props.updated,
            props.text_end
        ),
        (
            old_props.kind,
            old_props.parent.as_str(),
            old_props.updated,
            old_props.text_end
        )
    );
    assert_eq!(props.user_created, note.user_created);
    assert_eq!(
        native::text_hash(&data, "to.md", props.text_end).unwrap(),
        hash
    );
    assert_eq!(
        hash,
        item::text_hash(&old, "to.md", old_props.text_end).unwrap()
    );
    assert_eq!(native::read_props(&data, "none.md"), Err(Error::NotFound));
}

#[test]
fn the_title_and_author_are_read_as_interface_6_reads_them() {
    for text in notes() {
        let old = MemoryStorage::new().with("n.md", text.as_str());
        let new = Mem::with("n.md", text.as_bytes());
        let (old, new) = (
            markdown::head(&old, "n.md").unwrap(),
            native::head(&new, "n.md").unwrap(),
        );
        assert_eq!((old.title, old.author), (new.title, new.author));
    }
}

#[test]
fn the_body_is_the_same_in_any_pieces() {
    for text in notes() {
        let want = {
            let mut whole = Vec::new();
            let mut body = Body::default();
            body.feed(text.as_bytes(), &mut |p| {
                whole.extend_from_slice(p);
                Ok(())
            })
            .unwrap();
            whole
        };
        for size in [1, 2, 3, 7, 64, 4096] {
            let mut body = Body::default();
            let mut got = Vec::new();
            for piece in text.as_bytes().chunks(size) {
                body.feed(piece, &mut |p| {
                    got.extend_from_slice(p);
                    Ok(())
                })
                .unwrap();
            }
            assert_eq!(got, want, "pieces of {size} of {text:.30?}");
            assert_eq!(body.written(), !got.is_empty());
        }
    }
}

#[test]
fn a_run_of_blank_lines_held_back_is_bounded() {
    let mut text = String::from("# T\n\nx");
    text.push_str(&"\n".repeat(100_000));
    text.push('y');
    let mut got = Vec::new();
    let mut body = Body::default();
    for piece in text.as_bytes().chunks(1024) {
        body.feed(piece, &mut |p| {
            got.extend_from_slice(p);
            Ok(())
        })
        .unwrap();
    }
    // At most the bounded number of breaks come between the texts.
    assert!(got.len() < 5000, "{}", got.len());
    assert!(got.starts_with(b"x\n") && got.ends_with(b"y"));
}

/// 2026-10-20 08:00:00 UTC.
const NOW: i64 = 1_792_483_200_000;

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

/// Both decisions on one listing: equal, and what they are.
fn both(service: Service, text: &str) -> bool {
    let old_service = match service {
        Service::WebDav => service::Service::WebDav,
        Service::Server => service::Service::Server,
        _ => service::Service::Cloud,
    };
    let old = MemoryStorage::new().with("l", text);
    let want = locks::upgrading(old_service, &old, "l", NOW).expect("read");
    let new = Mem::with("l", text.as_bytes());
    let got = native_locks::upgrading(service, &new, "l", NOW).unwrap();
    assert_eq!(got, want, "{text:.80?}");
    got
}

#[test]
fn exclusive_locks_in_a_webdav_folder_are_found_as_before() {
    let fresh = "Tue, 20 Oct 2026 07:58:30 GMT";
    let old = "Tue, 20 Oct 2026 07:56:30 GMT";
    let long = "x".repeat(64 * 1024);
    assert!(both(Service::WebDav, &listing(&[("2_1_abc.json", fresh)])));
    for (name, modified) in [
        ("2_1_abc.json", old),
        ("1_1_abc.json", fresh),
        ("2_1_abc.txt", fresh),
    ] {
        assert!(!both(Service::WebDav, &listing(&[(name, modified)])));
    }
    assert!(both(Service::WebDav, &listing(&[("2_1_ABC.JSON", fresh)])));
    assert!(!both(Service::WebDav, &listing(&[])));
    assert!(!both(Service::WebDav, ""));
    assert!(both(
        Service::WebDav,
        &listing(&[("1_2_m.json", fresh), ("2_3_cli.json", fresh)])
    ));
    // A huge name and date do not hide the lock that follows.
    assert!(both(
        Service::WebDav,
        &listing(&[(long.as_str(), long.as_str()), ("2_1_abc.json", fresh)])
    ));
}

#[test]
fn exclusive_locks_of_joplin_server_are_found_as_before() {
    assert!(both(Service::Server, &server_list(&[(2, 30_000)])));
    assert!(!both(
        Service::Server,
        &server_list(&[(2, native_locks::TTL + 1)])
    ));
    assert!(!both(Service::Server, &server_list(&[(1, 30_000)])));
    assert!(both(
        Service::Server,
        &server_list(&[(1, 30_000), (2, 60_000)])
    ));
    assert!(!both(Service::Server, &server_list(&[])));
    assert!(!both(Service::Server, "not json"));
    assert!(!both(Service::Cloud, "{}"));
}

/// A read-only volume that allocates nothing, so a sweep refuses only the
/// plugin's own allocations.
struct Fixed(&'static [u8]);

impl Volume for Fixed {
    fn size(&self, _: &str) -> Result<u64, Error> {
        Ok(self.0.len() as u64)
    }
    fn read_at(&self, _: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        let from = usize::try_from(at)
            .map_err(|_| Error::Invalid)?
            .min(self.0.len());
        let n = out.len().min(self.0.len() - from);
        out[..n].copy_from_slice(&self.0[from..from + n]);
        Ok(n)
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), Error> {
        Ok(())
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), Error> {
        Ok(())
    }
    fn rename(&self, _: &str, _: &str) -> Result<(), Error> {
        Ok(())
    }
    fn remove(&self, _: &str) -> Result<(), Error> {
        Ok(())
    }
    fn sync(&self) -> Result<(), Error> {
        Ok(())
    }
}

const XML: &str = "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\"><d:response>\
    <d:href>/dav/locks/2_1_abc.json</d:href><d:propstat><d:prop><d:getlastmodified>\
    Tue, 20 Oct 2026 07:58:30 GMT</d:getlastmodified></d:prop></d:propstat></d:response>\
    </d:multistatus>";
const JSON: &str = "{\"items\":[{\"type\":2,\"updatedTime\":1792483170000}],\"has_more\":false}";

#[test]
fn refused_memory_ends_in_no_memory_never_an_abort() {
    let xml = Fixed(XML.as_bytes());
    sweep(|| native_locks::upgrading(Service::WebDav, &xml, "l", NOW));
    let json = Fixed(JSON.as_bytes());
    sweep(|| native_locks::upgrading(Service::Server, &json, "l", NOW));
    let notes = Fixed(FIRST.as_bytes());
    sweep(|| native::head(&notes, "n").map(|h| h.title.len()));
    sweep(|| native::text_hash(&notes, "n", 20));
    sweep(|| native::read_props(&notes, "n").map(|p| p.parent.len()));
    // The body copy and the note: what is written is counted, not kept.
    let note = a_note("Alice");
    sweep(|| native::write_note(&note, &notes, "n", &Fixed(&[]), "to").map(|h| h.len()));
    sweep(|| {
        let mut body = Body::default();
        let mut n = 0;
        body.feed(b"# T\n\ntext\n\n\nmore", &mut |p| {
            n += p.len();
            Ok(())
        })?;
        Ok(n)
    });
}

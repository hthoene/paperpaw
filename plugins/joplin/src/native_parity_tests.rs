//! The native decisions (`native_joplin`) against the interface-6 ones
//! (`service`, `item`, `markdown`): equal on the same input, and refusing
//! instead of aborting when memory runs short.
use crate::fail_alloc::sweep;
use crate::native_joplin as native;
use crate::{item, markdown, service};
use alloc::format;

#[test]
fn services_are_those_of_interface_6() {
    assert_eq!(native::CLOUD, service::CLOUD);
    assert_eq!(native::Service::KEYS, service::Service::KEYS);
    assert_eq!(native::SYNC_VERSION, item::SYNC_VERSION);
    assert_eq!(
        (native::NOTE, native::NOTEBOOK),
        (item::NOTE, item::NOTEBOOK)
    );
    for (old, new) in service::Service::ALL.into_iter().zip(native::Service::ALL) {
        assert_eq!(old.key(), new.key());
        assert_eq!(old.next().key(), new.next().key());
        assert_eq!(old.has_address(), new.has_address());
        assert_eq!(old.signs_in(), new.signs_in());
        for base in ["https://j.example", "http://h:8080/x", ""] {
            for name in ["info.json", "a b.md", ""] {
                assert_eq!(old.url(base, name), new.url(base, name).unwrap());
            }
            assert_eq!(old.locks_url(base), new.locks_url(base).unwrap());
        }
        assert_eq!(native::Service::of_key(new.key()).key(), old.key());
    }
    assert_eq!(
        native::Service::of_key("anything else"),
        native::Service::Cloud
    );
}

#[test]
fn ids_and_file_names_are_those_of_interface_6() {
    for (seed, replaces) in [
        ("a book 1", None),
        ("a book 1", Some("abc")),
        ("", None),
        ("ü", Some("")),
    ] {
        let id = item::new_id(seed, replaces);
        assert_eq!(native::new_id(seed, replaces).unwrap(), id);
        assert!(native::is_id(&id));
        assert_eq!(native::file_name(&id).unwrap(), item::file_name(&id));
    }
    for text in [
        "",
        "info",
        &"g".repeat(32),
        &"a".repeat(31),
        &"A".repeat(32),
        &"0".repeat(33),
    ] {
        assert_eq!(native::is_id(text), item::is_id(text), "{text:?}");
    }
}

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

#[test]
fn properties_are_read_as_before() {
    let trashed = JOPLIN_NOTE.replace("deleted_time: 0", "deleted_time: 1792448650090");
    let encrypted = JOPLIN_NOTE.replace("encryption_applied: 0", "encryption_applied: 1");
    for text in [
        JOPLIN_NOTE,
        &trashed,
        &encrypted,
        "",
        "type_: 2",
        "no props at all",
        "title\n\nbody\n\nparent_id: x\nupdated_time: bad\norder: z\ntype_: q",
        "a: b\n\nupdated_time: 2026-10-19T22:24:11.708Z\n\n",
        "x\n\nkey without colon\nparent_id : spaced \n deleted_time:  \ntype_ : 1\n",
    ] {
        let old = item::parse_props(text);
        let new = native::parse_props(text).unwrap();
        assert_eq!(
            (old.kind, &old.parent, &old.created, old.updated),
            (new.kind, &new.parent, &new.created, new.updated),
            "{text:?}"
        );
        assert_eq!(
            (&old.user_created, old.order, old.deleted, old.encrypted),
            (&new.user_created, new.order, new.deleted, new.encrypted),
            "{text:?}"
        );
    }
}

#[test]
fn notebooks_and_notes_are_written_as_before() {
    let now = "2026-10-19T22:24:10.090Z";
    for title in ["Books", " Alice's\n Adventures ", "", "ünï\t😀", "  "] {
        let id = "7db793874c624138b3b4520ae5cdf20c";
        assert_eq!(
            native::notebook(id, title, now).unwrap(),
            item::notebook(id, title, now)
        );
    }
    for (author, order) in [
        ("", 0),
        ("Lewis Carroll", 1_792_448_650_089),
        ("a\nb\\nc\r\\r\\\\n", -5),
        ("ü", i64::MIN),
    ] {
        let (id, parent) = (
            "1ae79351dd744c2794eebb334b1629a7",
            "7db793874c624138b3b4520ae5cdf20c",
        );
        let old = item::Note {
            id,
            parent,
            title: "T",
            author,
            created: "c",
            user_created: "u",
            updated: "x",
            order,
        };
        let new = native::Note {
            id,
            parent,
            title: "T",
            author,
            created: "c",
            user_created: "u",
            updated: "x",
            order,
        };
        assert_eq!(new.props().unwrap(), old.props(), "{author:?}");
    }
    for text in [
        "", "a\nb", "a\\nb", "\\n\\r", "\\\\n", "\\", "\r\n", "ü\\nü", "\\\\\\n",
    ] {
        let old = item::Note {
            id: "i",
            parent: "p",
            title: "T",
            author: text,
            created: "c",
            user_created: "u",
            updated: "x",
            order: 0,
        };
        let new = native::Note {
            id: "i",
            parent: "p",
            title: "T",
            author: text,
            created: "c",
            user_created: "u",
            updated: "x",
            order: 0,
        };
        assert_eq!(new.props().unwrap(), old.props(), "{text:?}");
        assert!(
            old.props()
                .contains(&format!("author: {}", item::escape(text)))
        );
    }
    assert_eq!(item::one_line(" a  b\n c "), "a b c");
}

#[test]
fn the_head_of_notes_is_read_as_before() {
    for text in [
        "# Alice's Adventures\n\nLewis Carroll\n\n## Highlights\n\n> x\n",
        "# Title\n\n## Bookmarks\n",
        "",
        "Author first\n# Title\nAuthor\n",
        "# T1\n# T2\nA\nB\n",
        "  # Indented\n  Author  \n",
        "## Only a heading\n# After\nA\n",
    ] {
        let old = markdown::parse_head(text);
        let new = native::parse_head(text).unwrap();
        assert_eq!((old.title, old.author), (new.title, new.author), "{text:?}");
    }
}

#[test]
fn refused_allocations_end_in_no_memory() {
    sweep(|| native::new_id("a book 1", Some("old")).map(|id| id.len()));
    sweep(|| {
        native::Service::Server
            .url("https://j.example", "info.json")
            .map(|u| u.len())
    });
    sweep(|| {
        native::Service::WebDav
            .locks_url("https://j.example")
            .map(|u| u.len())
    });
    sweep(|| native::parse_props(JOPLIN_NOTE).map(|p| p.kind));
    sweep(|| native::notebook("id", " A  B ", "now").map(|t| t.len()));
    let note = native::Note {
        id: "i",
        parent: "p",
        title: "T",
        author: "a\nb",
        created: "c",
        user_created: "u",
        updated: "x",
        order: 7,
    };
    sweep(|| note.props().map(|t| t.len()));
    sweep(|| native::parse_head("# Title\n\nAuthor\n").map(|h| h.title.len()));
}

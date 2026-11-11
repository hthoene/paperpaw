//! Tests of the write ops (ops 8-12, 14, 17-21, 23, 24, 26): status, files,
//! positions, fields and provided metadata. Transactions are in `edit.rs`.
use super::*;
use crate::native_host::lib_service::write as w;
use crate::native_host::lib_service::{
    query::LibQuery,
    write::{FieldKind, ImportRequest, LibWrite, StoredPosition, Transactions},
};
use alloc::rc::Rc;
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{
    ERR_FULL, ERR_INVALID, ERR_IO, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE,
};
use std::{collections::BTreeMap, string::String};
use wire::{
    ServiceLibFieldSet as FieldSet, ServiceLibImport as Import, ServiceLibMove as Move,
    ServiceLibPaths as Paths, ServiceLibPosition as Position, ServiceLibPositionSet as PositionSet,
    ServiceLibProvided as Provided, ServiceLibRef as Ref, ServiceLibStatus as Status,
};

#[path = "edit.rs"]
pub(super) mod edit;

/// The host's side, recording what it was asked.
pub(super) struct Fake {
    pub(super) log: RefCell<std::vec::Vec<String>>,
    pub(super) transactions: Transactions,
    pub(super) now: Cell<u64>,
    pub(super) stored: Cell<Option<StoredPosition>>,
    pub(super) facts: RefCell<BTreeMap<String, (i32, u32)>>,
    pub(super) library: RefCell<String>,
    pub(super) stage_fails: Cell<Option<i32>>,
    pub(super) commit: Cell<Result<w::Committed, i32>>,
    pub(super) fields: RefCell<BTreeMap<String, bool>>,
    pub(super) receipts: RefCell<std::vec::Vec<(u32, u32, u32)>>,
}
impl Fake {
    pub(super) fn new() -> Rc<Self> {
        Rc::new(Self {
            log: RefCell::new(std::vec::Vec::new()),
            transactions: Transactions::try_new().unwrap(),
            now: Cell::new(1_000),
            stored: Cell::new(None),
            facts: RefCell::new(BTreeMap::new()),
            library: RefCell::new(String::from("Books")),
            stage_fails: Cell::new(None),
            commit: Cell::new(Ok(w::Committed {
                annotation_generation: 9,
                edits: 0,
                added: 0,
            })),
            fields: RefCell::new(BTreeMap::from([(String::from("files.rating"), true)])),
            receipts: RefCell::new(std::vec::Vec::new()),
        })
    }
    pub(super) fn note(&self, line: impl Into<String>) {
        self.log.borrow_mut().push(line.into());
    }
    pub(super) fn last(&self) -> String {
        self.log.borrow().last().cloned().unwrap_or_default()
    }
}
impl LibQuery for Fake {}
impl super::super::LibExcluded for Fake {
    /// A short alias the fake card resolves to `.paperpaw`.
    fn excluded(&self, first: &str) -> bool {
        first == "PAPERP~1"
    }
}
impl LibWrite for Fake {
    fn resolve_book(&self, book: BookRef<'_>) -> Result<w::BookId, i32> {
        match book {
            BookRef::Path("Books/gone.epub") => Err(ERR_NOT_FOUND),
            // The book of `book()`; any other path is a book of its own.
            BookRef::Path("Books/a.epub") => Ok(w::BookId(77)),
            BookRef::Path(path) => Ok(w::BookId(1000 + path.len() as u64)),
            _ => Ok(w::BookId(77)),
        }
    }
    fn book_path(&self, _: BookRef<'_>, visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        visit("Books/a.epub");
        Ok(())
    }
    fn set_status(&self, book: BookRef<'_>, set: u32, clear: u32) -> Result<(), i32> {
        self.note(std::format!("status {book:?} {set} {clear}"));
        Ok(())
    }
    fn can_delete(&self, path: &str) -> (i32, u32) {
        self.note(std::format!("can_delete {path}"));
        self.facts.borrow().get(path).copied().unwrap_or((0, 0))
    }
    fn delete(&self, path: &str) -> (i32, u32) {
        self.note(std::format!("delete {path}"));
        (0, 0)
    }
    fn move_book(&self, from: &str, to: &str, replace: bool) -> Result<(), i32> {
        self.note(std::format!("move {from} {to} {replace}"));
        Ok(())
    }
    fn library_folder(&self, visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        visit(&self.library.borrow());
        Ok(())
    }
    fn import(
        &self,
        who: &Caller<'_>,
        request: &ImportRequest<'_>,
        visit: &mut dyn FnMut(&str),
    ) -> Result<(), i32> {
        self.note(std::format!(
            "import {} {} {} {} {} {}",
            who.plugin,
            request.from,
            request.folder,
            request.name,
            request.replace,
            request.free_name
        ));
        visit("Books/taken.epub");
        Ok(())
    }
    fn stored_position(&self, _: BookRef<'_>) -> Result<Option<StoredPosition>, i32> {
        Ok(self.stored.get())
    }
    fn write_position(&self, _: BookRef<'_>, p: &Position<'_>, from_sync: bool) -> Result<(), i32> {
        self.note(std::format!(
            "position {} {} {from_sync}",
            p.section,
            p.updated
        ));
        Ok(())
    }
    fn field_kind(&self, field: &str) -> Result<FieldKind, i32> {
        self.fields
            .borrow()
            .get(field)
            .map(|in_catalog| FieldKind {
                in_catalog: *in_catalog,
            })
            .ok_or(ERR_NOT_FOUND)
    }
    fn set_field(&self, _: BookRef<'_>, field: &str, value: &str) -> Result<(), i32> {
        self.note(std::format!("field {field}={value}"));
        Ok(())
    }
    fn provide(&self, _: BookRef<'_>, p: &Provided<'_>) -> Result<(), i32> {
        self.note(std::format!("provide {} {}", p.flags, p.tags));
        Ok(())
    }
    fn now_ms(&self) -> u64 {
        self.now.get()
    }
    fn transactions(&self) -> Option<&Transactions> {
        Some(&self.transactions)
    }
    fn stage(&self, transaction: u32, item: &w::StageItem<'_>) -> Result<(), i32> {
        if let Some(error) = self.stage_fails.get() {
            return Err(error);
        }
        match item {
            w::StageItem::Edit { index, edit } => {
                self.note(std::format!(
                    "stage {transaction} edit {index} action {}",
                    edit.action
                ));
            }
            w::StageItem::Part {
                edit,
                field,
                offset,
                data,
                last,
            } => self.note(std::format!(
                "stage {transaction} part {edit}/{field}@{offset}+{} {last}",
                data.len()
            )),
        }
        Ok(())
    }
    fn abort_staged(&self, transaction: u32) {
        self.note(std::format!("abort {transaction}"));
    }
    fn commit(&self, request: &w::CommitRequest) -> Result<w::Committed, i32> {
        self.note(std::format!(
            "commit {} owner {} base {} edits {}",
            request.transaction,
            request.owner,
            request.base_generation,
            request.edits
        ));
        self.commit.get().map(|c| w::Committed {
            edits: if c.edits == 0 { request.edits } else { c.edits },
            ..c
        })
    }
    fn receipt(
        &self,
        _: &Caller<'_>,
        transaction: u32,
        cursor: u32,
        visit: &mut dyn FnMut(Record<'_>) -> core::ops::ControlFlow<()>,
    ) -> Result<crate::abi7::context::RowsEnd, i32> {
        let rows = self.receipts.borrow();
        if rows.is_empty() || transaction != 4 {
            return Err(ERR_NOT_FOUND);
        }
        let total = u32::try_from(rows.len()).unwrap();
        let mut next = cursor.min(total);
        for (edit, id, action) in rows.iter().skip(next as usize) {
            let row = Record::ServiceLibReceipt(wire::ServiceLibReceipt {
                edit: *edit,
                id: *id,
                action: *action,
            });
            if visit(row).is_break() {
                break;
            }
            next += 1;
        }
        Ok(crate::abi7::context::RowsEnd {
            total,
            generation: transaction,
            next,
        })
    }
}

pub(super) fn host(fake: &Rc<Fake>) -> Rc<dyn LibHost> {
    fake.clone()
}
pub(super) fn book() -> Record<'static> {
    Record::ServiceLibRef(Ref {
        path: "Books/a.epub",
        generation: 0,
        key: None,
    })
}
/// Ops 9 and 10 answer one `lib_result` per path and one `end`; the reply is
/// checked against the generated schema like any other.
pub(super) fn go_rows(
    fake: &Rc<Fake>,
    op: u32,
    records: &[Record<'_>],
) -> Result<std::vec::Vec<u8>, i32> {
    go(fake, op, records)
}
pub(super) fn go(
    fake: &Rc<Fake>,
    op: u32,
    records: &[Record<'_>],
) -> Result<std::vec::Vec<u8>, i32> {
    call(&host(fake), op, records)
}
fn paths(paths: &str) -> Record<'_> {
    Record::ServiceLibPaths(Paths { paths, flags: 0 })
}
fn result_of(bytes: &[u8]) -> wire::ServiceLibResult<'_> {
    let Record::ServiceLibResult(result) = wire::record_decode_one(bytes).unwrap() else {
        panic!("not a lib_result")
    };
    result
}
fn results(bytes: &[u8]) -> std::vec::Vec<(String, i32, u32)> {
    let mut rows = std::vec::Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let (record, used) = wire::record_decode_prefix(&bytes[at..]).unwrap();
        at += used;
        match record {
            Record::ServiceLibResult(r) => rows.push((r.path.into(), r.status, r.flags)),
            Record::ServiceEnd(end) => {
                assert_eq!(at, bytes.len(), "the end is last");
                assert_eq!(
                    (end.total as usize, end.next, end.generation),
                    (rows.len(), 0, 0)
                );
            }
            _ => panic!("not a result"),
        }
    }
    rows
}

#[test]
fn set_status_takes_a_book_then_both_masks_once() {
    let fake = Fake::new();
    let status = |set, clear| Record::ServiceLibStatus(Status { set, clear });
    assert!(go(&fake, 8, &[book(), status(0b001, 0b110)]).is_ok());
    assert_eq!(fake.last(), "status Path(\"Books/a.epub\") 1 6");
    let by_key = Record::ServiceLibRef(Ref {
        path: "",
        generation: 4,
        key: Some(12),
    });
    assert!(go(&fake, 8, &[by_key, status(0, 0)]).is_ok());
    assert_eq!(fake.last(), "status Key { key: 12, generation: 4 } 0 0");
    let before = fake.log.borrow().len();
    for bad in [
        std::vec![status(1, 0)],
        std::vec![book()],
        std::vec![status(1, 0), book()],
        std::vec![book(), book()],
        std::vec![book(), status(0b1000, 0)],
        std::vec![book(), status(0, 0b1000)],
        std::vec![book(), status(0b010, 0b010)],
        std::vec![
            Record::ServiceLibRef(Ref {
                path: "",
                generation: 0,
                key: None
            }),
            status(0, 0)
        ],
        std::vec![
            Record::ServiceLibRef(Ref {
                path: "a/b.epub",
                generation: 3,
                key: Some(1)
            }),
            status(0, 0)
        ],
        std::vec![
            Record::ServiceLibRef(Ref {
                path: "../a.epub",
                generation: 0,
                key: None
            }),
            status(0, 0)
        ],
    ] {
        assert_eq!(go(&fake, 8, &bad), Err(ERR_INVALID), "{bad:?}");
    }
    assert_eq!(
        go(&fake, 8, &[book(), status(1, 0), status(1, 0)]).map(drop),
        Err(ERR_NO_SPACE)
    );
    // A key from no generation is stale.
    let stale = Record::ServiceLibRef(Ref {
        path: "",
        generation: 0,
        key: Some(1),
    });
    assert_eq!(go(&fake, 8, &[stale, status(0, 0)]), Err(ERR_STATE));
    assert_eq!(fake.log.borrow().len(), before, "nothing reached the host");
}

#[test]
fn can_delete_answers_one_result_per_path_and_changes_nothing() {
    let fake = Fake::new();
    fake.facts.borrow_mut().insert("a.epub".into(), (0, 0b011));
    fake.facts
        .borrow_mut()
        .insert("b.txt".into(), (ERR_INVALID, 0));
    let reply = go_rows(&fake, 9, &[paths("a.epub\nb.txt\nc.epub")]).unwrap();
    assert_eq!(
        results(&reply),
        [
            ("a.epub".into(), 0, 3),
            ("b.txt".into(), ERR_INVALID, 0),
            ("c.epub".into(), 0, 0)
        ]
    );
    assert!(!fake.log.borrow().iter().any(|l| l.starts_with("delete")));
    // Seven paths of 512 bytes fit; eight do not.
    let long = |n: usize| -> String { std::format!("{:0>511}", n) + "x" };
    let seven: std::vec::Vec<String> = (0..7).map(long).collect();
    assert_eq!(seven.join("\n").len(), 7 * 512 + 6);
    assert!(go_rows(&fake, 9, &[paths(&seven.join("\n"))]).is_ok());
    assert_eq!(
        go(&fake, 9, &[paths("a\nb\nc\nd\ne\nf\ng\nh")]),
        Err(ERR_NO_SPACE)
    );
    for bad in [
        "", "a\n\nb", "a\n", "\na", "../a", "a/../b", "a\u{0}b", "/abs", "a\rb",
    ] {
        assert_eq!(go(&fake, 9, &[paths(bad)]), Err(ERR_INVALID), "{bad:?}");
    }
    // Flag bit 0 (with_notes) is delete's.
    assert_eq!(
        go(
            &fake,
            9,
            &[Record::ServiceLibPaths(Paths {
                paths: "a",
                flags: 1
            })]
        ),
        Err(ERR_INVALID)
    );
}

#[test]
fn delete_needs_storage_and_keeps_a_book_with_notes_unless_told() {
    let fake = Fake::new();
    fake.facts
        .borrow_mut()
        .insert("noted.epub".into(), (0, 0b001));
    fake.facts
        .borrow_mut()
        .insert("gone.epub".into(), (ERR_NOT_FOUND, 0));
    fake.facts
        .borrow_mut()
        .insert("txt.txt".into(), (ERR_INVALID, 0));
    fake.facts
        .borrow_mut()
        .insert("bad.epub".into(), (ERR_IO, 0));
    let del = |paths, flags| {
        go_rows(
            &fake,
            10,
            &[Record::ServiceLibPaths(Paths { paths, flags })],
        )
    };
    let reply = del("noted.epub\nplain.epub\ngone.epub\ntxt.txt\nbad.epub", 0).unwrap();
    assert_eq!(
        results(&reply),
        [
            ("noted.epub".into(), ERR_STATE, 1),
            ("plain.epub".into(), 0, 0),
            ("gone.epub".into(), 0, 0),
            ("txt.txt".into(), ERR_INVALID, 0),
            ("bad.epub".into(), ERR_IO, 0),
        ]
    );
    let deleted: std::vec::Vec<String> = fake
        .log
        .borrow()
        .iter()
        .filter(|l| l.starts_with("delete"))
        .cloned()
        .collect();
    assert_eq!(
        deleted,
        ["delete plain.epub", "delete gone.epub"],
        "a gone book is only forgotten"
    );
    let reply = del("noted.epub", 1).unwrap();
    assert_eq!(results(&reply), [("noted.epub".into(), 0, 0)]);
    assert_eq!(fake.last(), "delete noted.epub");
    assert_eq!(del("a", 2), Err(ERR_INVALID));
    // Without `uses.storage` nothing is touched, can_delete excepted.
    STORAGE.with(|s| s.set(false));
    let before = fake.log.borrow().len();
    assert_eq!(del("plain.epub", 0), Err(ERR_DENIED));
    assert_eq!(fake.log.borrow().len(), before);
    assert!(
        go(
            &fake,
            9,
            &[Record::ServiceLibPaths(Paths {
                paths: "plain.epub",
                flags: 0
            })]
        )
        .is_ok()
    );
}

#[test]
fn move_checks_both_paths_and_needs_storage() {
    let fake = Fake::new();
    let mv = |from, to, flags| {
        go(
            &fake,
            11,
            &[Record::ServiceLibMove(Move { from, to, flags })],
        )
    };
    let reply = mv("a/x.epub", "b/x.epub", 1).unwrap();
    assert_eq!(
        (result_of(&reply).path, result_of(&reply).status),
        ("b/x.epub", 0)
    );
    assert_eq!(fake.last(), "move a/x.epub b/x.epub true");
    assert_eq!(mv("a", "a", 0), Err(ERR_INVALID));
    assert_eq!(mv("", "b", 0), Err(ERR_INVALID));
    assert_eq!(mv("a", "../b", 0), Err(ERR_INVALID));
    assert_eq!(mv("a", "b", 2), Err(ERR_INVALID));
    STORAGE.with(|s| s.set(false));
    assert_eq!(mv("a", "b", 0), Err(ERR_DENIED));
}

fn import(fake: &Rc<Fake>, folder: &str, name: &str, flags: u32) -> Result<std::vec::Vec<u8>, i32> {
    go(
        fake,
        12,
        &[Record::ServiceLibImport(Import {
            from: "downloads/f.epub",
            folder,
            name,
            flags,
        })],
    )
}

#[test]
fn import_names_come_from_outside_and_are_checked_before_anything_is_written() {
    let fake = Fake::new();
    let reply = import(&fake, "news/2026", "Issue 12.epub", 0).unwrap();
    assert_eq!(result_of(&reply).path, "Books/taken.epub");
    assert_eq!(
        fake.last(),
        "import files downloads/f.epub news/2026 Issue 12.epub false false"
    );
    assert!(import(&fake, "", "a.epub", 1).is_ok());
    assert_eq!(
        fake.last(),
        "import files downloads/f.epub  a.epub true false"
    );
    assert!(import(&fake, "", "a.epub", 2).is_ok());
    assert_eq!(
        fake.last(),
        "import files downloads/f.epub  a.epub false true"
    );
    let before = fake.log.borrow().len();
    for (folder, name) in [
        ("", ""),
        ("a//b", "x.epub"),
        ("/a", "x.epub"),
        ("a/", "x.epub"),
        ("a", "x/y.epub"),
        ("a", "x\\y.epub"),
        ("a", ".."),
        ("a", "."),
        ("./a", "x.epub"),
        ("a/../b", "x.epub"),
        ("a", "x\u{0}.epub"),
        ("a", "x\n.epub"),
        ("a", "x\t.epub"),
        ("a", "x\u{7f}.epub"),
        ("a", "q\"x.epub"),
        ("a", "q*x.epub"),
        ("a", "q:x.epub"),
        ("a", "q<x.epub"),
        ("a", "q>x.epub"),
        ("a", "q?x.epub"),
        ("a", "q|x.epub"),
        ("a", "name."),
        ("a", "name "),
        ("a", "CON"),
        ("a", "nul.epub"),
        ("a", "Com1.tar.gz"),
        ("a", "LPT9"),
        ("COM3", "x.epub"),
        ("a", ".hidden.epub"),
        ("a", "~tmp.epub"),
    ] {
        assert_eq!(
            import(&fake, folder, name, 0),
            Err(ERR_INVALID),
            "{folder:?} {name:?}"
        );
    }
    // A segment is at most 255 UTF-16 units: 255 ASCII fit, 256 do not; a
    // character outside the BMP is two units.
    let ascii = |n: usize| "a".repeat(n - 5) + ".epub";
    assert!(import(&fake, "", &ascii(255), 0).is_ok());
    assert_eq!(import(&fake, "", &ascii(256), 0), Err(ERR_INVALID));
    let wide = "\u{1f4da}".repeat(126) + ".epub"; // 252 + 5 = 257 units
    assert_eq!(import(&fake, "", &wide, 0), Err(ERR_INVALID));
    let wide = "\u{1f4da}".repeat(125) + ".epub"; // 250 + 5 = 255 units, 505 bytes
    assert!(import(&fake, "", &wide, 0).is_ok());
    // A FAT name of 255 units of 3-byte characters is 765 bytes: it fits the wire
    // (`lib_import.name`) but not a 512-byte root-1 path, so the answer's path
    // could not carry it: refused as too long, not as a bad name.
    let cjk = "\u{4e66}".repeat(255);
    assert_eq!(cjk.len(), 765);
    assert_eq!(import(&fake, "", &cjk, 0), Err(ERR_NO_SPACE));
    assert!(import(&fake, "", &"\u{4e66}".repeat(160), 0).is_ok());
    // Contradictory or unknown flags.
    assert_eq!(import(&fake, "", "a.epub", 3), Err(ERR_INVALID));
    assert_eq!(import(&fake, "", "a.epub", 4), Err(ERR_INVALID));
    let after = fake.log.borrow().len();
    assert_eq!(
        after - before,
        3,
        "only the three good imports above reached the host"
    );
}

#[test]
fn import_never_lands_in_the_hosts_folder_or_beyond_a_path() {
    let fake = Fake::new();
    *fake.library.borrow_mut() = String::new();
    assert_eq!(import(&fake, ".paperpaw", "x.epub", 0), Err(ERR_INVALID));
    assert_eq!(
        import(&fake, ".PaperPaw/cache", "x.epub", 0),
        Err(ERR_INVALID)
    );
    assert!(import(&fake, "paperpaw", "x.epub", 0).is_ok());
    *fake.library.borrow_mut() = ".paperpaw".into();
    assert_eq!(import(&fake, "", "x.epub", 0), Err(ERR_INVALID));
    *fake.library.borrow_mut() = "Books".into();
    assert!(
        import(&fake, ".paperpaw", "x.epub", 0).is_ok(),
        "below the library folder it is a folder like any"
    );
    // The joined path must fit the 512 bytes of lib_result.path.
    let folder = ["f".repeat(200), "g".repeat(200)].join("/"); // 401
    assert_eq!(
        import(&fake, &folder, &"n".repeat(120), 0),
        Err(ERR_NO_SPACE)
    );
    assert!(import(&fake, &folder, &"n".repeat(90), 0).is_ok());
}

fn paths_op(fake: &Rc<Fake>, paths: &str, op: u32) -> Result<std::vec::Vec<u8>, i32> {
    go_rows(
        fake,
        op,
        &[Record::ServiceLibPaths(Paths { paths, flags: 0 })],
    )
}

#[test]
fn no_library_path_reaches_plugins_or_the_hosts_folder() {
    // The same as root 1 of the files service: by identity, not spelling.
    let fake = Fake::new();
    let before = fake.log.borrow().len();
    let excluded = [
        "plugins/x/lib.so",
        "Plugins/x.epub",
        "PLUGINS",
        ".paperpaw/library/keys",
        ".PaperPaw/x.epub",
        "PAPERP~1/x.epub",
        // The kernel's settings file: its own 8.3 name, in any case.
        "paperpaw.ini",
        "PAPERPAW.INI",
        "Paperpaw.Ini",
        "paperpaw.ını",
    ];
    for path in excluded {
        // One line among others refuses the whole list, before any is touched.
        let list = std::format!("ok.epub\n{path}");
        assert_eq!(paths_op(&fake, &list, 10), Err(ERR_INVALID), "{path}");
        assert_eq!(paths_op(&fake, path, 9), Err(ERR_INVALID));
        let mv = |from, to| {
            go(
                &fake,
                11,
                &[Record::ServiceLibMove(Move { from, to, flags: 0 })],
            )
        };
        assert_eq!(mv(path, "b.epub"), Err(ERR_INVALID), "from {path}");
        assert_eq!(mv("a.epub", path), Err(ERR_INVALID), "to {path}");
        let from = go(
            &fake,
            12,
            &[Record::ServiceLibImport(Import {
                from: path,
                folder: "",
                name: "x.epub",
                flags: 0,
            })],
        );
        // The source is the caller's root 0: its own `plugins/…` or
        // `.paperpaw/…` are plain data there, and import it.
        assert!(from.is_ok(), "import from {path}");
        let book = go(
            &fake,
            14,
            &[
                Record::ServiceLibRef(wire::ServiceLibRef {
                    path,
                    generation: 0,
                    key: None,
                }),
                position(1, 0),
                Record::ServiceLibPositionSet(PositionSet { flags: 0 }),
            ],
        );
        assert_eq!(book, Err(ERR_INVALID), "position of {path}");
    }
    // Only the imports (a root-0 source, a fine destination) reached the host.
    assert!(
        fake.log.borrow()[before..]
            .iter()
            .all(|line| line.starts_with("import")),
        "nothing else was touched"
    );
    // Into the library folder at the card's root: plugins and the host's folder.
    *fake.library.borrow_mut() = String::new();
    for folder in [
        "plugins",
        "Plugins/x",
        "PAPERP~1",
        ".paperpaw",
        "PAPERPAW.INI",
    ] {
        assert_eq!(
            import(&fake, folder, "x.epub", 0),
            Err(ERR_INVALID),
            "{folder}"
        );
    }
    assert_eq!(
        import(&fake, "", "plugins", 0),
        Err(ERR_INVALID),
        "a file named so"
    );
    for name in ["paperpaw.ini", "PAPERPAW.INI", "Paperpaw.ini"] {
        assert_eq!(
            import(&fake, "", name, 0),
            Err(ERR_INVALID),
            "a file named {name} at the top"
        );
    }
    *fake.library.borrow_mut() = "Plugins".into();
    assert_eq!(
        import(&fake, "", "x.epub", 0),
        Err(ERR_INVALID),
        "library folder"
    );
    *fake.library.borrow_mut() = "Books".into();
    assert!(
        import(&fake, "plugins", "x.epub", 0).is_ok(),
        "a folder like any below it"
    );
    assert!(paths_op(&fake, "Books/plugins/x.epub", 10).is_ok());
}

fn position(updated: u64, flags: u32) -> Record<'static> {
    Record::ServiceLibPosition(Position {
        section: 3,
        offset: 40,
        insert: 0,
        progress_ppm: 500_000,
        conversion: 0,
        updated,
        xpointer: if flags & 1 != 0 { "/body/p[3]" } else { "" },
        flags,
    })
}
fn set_position(fake: &Rc<Fake>, updated: u64, how: u32) -> Result<std::vec::Vec<u8>, i32> {
    go(
        fake,
        14,
        &[
            book(),
            position(updated, 0),
            Record::ServiceLibPositionSet(PositionSet { flags: how }),
        ],
    )
}

#[test]
fn a_local_position_always_wins_and_is_stored_as_local() {
    let fake = Fake::new();
    fake.stored.set(Some(StoredPosition {
        from_sync: true,
        updated: 9_999,
    }));
    let reply = set_position(&fake, 5, 0).unwrap();
    assert_eq!(result_of(&reply).path, "Books/a.epub");
    assert_eq!(fake.last(), "position 3 5 false");
    // An X4 without a valid clock writes 0 and still wins.
    fake.stored.set(Some(StoredPosition {
        from_sync: false,
        updated: 9_999,
    }));
    set_position(&fake, 0, 0).unwrap();
    assert_eq!(fake.last(), "position 3 0 false");
}

#[test]
fn a_sync_position_never_overwrites_a_local_one_silently() {
    let fake = Fake::new();
    let sync = 1;
    fake.stored.set(Some(StoredPosition {
        from_sync: false,
        updated: 1,
    }));
    let before = fake.log.borrow().len();
    assert_eq!(set_position(&fake, 99_999, sync), Err(ERR_STATE));
    assert_eq!(fake.log.borrow().len(), before, "nothing stored");
    // Confirmed by the user: it counts as local, and is stored as local.
    set_position(&fake, 99_999, sync | 2).unwrap();
    assert_eq!(fake.last(), "position 3 99999 false");
    // Over a sync position: newer wins, an equal or older one keeps the stored.
    fake.stored.set(Some(StoredPosition {
        from_sync: true,
        updated: 100,
    }));
    set_position(&fake, 101, sync).unwrap();
    assert_eq!(fake.last(), "position 3 101 true");
    let before = fake.log.borrow().len();
    assert!(
        set_position(&fake, 100, sync).is_ok(),
        "an equal updated keeps the stored one"
    );
    assert!(set_position(&fake, 50, sync).is_ok());
    assert!(
        set_position(&fake, 0, sync).is_ok(),
        "an unknown clock does not beat a known one"
    );
    assert_eq!(fake.log.borrow().len(), before, "none of the three wrote");
    // A stored sync position with an unknown clock gives way.
    fake.stored.set(Some(StoredPosition {
        from_sync: true,
        updated: 0,
    }));
    set_position(&fake, 7, sync).unwrap();
    assert_eq!(fake.last(), "position 3 7 true");
    // Nothing stored: a sync write is the first.
    fake.stored.set(None);
    set_position(&fake, 7, sync).unwrap();
}

#[test]
fn position_inputs_are_checked_whole() {
    let fake = Fake::new();
    let set = |flags| Record::ServiceLibPositionSet(PositionSet { flags });
    let with = |edit: &dyn Fn(&mut Position<'static>)| {
        let Record::ServiceLibPosition(mut p) = position(1, 0) else {
            unreachable!()
        };
        edit(&mut p);
        go(&fake, 14, &[book(), Record::ServiceLibPosition(p), set(0)])
    };
    assert!(with(&|_| ()).is_ok());
    assert!(with(&|p| p.progress_ppm = 1_000_000).is_ok());
    assert_eq!(with(&|p| p.progress_ppm = 1_000_001), Err(ERR_INVALID));
    assert_eq!(with(&|p| p.flags = 8), Err(ERR_INVALID));
    assert_eq!(
        with(&|p| p.flags = 1),
        Err(ERR_INVALID),
        "flag without an XPointer"
    );
    assert_eq!(
        with(&|p| p.xpointer = "/p"),
        Err(ERR_INVALID),
        "XPointer without its flag"
    );
    assert_eq!(
        with(&|p| p.conversion = 7),
        Err(ERR_INVALID),
        "conversion without its flag"
    );
    assert!(
        with(&|p| {
            p.flags = 2;
            p.conversion = 0;
        })
        .is_ok(),
        "present and 0 differs from absent"
    );
    assert!(
        with(&|p| {
            p.flags = 2;
            p.conversion = u64::MAX;
        })
        .is_ok()
    );
    assert_eq!(
        go(&fake, 14, &[book(), position(1, 0), set(4)]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        go(&fake, 14, &[book(), position(1, 0), set(2)]),
        Err(ERR_INVALID),
        "confirmed is a sync flag"
    );
    // Order and count: lib_ref, lib_position, lib_position_set.
    assert_eq!(
        go(&fake, 14, &[book(), set(0), position(1, 0)]),
        Err(ERR_INVALID)
    );
    assert_eq!(go(&fake, 14, &[book(), position(1, 0)]), Err(ERR_INVALID));
    assert_eq!(go(&fake, 14, &[position(1, 0), set(0)]), Err(ERR_INVALID));
    assert_eq!(
        go(&fake, 14, &[book(), position(1, 0), position(1, 0)]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        go(&fake, 14, &[book(), position(1, 0), set(0), set(0)]).map(drop),
        Err(ERR_NO_SPACE)
    );
}

#[test]
fn fields_are_the_callers_own_and_catalog_values_are_short() {
    let fake = Fake::new();
    fn set(fake: &Rc<Fake>, field: &str, value: &str) -> Result<std::vec::Vec<u8>, i32> {
        go(
            fake,
            23,
            &[
                book(),
                Record::ServiceLibFieldSet(FieldSet { field, value }),
            ],
        )
    }
    let set = |field: &str, value: &str| set(&fake, field, value);
    assert!(set("files.rating", "5").is_ok());
    assert_eq!(fake.last(), "field files.rating=5");
    assert!(set("files.rating", "").is_ok(), "empty removes");
    assert_eq!(set("other.rating", "5"), Err(ERR_DENIED));
    assert_eq!(set("files-extra.rating", "5"), Err(ERR_DENIED));
    assert_eq!(set("files.unknown", "5"), Err(ERR_NOT_FOUND));
    assert_eq!(set("files.", "5"), Err(ERR_INVALID));
    assert_eq!(set("files", "5"), Err(ERR_INVALID));
    assert_eq!(set("", "5"), Err(ERR_INVALID));
    assert!(set("files.rating", &"v".repeat(64)).is_ok());
    assert_eq!(set("files.rating", &"v".repeat(65)), Err(ERR_NO_SPACE));
    fake.fields.borrow_mut().insert("files.long".into(), false);
    assert!(set("files.long", &"v".repeat(1024)).is_ok());
    assert!(go(&fake, 23, &[book()]).is_err());
}

fn provided(flags: u32, title: &'static str, tags: &'static str) -> Record<'static> {
    Record::ServiceLibProvided(Provided {
        title,
        author: "",
        series: "",
        number: "",
        tags,
        flags,
    })
}

/// A record with all four text fields, for the per-field rules.
fn provided_all(flags: u32, texts: [&'static str; 4], tags: &'static str) -> Record<'static> {
    Record::ServiceLibProvided(Provided {
        title: texts[0],
        author: texts[1],
        series: texts[2],
        number: texts[3],
        tags,
        flags,
    })
}

#[test]
fn provided_metadata_names_the_fields_it_sets_and_nothing_else() {
    let fake = Fake::new();
    let provide = |record| go(&fake, 24, &[book(), record]);
    assert!(provide(provided(0b00001, "T", "")).is_ok());
    assert_eq!(
        provide(provided(0b00000, "T", "")),
        Err(ERR_INVALID),
        "a value that is not provided"
    );
    assert!(provide(provided(0b10000, "", "a\nb")).is_ok());
    assert_eq!(fake.last(), "provide 16 a\nb");
    assert!(go(&fake, 24, &[provided(1, "T", "")]).is_err());
}

#[test]
fn provided_flags_follow_the_records_field_order() {
    let fake = Fake::new();
    let provide = |record| go(&fake, 24, &[book(), record]);
    // Bit n provides field n: title, author, series, number, tags. Each field
    // alone with a value is accepted; the value of any other field is refused.
    let texts = ["t", "a", "s", "n"];
    for bit in 0..4_usize {
        let mut only = ["", "", "", ""];
        only[bit] = texts[bit];
        assert!(
            provide(provided_all(1 << bit, only, "")).is_ok(),
            "bit {bit}"
        );
        for other in (0..4_usize).filter(|o| *o != bit) {
            let mut wrong = ["", "", "", ""];
            wrong[other] = texts[other];
            assert_eq!(
                provide(provided_all(1 << bit, wrong, "")),
                Err(ERR_INVALID),
                "bit {bit}, field {other} not provided"
            );
        }
        // Its own bit with a blank value is no value.
        assert_eq!(
            provide(provided_all(1 << bit, ["", "", "", ""], "")),
            Err(ERR_INVALID),
            "bit {bit} blank"
        );
    }
    assert!(provide(provided_all(0b1_1111, texts, "x")).is_ok());
    // A tag list without its bit.
    assert_eq!(provide(provided(0b00001, "T", "x")), Err(ERR_INVALID));
}

#[test]
fn provided_refuses_flags_0_unknown_bits_blank_values_and_a_tags_bit_without_a_tag() {
    let fake = Fake::new();
    let provide = |record| go(&fake, 24, &[book(), record]);
    assert_eq!(provide(provided(0, "", "")), Err(ERR_INVALID), "flags 0");
    for bit in [5_u32, 6, 31] {
        assert_eq!(
            provide(provided(1 << bit | 1, "T", "")),
            Err(ERR_INVALID),
            "unknown bit {bit}"
        );
    }
    // Blank: empty, and Unicode White_Space of every kind (`str::trim`).
    for blank in [
        "",
        " ",
        "\t",
        "  \u{a0}",
        "\u{2003}\u{3000}",
        "\u{85}",
        "\u{2028}",
    ] {
        let blank: &'static str = std::boxed::Box::leak(blank.into());
        // (a control character in it is refused as well, as another rule)
        assert_eq!(
            provide(provided(0b0001, blank, "")),
            Err(ERR_INVALID),
            "{blank:?}"
        );
        assert_eq!(
            provide(provided_all(0b0010, ["", blank, "", ""], "")),
            Err(ERR_INVALID),
            "{blank:?}"
        );
    }
    // The tags bit with no tag, and with blank ones.
    for tags in ["", " ", "\n", "a\n ", "\u{3000}"] {
        assert_eq!(
            provide(provided(0b10000, "", tags)),
            Err(ERR_INVALID),
            "{tags:?}"
        );
    }
    // Text that is not blank is kept as it is, spaces and all.
    assert!(provide(provided(0b0001, " T ", "")).is_ok());
}

#[test]
fn provided_refuses_control_characters_and_never_escapes_them() {
    let fake = Fake::new();
    let provide = |record| go(&fake, 24, &[book(), record]);
    let mut bad = std::vec::Vec::new();
    for c in (0_u32..=0x1f).chain(0x7f..=0x9f).chain([0x2028, 0x2029]) {
        bad.push(std::format!("a{}b", char::from_u32(c).unwrap()));
    }
    for text in bad {
        let text: &'static str = std::boxed::Box::leak(text.into_boxed_str());
        for bit in 0..4_usize {
            let mut texts = ["", "", "", ""];
            texts[bit] = text;
            assert_eq!(
                provide(provided_all(1 << bit, texts, "")),
                Err(ERR_INVALID),
                "{text:?} in field {bit}"
            );
        }
        // In a tag too; `\n` is the one separator (`\n` itself is no
        // control character of a tag: it splits it).
        if text != "a\nb" {
            assert_eq!(
                provide(provided(0b10000, "", text)),
                Err(ERR_INVALID),
                "{text:?} in a tag"
            );
            assert_eq!(
                provide(provided(
                    0b10000,
                    "",
                    std::boxed::Box::leak(std::format!("ok\n{text}").into_boxed_str())
                )),
                Err(ERR_INVALID),
                "{text:?} in a second tag"
            );
        }
    }
    // Other characters, non-ASCII among them (U+00A0 is the first after the C1 range), are text.
    assert!(provide(provided(0b0001, "Caf\u{e9} \u{a0}x\u{2027}\u{202a}", "")).is_ok());
    assert!(provide(provided(0b10000, "", "\u{e9}l\u{fc}\n\u{4e2d}")).is_ok());
}

#[test]
fn provided_tags_are_one_to_32_lines_of_1_to_128_bytes() {
    let fake = Fake::new();
    let provide = |record| go(&fake, 24, &[book(), record]);
    let tags = |n: usize| -> &'static str {
        std::boxed::Box::leak(
            ((0..n)
                .map(|i| std::format!("t{i}"))
                .collect::<std::vec::Vec<_>>()
                .join("\n"))
            .into_boxed_str(),
        )
    };
    assert!(provide(provided(0b10000, "", tags(1))).is_ok());
    assert!(provide(provided(0b10000, "", tags(32))).is_ok());
    // A cap past its count is `ERR_NO_SPACE`, as everywhere.
    assert_eq!(provide(provided(0b10000, "", tags(33))), Err(ERR_NO_SPACE));
    // Empty lines are no tags: in the middle, at the ends.
    for empty in ["a\n\nb", "\na", "a\n"] {
        assert_eq!(
            provide(provided(0b10000, "", empty)),
            Err(ERR_INVALID),
            "{empty:?}"
        );
    }
    let tag = |n: usize| -> &'static str { std::boxed::Box::leak("x".repeat(n).into_boxed_str()) };
    assert!(provide(provided(0b10000, "", tag(128))).is_ok());
    assert_eq!(provide(provided(0b10000, "", tag(129))), Err(ERR_NO_SPACE));
}

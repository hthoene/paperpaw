//! Which format reads a file, the rows of ops 19 and 21, limits and removal.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
use super::*;
use alloc::{format, vec};
use paperpaw_native::host_glue::{ERR_FULL, ERR_INVALID, ERR_NO_SPACE, ERR_STATE};

fn spec<'a>(name: &'a str, extensions: &'a str, flags: u32, priority: i32) -> wire::FormatSpec<'a> {
    wire::FormatSpec {
        name,
        title: "format.title",
        extensions,
        flags,
        priority,
        token: 9,
    }
}

fn none(_: &str, _: &mut dyn FnMut(&str)) {}

#[test]
fn the_longest_extension_wins_then_the_users_choice_then_priority_then_load_order() {
    let mut index = FormatIndex::new();
    let zip = index
        .declare_format(0, "format-zip", &spec("zip", "zip", 0, 0))
        .unwrap();
    let fb2zip = index
        .declare_format(1, "format-fb2", &spec("fb2", "fb2\nfb2.zip", 0, 0))
        .unwrap();
    let a = index
        .declare_format(2, "a", &spec("txt", "txt", 0, 5))
        .unwrap();
    let b = index
        .declare_format(3, "b", &spec("txt", "txt", 0, 5))
        .unwrap();
    let c = index
        .declare_format(4, "c", &spec("txt", "txt", 0, 7))
        .unwrap();
    // The longest match, whatever the case of the file name.
    assert_eq!(index.select("Books/x.FB2.zip", "", &none), Ok(Some(fb2zip)));
    assert_eq!(index.select("Books/x.zip", "", &none), Ok(Some(zip)));
    // Priority, then the first loaded.
    assert_eq!(index.select("n.txt", "", &none), Ok(Some(c)));
    // The user's choice (a claimant's id) wins over priority; a stranger's does not.
    let choose = |wanted: &'static str| move |_: &str, visit: &mut dyn FnMut(&str)| visit(wanted);
    assert_eq!(index.select("n.txt", "", &choose("a.txt")), Ok(Some(a)));
    assert_eq!(index.select("n.txt", "", &choose("b.txt")), Ok(Some(b)));
    assert_eq!(
        index.select("n.txt", "", &choose("format-zip.zip")),
        Ok(Some(c))
    );
    // The choice is asked for the matched extension only.
    let asked = core::cell::RefCell::new(Vec::new());
    let record = |extension: &str, _: &mut dyn FnMut(&str)| {
        asked.borrow_mut().push(Box::<str>::from(extension));
    };
    index.select("n.fb2.zip", "", &record).unwrap();
    assert_eq!(&*asked.borrow(), &[Box::<str>::from("fb2.zip")]);
    // No extension, no format; "txt" alone is a name; ".txt" ends in the extension.
    assert_eq!(index.select("README", "", &none), Ok(None));
    assert_eq!(index.select("txt", "", &none), Ok(None));
    assert_eq!(index.select(".txt", "", &none), Ok(Some(c)));
}

#[test]
fn a_requested_format_is_taken_as_given_and_an_unknown_one_is_stale() {
    let mut index = FormatIndex::new();
    let epub = index
        .declare_format(0, "format-epub", &spec("epub", "epub", 0, 0))
        .unwrap();
    assert_eq!(
        index.select("x.txt", "format-epub.epub", &none),
        Ok(Some(epub))
    );
    assert_eq!(index.select("x.epub", "nobody.none", &none), Err(ERR_STATE));
}

#[test]
fn sniffing_formats_come_in_priority_order() {
    let mut index = FormatIndex::new();
    index
        .declare_format(0, "a", &spec("one", "one", SNIFFS, 1))
        .unwrap();
    let high = index
        .declare_format(1, "b", &spec("two", "two", SNIFFS, 9))
        .unwrap();
    index
        .declare_format(2, "c", &spec("three", "three", 0, 99))
        .unwrap();
    let tie = index
        .declare_format(3, "d", &spec("four", "four", SNIFFS, 1))
        .unwrap();
    let mut seen = Vec::new();
    index.sniffers(|reference| {
        seen.push(reference);
        ControlFlow::Continue(())
    });
    assert_eq!(seen.len(), 3);
    assert_eq!(seen[0], high);
    assert_eq!(seen[2], tie, "later loaded among equals");
    let mut first = Vec::new();
    index.sniffers(|reference| {
        first.push(reference);
        ControlFlow::Break(())
    });
    assert_eq!(first, [high]);
}

#[test]
fn eight_formats_per_plugin_thirty_two_on_the_device_and_unique_names_and_claims() {
    let mut index = FormatIndex::new();
    for i in 0..8 {
        index
            .declare_format(0, "p", &spec(&format!("f{i}"), &format!("e{i}"), 0, 0))
            .unwrap();
    }
    assert_eq!(
        index.declare_format(0, "p", &spec("f8", "e8", 0, 0)),
        Err(ERR_FULL)
    );
    // The same name, the same plugin's claim again: invalid.
    assert_eq!(
        index
            .declare_format(1, "q", &spec("f", "e0", 0, 0))
            .map(drop),
        Ok(())
    );
    assert_eq!(
        index.declare_format(1, "q", &spec("f", "e9", 0, 0)),
        Err(ERR_INVALID)
    );
    assert_eq!(
        index.declare_format(1, "q", &spec("g", "e0", 0, 0)),
        Err(ERR_INVALID)
    );
    // Another plugin may claim the same extension.
    index
        .declare_format(2, "r", &spec("f", "e0", 0, 0))
        .unwrap();
    // The device total: 8 + 2 so far; fill to 32, then one more is ERR_FULL.
    let mut plugin = 3;
    while index.formats.len() < DEVICE {
        let id = format!("x{plugin}");
        index
            .declare_format(plugin, &id, &spec("f", &format!("z{plugin}"), 0, 0))
            .unwrap();
        plugin += 1;
    }
    assert_eq!(
        index.declare_format(99, "y", &spec("f", "y", 0, 0)),
        Err(ERR_FULL)
    );
}

#[test]
fn rows_page_by_declaration_order_with_a_generation_that_changes() {
    let mut index = FormatIndex::new();
    for i in 0..5 {
        index
            .declare_format(
                i,
                &format!("p{i}"),
                &spec("f", &format!("e{i}"), 0, i as i32),
            )
            .unwrap();
    }
    let mut ids = Vec::new();
    let end = index
        .formats(0, 0, &mut |row| {
            let Record::ServiceDocFormat(row) = row else {
                unreachable!()
            };
            ids.push(String::from(row.id));
            if ids.len() == 2 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();
    assert_eq!(
        (end.total, end.next),
        (5, 1),
        "the broken row is not counted"
    );
    let generation = end.generation;
    assert_ne!(generation, 0);
    // The rest from the cursor, under the same generation.
    let mut rest = Vec::new();
    let end = index
        .formats(end.next, generation, &mut |row| {
            let Record::ServiceDocFormat(row) = row else {
                unreachable!()
            };
            rest.push(String::from(row.id));
            ControlFlow::Continue(())
        })
        .unwrap();
    assert_eq!((end.total, end.next), (5, 5));
    assert_eq!(rest, ["p1.f", "p2.f", "p3.f", "p4.f"]);
    // A change makes the old generation stale; 0 always starts again.
    index.remove_plugin(4, "p4", |_| ());
    assert_eq!(
        index.formats(1, generation, &mut |_| ControlFlow::Continue(())),
        Err(ERR_STATE)
    );
    assert!(
        index
            .formats(0, 0, &mut |_| ControlFlow::Continue(()))
            .is_ok()
    );
}

#[test]
fn readers_are_listed_for_a_format_or_for_all_and_leave_with_their_plugin() {
    let mut index = FormatIndex::new();
    let reader = |name, formats| wire::ReaderSpec {
        name,
        screen: "main",
        formats,
        flags: 0,
    };
    index
        .declare_reader(
            "reader",
            &reader("main", "format-epub.epub\nformat-txt.txt"),
            "Reader",
        )
        .unwrap();
    index
        .declare_reader("images", &reader("main", "format-images.cbz"), "Images")
        .unwrap();
    index
        .declare_reader("any", &reader("main", ""), "Any")
        .unwrap();
    assert_eq!(
        index.declare_reader("any", &reader("main", ""), "Any"),
        Err(ERR_INVALID)
    );
    let list = |index: &FormatIndex, scope: &str| {
        let mut ids = Vec::new();
        let end = index
            .readers(scope, 0, 0, &mut |row| {
                let Record::ServiceDocReader(row) = row else {
                    unreachable!()
                };
                ids.push(String::from(row.id));
                ControlFlow::Continue(())
            })
            .unwrap();
        assert_eq!(usize::try_from(end.total).unwrap(), ids.len());
        ids
    };
    assert_eq!(list(&index, ""), ["reader.main", "images.main", "any.main"]);
    assert_eq!(
        list(&index, "format-epub.epub"),
        ["reader.main", "any.main"]
    );
    assert_eq!(
        list(&index, "format-images.cbz"),
        ["images.main", "any.main"]
    );
    // A plugin whose id is a prefix of another's keeps the other's readers.
    index
        .declare_reader("any-more", &reader("main", ""), "More")
        .unwrap();
    index.remove_plugin(7, "any", |_| ());
    assert_eq!(
        list(&index, ""),
        ["reader.main", "images.main", "any-more.main"]
    );
}

#[test]
fn a_removed_format_is_gone_for_good_and_its_documents_can_be_ended() {
    let mut index = FormatIndex::new();
    let first = index.declare_format(0, "p", &spec("f", "e", 0, 0)).unwrap();
    assert_eq!(
        index.target(first),
        Some(Target {
            plugin: 0,
            token: 9
        })
    );
    let mut gone = vec![];
    index.remove_plugin(0, "p", |reference| gone.push(reference));
    assert_eq!(gone, [first]);
    assert_eq!(index.target(first), None);
    // Declared again: a new reference, never the old one.
    let second = index.declare_format(0, "p", &spec("f", "e", 0, 0)).unwrap();
    assert_ne!(first, second);
    assert_eq!(index.target(first), None);
    let mut name = String::new();
    index.id(second, &mut |id| name.push_str(id)).unwrap();
    assert_eq!(name, "p.f");
    assert_eq!(index.id(first, &mut |_| ()), Err(ERR_STATE));
}

#[test]
fn spent_references_refuse_a_declaration_and_change_nothing() {
    let mut index = FormatIndex::new();
    index.declare_format(0, "p", &spec("f", "e", 0, 0)).unwrap();
    let generation = index.generation.get();
    index.next_ref.set(u32::MAX);
    assert_eq!(
        index.declare_format(0, "p", &spec("g", "h", 0, 0)),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(
        (
            index.formats.len(),
            index.generation.get(),
            index.next_ref.get()
        ),
        (1, generation, u32::MAX)
    );
    // The last reference but one still works, and then the counter is spent.
    index.next_ref.set(u32::MAX - 1);
    let last = index.declare_format(0, "p", &spec("g", "h", 0, 0)).unwrap();
    assert_eq!(last, FormatRef(u32::MAX - 1));
    assert_eq!(
        index.declare_format(0, "p", &spec("i", "j", 0, 0)),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(index.formats.len(), 2);
}

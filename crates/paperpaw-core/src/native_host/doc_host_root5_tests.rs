//! `doc_open` root 5, the file a hand-over names (`document.md` 1, draft 25):
//! only that file, read only, in a folder of the plugin that made the
//! hand-over; the maker may be gone.
use super::*;
use crate::native_host::{
    shared::Shared,
    ui_service::UiServices,
    ui_store::{Limits as UiLimits, Param, Stamp, UiStore},
};
use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID};

const MAKER: u64 = 21;

fn open5(w: &World<'_>, path: &str) -> Result<Vec<u8>, i32> {
    w.call(
        AdmittedFeatures::NONE,
        1,
        &[Record::ServiceDocOpen(wire::ServiceDocOpen {
            root: 5,
            path,
            format: "",
            flags: 0,
            image_side: 0,
            cover_side: 0,
            css_root: 0,
            css_path: "",
        })],
    )
}

/// A store with one hand-over made by `maker` and owned by `instance`.
fn hand_over(w: &World<'_>, maker: u64, instance: u32, params: &[Param<'_>]) {
    let store = Shared::try_new(UiStore::new(UiLimits::ELSEWHERE)).unwrap();
    let who = Stamp::new(maker, 1).unwrap();
    {
        let s = &*store;
        let id = s.handover_make(who, params).unwrap();
        s.handover_check(who, id).unwrap();
        let claims = s.claim(who, &[id]).unwrap();
        s.attach(Some(id), instance).unwrap();
        drop(claims);
    }
    w.hosts.wire_ui(UiServices::new(store, false)).unwrap();
    w.instance.set(Some(instance));
}

fn p<'a>(key: &'a str, value: &'a str) -> Param<'a> {
    Param { key, value }
}

fn card_path(root: u32, path: &str) -> std::string::String {
    crate::native_host::owner_paths::OwnerPaths::join(Some("maker"), root, path).unwrap()
}

#[test]
fn the_file_a_hand_over_names_opens_read_only_from_its_makers_folder() {
    for (root, text) in [(2, "2"), (0, "0")] {
        world(|w| {
            w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
            w.install("maker", MAKER, &[]);
            hand_over(w, MAKER, 4, &[p("root", text), p("path", "docs/a.epub")]);
            let joined = card_path(root, "docs/a.epub");
            w.card.write(&joined, b"book").unwrap();
            let reply = open5(w, "docs/a.epub").unwrap();
            assert!(matches!(
                wire::record_decode_one(&reply).unwrap(),
                Record::ServiceDocState(_)
            ));
            // The format reads it as its book, the card path is held.
            assert_eq!(w.scoped.borrow()[0].0, joined);
            assert!(w.hosts.in_use().holds(&joined, false));
        });
    }
}

#[test]
fn another_path_no_hand_over_or_a_css_file_there_is_denied() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.install("maker", MAKER, &[]);
        w.card.write(&card_path(2, "docs/a.epub"), b"x").unwrap();
        w.card.write(&card_path(2, "docs/b.epub"), b"x").unwrap();
        // No store wired yet.
        assert_eq!(open5(w, "docs/a.epub"), Err(ERR_UNSUPPORTED));
        hand_over(w, MAKER, 4, &[p("root", "2"), p("path", "docs/a.epub")]);
        // Another file of the maker.
        assert_eq!(open5(w, "docs/b.epub"), Err(ERR_DENIED));
        // An instance that has no hand-over, and a call outside a screen.
        w.instance.set(Some(9));
        assert_eq!(open5(w, "docs/a.epub"), Err(ERR_DENIED));
        w.instance.set(None);
        assert_eq!(open5(w, "docs/a.epub"), Err(ERR_DENIED));
        w.instance.set(Some(4));
        // root 5 is no CSS root.
        let css = w.call(
            AdmittedFeatures::NONE,
            1,
            &[Record::ServiceDocOpen(wire::ServiceDocOpen {
                root: 0,
                path: "a.epub",
                format: "",
                flags: 0,
                image_side: 0,
                cover_side: 0,
                css_root: 5,
                css_path: "docs/a.css",
            })],
        );
        assert_eq!(css, Err(ERR_DENIED));
        assert!(w.calls.borrow().is_empty(), "no format was called");
    });
}

#[test]
fn a_maker_that_is_gone_or_a_missing_file_is_not_found() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.install("maker", MAKER, &[]);
        // Another plugin is installed: its key is not the maker's.
        w.install("other", 22, &[]);
        hand_over(w, MAKER, 4, &[p("root", "2"), p("path", "docs/a.epub")]);
        assert_eq!(open5(w, "docs/a.epub"), Err(ERR_NOT_FOUND), "no such file");
        w.card.write(&card_path(2, "docs/a.epub"), b"x").unwrap();
        w.stop("maker");
        assert_eq!(open5(w, "docs/a.epub"), Err(ERR_NOT_FOUND), "maker gone");
        assert!(w.calls.borrow().is_empty());
        let _ = ERR_INVALID;
    });
}

#[test]
fn a_maker_that_goes_while_its_file_is_open_ends_the_document_and_its_hold() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.install("maker", MAKER, &[]);
        hand_over(w, MAKER, 4, &[p("root", "2"), p("path", "docs/a.epub")]);
        let joined = card_path(2, "docs/a.epub");
        w.card.write(&joined, b"book").unwrap();
        let reply = open5(w, "docs/a.epub").unwrap();
        let Record::ServiceDocState(state) = wire::record_decode_one(&reply).unwrap() else {
            panic!("not a state")
        };
        let document = state.document;
        assert!(w.hosts.in_use().holds(&joined, false));
        // The maker is updated or removed: the folder may go under the reader.
        w.hosts.owner_retired(MAKER).unwrap();
        assert!(
            !w.hosts.in_use().holds(&joined, false),
            "the hold is let go"
        );
        let mut out = [0_u8; 64];
        let length = w.state.next_event(OPENER, &mut out).unwrap().unwrap();
        match wire::record_decode_one(&out[..length]).unwrap() {
            Record::EventDocumentClosed(event) => {
                assert_eq!((event.document, event.reason), (document, 3));
            }
            other => panic!("{other:?}"),
        }
        // Another maker's retirement leaves a document alone.
        assert_eq!(w.hosts.owner_retired(MAKER), Ok(()), "safe to repeat");
    });
}

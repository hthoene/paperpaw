//! Acceptance of the interface-7 reader, pending on the host and the later
//! slices. The interface-6 tests drive a kernel (`tests/*.rs`, `src/**`) and
//! stay as the references; none of these cases passes by itself.
#![allow(clippy::unwrap_used, reason = "acceptance assertions")]
#[path = "../../test-support/pending.rs"]
mod pending;
use pending::Case;

const CASES: [Case; 11] = [
    // A book opens at the position it was left at, in the reading font and
    // the settings of the book (the user's reading settings win over a
    // plugin's defaults, decision 131), pages turn, the page index builds in
    // the background and the page numbers and the progress are right.
    Case {
        name: "reader-pages",
        golden: None,
        blocked_by: &["DOC-open-layout", "LIB-position", "H1", "H3", "H4"],
    },
    // Taps, swipes, pinch, long presses and the buttons: links followed
    // (Back returns), text selected, highlights and notes, corner bookmark.
    Case {
        name: "reader-input-and-selection",
        golden: None,
        blocked_by: &["H4", "DOC-open-layout", "H2"],
    },
    // The table of contents, the bookmarks and highlights, the search results
    // and their keyboard, going to a page or a percentage, skimming and the
    // footnote over the page.
    Case {
        name: "reader-screens",
        golden: None,
        blocked_by: &["H1", "H2", "H4", "DOC-open-layout"],
    },
    // Reading aloud, sentence by sentence, with its timer, highlight and page
    // turning; the pages turned by themselves.
    // The notes export: the list reads the library's annotations, the library
    // folder setting is read, the Notes folder is made where it is missing and
    // the Markdown is written to `<file>.new` and moved into place (an old file
    // stays where a write is refused), then the stage runs.
    Case {
        name: "reader-annotations-export",
        golden: None,
        blocked_by: &["H1", "H2", "LIB-annotations", "FS-write"],
    },
    Case {
        name: "reader-aloud-and-auto-turn",
        golden: None,
        blocked_by: &["SPEECH-service", "WORK-timers/H4"],
    },
    // The status lines above and below the text (the lines, the items, the
    // places, their settings) and the sample above the reading settings.
    Case {
        name: "reader-status-lines-and-sample",
        golden: None,
        blocked_by: &["status-declarations", "widget-declarations"],
    },
    // The reader's own extension points (#241): inserted blocks between
    // lines, text style, an input filter; page numbers from the source text.
    Case {
        name: "reader-stages",
        golden: None,
        blocked_by: &["reader-api-224", "stage-run-host"],
    },
    // A book handed over with `section` and `offset` (notes overview, an
    // annotation's start) opens at that place, over `page` and the saved
    // place; a place the book lacks opens it as without. The rule is
    // `native_open`; the document host's answer on the place is pending.
    Case {
        name: "reader-open-at-place",
        golden: None,
        blocked_by: &["DOC-open-layout", "H1"],
    },
    // A word looked up in a book runs the stage `reader.word-looked-up@1`
    // once, a subscriber (vocabulary) gets word, sentence, the book's file and
    // title and the place; the lookup goes on whatever it answers. The input
    // is `native_words` (tested without a host); the run needs the stage host.
    Case {
        name: "reader-word-looked-up-run",
        golden: None,
        blocked_by: &["stage-run-host", "H4"],
    },
    // Choosing words and the bar over a real page: a long press chooses a
    // word, *Copy* writes the clipboard, *Look up* opens `core.lookup` and runs
    // the stage, and every action uses the plugin's one `core.ui` handle (none
    // is opened and left). The model and the movement are tested without a host.
    Case {
        name: "reader-selection-host-flow",
        golden: None,
        blocked_by: &["DOC-open-layout", "H4"],
    },
    // A link on a real page: followed (a place, a note over the page, the web),
    // *Back* returns, a turn past the last link goes on to the next page.
    Case {
        name: "reader-link-follow-back",
        golden: None,
        blocked_by: &["DOC-open-layout", "H4"],
    },
];

#[test]
fn every_case_still_has_its_reference() {
    for case in &CASES {
        assert!(case.golden_exists(), "{}", case.name);
        let error = case.refuse().unwrap_err();
        assert!(error.starts_with("BLOCKED "), "{error}");
        assert!(error.contains(case.blocked_by[0]), "{error}");
    }
}

#[test]
#[ignore = "BLOCKED DOC-open-layout, LIB-position, H1/H3/H4: no document host, no position store, no native screen"]
fn pages() {
    CASES[0].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H4, H2: native input and dialogs are not qualified"]
fn input_and_selection() {
    CASES[1].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED H1/H2/H4: native screens, dialogs and input are not qualified"]
fn screens() {
    CASES[2].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED SPEECH-service, WORK-timers/H4: no speech facade, no timers"]
fn aloud_and_auto_turn() {
    CASES[3].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED status-declarations, widget-declarations: no native declaration for status places and lines, nor for a preview widget"]
fn status_lines_and_sample() {
    CASES[4].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED reader-api-224, stage-run-host: the reader's stage schemas (#224) are not on integration"]
fn stages() {
    CASES[5].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED DOC-open-layout, H1: no document host to check the place against, no native screen to open"]
fn open_at_place() {
    CASES[6].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED stage-run-host (#828), H4: the host does not run stages for plugins yet, and the native selection flow is not there"]
fn word_looked_up_run() {
    CASES[7].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED DOC-open-layout, H4: no document host to open a book with words, no native input"]
fn selection_host_flow() {
    CASES[8].refuse().unwrap();
}

#[test]
#[ignore = "BLOCKED DOC-open-layout, H4: no document host to open a book with links, no native input"]
fn link_follow_back() {
    CASES[9].refuse().unwrap();
}

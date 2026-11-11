//! Ops 3-17: arguments, the call made, the checks of the answer, strikes.
use super::*;
use wire::{
    ServiceDocFont as Font, ServiceDocHref as Href, ServiceDocIndex as Index,
    ServiceDocLink as Link, ServiceDocMeta as Meta, ServiceDocProgress as Progress,
    ServiceDocSection as Section, ServiceDocSpan as Span, ServiceDocText as Text,
    ServiceDocTextAt as TextAt, ServiceDocToc as Toc, ServiceDocXpointer as Xpointer,
};

fn at(document: u32) -> At {
    At {
        document,
        section: 4,
        offset: 9,
        insert: 0,
        cursor: 0,
        generation: 0,
    }
}
fn page(document: u32, cursor: u32, generation: u32) -> Record<'static> {
    Record::ServiceDocPage(Page {
        document,
        scope: "",
        cursor,
        generation,
    })
}

/// A valid input batch for `op` naming `document`.
pub(super) fn input_for(op: u32, document: u32) -> Vec<u8> {
    let record = match op {
        3 | 15 => doc_ref(document),
        4 => Record::ServiceDocTextAt(TextAt {
            document,
            kind: 0,
            index: 0,
            offset: 0,
        }),
        5 | 7 | 14 => page(document, 0, 0),
        6 | 9 | 10 | 17 => Record::ServiceDocAt(at(document)),
        8 => Record::ServiceDocIndex(Index { document, index: 3 }),
        11 => Record::ServiceDocXpointer(Xpointer {
            document,
            text: "/body/p[1]",
        }),
        12 => Record::ServiceDocSpan(Span {
            document,
            section: 1,
            from: 2,
            to: 9,
            cursor: 0,
            generation: 0,
        }),
        13 => Record::ServiceDocHref(Href {
            document,
            href: "ch2.xhtml#a",
        }),
        16 => Record::ServiceDocImageAsk(wire::ServiceDocImageAsk {
            document,
            index: 6,
            side: 0,
        }),
        _ => unreachable!(),
    };
    batch(&[record])
}

fn meta() -> Record<'static> {
    Record::ServiceDocMeta(Meta {
        title: "T",
        language: "en",
        series: "",
        series_index: "",
        direction: 0,
        writing_mode: 0,
        sections: 3,
        authors: 1,
        subjects: 0,
        description_bytes: 0,
        flags: 0,
    })
}
fn end(flags: u32, total: u32, generation: u32, next: u32) -> Record<'static> {
    Record::ServiceEnd(wire::ServiceEnd {
        flags,
        total,
        generation,
        next,
        cursor: None,
    })
}
fn section(index: u32) -> Record<'static> {
    Record::ServiceDocSection(Section {
        index,
        length: 100,
        flags: 0,
    })
}
fn toc(index: u32, level: u32) -> Record<'static> {
    Record::ServiceDocToc(Toc {
        index,
        level,
        section: 0,
        offset: 0,
        flags: 0,
        title: "A",
    })
}
fn font(index: u32, family: u32) -> Record<'static> {
    Record::ServiceDocFont(Font {
        index,
        family,
        flags: 0,
        bytes: 10,
    })
}
fn link(kind: u32) -> Record<'static> {
    Record::ServiceDocLink(Link {
        start: 1,
        end: 3,
        kind,
        href: "x",
    })
}
fn text(kind: u32, total: u32, text: &'static str) -> Record<'static> {
    Record::ServiceDocText(Text {
        kind,
        index: 0,
        total,
        text,
    })
}
fn position(document: u32) -> Record<'static> {
    Record::ServiceDocAt(at(document))
}
fn picture() -> Record<'static> {
    Record::ServiceDocImage(Image {
        image: 0,
        index: 6,
        width: 100,
        height: 50,
        flags: 0,
    })
}

/// (op, answer, the format op it must call) for a good answer to `document`.
fn happy(document: u32) -> Vec<(u32, Vec<u8>, u32)> {
    let last = || end(1, 1, 5, 1);
    std::vec![
        (3, stream(&[meta()]), fmt::META),
        (4, stream(&[text(0, 5, "hello")]), fmt::META_TEXT),
        (5, stream(&[section(0), last()]), fmt::SECTIONS),
        (7, stream(&[toc(0, 0), toc(1, 15), last()]), fmt::TOC),
        (8, stream(&[position(document)]), fmt::TOC_TARGET),
        (
            9,
            stream(&[Record::ServiceDocLabel(wire::ServiceDocLabel {
                text: "iv"
            })]),
            fmt::PAGE_LABEL
        ),
        (
            10,
            stream(&[Record::ServiceDocXpointer(Xpointer {
                document,
                text: "/p"
            })]),
            fmt::XPOINTER
        ),
        (11, stream(&[position(document)]), fmt::LOCATE),
        (12, stream(&[link(0), link(2), last()]), fmt::LINKS),
        (13, stream(&[position(document)]), fmt::LINK_TARGET),
        (14, stream(&[font(0, 1), font(123, 31), last()]), fmt::FONTS),
        (
            17,
            stream(&[Record::ServiceDocProgress(Progress {
                progress_ppm: 1_000_000
            })]),
            fmt::PROGRESS
        ),
    ]
}

#[test]
fn every_read_op_calls_its_format_op_and_passes_the_checked_answer_on() {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    for (op, answer, format_op) in happy(id) {
        *fake.answer.borrow_mut() = answer.clone();
        let reply = call_raw(&fake, op, &input_for(op, id)).unwrap();
        assert_eq!(reply, answer, "op {op}");
        let last = *fake.calls.borrow().last().unwrap();
        assert_eq!((last[0], last[1]), (format_op, id), "op {op}");
    }
}

#[test]
fn arguments_reach_the_format_as_the_contract_maps_them() {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    let last = |fake: &Rc<Fake>| *fake.calls.borrow().last().unwrap();
    // [op, document, cursor, generation, flags, index, image_side, cover_side]
    fake.say(&[Record::ServiceDocText(Text {
        kind: 2,
        index: 4,
        total: 9,
        text: "",
    })]);
    call(
        &fake,
        4,
        &[Record::ServiceDocTextAt(TextAt {
            document: id,
            kind: 2,
            index: 4,
            offset: 9,
        })],
    )
    .unwrap();
    assert_eq!(last(&fake), [fmt::META_TEXT, id, 0, 0, 2, 4, 0, 0]);
    fake.say(&[section(40), end(1, 41, 5, 41)]);
    call(&fake, 5, &[page(id, 40, 5)]).unwrap();
    assert_eq!(last(&fake), [fmt::SECTIONS, id, 40, 5, 0, 0, 0, 0]);
    fake.say(&[link(1), end(1, 1, 5, 1)]);
    call(
        &fake,
        12,
        &[Record::ServiceDocSpan(Span {
            document: id,
            section: 1,
            from: 2,
            to: 9,
            cursor: 0,
            generation: 0,
        })],
    )
    .unwrap();
    assert_eq!(
        last(&fake),
        [fmt::LINKS, id, 0, 0, 0, 9, 0, 0],
        "to is the index, from the offset"
    );
    fake.say(&[position(id)]);
    call(
        &fake,
        8,
        &[Record::ServiceDocIndex(Index {
            document: id,
            index: 31,
        })],
    )
    .unwrap();
    assert_eq!(last(&fake)[5], 31);
    call(
        &fake,
        11,
        &[Record::ServiceDocXpointer(Xpointer {
            document: id,
            text: "/body/p[2]",
        })],
    )
    .unwrap();
    assert_eq!(fake.texts.borrow().last().unwrap(), "/body/p[2]");
    call(
        &fake,
        13,
        &[Record::ServiceDocHref(Href {
            document: id,
            href: "a.xhtml#b",
        })],
    )
    .unwrap();
    assert_eq!(fake.texts.borrow().last().unwrap(), "a.xhtml#b");
    // Pictures: the cover at the opened side, `image` at the page's size or the zoom.
    fake.say(&[picture()]);
    call(&fake, 15, &[doc_ref(id)]).unwrap();
    assert_eq!(last(&fake)[7], 300);
    let ask = |side| {
        Record::ServiceDocImageAsk(wire::ServiceDocImageAsk {
            document: id,
            index: 6,
            side,
        })
    };
    call(&fake, 16, &[ask(0)]).unwrap();
    assert_eq!((last(&fake)[5], last(&fake)[6]), (6, 1600));
    call(&fake, 16, &[ask(1200)]).unwrap();
    assert_eq!(last(&fake)[6], 1200);
    call(&fake, 16, &[ask(1600)]).unwrap();
    assert_eq!(last(&fake)[6], 1600);
}

#[test]
fn image_index_max_is_the_cover() {
    let last = |fake: &Rc<Fake>| *fake.calls.borrow().last().unwrap();
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    fake.say(&[picture()]);
    // `index` `u32::MAX` is the cover: format op 17 at the cover's side, or larger.
    let cover = |side| {
        Record::ServiceDocImageAsk(wire::ServiceDocImageAsk {
            document: id,
            index: u32::MAX,
            side,
        })
    };
    call(&fake, 16, &[cover(0)]).unwrap();
    assert_eq!((last(&fake)[0], last(&fake)[7]), (fmt::COVER, 300));
    call(&fake, 16, &[cover(1200)]).unwrap();
    assert_eq!((last(&fake)[0], last(&fake)[7]), (fmt::COVER, 1200));
}

#[test]
fn bad_arguments_are_refused_before_the_format_is_asked() {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    let before = fake.calls.borrow().len();
    let bad = |op: u32, record: Record<'_>| call(&fake, op, &[record]);
    let text_at = |kind, index| {
        Record::ServiceDocTextAt(TextAt {
            document: id,
            kind,
            index,
            offset: 0,
        })
    };
    assert_eq!(bad(4, text_at(5, 0)), Err(ERR_INVALID));
    assert_eq!(bad(4, text_at(0, 1)), Err(ERR_NOT_FOUND), "one title");
    assert_eq!(bad(4, text_at(3, 1)), Err(ERR_NOT_FOUND), "one description");
    assert_eq!(bad(4, text_at(4, 1)), Err(ERR_NOT_FOUND), "one series");
    let scoped = Record::ServiceDocPage(Page {
        document: id,
        scope: "x.y",
        cursor: 0,
        generation: 0,
    });
    for op in [5, 7, 14] {
        assert_eq!(bad(op, scoped), Err(ERR_INVALID), "op {op}");
        assert_eq!(
            bad(op, page(id, 3, 0)),
            Err(ERR_STATE),
            "op {op}: a cursor needs its generation"
        );
    }
    let block = |insert, cursor, generation| {
        Record::ServiceDocAt(At {
            document: id,
            section: 0,
            offset: 0,
            insert,
            cursor,
            generation,
        })
    };
    assert_eq!(bad(6, block(1, 0, 0)), Err(ERR_INVALID));
    assert_eq!(bad(6, block(0, 4, 0)), Err(ERR_STATE));
    for op in [9, 10, 17] {
        assert_eq!(bad(op, block(1, 0, 0)), Err(ERR_INVALID), "op {op}");
        assert_eq!(bad(op, block(0, 1, 1)), Err(ERR_INVALID), "op {op}");
    }
    let span = |from, to, cursor, generation| {
        Record::ServiceDocSpan(Span {
            document: id,
            section: 0,
            from,
            to,
            cursor,
            generation,
        })
    };
    assert_eq!(bad(12, span(9, 2, 0, 0)), Err(ERR_INVALID));
    assert_eq!(bad(12, span(1, 2, 3, 0)), Err(ERR_STATE));
    assert_eq!(
        bad(
            11,
            Record::ServiceDocXpointer(Xpointer {
                document: id,
                text: ""
            })
        ),
        Err(ERR_INVALID)
    );
    assert_eq!(
        bad(
            13,
            Record::ServiceDocHref(Href {
                document: id,
                href: ""
            })
        ),
        Err(ERR_INVALID)
    );
    let ask = |side| {
        Record::ServiceDocImageAsk(wire::ServiceDocImageAsk {
            document: id,
            index: 0,
            side,
        })
    };
    assert_eq!(bad(16, ask(1601)), Err(ERR_INVALID));
    // A record of another op is not this op's input.
    assert_eq!(bad(3, position(id)), Err(ERR_INVALID));
    assert_eq!(fake.calls.borrow().len(), before);
}

#[test]
fn every_read_op_refuses_another_openers_or_an_unknown_document() {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    for op in (3..=17).filter(|op| *op != 6) {
        fake.owner.set(8);
        assert_eq!(
            call_raw(&fake, op, &input_for(op, id)),
            Err(ERR_DENIED),
            "op {op}"
        );
        fake.owner.set(OWNER);
        assert_eq!(
            call_raw(&fake, op, &input_for(op, id + 50)),
            Err(ERR_NOT_FOUND),
            "op {op}"
        );
    }
    assert_eq!(fake.calls.borrow().len(), 1);
}

#[test]
fn a_format_status_passes_through_and_is_no_strike() {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    for status in [ERR_IO, ERR_NOT_FOUND, ERR_UNSUPPORTED, ERR_INVALID] {
        fake.fails.set(Some(status));
        for op in [3, 9, 17] {
            assert_eq!(call_raw(&fake, op, &input_for(op, id)), Err(status));
        }
    }
    assert!(fake.closed.borrow().is_empty());
    assert!(fake.table.ready(OWNER, id).is_ok());
}

/// Malformed answers: each op, an answer that breaks one rule.
fn malformed_details(document: u32) -> Vec<(u32, Vec<u8>)> {
    let bad_meta = |direction, writing_mode, flags| {
        stream(&[Record::ServiceDocMeta(Meta {
            title: "",
            language: "",
            series: "",
            series_index: "",
            direction,
            writing_mode,
            sections: 0,
            authors: 0,
            subjects: 0,
            description_bytes: 0,
            flags,
        })])
    };
    std::vec![
        (3, stream(&[])),
        (3, stream(&[meta(), meta()])),
        (3, bad_meta(2, 0, 0)),
        (3, bad_meta(0, 3, 0)),
        (3, bad_meta(0, 0, 4)),
        (3, stream(&[position(document)])),
        (3, [stream(&[meta()]), std::vec![0xff]].concat()),
        (4, stream(&[text(1, 5, "hello")])),
        (4, stream(&[text(0, 5, "toolong!")])),
        (4, stream(&[text(0, 5, "he")])),
    ]
}

fn malformed_pages(document: u32) -> Vec<(u32, Vec<u8>)> {
    let last = || end(1, 1, 5, 1);
    std::vec![
        (5, stream(&[section(0)])),
        (5, stream(&[last(), section(0)])),
        (5, stream(&[section(0), last(), last()])),
        (
            5,
            stream(&[
                Record::ServiceDocSection(Section {
                    index: 0,
                    length: 1,
                    flags: 2
                }),
                last()
            ])
        ),
        (5, stream(&[section(1), section(1), last()])),
        (5, stream(&[section(0), end(1, 1, 0, 1)])),
        (5, stream(&[section(0), end(2, 1, 5, 1)])),
        (5, stream(&[section(0), end(0, 9, 5, 0)])),
        (7, stream(&[toc(0, 16), last()])),
        (
            7,
            stream(&[
                Record::ServiceDocToc(Toc {
                    index: 0,
                    level: 0,
                    section: 0,
                    offset: 0,
                    flags: 4,
                    title: ""
                }),
                last()
            ])
        ),
        (7, stream(&[section(0), last()])),
        (8, stream(&[position(document + 1)])),
        (
            8,
            stream(&[Record::ServiceDocAt(At {
                document,
                section: 0,
                offset: 0,
                insert: 1,
                cursor: 0,
                generation: 0
            })])
        ),
        (
            8,
            stream(&[Record::ServiceDocAt(At {
                document,
                section: 0,
                offset: 0,
                insert: 0,
                cursor: 3,
                generation: 0
            })])
        ),
        (9, stream(&[])),
        (
            10,
            stream(&[Record::ServiceDocXpointer(Xpointer {
                document: document + 1,
                text: "/p"
            })])
        ),
        (
            10,
            stream(&[Record::ServiceDocXpointer(Xpointer { document, text: "" })])
        ),
        (12, stream(&[link(3), last()])),
        (
            12,
            stream(&[
                Record::ServiceDocLink(Link {
                    start: 4,
                    end: 3,
                    kind: 0,
                    href: "x"
                }),
                last()
            ])
        ),
        (14, stream(&[font(124, 1), last()])),
        (14, stream(&[font(0, 0), last()])),
        (14, stream(&[font(0, 32), last()])),
        (
            17,
            stream(&[Record::ServiceDocProgress(Progress {
                progress_ppm: 1_000_001
            })])
        ),
    ]
}

#[test]
fn a_malformed_answer_is_invalid_and_never_passed_on() {
    for (op, answer) in [malformed_details(1), malformed_pages(1)].concat() {
        let fake = Fake::new(Limits::X4);
        let id = opened(&fake);
        assert_eq!(id, 1);
        *fake.answer.borrow_mut() = answer;
        assert_eq!(
            call_raw(&fake, op, &input_for(op, id)),
            Err(ERR_INVALID),
            "op {op}"
        );
    }
}

#[test]
fn three_malformed_answers_in_a_row_close_the_document_a_good_one_resets_the_run() {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    let bad = stream(&[]);
    let (good_op, good, _) = happy(id).remove(0);
    for _ in 0..2 {
        *fake.answer.borrow_mut() = bad.clone();
        assert_eq!(call_raw(&fake, 3, &input_for(3, id)), Err(ERR_INVALID));
    }
    *fake.answer.borrow_mut() = good;
    call_raw(&fake, good_op, &input_for(good_op, id)).unwrap();
    for _ in 0..2 {
        *fake.answer.borrow_mut() = bad.clone();
        assert_eq!(call_raw(&fake, 3, &input_for(3, id)), Err(ERR_INVALID));
    }
    assert!(fake.closed.borrow().is_empty());
    assert_eq!(call_raw(&fake, 3, &input_for(3, id)), Err(ERR_INVALID));
    assert_eq!(*fake.closed.borrow(), [(id, 1)]);
    // From now on the id is cancelled until the opener closes it.
    assert_eq!(call_raw(&fake, 3, &input_for(3, id)), Err(ERR_CANCELLED));
    call(&fake, 18, &[doc_ref(id)]).unwrap();
}

#[test]
fn pages_are_checked_for_generation_progress_and_row_cap() {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    let ask = |cursor, generation| call(&fake, 5, &[page(id, cursor, generation)]);
    // The generation the caller named is the one that comes back.
    fake.say(&[section(10), end(1, 11, 6, 11)]);
    assert_eq!(ask(10, 5), Err(ERR_STATE));
    fake.say(&[section(10), end(1, 11, 5, 11)]);
    assert!(ask(10, 5).is_ok());
    assert!(ask(0, 0).is_ok(), "the first call names none");
    // A page that is not the last must move the cursor forward.
    fake.say(&[section(10), end(0, 99, 5, 10)]);
    assert_eq!(ask(10, 5), Err(ERR_INVALID));
    fake.say(&[section(11), end(0, 99, 5, 12)]);
    assert!(ask(10, 5).is_ok());
    // Rows start at the cursor and only go forward.
    fake.say(&[section(9), end(0, 99, 5, 12)]);
    assert_eq!(ask(10, 5), Err(ERR_INVALID));
    // 64 rows (`page_rows`) are a page; 65 are not.
    let rows = |n: u32| -> Vec<u8> {
        let mut all: Vec<Record<'static>> = (0..n).map(section).collect();
        all.push(end(0, 99, 5, n));
        stream(&all)
    };
    *fake.answer.borrow_mut() = rows(64);
    assert!(ask(0, 0).is_ok());
    *fake.answer.borrow_mut() = rows(65);
    assert_eq!(ask(0, 0), Err(ERR_INVALID));
}

#[test]
fn a_toc_page_of_seven_long_titles_and_a_link_page_of_three_fit_the_reply() {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    let title = "t".repeat(512);
    let rows: Vec<Record<'_>> = (0..7)
        .map(|index| {
            Record::ServiceDocToc(Toc {
                index,
                level: 1,
                section: 0,
                offset: 0,
                flags: 0,
                title: &title,
            })
        })
        .chain([end(0, 20, 5, 7)])
        .collect();
    *fake.answer.borrow_mut() = stream(&rows);
    let reply = call_raw(&fake, 7, &input_for(7, id)).unwrap();
    assert!(reply.len() <= 4096);
    let href = "h".repeat(1024);
    let rows: Vec<Record<'_>> = (0..3)
        .map(|_| {
            Record::ServiceDocLink(Link {
                start: 0,
                end: 1,
                kind: 1,
                href: &href,
            })
        })
        .chain([end(1, 3, 5, 3)])
        .collect();
    *fake.answer.borrow_mut() = stream(&rows);
    assert!(call_raw(&fake, 12, &input_for(12, id)).is_ok());
}

#[test]
fn meta_text_reads_a_long_text_in_offsets_without_cutting_it() {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    let long = "é".repeat(700); // 1,400 bytes
    let ask = |offset| {
        Record::ServiceDocTextAt(TextAt {
            document: id,
            kind: 3,
            index: 0,
            offset,
        })
    };
    let part = |from: usize, to: usize| {
        Record::ServiceDocText(Text {
            kind: 3,
            index: 0,
            total: 1400,
            text: &long[from..to],
        })
    };
    fake.say(&[part(0, 1024)]);
    assert!(call(&fake, 4, &[ask(0)]).is_ok());
    // Up to three bytes short of the cap is a UTF-8 cut; more is a loss.
    fake.say(&[part(0, 1022)]);
    assert!(call(&fake, 4, &[ask(0)]).is_ok());
    fake.say(&[part(0, 1018)]);
    assert_eq!(call(&fake, 4, &[ask(0)]), Err(ERR_INVALID));
    // The rest, from an offset: all of it, exactly.
    fake.say(&[part(1024, 1400)]);
    assert!(call(&fake, 4, &[ask(1024)]).is_ok());
    fake.say(&[part(1024, 1398)]);
    assert_eq!(call(&fake, 4, &[ask(1024)]), Err(ERR_INVALID));
    // An offset past the end has no text to give.
    fake.say(&[Record::ServiceDocText(Text {
        kind: 3,
        index: 0,
        total: 1400,
        text: "",
    })]);
    assert!(call(&fake, 4, &[ask(1400)]).is_ok());
    assert_eq!(call(&fake, 4, &[ask(1401)]), Err(ERR_INVALID));
}

#[test]
fn pictures_get_the_openers_image_id_and_a_failed_one_is_no_picture() {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    fake.say(&[picture()]);
    for op in [15, 16] {
        let reply = call_raw(&fake, op, &input_for(op, id)).unwrap();
        let Record::ServiceDocImage(image) = wire::record_decode_one(&reply).unwrap() else {
            panic!("not an image")
        };
        assert_eq!(
            (image.image, image.index, image.width, image.height),
            (41, 6, 100, 50)
        );
    }
    // All ids out and none drawn: op 16 is busy for that picture, never a hang.
    fake.image.set(Some(Some(0)));
    assert_eq!(call_raw(&fake, 16, &input_for(16, id)), Err(ERR_BUSY));
    // A picture that fails the cache-image check is none (`ERR_NOT_FOUND`), not
    // a malformed answer: the document is not struck for it.
    fake.image.set(Some(None));
    assert_eq!(call_raw(&fake, 15, &input_for(15, id)), Err(ERR_NOT_FOUND));
    // The answer must be one picture.
    fake.image.set(Some(Some(41)));
    fake.say(&[picture(), picture()]);
    assert_eq!(call_raw(&fake, 16, &input_for(16, id)), Err(ERR_INVALID));
}

/// Senior #1110 D1: once the format plugin stopped, no path calls it again,
/// and every image id still ends.
#[test]
fn no_path_calls_a_format_after_it_stopped() {
    // The format stops, then the opener stops.
    let fake = Fake::new(Limits::LARGE);
    let a = opened(&fake);
    let b = opened(&fake);
    assert_eq!(format_stopped(&*fake, FORMAT), Ok(2));
    assert_eq!(opener_stopped(&*fake, OWNER), Ok(2));
    assert!(
        fake.ended.borrow().is_empty(),
        "no op 4 into a stopped format"
    );
    assert_eq!(*fake.closed.borrow(), [(a, 1), (b, 1)], "told once");
    assert!(fake.images_ended.borrow().contains(&a));
    assert!(fake.images_ended.borrow().contains(&b));

    // The format crashes during op 1: the cleanup must not call it again.
    let fake = Fake::new(Limits::X4);
    fake.stops_during_call.set(true);
    fake.fails.set(Some(ERR_IO));
    assert_eq!(
        call(&fake, 1, &[Record::ServiceDocOpen(open_record())]),
        Err(ERR_IO)
    );
    assert_eq!(fake.table.len(), 0);
    assert!(fake.ended.borrow().is_empty());

    // Three malformed answers close the document (the format is alive), then
    // the format stops, then the opener closes: no op 4, one event.
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    *fake.answer.borrow_mut() = stream(&[]);
    for _ in 0..3 {
        assert_eq!(call_raw(&fake, 3, &input_for(3, id)), Err(ERR_INVALID));
    }
    assert_eq!(*fake.closed.borrow(), [(id, 1)]);
    assert_eq!(
        format_stopped(&*fake, FORMAT),
        Ok(0),
        "already closed: no second event"
    );
    call(&fake, 18, &[doc_ref(id)]).unwrap();
    assert!(fake.ended.borrow().is_empty());
    assert_eq!(*fake.closed.borrow(), [(id, 1)]);
    assert!(fake.images_ended.borrow().contains(&id));

    // A strike-closed document of a live format: the opener's close still
    // tells the format.
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    *fake.answer.borrow_mut() = stream(&[]);
    for _ in 0..3 {
        let _ = call_raw(&fake, 3, &input_for(3, id));
    }
    call(&fake, 18, &[doc_ref(id)]).unwrap();
    assert_eq!(*fake.ended.borrow(), [id]);
}

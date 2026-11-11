//! The `core.format` stage adapter: the encoded call, the translation of the
//! reply to service tags, the pictures, the refusals.
#![allow(clippy::unwrap_used, reason = "fixture refusal must fail the test")]
use super::*;
use crate::native_host::{
    doc_service::{FormatRef, Options},
    format_call::tests::{Behavior, Harness},
};
use alloc::vec;
use core::cell::RefCell;
use local::Record as L;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_CANCELLED};

/// A fake picture mapper: maps every picture to id 77, or refuses.
struct Images {
    answer: RefCell<Result<Option<u32>, i32>>,
    seen: RefCell<Vec<(u64, u32, bool, std::string::String)>>,
}

impl ImageMap for Images {
    fn map(
        &self,
        owner: u64,
        document: u32,
        image: &local::FmtImage<'_>,
        in_block: bool,
    ) -> Result<Option<wire::ServiceDocImage>, i32> {
        self.seen
            .borrow_mut()
            .push((owner, document, in_block, image.path.into()));
        let answer = *self.answer.borrow();
        Ok(answer?.map(|id| wire::ServiceDocImage {
            image: id,
            index: image.index,
            width: image.width,
            height: image.height,
            flags: image.flags,
        }))
    }
}

fn images(answer: Result<Option<u32>, i32>) -> Images {
    Images {
        answer: RefCell::new(answer),
        seen: RefCell::new(Vec::new()),
    }
}

extern crate std;

/// The reply batch of `records`, leaked for the fake format.
fn reply(records: &[L<'_>]) -> &'static [u8] {
    let mut bytes = vec![0_u8; 3840];
    let n = local::reply_encode(records, &mut bytes).unwrap();
    bytes.truncate(n);
    std::boxed::Box::leak(bytes.into_boxed_slice())
}

fn kernel() -> crate::Kernel {
    crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap()
}

fn call(op: u32) -> FormatCall<'static> {
    let mut call = FormatCall {
        format: FormatRef(1),
        op,
        document: 9,
        section: 3,
        offset: 4,
        index: 5,
        cursor: 6,
        generation: 7,
        flags: 8,
        image_side: 600,
        cover_side: 300,
        conversion: 0x1122_3344_5566_7788,
        text: "",
        options: Options::NONE,
        css_root: 0,
        css_path: "",
    };
    call.op = op;
    call
}

/// Runs `call` against a format that answers `behavior`.
fn go(
    behavior: Behavior,
    images: &dyn ImageMap,
    sibling: &str,
    call: &FormatCall<'_>,
) -> (Result<usize, i32>, Vec<u8>, Harness) {
    go_sized(behavior, images, sibling, call, 4096)
}

/// As [`go`] with an answer buffer of `size` bytes.
fn go_sized(
    behavior: Behavior,
    images: &dyn ImageMap,
    sibling: &str,
    call: &FormatCall<'_>,
    size: usize,
) -> (Result<usize, i32>, Vec<u8>, Harness) {
    let mut h = Harness::new();
    let plugin = h.add("format-test", behavior);
    let k = kernel();
    let env = k.shared.env(paperpaw_platform::Instant(0));
    let stage = Stage {
        route: &Registry {
            registry: &h.registry,
            env: &env,
        },
        images,
    };
    let mut out = vec![0_u8; size];
    let who = Opener { owner: 5, sibling };
    let r = stage.call(who, Target { plugin, token: 7 }, call, &mut out);
    (r, out, h)
}

/// The service records of a bare stream.
fn records(bytes: &[u8]) -> Vec<Record<'_>> {
    let mut at = 0;
    let mut all = Vec::new();
    while at < bytes.len() {
        let (record, used) = wire::record_decode_prefix(&bytes[at..]).unwrap();
        all.push(record);
        at += used;
    }
    all
}

/// The records of the counted batch the format was sent.
fn rows(payload: &[u8]) -> Vec<L<'_>> {
    local::records_decode(payload)
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

/// The `fmt.call` record the format was sent.
fn sent(h: &Harness) -> (Vec<u8>, usize) {
    let seen = h.seen.borrow();
    let Record::StageCall(framed) = wire::records_decode_single(&seen[0].1).unwrap() else {
        panic!("stage.call")
    };
    (framed.payload.to_vec(), seen.len())
}

#[test]
fn a_call_is_encoded_with_the_generated_codec_and_a_state_gets_its_service_tag() {
    let state = reply(&[L::FmtState(local::FmtState::new(9, 0, 0, 0, 0, 5, "epub"))]);
    let (r, out, h) = go(
        Behavior::Raw(state),
        &images(Ok(None)),
        "",
        &call(fmt::OPEN),
    );
    let n = r.unwrap();
    let got = records(&out[..n]);
    assert!(
        matches!(&got[..], [Record::ServiceDocState(s)] if (s.document, s.conversion) == (9, 5)),
        "{got:?}"
    );
    let (payload, calls) = sent(&h);
    assert_eq!(calls, 1);
    let rows = rows(&payload);
    let L::FmtCall(c) = rows[0] else { panic!() };
    assert_eq!(
        (
            c.op,
            c.document,
            c.section,
            c.offset,
            c.index,
            c.cursor,
            c.generation
        ),
        (2, 9, 3, 4, 5, 6, 7)
    );
    assert_eq!(
        (c.flags, c.image_side, c.cover_side, c.conversion),
        (8, 600, 300, 0x1122_3344_5566_7788)
    );
}

#[test]
fn lists_keep_their_order_and_the_end_row_gets_its_tag() {
    let sections = reply(&[
        L::FmtSection(local::FmtSection::new(0, 100, 0)),
        L::FmtSection(local::FmtSection::new(1, 50, 0)),
        L::FmtEnd(local::FmtEnd::new(0, 2, 0, 0, None)),
    ]);
    let (r, out, _) = go(
        Behavior::Raw(sections),
        &images(Ok(None)),
        "",
        &call(fmt::SECTIONS),
    );
    let got = records(&out[..r.unwrap()]);
    assert!(matches!(
        &got[..],
        [
            Record::ServiceDocSection(a),
            Record::ServiceDocSection(b),
            Record::ServiceEnd(e)
        ] if (a.index, b.index, e.total) == (0, 1, 2)
    ));
}

#[test]
fn a_picture_becomes_the_openers_doc_image_or_is_left_out() {
    let block = reply(&[
        L::FmtImage(local::FmtImage::new("c/a.gray", 6, 8, 4, 1)),
        L::FmtEnd(local::FmtEnd::new(0, 0, 0, 0, None)),
    ]);
    let mapper = images(Ok(Some(77)));
    let (r, out, _) = go(Behavior::Raw(block), &mapper, "", &call(fmt::BLOCK));
    let got = records(&out[..r.unwrap()]);
    assert!(matches!(
        &got[..],
        [Record::ServiceDocImage(i), Record::ServiceEnd(_)]
            if (i.image, i.index, i.width, i.height, i.flags) == (77, 6, 8, 4, 1)
    ));
    // The mapper saw the opener, the document, the block context, the path.
    assert_eq!(
        mapper.seen.borrow()[0],
        (5, 9, true, std::string::String::from("c/a.gray"))
    );
    // Left out: the record is gone, the rest stays.
    let (r, out, _) = go(
        Behavior::Raw(block),
        &images(Ok(None)),
        "",
        &call(fmt::BLOCK),
    );
    let got = records(&out[..r.unwrap()]);
    assert!(matches!(&got[..], [Record::ServiceEnd(_)]));
    // A mapper's refusal is the call's: nothing half.
    let (r, _, _) = go(
        Behavior::Raw(block),
        &images(Err(ERR_BUSY)),
        "",
        &call(fmt::BLOCK),
    );
    assert_eq!(r, Err(ERR_BUSY));
    // Cover: not a block.
    let cover = reply(&[L::FmtImage(local::FmtImage::new(
        "c/a.gray",
        u32::MAX,
        8,
        4,
        0,
    ))]);
    let mapper = images(Ok(Some(3)));
    let (r, _, _) = go(Behavior::Raw(cover), &mapper, "", &call(fmt::COVER));
    assert!(r.is_ok());
    assert!(!mapper.seen.borrow()[0].2);
}

#[test]
fn a_record_the_op_does_not_answer_with_is_invalid() {
    let state = reply(&[L::FmtState(local::FmtState::new(9, 0, 0, 0, 0, 5, "epub"))]);
    for op in [fmt::BLOCK, fmt::SECTIONS, fmt::PROGRESS, fmt::COVER] {
        let (r, _, _) = go(Behavior::Raw(state), &images(Ok(None)), "", &call(op));
        assert_eq!(r, Err(ERR_INVALID), "op {op}");
    }
    // A picture where none is answered.
    let image = reply(&[L::FmtImage(local::FmtImage::new("a", 0, 1, 1, 0))]);
    let (r, _, _) = go(
        Behavior::Raw(image),
        &images(Ok(Some(1))),
        "",
        &call(fmt::META),
    );
    assert_eq!(r, Err(ERR_INVALID));
    // `fmt.yes` has no service record.
    let yes = reply(&[L::FmtYes(local::FmtYes::new(0))]);
    let (r, _, _) = go(Behavior::Raw(yes), &images(Ok(None)), "", &call(fmt::OPEN));
    assert_eq!(r, Err(ERR_INVALID));
}

#[test]
fn ops_the_service_does_not_carry_are_unsupported_and_nothing_is_called() {
    for op in [0, 1, 4, 20, 99] {
        let (r, _, h) = go(Behavior::Good, &images(Ok(None)), "", &call(op));
        assert_eq!(r, Err(ERR_UNSUPPORTED), "op {op}");
        assert!(h.seen.borrow().is_empty());
    }
}

#[test]
fn the_siblings_name_rides_in_text_unless_the_op_names_its_own() {
    let state = reply(&[L::FmtState(local::FmtState::new(9, 0, 0, 0, 0, 5, "x"))]);
    let (_, _, h) = go(
        Behavior::Raw(state),
        &images(Ok(None)),
        "page-1.xhtml",
        &call(fmt::OPEN),
    );
    let (payload, _) = sent(&h);
    let L::FmtCall(c) = rows(&payload)[0] else {
        panic!()
    };
    assert_eq!(c.text, "page-1.xhtml");
    let mut locate = call(fmt::LOCATE);
    locate.text = "/body/p[2]";
    let at = reply(&[L::FmtAt(local::FmtAt::new(9, 2, 0, 0, 0, 0))]);
    let (r, _, h) = go(
        Behavior::Raw(at),
        &images(Ok(None)),
        "page-1.xhtml",
        &locate,
    );
    assert!(r.is_ok());
    let (payload, _) = sent(&h);
    let L::FmtCall(c) = rows(&payload)[0] else {
        panic!()
    };
    assert_eq!(c.text, "/body/p[2]");
}

#[test]
fn the_formats_own_failures_pass_through() {
    for (behavior, expected) in [
        (Behavior::Status(-77), -77),
        (Behavior::Fail(ERR_BUSY), ERR_BUSY),
        (Behavior::Oversize, ERR_INVALID),
        (Behavior::TooMany, ERR_INVALID),
    ] {
        let (r, _, _) = go(behavior, &images(Ok(None)), "", &call(fmt::META));
        assert_eq!(r, Err(expected));
    }
    // A stopped plugin: cancelled.
    let mut h = Harness::new();
    let plugin = h.add("format-test", Behavior::Good);
    h.registry.stop(plugin).unwrap();
    let k = kernel();
    let env = k.shared.env(paperpaw_platform::Instant(0));
    let stage = Stage {
        route: &Registry {
            registry: &h.registry,
            env: &env,
        },
        images: &images(Ok(None)),
    };
    let mut out = vec![0_u8; 4096];
    let r = stage.call(
        Opener {
            owner: 5,
            sibling: "",
        },
        Target { plugin, token: 7 },
        &call(fmt::META),
        &mut out,
    );
    assert_eq!(r, Err(ERR_CANCELLED));
}

fn options(pairs: &[(&str, &str)]) -> &'static [u8] {
    let rows: Vec<Record<'_>> = pairs
        .iter()
        .map(|(key, value)| Record::ServiceDocOption(wire::ServiceDocOption { key, value }))
        .collect();
    let mut bytes = vec![0_u8; 4096];
    let n = wire::records_encode_counted(&rows, 64, 4096, &mut bytes).unwrap();
    bytes.truncate(n);
    std::boxed::Box::leak(bytes.into_boxed_slice())
}

#[test]
fn open_forwards_up_to_twelve_options_whole_and_in_order_after_the_call() {
    let state = reply(&[L::FmtState(local::FmtState::new(9, 0, 0, 0, 0, 5, "x"))]);
    let pairs: Vec<(std::string::String, std::string::String)> = (0..12)
        .map(|n| (std::format!("k{n}"), std::format!("v{n}")))
        .collect();
    let borrowed: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (&**k, &**v)).collect();
    let mut open = call(fmt::OPEN);
    open.options = Options::from_batch(options(&borrowed));
    let (r, _, h) = go(Behavior::Raw(state), &images(Ok(None)), "", &open);
    assert!(r.is_ok());
    let (payload, _) = sent(&h);
    let sent = rows(&payload);
    assert_eq!(sent.len(), 13);
    assert!(matches!(sent[0], L::FmtCall(_)));
    for (n, row) in sent[1..].iter().enumerate() {
        let L::FmtOption(o) = row else { panic!() };
        assert_eq!((o.key, o.value), (pairs[n].0.as_str(), pairs[n].1.as_str()));
    }
    // A thirteenth is refused before the format is entered.
    let mut many = borrowed.clone();
    many.push(("k12", "v12"));
    open.options = Options::from_batch(options(&many));
    let (r, _, h) = go(Behavior::Raw(state), &images(Ok(None)), "", &open);
    assert_eq!(r, Err(ERR_NO_SPACE));
    assert!(h.seen.borrow().is_empty());
}

#[test]
fn only_open_carries_options() {
    let meta = reply(&[L::FmtMeta(local::FmtMeta::new(
        "t", "en", "", "", 0, 0, 1, 0, 0, 0, 0,
    ))]);
    let pairs: Vec<(std::string::String, std::string::String)> = (0..13)
        .map(|n| (std::format!("k{n}"), std::format!("v{n}")))
        .collect();
    let borrowed: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (&**k, &**v)).collect();
    let mut other = call(fmt::META);
    other.options = Options::from_batch(options(&borrowed));
    let (r, _, h) = go(Behavior::Raw(meta), &images(Ok(None)), "", &other);
    // Not even the thirteenth is looked at: the call has no option rows.
    assert!(r.is_ok(), "{r:?}");
    let (payload, _) = sent(&h);
    assert_eq!(rows(&payload).len(), 1);
}

#[test]
fn sniff_says_yes_or_not_mine_and_close_never_fails() {
    let yes = reply(&[L::FmtYes(local::FmtYes::new(0))]);
    let none = reply(&[]);
    let state = reply(&[L::FmtState(local::FmtState::new(9, 0, 0, 0, 0, 5, "x"))]);
    for (behavior, expected) in [
        (Behavior::Raw(yes), Ok(true)),
        (Behavior::Raw(none), Ok(false)),
        (Behavior::Raw(state), Err(ERR_INVALID)),
        (Behavior::Status(-9), Err(-9)),
    ] {
        let mut h = Harness::new();
        let plugin = h.add("format-test", behavior);
        let k = kernel();
        let env = k.shared.env(paperpaw_platform::Instant(0));
        let mapper = images(Ok(None));
        let stage = Stage {
            route: &Registry {
                registry: &h.registry,
                env: &env,
            },
            images: &mapper,
        };
        let mut out = vec![0_u8; 4096];
        let first = [0x50_u8; 600];
        let r = stage.sniff(Target { plugin, token: 7 }, &first, &mut out);
        assert_eq!(r, expected);
        // The first 512 bytes only.
        let (payload, _) = sent(&h);
        let L::FmtCall(c) = rows(&payload)[0] else {
            panic!()
        };
        assert_eq!((c.op, c.arg.len()), (1, 512));
        stage.close(Target { plugin, token: 7 }, 9, &mut out);
        let seen = h.seen.borrow();
        assert_eq!(seen.len(), 2);
    }
}

/// The fullest reply a format can send for `links`: 63 links of the longest
/// href that still fits 3,840 bytes, then the end row (64 records, each
/// growing by one byte in service tags). Returns the batch and its href size.
fn fullest_links() -> (&'static [u8], usize) {
    let hrefs: Vec<std::string::String> = (0..63)
        .map(|i| std::format!("{i:02}{}", "h".repeat(120)))
        .collect();
    for cut in 0..=120 {
        let mut records: Vec<L<'_>> = Vec::new();
        for (i, href) in hrefs.iter().enumerate() {
            let href = &href[..href.len() - cut];
            records.push(L::FmtLink(local::FmtLink::new(i as u32, 1, 0, href)));
        }
        records.push(L::FmtEnd(local::FmtEnd::new(0, 63, 0, 0, None)));
        let mut bytes = vec![0_u8; 3840];
        if let Ok(n) = local::reply_encode(&records, &mut bytes) {
            bytes.truncate(n);
            return (std::boxed::Box::leak(bytes.into_boxed_slice()), 122 - cut);
        }
    }
    panic!("no fullest reply")
}

#[test]
fn the_fullest_reply_is_rewritten_in_place_to_the_byte() {
    let (batch, href) = fullest_links();
    assert!(batch.len() > 3500, "not the worst case: {}", batch.len());
    let rows = 64;
    // One byte of room per record is all the rewrite needs.
    let (r, out, _) = go_sized(
        Behavior::Raw(batch),
        &images(Ok(None)),
        "",
        &call(fmt::LINKS),
        batch.len() + rows,
    );
    let got = records(&out[..r.unwrap()]);
    assert_eq!(got.len(), rows);
    let Record::ServiceDocLink(first) = &got[0] else {
        panic!("first row")
    };
    assert_eq!(first.href.len(), href);
    assert!(first.href.starts_with("00h"));
    let Record::ServiceDocLink(last) = &got[62] else {
        panic!("last link")
    };
    assert!(last.href.starts_with("62h") && last.href.len() == href);
    assert!(matches!(&got[63], Record::ServiceEnd(e) if e.total == 63));
    // One byte less is refused, nothing is cut.
    let (r, _, _) = go_sized(
        Behavior::Raw(batch),
        &images(Ok(None)),
        "",
        &call(fmt::LINKS),
        batch.len() + rows - 1,
    );
    assert_eq!(r, Err(ERR_NO_SPACE));
}

#[test]
fn a_malformed_reply_is_invalid_and_never_translated() {
    let cases: [&'static [u8]; 4] = [
        // One record of tag 5 claiming 127 bytes with two present.
        &[1, 5, 127, 1, 2],
        // A tag no op answers with.
        &[1, 99, 1, 0],
        // A count with nothing behind it.
        &[3],
        // Trailing garbage after the counted records.
        &[0, 1, 2, 3],
    ];
    for (i, bytes) in cases.into_iter().enumerate() {
        let (r, _, _) = go(
            Behavior::Raw(bytes),
            &images(Ok(None)),
            "",
            &call(fmt::LINKS),
        );
        assert!(r.is_err(), "case {i}: {r:?}");
    }
}

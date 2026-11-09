#![allow(clippy::unwrap_used, reason = "test assertions")]

use alloc::vec;
use alloc::vec::Vec;

use super::super::registrations::{
    Record, STAGE_REPLY_PAYLOAD_MAX_BYTES, StageReply, record_encode_one,
};
use super::{ReplyHead, record_encode_tail, reply_room};
use paperpaw_native::host_glue::ERR_NO_SPACE;

const IDS: [&str; 3] = [
    "",
    "a",
    "plugin.with.a.long.identifier.that.fills.all.sixty.four.bytes!!!",
];
const PAYLOADS: [usize; 6] = [0, 1, 127, 128, 3_000, STAGE_REPLY_PAYLOAD_MAX_BYTES];
const CAPS: [usize; 4] = [0, 127, 128, STAGE_REPLY_PAYLOAD_MAX_BYTES];

fn head(subscriber: &str, status: i32) -> ReplyHead<'_> {
    ReplyHead {
        subscriber,
        index: 7,
        status,
        flags: 1,
        signature: 0x0102_0304_0506_0708,
    }
}

fn expected(head: &ReplyHead<'_>, payload: &[u8]) -> Vec<u8> {
    let record = Record::StageReply(StageReply {
        subscriber: head.subscriber,
        index: head.index,
        status: head.status,
        flags: head.flags,
        signature: head.signature,
        payload,
    });
    let mut out = vec![0; 4_096];
    let n = record_encode_one(&record, &mut out).unwrap();
    out.truncate(n);
    out
}

/// What the subscriber writes: bytes that differ from place to place.
fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8 + 1).collect()
}

#[test]
fn the_tail_encoding_is_record_encode_one_for_every_prefix_width() {
    assert_eq!(IDS[2].len(), 64);
    for id in IDS {
        for cap in CAPS {
            for len in PAYLOADS.into_iter().filter(|&len| len <= cap) {
                let head = head(id, 0);
                let room = reply_room(id, cap).unwrap();
                let mut out = vec![0xAA; room + cap];
                let bytes = payload(len);
                out[room..room + len].copy_from_slice(&bytes);
                let n = record_encode_tail(&head, room, cap, len, &mut out).unwrap();
                assert!(
                    out[n..].iter().all(|&b| b == 0),
                    "id {id:?} cap {cap} len {len}"
                );
                assert_eq!(
                    out[..n],
                    expected(&head, &bytes)[..],
                    "id {id:?} cap {cap} len {len}"
                );
            }
        }
    }
}

#[test]
fn a_refusal_has_an_empty_payload_and_leaves_nothing_of_the_subscriber() {
    for id in IDS {
        let head = head(id, -9);
        let cap = STAGE_REPLY_PAYLOAD_MAX_BYTES;
        let room = reply_room(id, cap).unwrap();
        let mut out = vec![0xAA; room + cap];
        out[room..].fill(0x55);
        let n = record_encode_tail(&head, room, cap, 200, &mut out).unwrap();
        assert_eq!(out[..n], expected(&head, &[])[..]);
        assert!(out[n..].iter().all(|&b| b == 0));
    }
}

#[test]
fn wrong_room_sizes_and_caps_change_nothing() {
    let id = IDS[1];
    let cap = 128;
    let room = reply_room(id, cap).unwrap();
    let fresh = vec![0xAA; room + cap];
    let cases: [(usize, usize, usize, usize); 4] = [
        (room - 1, cap, 0, room + cap),   // not this reply's room
        (room, cap, cap + 1, room + cap), // more than lent
        (room, cap, 0, room + cap - 1),   // out shorter than room + cap
        (room, STAGE_REPLY_PAYLOAD_MAX_BYTES + 1, 0, room + cap), // over the record's cap
    ];
    for (room, cap, len, size) in cases {
        let mut out = fresh[..size].to_vec();
        let before = out.clone();
        assert_eq!(
            record_encode_tail(&head(id, 0), room, cap, len, &mut out),
            Err(ERR_NO_SPACE)
        );
        assert_eq!(out, before);
    }
    let long = "x".repeat(65);
    assert_eq!(reply_room(&long, 0), Err(ERR_NO_SPACE));
}

#[test]
fn the_room_fits_the_largest_reply_the_stage_run_checks_for() {
    // stage.run refuses an `out` smaller than a 64-byte id's reply with
    // `max_reply` payload bytes: every subscriber's room and cap fit in it.
    let cap = STAGE_REPLY_PAYLOAD_MAX_BYTES;
    let largest = expected(&head(IDS[2], 0), &payload(cap)).len();
    for id in IDS {
        assert!(reply_room(id, cap).unwrap() + cap <= largest);
    }
}

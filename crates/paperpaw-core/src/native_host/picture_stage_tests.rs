#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The host asks `core.picture` and keeps the first answer that says something.
extern crate std;
use super::*;
use crate::native_host::image_service::ImageTable;
use paperpaw_graphics::Framebuffer;
use paperpaw_native::host_glue::ERR_CANCELLED;
use paperpaw_platform::{PixelFormat, Point};
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::image::{GrayWriter, ImageSink};
use std::vec::Vec;

const A: u64 = 1;

/// Subscribers that answer in turn what the test wrote for them.
struct Fake {
    answers: Vec<Result<Vec<u8>, i32>>,
    asked: Vec<(u64, Vec<u8>)>,
}

impl FilterRunner for Fake {
    fn call(
        &mut self,
        filter: &Subscription<'_>,
        payload: &[u8],
        reply: &mut [u8],
    ) -> Result<usize, i32> {
        let answer = self.answers[self.asked.len()].clone();
        self.asked.push((filter.plugin, payload.to_vec()));
        let bytes = answer?;
        reply[..bytes.len()].copy_from_slice(&bytes);
        Ok(bytes.len())
    }
}

fn subscriber(plugin: u64) -> Subscription<'static> {
    Subscription {
        stage: stage::KEY,
        plugin,
        token: 1,
        order: 0,
        index: 0,
        signature: 0,
        enabled: true,
        style_granted: false,
    }
}

fn tone(curve: &[u8], flags: u32) -> Result<Vec<u8>, i32> {
    let mut out = [0_u8; stage::MAX_REPLY];
    let record = stage::Record::PictureTone(stage::PictureTone::new(curve, flags));
    let n = stage::reply_encode(&[record], &mut out).unwrap();
    Ok(out[..n].to_vec())
}

/// A table with one prepared picture of 8 by 4, every pixel gray 100.
fn prepared(storage: &MemoryStorage) -> (ImageTable, Wanted) {
    let t = ImageTable::new();
    let id = t.add_prepared(A, 3, 8, 4, false).unwrap();
    let wanted = t.tone_wanted().unwrap();
    assert_eq!(wanted.id, id);
    let mut writer = GrayWriter::new(storage, &wanted.path);
    writer.start(8, 4).unwrap();
    for _ in 0..4 {
        writer.row(&[100; 8]).unwrap();
    }
    writer.finish().unwrap();
    (t, wanted)
}

fn shown(t: &ImageTable, storage: &MemoryStorage, id: u32) -> u8 {
    let mut fb = Framebuffer::new(8, 8, PixelFormat::Gray8).unwrap();
    t.draw(A, id, &mut fb.canvas(), storage, 0, 0, 8, 4, 0)
        .unwrap();
    fb.canvas().gray_at(Point::new(4, 2)).unwrap()
}

fn darker() -> [u8; 256] {
    let mut curve = [0_u8; 256];
    for (value, slot) in curve.iter_mut().enumerate() {
        *slot = (value / 2) as u8;
    }
    curve
}

#[test]
fn the_first_answer_that_says_something_is_kept_with_the_picture() {
    let storage = MemoryStorage::new();
    let (t, wanted) = prepared(&storage);
    let subscribers = [subscriber(7), subscriber(8)];
    // The first leaves the picture alone, the second answers a curve.
    let mut fake = Fake {
        answers: std::vec![tone(&[], 0), tone(&darker(), 0)],
        asked: Vec::new(),
    };
    ask(&mut fake, &subscribers, &storage, &t, &wanted);
    assert_eq!(fake.asked.len(), 2);
    assert_eq!((fake.asked[0].0, fake.asked[1].0), (7, 8));
    assert_eq!(shown(&t, &storage, wanted.id), 50);
    // What they were asked: a plugin's own picture, its size and its grays.
    let mut rows = stage::input_decode(&fake.asked[0].1).unwrap();
    let Some(Ok(stage::Record::PictureAsk(asked))) = rows.next() else {
        panic!("no picture.ask");
    };
    assert_eq!(
        (asked.place, asked.width, asked.height, asked.flags),
        (5, 8, 4, 0)
    );
    assert_eq!(asked.histogram.len(), 16);
    assert_eq!((asked.histogram[6], asked.histogram[0]), (255, 0));
    assert_eq!(asked.path, "");
}

#[test]
fn an_answer_ends_the_asking_and_a_failed_or_bad_one_does_not() {
    let storage = MemoryStorage::new();
    // The first answers: the second is not asked.
    let (t, wanted) = prepared(&storage);
    let subscribers = [subscriber(7), subscriber(8)];
    let mut fake = Fake {
        answers: std::vec![tone(&darker(), 0), tone(&[], 1)],
        asked: Vec::new(),
    };
    ask(&mut fake, &subscribers, &storage, &t, &wanted);
    assert_eq!(fake.asked.len(), 1);
    assert_eq!(shown(&t, &storage, wanted.id), 50);
    // A subscriber that fails, and one whose curve is no curve, change
    // nothing: the picture is drawn as it is.
    let (t, wanted) = prepared(&storage);
    let mut fake = Fake {
        answers: std::vec![Err(ERR_CANCELLED), tone(&[1, 2, 3], 0)],
        asked: Vec::new(),
    };
    ask(&mut fake, &subscribers, &storage, &t, &wanted);
    assert_eq!(fake.asked.len(), 2);
    assert_eq!(shown(&t, &storage, wanted.id), 100);
    // A picture that cannot be read is not asked about.
    let (t, wanted) = prepared(&storage);
    storage.remove(&wanted.path).unwrap();
    let mut fake = Fake {
        answers: std::vec![tone(&darker(), 0)],
        asked: Vec::new(),
    };
    ask(&mut fake, &subscribers, &storage, &t, &wanted);
    assert!(fake.asked.is_empty());
}

//! The host resolves the picture sides: a format never asks the screen and
//! never gets 0 (`document.md` 3).
use super::*;

fn open_with(fake: &Rc<Fake>, image_side: u32, cover_side: u32) -> u32 {
    *fake.answer.borrow_mut() = state_answer(0, 0);
    let mut open = open_record();
    open.image_side = image_side;
    open.cover_side = cover_side;
    state_of(&call(fake, 1, &[Record::ServiceDocOpen(open)]).unwrap()).document
}

#[test]
fn zero_sides_are_the_defaults_of_the_screen_and_share_the_cache() {
    for (long_side, cover) in [(800, 300), (1448, 543)] {
        // Two opens at once: the large device's limit, not the X4's.
        let fake = Fake::new(Limits::LARGE);
        fake.long_side.set(long_side);
        open_with(&fake, 0, 0);
        // An explicit open of the same sides is the same conversion key
        // input, so the two share one cache.
        open_with(&fake, long_side, cover);
        let keyed = fake.keyed.borrow().clone();
        let sides = (long_side, cover);
        assert_eq!(keyed, [sides, sides], "screen {long_side}");
        let calls = fake.calls.borrow();
        assert_eq!((calls[0][6], calls[0][7]), sides);
        assert_eq!((calls[1][6], calls[1][7]), sides);
    }
}

/// An open that names no picture side decodes page pictures no larger than
/// the screen is long, as Beta 3 did, and never larger than 1,600: the X4
/// family's 800, the X3's 792, a 1,448 and a 1,680 pixel screen (#250). The
/// side is the long one, so a turned screen is the same conversion.
#[test]
fn a_zero_image_side_is_the_screens_long_side_at_most_1600() {
    for (long_side, wanted) in [(800, 800), (792, 792), (1448, 1448), (1680, 1600)] {
        let fake = Fake::new(Limits::X4);
        fake.long_side.set(long_side);
        open_with(&fake, 0, 300);
        assert_eq!(
            fake.keyed.borrow()[0].0,
            wanted,
            "screen {long_side}: the key"
        );
        assert_eq!(
            fake.calls.borrow()[0][6],
            wanted,
            "screen {long_side}: the format"
        );
    }
    // No screen told: never 0 to a format.
    assert_eq!(args::default_image_side(0), 1600);
}

/// A side an open names is taken as it is: a reader that wants the most
/// still gets 1,600 on a small screen.
#[test]
fn a_named_image_side_is_not_bound_by_the_screen() {
    let fake = Fake::new(Limits::X4);
    fake.long_side.set(800);
    open_with(&fake, 1600, 300);
    assert_eq!(fake.keyed.borrow()[0].0, 1600);
    assert_eq!(fake.calls.borrow()[0][6], 1600);
}

#[test]
fn no_stage_call_carries_a_zero_side() {
    let fake = Fake::new(Limits::X4);
    fake.long_side.set(1448);
    let id = open_with(&fake, 0, 0);
    fake.say(&[Record::ServiceDocImage(Image {
        image: 0,
        index: 0,
        width: 1,
        height: 1,
        flags: 0,
    })]);
    let ask = |index, side| {
        Record::ServiceDocImageAsk(wire::ServiceDocImageAsk {
            document: id,
            index,
            side,
            flags: 0,
        })
    };
    // The cover (document op 15 and op 16 with the cover index), and a
    // page picture asked for with side 0 and with a side.
    call(&fake, 15, &[doc_ref(id)]).unwrap();
    call(&fake, 16, &[ask(u32::MAX, 0)]).unwrap();
    call(&fake, 16, &[ask(u32::MAX, 900)]).unwrap();
    call(&fake, 16, &[ask(3, 0)]).unwrap();
    call(&fake, 16, &[ask(3, 1200)]).unwrap();
    let calls = fake.calls.borrow();
    let wanted: Vec<_> = calls
        .iter()
        .filter(|call| matches!(call[0], fmt::OPEN | fmt::COVER | fmt::IMAGE))
        .collect();
    assert_eq!(wanted.len(), 6);
    for call in wanted {
        assert!(call[6] != 0 && call[7] != 0, "a zero side in {call:?}");
    }
}

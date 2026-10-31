//! The last page of a book says the book is read to the end.

use super::*;

#[test]
fn the_last_page_says_the_book_is_read_to_the_end() {
    let mut reading = Reading::new(Shared::default());
    reading.open();
    // Turned until a press turns no more.
    let turns = |reading: &Reading| reading.heard().iter().filter(|e| **e == "turned").count();
    let mut turned = turns(&reading);
    loop {
        reading.run(core_actions::NEXT_PAGE);
        let now = turns(&reading);
        if now == turned {
            break;
        }
        turned = now;
        assert!(turned < 2000, "the book has no last page");
    }
    reading.now += 10;
    let effects = reading
        .kernel
        .run_action(Instant(reading.now), core_actions::BACK);
    reading.settle(effects);
    let heard = reading.heard.borrow();
    let progress = |name: &str| -> Vec<f32> {
        heard
            .iter()
            .filter(|(n, _)| *n == name)
            .map(|(_, r)| r.progress)
            .collect()
    };
    // The pages turned say how far their start is; the book's closing,
    // after the last page was reached, says it is read (100).
    let turned = progress("turned");
    assert!(turned.iter().all(|p| *p < 100.0), "{turned:?}");
    assert_eq!(progress("closed"), [100.0]);
}

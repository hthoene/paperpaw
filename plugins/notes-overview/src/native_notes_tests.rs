//! The pages of a book's notes (`native_notes`) over a scripted library: the
//! same pages at any size of the replies, forward and back, the ends of the
//! list, a stale cursor, a host that never ends, and memory refused.
use crate::fail_alloc::sweep;
use crate::model::Kind;
use crate::native_notes::{
    Choice, Flow, Layout, MOST_NOTES, MOST_REPLIES, MOST_SCREEN_PAGES, Replies, Row, Show, Start,
    page,
};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::Item;

struct Fake {
    /// The notes: every fifth is of a kind that is not shown.
    notes: usize,
    size: usize,
    stale: bool,
    endless: bool,
    calls: usize,
    /// The text of every note, made once: what a reply allocates is what is
    /// counted by the sweep.
    texts: Vec<String>,
}

impl Fake {
    fn new(notes: usize, size: usize) -> Self {
        Self {
            notes,
            size,
            stale: false,
            endless: false,
            calls: 0,
            texts: (0..notes).map(|n| format!("n{n}")).collect(),
        }
    }

    fn kind(n: usize) -> Kind {
        if n % 5 == 4 {
            Kind::Other
        } else if n.is_multiple_of(2) {
            Kind::Highlight
        } else {
            Kind::Bookmark
        }
    }
}

impl Replies for Fake {
    type Cursor = usize;

    fn reply(
        &mut self,
        cursor: usize,
        each: &mut dyn FnMut(usize, &Row<'_>) -> Result<Flow, Error>,
    ) -> Result<Option<usize>, Error> {
        self.calls += 1;
        if self.stale {
            return Err(Error::State);
        }
        let end = (cursor + self.size).min(self.notes);
        for n in cursor..end {
            let row = Row {
                kind: Self::kind(n),
                text: &self.texts[n],
                note: "",
                percent: 1,
            };
            if each(n - cursor, &row)? == Flow::Stop {
                break;
            }
        }
        Ok((end < self.notes || self.endless).then_some(end))
    }
}

fn make(show: Show<'_, '_>) -> Result<Item, Error> {
    match show {
        Show::Previous => Item::row("<", ""),
        Show::More => Item::row(">", ""),
        Show::Note(row) => Item::row(row.text, ""),
    }
}

fn titles(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            Item::Row(row) => row.title.clone(),
            Item::Heading(text) => text.clone(),
        })
        .collect()
}

/// Every page of the book, forward from the first, and the pages' titles.
fn forward(fake: &mut Fake) -> Vec<(Layout, Vec<String>)> {
    let mut pages = alloc::vec![Start::default()];
    let mut seen = Vec::new();
    loop {
        let got = page(fake, &pages, &mut make).unwrap();
        seen.push((got.layout, titles(&got.items)));
        match got.next {
            Some(next) => pages.push(next),
            None => return seen,
        }
    }
}

fn shown(total: usize) -> Vec<String> {
    (0..total)
        .filter(|n| Fake::kind(*n) != Kind::Other)
        .map(|n| format!("n{n}"))
        .collect()
}

#[test]
fn the_pages_hold_the_notes_in_order_with_rows_to_turn() {
    let seen = forward(&mut Fake::new(300, 7));
    // 300 notes, 240 shown: pages of 100, 100 and 40.
    assert_eq!(seen.len(), 3);
    let mut all = Vec::new();
    for (index, (layout, rows)) in seen.iter().enumerate() {
        let first = index == 0;
        let last = index == seen.len() - 1;
        assert_eq!(layout.before, usize::from(!first));
        assert_eq!(layout.more, !last);
        assert_eq!(layout.notes, if last { 40 } else { MOST_NOTES });
        assert_eq!(
            rows.len(),
            layout.before + layout.notes + usize::from(layout.more)
        );
        assert_eq!(rows.first().map(String::as_str) == Some("<"), !first);
        assert_eq!(rows.last().map(String::as_str) == Some(">"), !last);
        all.extend(rows.iter().filter(|r| r.starts_with('n')).cloned());
    }
    assert_eq!(all, shown(300));
}

#[test]
fn any_size_of_reply_gives_the_same_pages() {
    let whole = forward(&mut Fake::new(300, 7));
    for size in [1, 2, 3, 99, 100, 101, 125, 1000] {
        assert_eq!(forward(&mut Fake::new(300, size)), whole, "{size}");
    }
}

#[test]
fn the_page_before_is_the_same_page_again() {
    let mut fake = Fake::new(300, 7);
    let mut pages = alloc::vec![Start::default()];
    let first = page(&mut fake, &pages, &mut make).unwrap();
    pages.push(first.next.unwrap());
    let second = page(&mut fake, &pages, &mut make).unwrap();
    pages.push(second.next.unwrap());
    let third = page(&mut fake, &pages, &mut make).unwrap();
    assert_eq!(third.layout.notes, 40);
    // Back to the second, then to the first.
    pages.pop();
    assert_eq!(
        titles(&page(&mut fake, &pages, &mut make).unwrap().items),
        titles(&second.items)
    );
    pages.pop();
    assert_eq!(
        titles(&page(&mut fake, &pages, &mut make).unwrap().items),
        titles(&first.items)
    );
}

#[test]
fn a_book_of_exactly_one_page_has_no_row_to_turn() {
    // 100 notes shown take 125 in the book (every fifth is not shown).
    let seen = forward(&mut Fake::new(125, 10));
    assert_eq!(seen.len(), 1);
    assert_eq!(
        seen[0].0,
        Layout {
            before: 0,
            notes: 100,
            more: false
        }
    );
    // Two more shown notes (and one not shown) make a second page of two.
    let seen = forward(&mut Fake::new(127, 10));
    assert_eq!(seen.len(), 2);
    assert_eq!(
        seen[1].0,
        Layout {
            before: 1,
            notes: 2,
            more: false
        }
    );
    // A book with no notes has no rows.
    let seen = forward(&mut Fake::new(0, 10));
    assert_eq!(seen, [(Layout::default(), Vec::new())]);
}

#[test]
fn a_stale_cursor_is_state_and_a_host_that_never_ends_is_stopped() {
    let mut fake = Fake::new(300, 7);
    fake.stale = true;
    let pages = [Start::default()];
    assert_eq!(
        page(&mut fake, &pages, &mut make).unwrap_err(),
        Error::State
    );
    // Replies with no end: the page is cut after a bounded number of them.
    let mut fake = Fake::new(10, 100);
    fake.endless = true;
    let got = page(&mut fake, &pages, &mut make).unwrap();
    assert!(got.next.is_some());
    assert_eq!(fake.calls, MOST_REPLIES);
}

#[test]
fn the_pages_that_can_be_reached_are_bounded() {
    let mut fake = Fake::new(1000, 50);
    let pages: Vec<Start<usize>> = (0..MOST_SCREEN_PAGES).map(|_| Start::default()).collect();
    let got = page(&mut fake, &pages, &mut make).unwrap();
    assert!(got.next.is_none());
    assert!(!got.layout.more);
}

#[test]
fn rows_say_what_choosing_them_does() {
    let middle = Layout {
        before: 1,
        notes: 3,
        more: true,
    };
    let choices: Vec<Choice> = (0..6).map(|i| middle.choice(i)).collect();
    use Choice::{Next, Note, Previous};
    assert_eq!(choices, [Previous, Note, Note, Note, Next, Note]);
    let first = Layout {
        before: 0,
        notes: 2,
        more: true,
    };
    assert_eq!([first.choice(0), first.choice(2)], [Note, Next]);
    let last = Layout {
        before: 1,
        notes: 2,
        more: false,
    };
    assert_eq!([last.choice(0), last.choice(3)], [Previous, Note]);
}

#[test]
fn refused_allocations_end_in_no_memory() {
    let mut fake = Fake::new(260, 7);
    let pages = [Start::default()];
    sweep(|| Ok(page(&mut fake, &pages, &mut make)?.items.len()));
}

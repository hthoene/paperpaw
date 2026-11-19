//! The mock content: a shelf of books, how far each is read.

pub struct Book {
    pub title: &'static str,
    pub author: &'static str,
    pub percent: i32,
    /// The sample library has this book: its cover is the library's.
    pub real: bool,
}

const fn book(title: &'static str, author: &'static str, percent: i32) -> Book {
    Book {
        title,
        author,
        percent,
        real: false,
    }
}

pub const BOOKS: [Book; 6] = [
    Book {
        title: "Alice's Adventures in Wonderland",
        author: "Lewis Carroll",
        percent: 42,
        real: true,
    },
    book("Die Verwandlung", "Franz Kafka", 78),
    book("Pride and Prejudice", "Jane Austen", 12),
    book("Moby-Dick", "Herman Melville", 0),
    book("Frankenstein", "Mary Shelley", 100),
    book("Walden", "Henry David Thoreau", 5),
];

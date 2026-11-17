//! An index of the library for views of every book (a list, a grid): each
//! book of the catalog with what it is sorted and filtered by, in 24
//! bytes, made in one pass over the catalog (`Index::build`). Its
//! books' lines are read again by where they start (`Index::record`)
//! for the page a view shows, never while drawing.
//!
//! On devices of little memory it holds `cap` books at most and counts
//! the others (`Index::total`). A view covered by a book can keep it in
//! a file (`Index::save`) and let it go, and read it back when it shows
//! again (`Index::load`).

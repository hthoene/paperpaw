//! The books' details and covers made small, kept by earlier versions in
//! one folder each (`books/<key>.ini`, `covers/<key>.thumb`), moved into
//! the folders split by keys that hold them now (`details/<k>/<key>.ini`,
//! `thumbs/<k>/<key>.thumb`, see [`kept_path`](super::kept_path)): once, a
//! few at a time, by the kernel. Until then they are read where they were.
//! Then what the folders of caches for books ([`BookData`], made again from
//! the book when needed) hold in the folder itself, as earlier versions of
//! their plugins kept it, goes: kept in the parts of their keys now, those
//! are out of date, and a folder of thousands is slow on a memory card.
//!
//! A step lists the next names of an old folder, or moves (or removes)
//! some of them: in the order the folder lists them, so that a memory card
//! finds each where it found the last (`paperpaw-fat`, noted folders), each
//! move whole or not made at all (cut off, the card's record of moves
//! settles it). Where what is moved is there already (written since), that
//! stays and the old one goes. What cannot be moved stays where it was,
//! read there; the folder goes once nothing is left of it.













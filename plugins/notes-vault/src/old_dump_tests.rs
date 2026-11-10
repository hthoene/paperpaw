//! TEMPORARY: writes the interface-6 answers as the transcript.
#![allow(clippy::unwrap_used, reason = "test")]
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;
use paperpaw_sdk::MemoryStorage;

use crate::{index, lines, note, page, source, tags};

mod shim {
    use super::*;
    

    pub fn lines_of(data: &[u8]) -> Vec<String> {
        let storage = MemoryStorage::new().with("f", data.to_vec());
        let mut v = Vec::new();
        lines::each(&storage, "f", &mut |l| {
            v.push(l.to_string());
            true
        })
        .unwrap();
        v
    }
    pub fn lines_stopped(data: &[u8]) -> usize {
        let storage = MemoryStorage::new().with("f", data.to_vec());
        let mut a = 0;
        lines::each(&storage, "f", &mut |_| {
            a += 1;
            a < 2
        })
        .unwrap();
        a
    }
    pub fn is_note(n: &str) -> bool { note::is_note(n) }
    pub fn is_hidden(n: &str) -> bool { note::is_hidden(n) }
    pub fn title(p: &str) -> String { note::title(p).to_string() }
    pub fn folder_in(p: &str, r: &str) -> String { note::folder_in(p, r).to_string() }
    pub fn excerpt(t: &str) -> String { note::excerpt(t) }
    pub fn excerpt_of_file(s: &MemoryStorage, p: &str) -> String { note::excerpt_of_file(s, p) }
    pub fn tags_of_text(t: &str) -> Vec<String> { tags::of_text(t) }
    pub fn tags_of_file(s: &MemoryStorage, p: &str) -> Vec<String> { tags::of_file(s, p) }
    pub fn windows(recent: bool, rows: &[R]) -> Vec<(bool, Vec<R>)> {
        let order = if recent { page::Order::Recent } else { page::Order::Name };
        let mut out = Vec::new();
        let mut after: Option<page::Key> = None;
        for _ in 0..5 {
            let mut feed = |emit: &mut dyn FnMut(page::Row)| {
                for (t, p, f, m) in rows {
                    emit(page::Row { title: t.clone(), path: p.clone(), folder: *f, modified: *m });
                }
            };
            let w = page::window(order, after.as_ref(), &mut feed);
            let list: Vec<R> = w.rows.iter().map(|r| (r.title.clone(), r.path.clone(), r.folder, r.modified)).collect();
            let last = w.rows.last().map(|r| order.key(r));
            out.push((w.more, list));
            let Some(last) = last else { break };
            after = Some(last);
            if !w.more { break; }
        }
        out
    }
    pub fn walk(storage: &MemoryStorage) -> Walk {
        let mut walk = index::Indexer::new("v/");
        let state = loop {
            match walk.step(storage) {
                index::Step::More => {}
                index::Step::Done(s) => break s,
                index::Step::Failed => panic!(),
            }
        };
        let mut tagger = index::Tagger::new();
        while !tagger.step(storage).unwrap() {}
        let counts = index::tag_counts(storage);
        let mut notes = Vec::new();
        index::notes(storage, &mut |m, p| { notes.push((m, p.to_string())); true });
        let mut lists = Vec::new();
        for s in [
            source::Source::Folder("v".into()),
            source::Source::Folder("v/A".into()),
            source::Source::Notes { folder: String::new(), query: String::new() },
            source::Source::Notes { folder: "v/A/".into(), query: "note 1".into() },
            source::Source::Tag("tag".into()),
            source::Source::Tag("one".into()),
        ] {
            let mut a = Vec::new();
            s.feed(storage, &mut |r| a.push((r.title, r.path, r.folder, r.modified)));
            lists.push(a);
        }
        Walk {
            state: (state.folder.clone(), state.notes, state.stamp),
            tagged: state.tagged,
            done: tagger.done,
            index: storage.get(&index::index_path()),
            tags: storage.get(&index::tags_path()),
            counts,
            notes,
            lists,
        }
    }
    pub fn state_file(folder: &str, built: i64, notes: u32, stamp: u64, tagged: bool) -> Vec<u8> {
        let old = MemoryStorage::new();
        index::State { folder: folder.into(), built, notes, stamp, tagged }.write(&old);
        old.get(&format!("{}/state.txt", paperpaw_sdk::Env::plugin_dir("notes-vault"))).unwrap()
    }
    pub fn search(index_text: &str, query: &str) -> Vec<String> {
        let storage = MemoryStorage::new().with(&index::index_path(), index_text.to_string());
        let mut a = Vec::new();
        source::Source::Notes { folder: String::new(), query: query.into() }
            .feed(&storage, &mut |r| a.push(r.title));
        a
    }
}

include!("native_golden_cases.rs");

#[test]
fn dump_old() {
    std::fs::write(
        concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/model.v1"),
        transcript(),
    )
    .unwrap();
}

//! The model (`native_model`: lines, names, excerpts, tags, windows, the index and
//! the lists made of it) on the inputs interface 6 was compared on, as a
//! transcript kept in `testdata/model.v1`: fixed and generated text and
//! vaults, every answer or a hash of it. The transcript was written while the
//! interface-6 code was still there and the native model was equal to it on
//! every line (the Greek final sigma apart: two queries are below); a change
//! here is a change of what the plugin shows, so it is reviewed
//! (`PAPERPAW_BLESS=1 cargo test` writes the file anew).
#![allow(clippy::unwrap_used, reason = "test assertions")]

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

use paperpaw_sdk::MemoryStorage;

mod shim {
    use super::{MemoryStorage, R, String, ToString, Vec, Walk};
    use crate::native_model as m;

    pub fn lines_of(data: &[u8]) -> Vec<String> {
        let storage = MemoryStorage::new().with("f", data.to_vec());
        let mut lines = Vec::new();
        m::lines::each(&storage, "f", &mut |line| {
            lines.push(line.to_string());
            true
        })
        .unwrap();
        lines
    }

    pub fn lines_stopped(data: &[u8]) -> usize {
        let storage = MemoryStorage::new().with("f", data.to_vec());
        let mut count = 0;
        m::lines::each(&storage, "f", &mut |_| {
            count += 1;
            count < 2
        })
        .unwrap();
        count
    }

    pub fn is_note(name: &str) -> bool {
        m::note::is_note(name)
    }

    pub fn is_hidden(name: &str) -> bool {
        m::note::is_hidden(name)
    }

    pub fn title(path: &str) -> String {
        m::note::title(path).to_string()
    }

    pub fn folder_in(path: &str, root: &str) -> String {
        m::note::folder_in(path, root).to_string()
    }

    pub fn excerpt(text: &str) -> String {
        m::note::excerpt(text).unwrap()
    }

    pub fn excerpt_of_file(storage: &MemoryStorage, path: &str) -> String {
        m::note::excerpt_of_file(storage, path).unwrap()
    }

    pub fn tags_of_text(text: &str) -> Vec<String> {
        m::tags::of_text(text).unwrap()
    }

    pub fn tags_of_file(storage: &MemoryStorage, path: &str) -> Vec<String> {
        m::tags::of_file(storage, path).unwrap()
    }

    pub fn windows(recent: bool, rows: &[R]) -> Vec<(bool, Vec<R>)> {
        let order = if recent {
            m::page::Order::Recent
        } else {
            m::page::Order::Name
        };
        let mut out = Vec::new();
        let mut after: Option<m::page::Row> = None;
        for _ in 0..5 {
            let mut feed = |emit: &mut m::page::Emit<'_>| {
                for (title, path, folder, modified) in rows {
                    emit(m::page::RowRef {
                        title,
                        path,
                        folder: *folder,
                        modified: *modified,
                    })?;
                }
                Ok(())
            };
            let window = m::page::window(order, after.as_ref(), &mut feed).unwrap();
            let list = window
                .rows
                .iter()
                .map(|r| (r.title.clone(), r.path.clone(), r.folder, r.modified))
                .collect();
            let (more, last) = (window.more, window.rows.last().cloned());
            out.push((more, list));
            let Some(last) = last else { break };
            after = Some(last);
            if !more {
                break;
            }
        }
        out
    }

    pub fn walk(vault: &MemoryStorage) -> Walk {
        let data = MemoryStorage::new();
        let mut walk = m::index::Indexer::new("v/").unwrap();
        let state = loop {
            match walk.step(vault, &data).unwrap() {
                m::index::Step::More => {}
                m::index::Step::Done(state) => break state,
                m::index::Step::Failed => panic!("walk failed"),
            }
        };
        let mut tagger = m::index::Tagger::new();
        while !tagger.step(vault, &data).unwrap().unwrap() {}
        let counts = m::index::tag_counts(&data).unwrap();
        let mut notes = Vec::new();
        m::index::notes(&data, &mut |modified, path| {
            notes.push((modified, path.to_string()));
            true
        })
        .unwrap();
        let mut lists = Vec::new();
        for source in [
            m::source::Source::Folder("v".into()),
            m::source::Source::Folder("v/A".into()),
            m::source::Source::Notes {
                folder: String::new(),
                query: String::new(),
            },
            m::source::Source::Notes {
                folder: "v/A/".into(),
                query: "note 1".into(),
            },
            m::source::Source::Tag("tag".into()),
            m::source::Source::Tag("one".into()),
        ] {
            let mut list = Vec::new();
            source
                .feed(vault, &data, &mut |r| {
                    list.push((
                        r.title.to_string(),
                        r.path.to_string(),
                        r.folder,
                        r.modified,
                    ));
                    Ok(())
                })
                .unwrap();
            lists.push(list);
        }
        Walk {
            state: (state.folder.clone(), state.notes, state.stamp),
            tagged: state.tagged,
            done: tagger.done,
            index: data.get(m::index::INDEX),
            tags: data.get(m::index::TAGS),
            counts,
            notes,
            lists,
        }
    }

    pub fn state_file(folder: &str, built: i64, notes: u32, stamp: u64, tagged: bool) -> Vec<u8> {
        let data = MemoryStorage::new();
        m::index::State {
            folder: folder.into(),
            built,
            notes,
            stamp,
            tagged,
        }
        .write(&data)
        .unwrap();
        data.get(m::index::STATE).unwrap()
    }

    pub fn search(index: &str, query: &str) -> Vec<String> {
        let data = MemoryStorage::new().with(m::index::INDEX, index.to_string());
        let mut found = Vec::new();
        m::source::Source::Notes {
            folder: String::new(),
            query: query.into(),
        }
        .feed(&MemoryStorage::new(), &data, &mut |r| {
            found.push(r.title.to_string());
            Ok(())
        })
        .unwrap();
        found
    }
}

include!("native_golden_cases.rs");

const FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/model.v1");

#[test]
fn the_model_answers_as_the_transcript_of_interface_6_says() {
    let made = transcript();
    if std::env::var_os("PAPERPAW_BLESS").is_some() {
        std::fs::write(FILE, &made).unwrap();
        return;
    }
    let kept = include_str!("../testdata/model.v1");
    for (n, (a, b)) in kept.lines().zip(made.lines()).enumerate() {
        assert_eq!(a, b, "line {}", n + 1);
    }
    assert_eq!(kept.lines().count(), made.lines().count());
}

/// The state file is read back, and not read where it is not this plugin's, not
/// whole or too long.
#[test]
fn the_state_is_read_back_and_bad_ones_are_none() {
    use crate::native_model::index::{State, kept};
    let data = MemoryStorage::new();
    let state = State {
        folder: "v\nw".into(),
        built: -5,
        notes: 3,
        stamp: u64::MAX,
        tagged: true,
    };
    state.write(&data).unwrap();
    let read = State::read(&data).unwrap().unwrap();
    assert_eq!(
        (read.built, read.notes, read.stamp, read.tagged),
        (-5, 3, u64::MAX, true)
    );
    assert_eq!(read.folder, "v w");
    for bad in [
        &b""[..],
        b"notes-vault 2\nv\n0\n0\n0\n0\n",
        b"notes-vault 1\nv\nx\n0\n0\n0\n",
        b"notes-vault 1\nv\n0\n0\n",
    ] {
        let data = MemoryStorage::new().with("state.txt", bad.to_vec());
        assert_eq!(State::read(&data).unwrap(), None);
    }
    let long = MemoryStorage::new().with("state.txt", alloc::vec![b'x'; 3000]);
    assert_eq!(State::read(&long).unwrap(), None);
    assert!(kept(&data, "v w").unwrap().is_none(), "no index file");
    let data = data.with("index.txt", b"".to_vec());
    assert!(kept(&data, "v w").unwrap().is_some());
    assert!(kept(&data, "other").unwrap().is_none());
}

/// What interface 6 missed in Greek: a query with the plain sigma at its end
/// finds the title with the final one (and the other way round); the search
/// takes `ς` for `σ`. Interface 6 found none for these queries.
#[test]
fn the_search_takes_the_final_sigma_for_the_plain_one() {
    let titles = ["ΟΔΟΣ", "οδος", "ΣΟΦΟΣ", "σοφος", "Σίσυφος", "Plain"];
    let index: String = titles
        .iter()
        .enumerate()
        .map(|(n, t)| format!("{n}\tv/{t}.md\n"))
        .collect();
    assert_eq!(shim::search(&index, "οδοσ"), ["ΟΔΟΣ", "οδος"]);
    assert_eq!(shim::search(&index, "ΣΟΦΟς").len(), 2);
    assert_eq!(shim::search(&index, "ΣΊΣ"), ["Σίσυφος"]);
}

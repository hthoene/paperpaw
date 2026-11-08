//! The native model gives the same answers as the interface-6 code it was
//! made from, on fixed cases and on generated text and vaults. (Known
//! differences, both about the Greek final sigma: names are put in order
//! character by character in lower case, and the search takes `ς` for `σ`.)
#![allow(clippy::unwrap_used, reason = "test assertions")]

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::MemoryStorage;

use crate::native_model as new;
use crate::{index, lines, note, page, source, tags};

/// A small deterministic generator, so that failures repeat.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Pieces of Markdown that matter to the readers, put together at random.
fn markdown(rng: &mut Rng, pieces: usize) -> String {
    const PARTS: [&str; 34] = [
        "---\n",
        "...\n",
        "tags: [a, \"b c\"]\n",
        "tags:\n",
        "  - one\n",
        "- two\n",
        "# Heading\n",
        "## Sub\n",
        "\n",
        "```\n",
        "~~~\n",
        "#tag ",
        "#Tag/Nested ",
        "#12 ",
        "`#code` ",
        "plain words ",
        "**bold** ",
        "[[Link|label]] ",
        "[[Other#part]] ",
        "[text](https://x.org) ",
        "> quote ",
        "| a | b |\n",
        "* * *\n",
        "- item\n",
        "\r\n",
        "\n",
        "é ü ß ",
        "日本語 ",
        "[[",
        "](",
        "x",
        "#",
        "a#b ",
        "tag: x\n",
    ];
    let mut out = String::new();
    for _ in 0..pieces {
        out.push_str(PARTS[rng.below(PARTS.len())]);
    }
    out
}

fn bytes(rng: &mut Rng, len: usize) -> Vec<u8> {
    const SOME: [u8; 8] = [b'a', b'\n', b'\r', b'\t', 0xC3, 0xA9, 0xFF, b'#'];
    (0..len)
        .map(|_| {
            if rng.below(4) == 0 {
                rng.next() as u8
            } else {
                SOME[rng.below(SOME.len())]
            }
        })
        .collect()
}

#[test]
fn lines_are_the_same() {
    let mut rng = Rng(1);
    let mut cases: Vec<Vec<u8>> = ["", "a", "a\n", "a\nb\r\n\nlast", "\n\n", "x\r"]
        .iter()
        .map(|s| s.as_bytes().to_vec())
        .collect();
    cases.push(format!("{}\nshort\n", "x".repeat(5000)).into_bytes());
    cases.push("é".repeat(1500).into_bytes());
    for len in [0, 1, 2, 5, 40, 300, 3000] {
        for _ in 0..10 {
            cases.push(bytes(&mut rng, len));
        }
    }
    for data in cases {
        let storage = MemoryStorage::new().with("f", data.clone());
        let mut old: Vec<String> = Vec::new();
        lines::each(&storage, "f", &mut |l| {
            old.push(l.to_string());
            true
        })
        .unwrap();
        let mut now: Vec<String> = Vec::new();
        new::lines::each(&storage, "f", &mut |l| {
            now.push(l.to_string());
            true
        })
        .unwrap();
        assert_eq!(old, now, "{data:?}");
        // Stopping early.
        let (mut a, mut b) = (0, 0);
        lines::each(&storage, "f", &mut |_| {
            a += 1;
            a < 2
        })
        .unwrap();
        new::lines::each(&storage, "f", &mut |_| {
            b += 1;
            b < 2
        })
        .unwrap();
        assert_eq!(a, b);
    }
    assert_eq!(
        new::lines::each(&MemoryStorage::new(), "none", &mut |_| true),
        Err(new::Error::NotFound)
    );
}

#[test]
fn names_and_excerpts_are_the_same() {
    for name in [
        "a.md",
        "A.MD",
        "b c.markdown",
        ".md",
        "a.txt",
        "md",
        "a.mdx",
        "",
    ] {
        assert_eq!(note::is_note(name), new::note::is_note(name), "{name}");
        assert_eq!(note::is_hidden(name), new::note::is_hidden(name));
    }
    for path in [
        "v/Projects/Plan.md",
        "Plan.v2.md",
        "noending",
        "a/b/",
        "/",
        "",
    ] {
        assert_eq!(note::title(path), new::note::title(path), "{path}");
        for root in ["v", "v/", "", "/", "vault", "a"] {
            assert_eq!(
                note::folder_in(path, root),
                new::note::folder_in(path, root),
                "{path} {root}"
            );
        }
    }
    let fixed = [
        "---\ntags: [a]\n---\n# Title\n\n> **Bold** and [[Other|a link]] with [x](https://y.org) `code`.\nMore of it.\n\nSecond paragraph.\n",
        "# Only a heading\n\n---\n",
        "```\ncode\n```\ntext",
        "---\nno end\nreally",
        "---\n---\n---\nbody",
        "---\r\nx\r\n---\r\nbody\r\n",
    ];
    let mut texts: Vec<String> = fixed.iter().map(ToString::to_string).collect();
    texts.push("word ".repeat(100));
    texts.push("[[".repeat(1000));
    texts.push("---\n".repeat(50));
    let mut rng = Rng(2);
    for pieces in [1, 3, 8, 20, 60] {
        for _ in 0..40 {
            texts.push(markdown(&mut rng, pieces));
        }
    }
    let mut shown = 0;
    for text in texts {
        let excerpt = new::note::excerpt(&text).unwrap();
        shown += usize::from(!excerpt.is_empty());
        assert_eq!(note::excerpt(&text), excerpt, "{text:?}");
    }
    assert!(shown > 50, "{shown} texts have an excerpt");
    // From a file: the first 2 KiB, bad bytes replaced; none where unreadable.
    let storage = MemoryStorage::new()
        .with("n.md", bytes(&mut rng, 3000))
        .with("m.md", "# t\n\nhello **world**\n");
    for path in ["n.md", "m.md", "missing.md"] {
        assert_eq!(
            note::excerpt_of_file(&storage, path),
            new::note::excerpt_of_file(&storage, path).unwrap(),
            "{path}"
        );
    }
}

#[test]
fn tags_are_the_same() {
    let fixed = [
        "---\ntitle: x\ntags: [a, \"b c\"]\n---\n# Heading\nText #inline and #nested/tag.\n",
        "---\ntags:\n  - one\n  - two\nother: x\n---\nbody #three",
        "# Heading\n#12 is a number, a#b is not, `#code`\n```\n#fenced\n```\n#Real\n",
        "---\ntags: x\n#t",
        "#A #a #b",
        "",
    ];
    let mut texts: Vec<String> = fixed.iter().map(ToString::to_string).collect();
    texts.push("#t ".repeat(10_000));
    texts.push((0..500).map(|n| format!("#t{n} ")).collect());
    texts.push("#".repeat(10_000));
    texts.push("---\n".repeat(1000));
    texts.push(format!("#{}", "x".repeat(200)));
    texts.push(format!(
        "---\ntags: [{}]\n---\n{}",
        "f, ".repeat(30),
        "#g ".repeat(30)
    ));
    let mut rng = Rng(3);
    for pieces in [1, 4, 12, 40, 120] {
        for _ in 0..60 {
            texts.push(markdown(&mut rng, pieces));
        }
    }
    for text in texts {
        assert_eq!(
            tags::of_text(&text),
            new::tags::of_text(&text).unwrap(),
            "{text:?}"
        );
    }
    let storage = MemoryStorage::new()
        .with("a.md", "---\ntags: [x]\n---\n#y")
        .with("big.md", format!("{}#late", "x ".repeat(6000)))
        .with("bad.md", bytes(&mut rng, 9000));
    for path in ["a.md", "big.md", "bad.md", "none.md"] {
        assert_eq!(
            tags::of_file(&storage, path),
            new::tags::of_file(&storage, path).unwrap(),
            "{path}"
        );
    }
}

fn old_row(title: &str, path: &str, folder: bool, modified: i64) -> page::Row {
    page::Row {
        title: title.into(),
        path: path.into(),
        folder,
        modified,
    }
}

#[test]
fn windows_are_the_same() {
    let mut rng = Rng(4);
    for size in [0usize, 1, 7, 99, 100, 101, 250, 340] {
        let rows: Vec<(String, String, bool, i64)> = (0..size)
            .map(|n| {
                let name = ["alpha", "Beta", "ÉCLAIR", "zeta", "Alpha"][rng.below(5)];
                let title = format!("{name}{}", rng.below(size.max(2) / 2 + 1));
                let folder = rng.below(8) == 0;
                (
                    title.clone(),
                    format!("v/{}/{title}", n % 3),
                    folder,
                    rng.below(50) as i64,
                )
            })
            .collect();
        for (old_order, new_order) in [
            (page::Order::Name, new::page::Order::Name),
            (page::Order::Recent, new::page::Order::Recent),
        ] {
            let mut old_after: Option<page::Key> = None;
            let mut new_after: Option<new::page::Row> = None;
            for _ in 0..5 {
                let mut feed_old = |emit: &mut dyn FnMut(page::Row)| {
                    for (t, p, f, m) in &rows {
                        emit(old_row(t, p, *f, *m));
                    }
                };
                let old = page::window(old_order, old_after.as_ref(), &mut feed_old);
                let mut feed_new = |emit: &mut new::page::Emit<'_>| {
                    for (t, p, f, m) in &rows {
                        emit(new::page::RowRef {
                            title: t,
                            path: p,
                            folder: *f,
                            modified: *m,
                        })?;
                    }
                    Ok(())
                };
                let now = new::page::window(new_order, new_after.as_ref(), &mut feed_new).unwrap();
                assert_eq!(old.more, now.more, "{size}");
                assert_eq!(old.rows.len(), now.rows.len(), "{size}");
                for (a, b) in old.rows.iter().zip(&now.rows) {
                    assert_eq!(
                        (&a.title, &a.path, a.folder, a.modified),
                        (&b.title, &b.path, b.folder, b.modified)
                    );
                }
                let Some(last) = old.rows.last() else { break };
                old_after = Some(old_order.key(last));
                new_after = now.rows.last().cloned();
                if !old.more {
                    break;
                }
            }
        }
    }
    // The oldest possible change is no overflow.
    let mut feed = |emit: &mut new::page::Emit<'_>| {
        emit(new::page::RowRef {
            title: "x",
            path: "x.md",
            folder: false,
            modified: i64::MIN,
        })
    };
    let window = new::page::window(new::page::Order::Recent, None, &mut feed).unwrap();
    assert_eq!(window.rows.len(), 1);
}

/// A vault of notes in folders, with tags in some of them.
fn vault(seed: u64, notes: usize) -> MemoryStorage {
    let mut rng = Rng(seed);
    let rng = &mut rng;
    let mut storage = MemoryStorage::new();
    for n in 0..notes {
        let dir = ["", "A/", "A/B/", "C d/", "A/B/C/D/", ".hidden/", "Z/"][rng.below(7)];
        let ending = ["md", "md", "markdown", "txt", "MD"][rng.below(5)];
        let pieces = 1 + rng.below(12);
        let body = markdown(rng, pieces);
        storage = storage.with(&format!("v/{dir}Note {n}.{ending}"), body);
    }
    storage
}

fn walk_old(storage: &MemoryStorage) -> index::State {
    let mut walk = index::Indexer::new("v/");
    for _ in 0..10_000 {
        match walk.step(storage) {
            index::Step::More => {}
            index::Step::Done(state) => return state,
            index::Step::Failed => panic!("walk failed"),
        }
    }
    panic!("walk does not end");
}

fn walk_new(vault: &MemoryStorage, data: &MemoryStorage) -> new::index::State {
    let mut walk = new::index::Indexer::new("v/").unwrap();
    for _ in 0..10_000 {
        match walk.step(vault, data).unwrap() {
            new::index::Step::More => {}
            new::index::Step::Done(state) => return state,
            new::index::Step::Failed => panic!("walk failed"),
        }
    }
    panic!("walk does not end");
}

#[test]
fn the_index_and_the_tags_are_the_same() {
    let mut rng = Rng(5);
    for notes in [0usize, 1, 9, 70, 150] {
        let seed = rng.next();
        let old_storage = vault(seed, notes);
        let vault_storage = vault(seed, notes);
        let data = MemoryStorage::new();
        let old_state = walk_old(&old_storage);
        let new_state = walk_new(&vault_storage, &data);
        assert_eq!(old_state.notes, new_state.notes);
        assert_eq!(old_state.stamp, new_state.stamp);
        assert_eq!(old_state.folder, new_state.folder);
        let old_index = old_storage.get(&index::index_path());
        assert_eq!(old_index, data.get(new::index::INDEX), "{notes} notes");
        // The tags.
        let mut tagger = index::Tagger::new();
        while !tagger.step(&old_storage).unwrap() {}
        let mut tagger = new::index::Tagger::new();
        while !tagger.step(&vault_storage, &data).unwrap().unwrap() {}
        assert_eq!(
            old_storage.get(&index::tags_path()),
            data.get(new::index::TAGS)
        );
        assert_eq!(tagger.done, {
            let mut t = index::Tagger::new();
            while !t.step(&old_storage).unwrap() {}
            t.done
        });
        // What is read back.
        let (old_counts, old_left) = index::tag_counts(&old_storage);
        let (now_counts, now_left) = new::index::tag_counts(&data).unwrap();
        // The vault is not empty of what is compared.
        assert!(notes < 70 || (old_state.notes > 10 && !old_counts.is_empty()));
        assert_eq!((old_counts, old_left), (now_counts, now_left));
        let mut old_notes: Vec<(i64, String)> = Vec::new();
        index::notes(&old_storage, &mut |m, p| {
            old_notes.push((m, p.to_string()));
            true
        });
        let mut now_notes: Vec<(i64, String)> = Vec::new();
        new::index::notes(&data, &mut |m, p| {
            now_notes.push((m, p.to_string()));
            true
        })
        .unwrap();
        assert_eq!(old_notes, now_notes);
        // The lists made of them.
        let sources = [
            (
                source::Source::Folder("v".into()),
                new::source::Source::Folder("v".into()),
            ),
            (
                source::Source::Folder("v/A".into()),
                new::source::Source::Folder("v/A".into()),
            ),
            (
                source::Source::Notes {
                    folder: String::new(),
                    query: String::new(),
                },
                new::source::Source::Notes {
                    folder: String::new(),
                    query: String::new(),
                },
            ),
            (
                source::Source::Notes {
                    folder: "v/A/".into(),
                    query: "note 1".into(),
                },
                new::source::Source::Notes {
                    folder: "v/A/".into(),
                    query: "note 1".into(),
                },
            ),
            (
                source::Source::Tag("tag".into()),
                new::source::Source::Tag("tag".into()),
            ),
            (
                source::Source::Tag("one".into()),
                new::source::Source::Tag("one".into()),
            ),
        ];
        for (old, now) in sources {
            let mut a: Vec<(String, String, bool, i64)> = Vec::new();
            old.feed(&old_storage, &mut |r| {
                a.push((r.title, r.path, r.folder, r.modified))
            });
            let mut b: Vec<(String, String, bool, i64)> = Vec::new();
            now.feed(&vault_storage, &data, &mut |r| {
                b.push((
                    r.title.to_string(),
                    r.path.to_string(),
                    r.folder,
                    r.modified,
                ));
                Ok(())
            })
            .unwrap();
            assert!(notes < 70 || !matches!(old, source::Source::Folder(_)) || !a.is_empty());
            assert_eq!(a, b, "{old:?} with {notes} notes");
        }
    }
}

#[test]
fn the_state_is_the_same() {
    let data = MemoryStorage::new();
    let state = new::index::State {
        folder: "v\nw".into(),
        built: -5,
        notes: 3,
        stamp: u64::MAX,
        tagged: true,
    };
    state.write(&data).unwrap();
    let old = MemoryStorage::new();
    index::State {
        folder: "v\nw".into(),
        built: -5,
        notes: 3,
        stamp: u64::MAX,
        tagged: true,
    }
    .write(&old);
    assert_eq!(
        old.get(&format!(
            "{}/state.txt",
            paperpaw_sdk::Env::plugin_dir("notes-vault")
        )),
        data.get(new::index::STATE)
    );
    let read = new::index::State::read(&data).unwrap().unwrap();
    assert_eq!(
        (read.built, read.notes, read.stamp, read.tagged),
        (-5, 3, u64::MAX, true)
    );
    assert_eq!(read.folder, "v w");
    // Not whole, not this plugin's, or too long: none.
    for bad in [
        &b""[..],
        b"notes-vault 2\nv\n0\n0\n0\n0\n",
        b"notes-vault 1\nv\nx\n0\n0\n0\n",
        b"notes-vault 1\nv\n0\n0\n",
    ] {
        let data = MemoryStorage::new().with("state.txt", bad.to_vec());
        assert_eq!(new::index::State::read(&data).unwrap(), None);
    }
    let long = MemoryStorage::new().with("state.txt", alloc::vec![b'x'; 3000]);
    assert_eq!(new::index::State::read(&long).unwrap(), None);
    assert!(
        new::index::kept(&data, "v w").unwrap().is_none(),
        "no index file"
    );
    let data = data.with("index.txt", b"".to_vec());
    assert!(new::index::kept(&data, "v w").unwrap().is_some());
    assert!(new::index::kept(&data, "other").unwrap().is_none());
}

/// The search finds the same notes, Greek final sigma included: `ΟΔΟΣ`,
/// `οδος` and `ΣΟΦΟΣ` against words in either case and either sigma.
#[test]
fn the_search_finds_greek_titles_as_interface_6_does() {
    let titles = ["ΟΔΟΣ", "οδος", "ΣΟΦΟΣ", "σοφος", "Σίσυφος", "Plain"];
    let index: String = titles
        .iter()
        .enumerate()
        .map(|(n, t)| format!("{n}\tv/{t}.md\n"))
        .collect();
    let old_storage = MemoryStorage::new().with(&index::index_path(), index.clone());
    let data = MemoryStorage::new().with("index.txt", index);
    let vault = MemoryStorage::new();
    let found = |query: &str| -> (Vec<String>, Vec<String>) {
        let mut a = Vec::new();
        source::Source::Notes {
            folder: String::new(),
            query: query.into(),
        }
        .feed(&old_storage, &mut |r| a.push(r.title));
        let mut b = Vec::new();
        new::source::Source::Notes {
            folder: String::new(),
            query: query.into(),
        }
        .feed(&vault, &data, &mut |r| {
            b.push(r.title.to_string());
            Ok(())
        })
        .unwrap();
        (a, b)
    };
    for query in [
        "οδος",
        "ΟΔΟΣ",
        "ΟΔ",
        "οδ",
        "σο",
        "ΣΟ",
        "σοφος",
        "ΣΟΦΟΣ",
        "σι",
        "plain",
        "PLAIN",
        "",
    ] {
        let (old, now) = found(query);
        assert_eq!(old, now, "{query}");
    }
    let (old, now) = found("οδος");
    assert_eq!(old, ["ΟΔΟΣ", "οδος"]);
    assert_eq!(now, old);
    // What interface 6 missed: a query with the plain sigma at its end finds
    // the title with the final one (and the other way round).
    let (old, now) = found("οδοσ");
    assert!(old.is_empty(), "{old:?}");
    assert_eq!(now, ["ΟΔΟΣ", "οδος"]);
    assert_eq!(found("ΣΟΦΟς").1.len(), 2);
    // The same for a plain sigma in the middle written as a capital at the end.
    let (old, now) = found("ΣΊΣ");
    assert!(old.is_empty(), "{old:?}");
    assert_eq!(now, ["Σίσυφος"]);
}

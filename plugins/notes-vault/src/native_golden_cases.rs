// The cases of the transcript (`native_golden_tests.rs`), written against the
// functions of a `shim` module that the including file provides. Included, not
// a module of its own, so that the same cases could be run against another
// implementation of the shim.

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

/// A row of a list: title, path, folder, modified.
type R = (String, String, bool, i64);

/// What walking a vault makes: the state, the files of the index and of the tags,
/// what is read back, and the lists the sources give.
struct Walk {
    /// Folder, notes, stamp.
    state: (String, u32, u64),
    tagged: bool,
    done: u32,
    index: Option<Vec<u8>>,
    tags: Option<Vec<u8>>,
    counts: (Vec<(String, u32)>, bool),
    notes: Vec<(i64, String)>,
    lists: Vec<Vec<R>>,
}

/// The FNV-1a hash of `bytes`, to say a long answer in a line.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A text as the transcript names it: whole where short, else its length and hash.
fn named(text: &str) -> String {
    if text.len() <= 64 {
        format!("{text:?}")
    } else {
        format!("<{} bytes {:016x}>", text.len(), fnv(text.as_bytes()))
    }
}

/// A list of rows as the transcript says it: how many, the first and last, and a hash of all.
fn rows_line(rows: &[R]) -> String {
    format!(
        "{} first {:?} last {:?} hash {:016x}",
        rows.len(),
        rows.first(),
        rows.last(),
        fnv(format!("{rows:?}").as_bytes())
    )
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

fn lines_cases(out: &mut String) {
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
    for (n, data) in cases.iter().enumerate() {
        let lines = shim::lines_of(data);
        let _ = writeln!(
            out,
            "lines {n} bytes {} -> {} lines hash {:016x} first {:?}; stopping after two: {}",
            data.len(),
            lines.len(),
            fnv(format!("{lines:?}").as_bytes()),
            lines.first().map(|l| named(l)),
            shim::lines_stopped(data)
        );
    }
}

fn note_cases(out: &mut String) {
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
        let _ = writeln!(
            out,
            "name {name:?} note {} hidden {}",
            shim::is_note(name),
            shim::is_hidden(name)
        );
    }
    for path in [
        "v/Projects/Plan.md",
        "Plan.v2.md",
        "noending",
        "a/b/",
        "/",
        "",
    ] {
        let _ = writeln!(out, "title {path:?} {:?}", shim::title(path));
        for root in ["v", "v/", "", "/", "vault", "a"] {
            let _ = writeln!(
                out,
                "folder {path:?} in {root:?} {:?}",
                shim::folder_in(path, root)
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
    for (n, text) in texts.iter().enumerate() {
        let _ = writeln!(
            out,
            "excerpt {n} {} -> {:?}",
            named(text),
            shim::excerpt(text)
        );
    }
    // From a file: the first 2 KiB, bad bytes replaced; none where unreadable.
    let storage = MemoryStorage::new()
        .with("n.md", bytes(&mut rng, 3000))
        .with("m.md", "# t\n\nhello **world**\n");
    for path in ["n.md", "m.md", "missing.md"] {
        let _ = writeln!(
            out,
            "excerpt of file {path:?} -> {:?}",
            shim::excerpt_of_file(&storage, path)
        );
    }
}

fn tag_cases(out: &mut String) {
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
    for (n, text) in texts.iter().enumerate() {
        let _ = writeln!(
            out,
            "tags {n} {} -> {:?}",
            named(text),
            shim::tags_of_text(text)
        );
    }
    let storage = MemoryStorage::new()
        .with("a.md", "---\ntags: [x]\n---\n#y")
        .with("big.md", format!("{}#late", "x ".repeat(6000)))
        .with("bad.md", bytes(&mut rng, 9000));
    for path in ["a.md", "big.md", "bad.md", "none.md"] {
        let _ = writeln!(
            out,
            "tags of file {path:?} -> {:?}",
            shim::tags_of_file(&storage, path)
        );
    }
}

fn window_cases(out: &mut String) {
    let mut rng = Rng(4);
    for size in [0usize, 1, 7, 99, 100, 101, 250, 340] {
        let rows: Vec<R> = (0..size)
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
        for recent in [false, true] {
            for (step, (more, window)) in shim::windows(recent, &rows).iter().enumerate() {
                let _ = writeln!(
                    out,
                    "window size {size} recent {recent} step {step} more {more}: {}",
                    rows_line(window)
                );
            }
        }
    }
}

fn index_cases(out: &mut String) {
    let mut rng = Rng(5);
    for notes in [0usize, 1, 9, 70, 150] {
        let seed = rng.next();
        let walk = shim::walk(&vault(seed, notes));
        let _ = writeln!(
            out,
            "walk {notes} notes: folder {:?} notes {} stamp {} tagged {} done {}",
            walk.state.0, walk.state.1, walk.state.2, walk.tagged, walk.done
        );
        let _ = writeln!(
            out,
            "walk {notes}: index {:?} tags {:?}",
            walk.index.as_deref().map(fnv),
            walk.tags.as_deref().map(fnv)
        );
        let _ = writeln!(
            out,
            "walk {notes}: counts {} left {} hash {:016x}; notes {} hash {:016x}",
            walk.counts.0.len(),
            walk.counts.1,
            fnv(format!("{:?}", walk.counts).as_bytes()),
            walk.notes.len(),
            fnv(format!("{:?}", walk.notes).as_bytes())
        );
        for (n, list) in walk.lists.iter().enumerate() {
            let _ = writeln!(out, "walk {notes}: list {n}: {}", rows_line(list));
        }
    }
}

fn state_cases(out: &mut String) {
    let written = shim::state_file("v\nw", -5, 3, u64::MAX, true);
    let _ = writeln!(out, "state file {:?}", String::from_utf8_lossy(&written));
}

/// The search finds titles whatever the case and the sigma (`ΟΔΟΣ`, `οδος`).
/// The two queries interface 6 missed are in the test itself.
fn search_cases(out: &mut String) {
    let titles = ["ΟΔΟΣ", "οδος", "ΣΟΦΟΣ", "σοφος", "Σίσυφος", "Plain"];
    let index: String = titles
        .iter()
        .enumerate()
        .map(|(n, t)| format!("{n}\tv/{t}.md\n"))
        .collect();
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
        let _ = writeln!(out, "search {query:?} {:?}", shim::search(&index, query));
    }
}

/// The whole transcript.
fn transcript() -> String {
    let mut out = String::new();
    lines_cases(&mut out);
    note_cases(&mut out);
    tag_cases(&mut out);
    window_cases(&mut out);
    index_cases(&mut out);
    state_cases(&mut out);
    search_cases(&mut out);
    out
}

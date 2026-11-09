//! Markdown (`.md`, `.markdown`): turned into HTML line by line once, then
//! read like an HTML file (see [`crate::html`]). Headings, paragraphs,
//! lists, block quotes, code blocks, rules, emphasis, inline code, links
//! and pictures beside the file (`![alt](picture.png)`); the first heading
//! is the title.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::{Block, Document, DocumentError, Link, Locator, Metadata, TocEntry};

use crate::PLUGIN;
use crate::fallible::{self, TryExtend, TryPush, TryPushStr};
use crate::html::HtmlDocument;

/// Changes whenever Markdown is turned into HTML differently.
const MD_VERSION: u32 = 2;

/// An open Markdown file.
#[derive(Debug)]
pub struct MarkdownDocument {
    html: HtmlDocument,
    metadata: Metadata,
}

impl MarkdownDocument {
    pub fn open(storage: &dyn Storage, path: &str, styles: bool) -> Result<Self, DocumentError> {
        let dir = fallible::kept_path(&fallible::plugin_dir(PLUGIN.as_str())?, "md-", path)?;
        let html_path = fallible::try_format!("{dir}/book.html")?;
        let stamp =
            fallible::try_format!("{MD_VERSION} {}\n", fallible::fingerprint(storage, path)?)?;
        let stamp_path = fallible::try_format!("{dir}/source")?;
        if !fallible::file_is(storage, &stamp_path, stamp.as_bytes())
            || storage.size(&html_path).is_err()
        {
            let _ = storage.create_dir(&dir);
            convert(storage, path, &html_path)?;
            storage.write(&stamp_path, stamp.as_bytes())?;
        }
        // Converted in the Markdown file's cache, so it goes with it.
        let html = HtmlDocument::open_with_pictures(
            storage,
            &html_path,
            styles,
            fallible::try_format!("{dir}/html")?,
        )?;
        let mut metadata = fallible::metadata(html.metadata())?;
        if metadata.title.as_deref().is_none_or(|t| t == "book") {
            let name = path.rsplit('/').next().unwrap_or(path);
            metadata.title = Some(fallible::string(
                name.rsplit_once('.').map_or(name, |(s, _)| s),
            )?);
        }
        Ok(Self { html, metadata })
    }
}

/// A line's inline markup as HTML: `**bold**`, `*italic*`, `_italic_`,
/// `` `code` ``, `[text](url)`, `![alt](src)` (a picture on the storage:
/// `src` beside the file, or from its root with `/`; elsewhere, as on the
/// web, its alternative text). `pictures` leads from the HTML to the
/// Markdown file's folder (see [`pictures_path`]). Each character is
/// looked at a few times at most, however the line is made.
fn inline(text: &str, pictures: &str, out: &mut String) -> Result<(), DocumentError> {
    inline_in(text, pictures, false, out)
}

/// As [`inline`]; in a link's label (`label`) no other link is read, so a
/// label is read once more at most.
fn inline_in(
    text: &str,
    pictures: &str,
    label: bool,
    out: &mut String,
) -> Result<(), DocumentError> {
    let (mut bold, mut italic) = (false, false);
    // Where the next `](` and the `)` after one are, from where each was
    // looked for: brackets before them all find them there.
    let mut closes = Next::default();
    let mut parens = Next::default();
    let mut i = 0;
    while let Some(c) = text[i..].chars().next() {
        let next = text[i + c.len_utf8()..].chars().next();
        let before = text[..i].chars().next_back();
        match c {
            '\\' if next.is_some_and(|n| n.is_ascii_punctuation()) => {
                fallible::escape(&text[i + 1..i + 2], out)?;
                i += 2;
            }
            '`' => match text[i + 1..].find('`') {
                Some(end) => {
                    out.try_push_str("<code>")?;
                    fallible::escape(&text[i + 1..i + 1 + end], out)?;
                    out.try_push_str("</code>")?;
                    i += end + 2;
                }
                None => {
                    out.try_push('`')?;
                    i += 1;
                }
            },
            '*' | '_' if next == Some(c) => {
                out.try_push_str(if bold { "</b>" } else { "<b>" })?;
                bold = !bold;
                i += 2;
            }
            '*' | '_'
                if italic
                    || next.is_some_and(|n| !n.is_whitespace())
                        && (c == '*' || before.is_none_or(|b| !b.is_alphanumeric())) =>
            {
                out.try_push_str(if italic { "</i>" } else { "<i>" })?;
                italic = !italic;
                i += 1;
            }
            '!' | '[' if !(label && c == '[') => {
                let image = c == '!';
                let open = if image { i + 1 } else { i };
                let link = text[open..]
                    .starts_with('[')
                    .then(|| {
                        let close = closes.find(text, "](", open + 1)?;
                        let end = parens.find(text, ")", close + 2)?;
                        Some((&text[open + 1..close], &text[close + 2..end], end + 1))
                    })
                    .flatten();
                match link {
                    Some((alt, src, end)) if image => {
                        push_image(alt, src, pictures, out)?;
                        i = end;
                    }
                    Some((words, url, end)) => {
                        let url = url.split_whitespace().next().unwrap_or_default();
                        out.try_push_str("<a href=\"")?;
                        fallible::escape(url, out)?;
                        out.try_push_str("\">")?;
                        inline_in(words, pictures, true, out)?;
                        out.try_push_str("</a>")?;
                        i = end;
                    }
                    None => {
                        fallible::escape(&text[i..i + 1], out)?;
                        i += 1;
                    }
                }
            }
            c => {
                fallible::escape(&text[i..i + c.len_utf8()], out)?;
                i += c.len_utf8();
            }
        }
    }
    if italic {
        out.try_push_str("</i>")?;
    }
    if bold {
        out.try_push_str("</b>")?;
    }
    Ok(())
}

/// A picture of the Markdown file's folder as `<img>`; elsewhere, as on
/// the web, its alternative text.
fn push_image(alt: &str, src: &str, pictures: &str, out: &mut String) -> Result<(), DocumentError> {
    let src = src.split_whitespace().next().unwrap_or_default();
    if src.contains("://") || src.is_empty() {
        return fallible::escape(alt, out);
    }
    out.try_push_str("<img src=\"")?;
    match src.strip_prefix('/') {
        // From the root: up to it, then down.
        Some(root) => {
            for _ in 0..pictures.matches("../").count() {
                fallible::escape("../", out)?;
            }
            fallible::escape(root, out)?;
        }
        None => {
            fallible::escape(pictures, out)?;
            fallible::escape(src, out)?;
        }
    }
    out.try_push_str("\" alt=\"")?;
    fallible::escape(alt, out)?;
    out.try_push_str("\"/>")
}

/// Where `what` is next in a text, at or after where it was asked for:
/// found once for all asks before it.
#[derive(Default)]
struct Next {
    /// Where it was looked for from, and where it was found.
    from: usize,
    found: Option<Option<usize>>,
}

impl Next {
    fn find(&mut self, text: &str, what: &str, from: usize) -> Option<usize> {
        match self.found {
            Some(Some(at)) if at >= from && self.from <= from => return Some(at),
            Some(None) if self.from <= from => return None,
            _ => {}
        }
        let found = text
            .get(from..)
            .and_then(|rest| rest.find(what))
            .map(|at| from + at);
        self.from = from;
        self.found = Some(found);
        found
    }
}

/// What is open while lines are read.
#[derive(Debug, Default)]
struct State {
    paragraph: bool,
    /// The kind of list open (`ul` or `ol`) and whether an item is.
    list: Option<&'static str>,
    item: bool,
    quote: bool,
    code: bool,
    /// The line read last ended in two spaces: a break after it.
    br: bool,
    /// The line read last was cut short (see [`MAX_LINE`]): this one is
    /// the rest of it.
    cut: bool,
    /// From the HTML's folder to the Markdown file's (see [`inline`]).
    pictures: String,
}

impl State {
    fn close_paragraph(&mut self, out: &mut String) -> Result<(), DocumentError> {
        if self.paragraph {
            out.try_push_str("</p>\n")?;
            self.paragraph = false;
        }
        if self.item {
            out.try_push_str("</li>\n")?;
            self.item = false;
        }
        Ok(())
    }

    fn close_blocks(&mut self, out: &mut String) -> Result<(), DocumentError> {
        self.close_paragraph(out)?;
        if let Some(list) = self.list.take() {
            out.try_push_str(&fallible::try_format!("</{list}>\n")?)?;
        }
        if self.quote {
            out.try_push_str("</blockquote>\n")?;
            self.quote = false;
        }
        Ok(())
    }
}

/// Turns one line into HTML; `cut` if it goes on in the next (a line
/// longer than [`MAX_LINE`]).
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn line(raw: &str, state: &mut State, out: &mut String, cut: bool) -> Result<(), DocumentError> {
    let br = core::mem::replace(&mut state.br, !cut && raw.ends_with("  "));
    if core::mem::replace(&mut state.cut, cut) {
        // The rest of a line cut short: more of the block it began, as
        // written (its white space was not trimmed at the cut).
        if state.code {
            fallible::escape(raw, out)?;
            if !cut {
                out.try_push('\n')?;
            }
            return Ok(());
        }
        if !state.paragraph && !state.item {
            out.try_push_str("<p>")?;
            state.paragraph = true;
        }
        inline(raw, &state.pictures, out)?;
        return Ok(());
    }
    let line = if cut { raw } else { raw.trim_end() };
    let trimmed = line.trim_start();
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        if state.code {
            out.try_push_str("</pre>\n")?;
            state.code = false;
        } else {
            state.close_blocks(out)?;
            out.try_push_str("<pre>")?;
            state.code = true;
        }
        return Ok(());
    }
    if state.code {
        fallible::escape(line, out)?;
        if !cut {
            out.try_push('\n')?;
        }
        return Ok(());
    }
    if trimmed.is_empty() {
        state.close_paragraph(out)?;
        return Ok(());
    }
    // A block quote: the rest of the line read on.
    let (quoted, text) = match trimmed.strip_prefix('>') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, trimmed),
    };
    if quoted != state.quote {
        state.close_blocks(out)?;
        if quoted {
            out.try_push_str("<blockquote>\n")?;
            state.quote = true;
        }
    }
    let heading = text.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&heading) && text[heading..].starts_with(' ') {
        state.close_blocks(out)?;
        let title = text[heading..].trim().trim_end_matches('#').trim();
        out.try_push_str(&fallible::try_format!("<h{heading}>")?)?;
        inline(title, &state.pictures, out)?;
        out.try_push_str(&fallible::try_format!("</h{heading}>\n")?)?;
        return Ok(());
    }
    // Three or more of one of `-`, `*`, `_`, white space between them aside.
    let mut marks = text.chars().filter(|c| !c.is_whitespace());
    let first = marks.next();
    if first.is_some_and(|f| matches!(f, '-' | '*' | '_'))
        && text.chars().filter(|c| !c.is_whitespace()).count() >= 3
        && marks.all(|c| Some(c) == first)
    {
        state.close_blocks(out)?;
        out.try_push_str("<hr/>\n")?;
        return Ok(());
    }
    let bullet = ["- ", "* ", "+ "].iter().find_map(|b| text.strip_prefix(b));
    let number = text
        .split_once(". ")
        .filter(|(n, _)| !n.is_empty() && n.len() <= 9 && n.chars().all(|c| c.is_ascii_digit()))
        .map(|(_, rest)| rest);
    if let Some((kind, rest)) = bullet.map(|r| ("ul", r)).or(number.map(|r| ("ol", r))) {
        state.close_paragraph(out)?;
        if state.list != Some(kind) {
            if let Some(list) = state.list.take() {
                out.try_push_str(&fallible::try_format!("</{list}>\n")?)?;
            }
            out.try_push_str(&fallible::try_format!("<{kind}>\n")?)?;
            state.list = Some(kind);
        }
        out.try_push_str("<li>")?;
        inline(rest.trim(), &state.pictures, out)?;
        state.item = true;
        return Ok(());
    }
    if state.item {
        // A list item going on.
        out.try_push(' ')?;
        inline(text, &state.pictures, out)?;
        return Ok(());
    }
    if state.list.is_some() && !state.item {
        state.close_blocks(out)?;
        if quoted {
            out.try_push_str("<blockquote>\n")?;
            state.quote = true;
        }
    }
    if state.paragraph {
        // Two spaces at the end of the line before break the line.
        out.try_push_str(if br { "<br/>" } else { " " })?;
    } else {
        out.try_push_str("<p>")?;
        state.paragraph = true;
    }
    inline(text, &state.pictures, out)
}

/// Longer lines are read in pieces of at most this length, cut after a
/// space, or else where a character ends: a file without line breaks is
/// read a piece at a time too.
const MAX_LINE: usize = 8 * 1024;

/// Writes the HTML for the Markdown file at `path` to `to`, a few lines
/// at a time.
fn convert(storage: &dyn Storage, path: &str, to: &str) -> Result<(), DocumentError> {
    storage.write(to, b"<html><body>\n")?;
    let mut state = State {
        pictures: pictures_path(path, to)?,
        ..State::default()
    };
    let mut out = String::new();
    let mut pending: Vec<u8> = Vec::new();
    // How much of `pending` is known to hold no line break.
    let mut searched = 0;
    let mut chunk = [0u8; 1024];
    let mut at = 0u64;
    let flush = |out: &mut String| -> Result<(), DocumentError> {
        storage.append(to, out.as_bytes())?;
        out.clear();
        Ok(())
    };
    loop {
        let n = storage.read_at(path, at, &mut chunk)?;
        at += n as u64;
        pending.try_extend_from_slice(&chunk[..n])?;
        loop {
            let (end, next, cut) = match pending[searched..].iter().position(|&b| b == b'\n') {
                Some(end) => (searched + end, searched + end + 1, false),
                None if pending.len() > MAX_LINE => {
                    // After the last space of the second half, so words
                    // and most markup stay whole.
                    let cut = match pending[MAX_LINE / 2..MAX_LINE]
                        .iter()
                        .rposition(|&b| b == b' ' || b == b'\t')
                    {
                        Some(space) => MAX_LINE / 2 + space + 1,
                        None => {
                            let mut cut = MAX_LINE;
                            while cut > 1 && pending[cut] & 0xc0 == 0x80 {
                                cut -= 1;
                            }
                            cut
                        }
                    };
                    (cut, cut, true)
                }
                None => {
                    searched = pending.len();
                    break;
                }
            };
            let text = fallible::lossy(&pending[..end])?;
            let text = if cut {
                &text
            } else {
                text.trim_end_matches('\r')
            };
            line(text, &mut state, &mut out, cut)?;
            pending.drain(..next);
            searched = 0;
            if out.len() > 2048 {
                flush(&mut out)?;
            }
        }
        if n == 0 {
            if !pending.is_empty() {
                let text = fallible::lossy(&pending)?;
                line(&text, &mut state, &mut out, false)?;
            }
            break;
        }
    }
    if state.code {
        out.try_push_str("</pre>\n")?;
    }
    state.close_blocks(&mut out)?;
    out.try_push_str("</body></html>\n")?;
    flush(&mut out)
}

/// The way from the folder of the HTML `to` to that of the Markdown file
/// `from`, both on the storage: up to the root, then down
/// (`../../notes/`).
fn pictures_path(from: &str, to: &str) -> Result<String, DocumentError> {
    let up = to
        .rsplit_once('/')
        .map_or(0, |(dir, _)| dir.split('/').count());
    let down = from.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut path = String::new();
    for _ in 0..up {
        path.try_push_str("../")?;
    }
    if !down.is_empty() {
        path.try_push_str(down)?;
        path.try_push('/')?;
    }
    Ok(path)
}

impl Document for MarkdownDocument {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn sections(&self) -> u32 {
        self.html.sections()
    }

    fn section_len(&self, section: u32) -> u32 {
        self.html.section_len(section)
    }

    fn block_at(&self, storage: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        self.html.block_at(storage, at)
    }

    fn toc_len(&self, storage: &dyn Storage) -> u32 {
        self.html.toc_len(storage)
    }

    fn toc_entry(&self, storage: &dyn Storage, index: u32) -> Option<TocEntry> {
        self.html.toc_entry(storage, index)
    }

    fn links(&self, storage: &dyn Storage, section: u32, from: u32, to: u32) -> Vec<Link> {
        self.html.links(storage, section, from, to)
    }

    fn link_target(&self, storage: &dyn Storage, href: &str) -> Option<Locator> {
        self.html.link_target(storage, href)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;
    use paperpaw_sdk::document::BlockKind;

    fn html(md: &str) -> String {
        let mut state = State::default();
        let mut out = String::new();
        for l in md.lines() {
            line(l, &mut state, &mut out, false).unwrap();
        }
        state.close_blocks(&mut out).unwrap();
        out
    }

    #[test]
    fn blocks_and_inline_markup() {
        assert_eq!(html("# Title #"), "<h1>Title</h1>\n");
        assert_eq!(
            html("Some **bold** and *it* or _it_,\nwith `a<b` and [a link](https://x.org \"t\")."),
            "<p>Some <b>bold</b> and <i>it</i> or <i>it</i>, with <code>a&lt;b</code> and \
             <a href=\"https://x.org\">a link</a>.</p>\n"
        );
        assert_eq!(
            html("- one\n- two\n\n1. first\n2. second"),
            "<ul>\n<li>one</li>\n<li>two</li>\n</ul>\n<ol>\n<li>first</li>\n<li>second</li>\n</ol>\n"
        );
        assert_eq!(
            html("> quoted\n> on\n\nafter"),
            "<blockquote>\n<p>quoted on</p>\n</blockquote>\n<p>after</p>\n"
        );
        assert_eq!(html("```\nlet a = 1;\n```"), "<pre>let a = 1;\n</pre>\n");
        assert_eq!(html("---"), "<hr/>\n");
        assert_eq!(html("snake_case_name"), "<p>snake_case_name</p>\n");
    }

    #[test]
    fn reads_as_a_document() {
        let md = "# Notes\n\nA *first* paragraph.\n\n## Part\n\n- item\n";
        let storage = MemoryStorage::new().with("n/notes.md", md.as_bytes().to_vec());
        let doc = MarkdownDocument::open(&storage, "n/notes.md", true).unwrap();
        let first = doc.block_at(&storage, Locator::START).unwrap().unwrap();
        assert_eq!(
            (first.kind, first.text.as_str()),
            (BlockKind::Heading(1), "Notes")
        );
        let mut texts = Vec::new();
        let mut at = Locator::START;
        while let Some(block) = doc.block_at(&storage, at).unwrap() {
            texts.push(block.text.clone());
            at = Locator::new(0, block.end);
        }
        assert_eq!(
            texts,
            ["Notes", "A first paragraph.", "Part", "•\u{a0}item"]
        );
    }

    #[test]
    fn pictures_beside_the_file_are_shown() {
        let way = pictures_path("notes/day.md", ".paperpaw/plugins/x/md-1/book.html").unwrap();
        assert_eq!(way, "../../../../notes/");
        let mut state = State {
            pictures: way,
            ..State::default()
        };
        let mut out = String::new();
        line(
            "![A cat](cat.png) ![Root](/pics/b.png)",
            &mut state,
            &mut out,
            false,
        )
        .unwrap();
        assert_eq!(
            out,
            "<p><img src=\"../../../../notes/cat.png\" alt=\"A cat\"/> \
             <img src=\"../../../../pics/b.png\" alt=\"Root\"/>"
        );
        // From the web: its alternative text, as before.
        assert_eq!(html("![Far](https://x.org/a.png)"), "<p>Far</p>\n");
        // The way resolves to the picture.
        let (path, _) =
            crate::package::resolve(".paperpaw/plugins/x/md-1/", "../../../../notes/cat.png");
        assert_eq!(path, "notes/cat.png");
    }
}

//! Markdown as notes vaults write it (`.md`, `.markdown`): turned into HTML line by line once, then
//! read like an HTML file (see [`HtmlDocument`], which the reader's formats
//! share). It replaces the built-in Markdown format while this plugin is on. Headings, paragraphs,
//! lists, block quotes, code blocks, rules, emphasis, inline code, links
//! and pictures beside the file (`![alt](picture.png)`); the first heading
//! is the title.
//!
//! What notes vaults write is read too: `[[wikilinks]]` and embeds (see
//! [`wiki`]), front matter, which is hidden but for its tags (see
//! [`front`]), callouts (`> [!note] Title`), `#tags` as they are, tables
//! (see [`table`]) and the anchors of headings, for links to them.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::{Block, Document, DocumentError, Link, Locator, Metadata, TocEntry};

use paperpaw_epub_family::HtmlDocument;

use crate::PLUGIN;
use crate::fallible::{self, TryExtend, TryPush, TryPushStr};

mod front;
mod table;
mod wiki;

use front::{Front, Step};
use wiki::Vault;

/// Changes whenever Markdown is turned into HTML differently.
const MD_VERSION: u32 = 3;

/// An open Markdown file.
#[derive(Debug)]
pub struct MarkdownDocument {
    html: HtmlDocument,
    metadata: Metadata,
}

impl MarkdownDocument {
    /// Opens the note at `path` of the card, which is in a vault.
    ///
    /// # Errors
    /// What reading and converting the note says; a refused allocation is
    /// [`DocumentError::OutOfMemory`].
    pub fn open(storage: &dyn Storage, path: &str, styles: bool) -> Result<Self, DocumentError> {
        Self::open_in(storage, path, styles, true)
    }

    /// As [`open`](Self::open); `in_vault` is false for a note that is not on
    /// the card (its pictures and links are not looked for there).
    ///
    /// # Errors
    /// As [`open`](Self::open).
    pub fn open_in(
        storage: &dyn Storage,
        path: &str,
        styles: bool,
        in_vault: bool,
    ) -> Result<Self, DocumentError> {
        let dir = fallible::kept_path(&fallible::plugin_dir(PLUGIN.as_str())?, "md-", path)?;
        let html_path = fallible::try_format!("{dir}/book.html")?;
        let stamp =
            fallible::try_format!("{MD_VERSION} {}\n", fallible::fingerprint(storage, path)?)?;
        let stamp_path = fallible::try_format!("{dir}/source")?;
        if !fallible::file_is(storage, &stamp_path, stamp.as_bytes())
            || storage.size(&html_path).is_err()
        {
            let _ = storage.create_dir(&dir);
            // A stamp that still matches would vouch for a half-written
            // `book.html` if the conversion stops (a refused allocation).
            let _ = storage.remove(&stamp_path);
            convert(storage, path, &html_path, in_vault)?;
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
fn inline(
    text: &str,
    pictures: &str,
    vault: Option<&Vault<'_>>,
    out: &mut String,
) -> Result<(), DocumentError> {
    inline_in(text, pictures, vault, false, out)
}

/// As [`inline`]; in a link's label (`label`) no other link is read, so a
/// label is read once more at most.
fn inline_in(
    text: &str,
    pictures: &str,
    vault: Option<&Vault<'_>>,
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
        // `[[note|label]]`, `![[picture.png]]` and `![[note]]`.
        if !label
            && matches!(c, '[' | '!')
            && let Some(used) = wikilink(&text[i..], pictures, vault, out)?
        {
            i += used;
            continue;
        }
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
                        picture(alt, src, pictures, out)?;
                        i = end;
                    }
                    Some((words, url, end)) => {
                        html_link(words, url, pictures, vault, out)?;
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

/// A picture `![alt](src)` as HTML: one on the storage, else (from the
/// web) its alternative text.
fn picture(alt: &str, src: &str, pictures: &str, out: &mut String) -> Result<(), DocumentError> {
    let src = src.split_whitespace().next().unwrap_or_default();
    if src.contains("://") || src.is_empty() {
        return fallible::escape(alt, out);
    }
    out.try_push_str("<img src=\"")?;
    match src.strip_prefix('/') {
        // From the root: up to it, then down.
        Some(root) => {
            up_to_root(pictures, out)?;
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

/// As many `../` as `pictures` has: from the HTML's folder up to the root.
fn up_to_root(pictures: &str, out: &mut String) -> Result<(), DocumentError> {
    for _ in 0..pictures.matches("../").count() {
        fallible::escape("../", out)?;
    }
    Ok(())
}

/// A link `[words](url)` as HTML; a note's own file is led to from the
/// root, where the reader finds it.
fn html_link(
    words: &str,
    url: &str,
    pictures: &str,
    vault: Option<&Vault<'_>>,
    out: &mut String,
) -> Result<(), DocumentError> {
    let url = url.split_whitespace().next().unwrap_or_default();
    out.try_push_str("<a href=\"")?;
    if url.is_empty() || url.starts_with('#') || paperpaw_sdk::document::is_external(url) {
        fallible::escape(url, out)?;
    } else if let Some(root) = url.strip_prefix('/') {
        up_to_root(pictures, out)?;
        fallible::escape(root, out)?;
    } else {
        fallible::escape(pictures, out)?;
        fallible::escape(url, out)?;
    }
    out.try_push_str("\">")?;
    inline_in(words, pictures, vault, true, out)?;
    out.try_push_str("</a>")
}

/// The wikilink or embed at the start of `text` as HTML, and how many
/// bytes it took; `None` where there is none.
fn wikilink(
    text: &str,
    pictures: &str,
    vault: Option<&Vault<'_>>,
    out: &mut String,
) -> Result<Option<usize>, DocumentError> {
    let embed = text.starts_with("![[");
    let Some((link, used)) = wiki::parse(&text[usize::from(embed)..]) else {
        return Ok(None);
    };
    let used = used + usize::from(embed);
    if embed && wiki::is_picture(link.target) {
        // The words shown where the picture is not: its name, not a size
        // (`![[a.png|200]]`).
        let sized = link
            .label
            .is_some_and(|l| l.chars().all(|c| c.is_ascii_digit() || c == 'x'));
        let alt = link.label.filter(|_| !sized).unwrap_or(link.target);
        let found = match vault {
            Some(vault) => vault.find(link.target)?,
            None => None,
        };
        match found {
            Some(path) => {
                // From the HTML's folder up to the root, then down.
                out.try_push_str("<img src=\"")?;
                up_to_root(pictures, out)?;
                fallible::escape(&path, out)?;
                out.try_push_str("\" alt=\"")?;
                fallible::escape(alt, out)?;
                out.try_push_str("\"/>")?;
            }
            None => fallible::escape(alt, out)?,
        }
        return Ok(Some(used));
    }
    let mut shown = String::new();
    link.shown(&mut shown)?;
    out.try_push_str("<a href=\"")?;
    link.href(pictures, out)?;
    out.try_push_str("\">")?;
    inline_in(&shown, pictures, vault, true, out)?;
    out.try_push_str("</a>")?;
    Ok(Some(used))
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
struct State<'a> {
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
    /// The vault the file is in, where pictures it embeds are looked for.
    vault: Option<Vault<'a>>,
    /// The file's beginning: front matter, if it has some.
    front: Front,
    /// A line that may be the first of a table, held until the next one
    /// says (see [`table`]).
    held: Option<String>,
    /// A table is open.
    table: bool,
    /// The block quote open is a callout: left-aligned without the first
    /// line's indent, which suits a narrow block.
    callout: bool,
}

impl State<'_> {
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
        if core::mem::take(&mut self.table) {
            out.try_push_str("</table>\n")?;
        }
        if let Some(list) = self.list.take() {
            out.try_push_str(&fallible::try_format!("</{list}>\n")?)?;
        }
        if self.quote {
            out.try_push_str("</blockquote>\n")?;
            self.quote = false;
            self.callout = false;
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
fn line(
    raw: &str,
    state: &mut State<'_>,
    out: &mut String,
    cut: bool,
) -> Result<(), DocumentError> {
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
        inline(raw, &state.pictures, state.vault.as_ref(), out)?;
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
    // A line cut short goes on: what ends it is not the line's end.
    let text = if cut { text } else { without_block_id(text) };
    let opened = quoted && !state.quote;
    let title = if opened { callout(text)? } else { None };
    if quoted != state.quote {
        state.close_blocks(out)?;
        if quoted {
            state.callout = title.is_some();
            out.try_push_str(if state.callout {
                "<blockquote style=\"text-align:left\">\n"
            } else {
                "<blockquote>\n"
            })?;
            state.quote = true;
        }
    }
    // A callout (`> [!note] Title`) is a quote that begins with its title.
    if let Some(title) = title {
        out.try_push_str(FLUSH_P)?;
        out.try_push_str("<b>")?;
        inline(&title, &state.pictures, state.vault.as_ref(), out)?;
        out.try_push_str("</b></p>\n")?;
        return Ok(());
    }
    let heading = text.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&heading) && text[heading..].starts_with(' ') {
        state.close_blocks(out)?;
        let title = text[heading..].trim().trim_end_matches('#').trim();
        let id = wiki::slug(title)?;
        if id.is_empty() {
            out.try_push_str(&fallible::try_format!("<h{heading}>")?)?;
        } else {
            out.try_push_str(&fallible::try_format!("<h{heading} id=\"{id}\">")?)?;
        }
        inline(title, &state.pictures, state.vault.as_ref(), out)?;
        out.try_push_str(&fallible::try_format!("</h{heading}>\n")?)?;
        return Ok(());
    }
    // Three or more of one of `-`, `*`, `_`, white space between them aside.
    let mut marks = text.chars().filter(|c| !c.is_whitespace());
    let first = marks.next();
    if first.is_some_and(|f| matches!(f, '-' | '*' | '_'))
        && text
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(char::len_utf8)
            .sum::<usize>()
            >= 3
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
        inline(rest.trim(), &state.pictures, state.vault.as_ref(), out)?;
        state.item = true;
        return Ok(());
    }
    if state.item {
        // A list item going on.
        out.try_push(' ')?;
        inline(text, &state.pictures, state.vault.as_ref(), out)?;
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
        out.try_push_str(if state.callout { FLUSH_P } else { "<p>" })?;
        state.paragraph = true;
    }
    inline(text, &state.pictures, state.vault.as_ref(), out)
}

/// A paragraph of a callout: no indent of the first line.
const FLUSH_P: &str = "<p style=\"text-indent:0\">";

/// The title of the callout that `text`, the first line of a quote,
/// begins: what follows `[!kind]` (and a `+` or `-` that folds it), else
/// the kind, in capitals as a name. `None` where it is no callout.
fn callout(text: &str) -> Result<Option<String>, DocumentError> {
    let Some(rest) = text.strip_prefix("[!") else {
        return Ok(None);
    };
    let Some((kind, rest)) = rest.split_once(']') else {
        return Ok(None);
    };
    if kind.is_empty()
        || kind.len() > 24
        || !kind
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Ok(None);
    }
    let title = rest.trim_start_matches(['+', '-']).trim();
    let mut chars = kind.chars();
    let mut name = String::new();
    // The kind in capitals as a name, then the title it is given.
    for c in chars.next().into_iter().flat_map(char::to_uppercase) {
        name.try_push(c)?;
    }
    let rest: Vec<char> = {
        let mut kept = Vec::new();
        fallible::reserve_exact(&mut kept, 24)?;
        kept.extend(chars);
        kept
    };
    for (at, &c) in rest.iter().enumerate() {
        // A capital sigma ends a word as `ς`, as `str::to_lowercase` writes it.
        let last = c == 'Σ'
            && at > 0
            && rest[at - 1].is_alphabetic()
            && !rest.get(at + 1).is_some_and(|n| n.is_alphabetic());
        if last {
            name.try_push('ς')?;
        } else {
            for lower in c.to_lowercase() {
                name.try_push(lower)?;
            }
        }
    }
    if !title.is_empty() {
        name.try_push_str(" · ")?;
        name.try_push_str(title)?;
    }
    Ok(Some(name))
}

/// `text` without the block id some notes end a line with (` ^a1b2`),
/// which names the line for links to it and is no text.
fn without_block_id(text: &str) -> &str {
    match text.trim_end().rsplit_once('^') {
        Some((head, id))
            if (head.is_empty() || head.ends_with(' '))
                && (3..=40).contains(&id.len())
                && !id.chars().all(|c| c.is_ascii_digit())
                && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') =>
        {
            head.trim_end()
        }
        _ => text,
    }
}

/// Reads one line of the file: front matter and tables, which take lines
/// together, then [`line`].
fn feed(
    raw: &str,
    state: &mut State<'_>,
    out: &mut String,
    cut: bool,
) -> Result<(), DocumentError> {
    match state.front.feed(raw)? {
        Step::Past => rows(raw, state, out, cut),
        Step::Taken => Ok(()),
        Step::Closed(tags) => {
            // The tags the properties named, as the tags of a note read.
            if !tags.is_empty() {
                out.try_push_str("<p>")?;
                for (n, tag) in tags.iter().enumerate() {
                    out.try_push_str(if n == 0 { "#" } else { " #" })?;
                    out.try_push_str(tag)?;
                }
                out.try_push_str("</p>\n")?;
            }
            Ok(())
        }
        Step::Text(lines) => {
            for text in lines {
                rows(&text, state, out, false)?;
            }
            Ok(())
        }
    }
}

/// The file ended: what was held is text after all.
fn finish(state: &mut State<'_>, out: &mut String) -> Result<(), DocumentError> {
    if let Step::Text(lines) = state.front.end()? {
        for text in lines {
            rows(&text, state, out, false)?;
        }
    }
    if let Some(head) = state.held.take() {
        line(&head, state, out, false)?;
    }
    Ok(())
}

/// Reads a line with the rows of tables: a line with pipes is held until
/// the next says whether it is a table's first.
fn rows(
    raw: &str,
    state: &mut State<'_>,
    out: &mut String,
    cut: bool,
) -> Result<(), DocumentError> {
    let cell = |state: &State<'_>, text: &str, out: &mut String| {
        inline(text, &state.pictures, state.vault.as_ref(), out)
    };
    if state.table {
        if !cut && table::is_candidate(raw) {
            return table::row(raw, "td", &mut |t, o| cell(state, t, o), out);
        }
        state.table = false;
        out.try_push_str("</table>\n")?;
    }
    if let Some(head) = state.held.take() {
        if !cut && table::is_separator(raw) {
            state.close_blocks(out)?;
            out.try_push_str("<table>\n")?;
            state.table = true;
            return table::row(&head, "th", &mut |t, o| cell(state, t, o), out);
        }
        line(&head, state, out, false)?;
    }
    if !cut && !state.code && !state.cut && table::is_candidate(raw) {
        state.held = Some(fallible::string(raw)?);
        return Ok(());
    }
    line(raw, state, out, cut)
}

/// Longer lines are read in pieces of at most this length, cut after a
/// space, or else where a character ends: a file without line breaks is
/// read a piece at a time too.
const MAX_LINE: usize = 8 * 1024;

/// Writes the HTML for the Markdown file at `path` to `to`, a few lines
/// at a time.
fn convert(
    storage: &dyn Storage,
    path: &str,
    to: &str,
    in_vault: bool,
) -> Result<(), DocumentError> {
    storage.write(to, b"<html><body>\n")?;
    let mut state = State {
        pictures: pictures_path(path, to)?,
        vault: if in_vault {
            Some(Vault::new(storage, path)?)
        } else {
            None
        },
        ..State::default()
    };
    // The pictures it embeds, looked for in one walk of the vault.
    if let Some(vault) = &state.vault {
        vault.prefetch(path)?;
    }
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
            feed(text, &mut state, &mut out, cut)?;
            pending.drain(..next);
            searched = 0;
            if out.len() > 2048 {
                flush(&mut out)?;
            }
        }
        if n == 0 {
            if !pending.is_empty() {
                let text = fallible::lossy(&pending)?;
                feed(&text, &mut state, &mut out, false)?;
            }
            break;
        }
    }
    finish(&mut state, &mut out)?;
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
    use alloc::format;
    use paperpaw_sdk::MemoryStorage;
    use paperpaw_sdk::document::BlockKind;

    fn html(md: &str) -> String {
        let mut state = State::default();
        let mut out = String::new();
        for l in md.lines() {
            feed(l, &mut state, &mut out, false).unwrap();
        }
        finish(&mut state, &mut out).unwrap();
        state.close_blocks(&mut out).unwrap();
        out
    }

    /// As [`html`], in a vault with `files` (a note at `v/a.md`).
    fn html_in(md: &str, files: &[&str]) -> String {
        let mut storage = MemoryStorage::new().with("v/.obsidian/app.json", b"{}".to_vec());
        for f in files {
            storage = storage.with(f, b"x".to_vec());
        }
        let mut state = State {
            pictures: pictures_path("v/a.md", ".paperpaw/plugins/x/md-1/book.html").unwrap(),
            vault: Some(Vault::new(&storage, "v/a.md").unwrap()),
            ..State::default()
        };
        let mut out = String::new();
        for l in md.lines() {
            feed(l, &mut state, &mut out, false).unwrap();
        }
        finish(&mut state, &mut out).unwrap();
        state.close_blocks(&mut out).unwrap();
        out
    }

    #[test]
    fn wikilinks_lead_by_name_and_show_their_label() {
        assert_eq!(
            html("See [[Other Note]] and [[Other|that one]] or [[Other#Big Part]]."),
            "<p>See <a href=\"[[Other Note]]\">Other Note</a> and \
             <a href=\"[[Other]]\">that one</a> or \
             <a href=\"[[Other|big-part]]\">Other &gt; Big Part</a>.</p>\n"
        );
        // A heading of this note: an anchor.
        assert_eq!(
            html("[[#Big Part]]"),
            "<p><a href=\"#big-part\">Big Part</a></p>\n"
        );
        // Emphasis in a label; no link in a link; no link without an end.
        assert_eq!(
            html("[[N|a *b*]] [[broken"),
            "<p><a href=\"[[N]]\">a <i>b</i></a> [[broken</p>\n"
        );
        // In a link's label no other link is read.
        assert_eq!(
            html("[a [[b]]](x.md)"),
            "<p><a href=\"x.md\">a [[b]]</a></p>\n"
        );
    }

    #[test]
    fn embeds_show_pictures_and_link_notes() {
        let way = "../../../../";
        assert_eq!(
            html_in(
                "![[cat.png]] ![[Other]] ![[gone.png|200]]",
                &["v/Files/cat.png"]
            ),
            format!(
                "<p><img src=\"{way}v/Files/cat.png\" alt=\"cat.png\"/> \
                 <a href=\"{way}[[Other]]\">Other</a> gone.png</p>\n"
            )
        );
        // Without a vault (in a test): the name.
        assert_eq!(html("![[cat.png|A cat]]"), "<p>A cat</p>\n");
    }

    #[test]
    fn front_matter_is_hidden_but_for_its_tags() {
        assert_eq!(
            html("---\ntitle: x\ntags: [a, b]\n---\n# Note"),
            "<p>#a #b</p>\n<h1 id=\"note\">Note</h1>\n"
        );
        assert_eq!(html("---\ntitle: x\n---\ntext"), "<p>text</p>\n");
        // Never closed: text. A rule later in the file is a rule.
        assert_eq!(html("---\nno end"), "<hr/>\n<p>no end</p>\n");
        assert_eq!(
            html("text\n---\nmore\n---"),
            "<p>text</p>\n<hr/>\n<p>more</p>\n<hr/>\n"
        );
    }

    #[test]
    fn callouts_are_quotes_that_begin_with_their_title() {
        assert_eq!(
            html("> [!warning] Careful now\n> The body."),
            "<blockquote style=\"text-align:left\">\n<p style=\"text-indent:0\"><b>Warning · Careful now</b></p>\n\
             <p style=\"text-indent:0\">The body.</p>\n</blockquote>\n"
        );
        assert_eq!(
            html("> [!note]-\n> Folded."),
            "<blockquote style=\"text-align:left\">\n<p style=\"text-indent:0\"><b>Note</b></p>\n\
             <p style=\"text-indent:0\">Folded.</p>\n</blockquote>\n"
        );
        // Not a callout: a quote.
        assert_eq!(
            html("> [!] x\n> [!a b] y"),
            "<blockquote>\n<p>[!] x [!a b] y</p>\n</blockquote>\n"
        );
    }

    #[test]
    fn tags_and_block_ids() {
        assert_eq!(
            html("A #tag and #nested/tag"),
            "<p>A #tag and #nested/tag</p>\n"
        );
        assert_eq!(html("#tag at the start"), "<p>#tag at the start</p>\n");
        assert_eq!(
            html("A line ^ab12c\n\nx ^2 and ^x"),
            "<p>A line</p>\n<p>x ^2 and ^x</p>\n"
        );
    }

    #[test]
    fn tables_are_rows_of_cells() {
        assert_eq!(
            html("| a | *b* |\n|---|:-:|\n| 1 | [[n|x]] |\n| 2 | 3 |\n\nafter"),
            "<table>\n<tr><th>a</th><th><i>b</i></th></tr>\n\
             <tr><td>1</td><td><a href=\"[[n]]\">x</a></td></tr>\n\
             <tr><td>2</td><td>3</td></tr>\n</table>\n<p>after</p>\n"
        );
        // Pipes without the row of dashes are text, as before.
        assert_eq!(html("a | b\nc | d"), "<p>a | b c | d</p>\n");
        assert_eq!(html("a | b"), "<p>a | b</p>\n");
        assert_eq!(
            html("```\na | b\n---|---\n```"),
            "<pre>a | b\n---|---\n</pre>\n"
        );
    }

    #[test]
    fn headings_have_anchors() {
        assert_eq!(
            html("## Big -- Part 2"),
            "<h2 id=\"big-part-2\">Big -- Part 2</h2>\n"
        );
        assert_eq!(html("## !!!"), "<h2>!!!</h2>\n");
    }

    #[test]
    fn hostile_notes_are_read_without_a_panic() {
        let long = "[[".repeat(5000);
        let _ = html(&long);
        let _ = html(&format!("![[{}]]", "a".repeat(20_000)));
        let _ = html(&"> ".repeat(3000));
        let _ = html(&format!("{}x", "|".repeat(10_000)));
        let _ = html(&"| a |\n|---|\n".repeat(500));
        let _ = html(&format!("---\n{}\n---", "k: [".repeat(5000)));
        let _ = html("[[a]]]]][[[b|]]]\n![[\n!![[x]]\n[[#]]\n[[|]]\n> [!");
        let _ = html(&"- [[x]]\n".repeat(5000));
        assert_eq!(html("[[|x]]"), "<p>[[|x]]</p>\n");
    }

    #[test]
    fn blocks_and_inline_markup() {
        assert_eq!(html("# Title #"), "<h1 id=\"title\">Title</h1>\n");
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

    /// A storage whose appends can be made to fail.
    struct FailingAppend(MemoryStorage, core::cell::Cell<bool>);

    impl Storage for FailingAppend {
        fn read_at(
            &self,
            p: &str,
            at: u64,
            b: &mut [u8],
        ) -> Result<usize, paperpaw_sdk::StorageError> {
            self.0.read_at(p, at, b)
        }
        fn size(&self, p: &str) -> Result<u64, paperpaw_sdk::StorageError> {
            self.0.size(p)
        }
        fn write(&self, p: &str, d: &[u8]) -> Result<(), paperpaw_sdk::StorageError> {
            self.0.write(p, d)
        }
        fn append(&self, p: &str, d: &[u8]) -> Result<(), paperpaw_sdk::StorageError> {
            if self.1.get() {
                return Err(paperpaw_sdk::StorageError::Io);
            }
            self.0.append(p, d)
        }
        fn list(
            &self,
            dir: &str,
            visit: &mut dyn FnMut(paperpaw_sdk::DirEntry<'_>),
        ) -> Result<(), paperpaw_sdk::StorageError> {
            self.0.list(dir, visit)
        }
        fn remove(&self, p: &str) -> Result<(), paperpaw_sdk::StorageError> {
            self.0.remove(p)
        }
        fn exists(&self, p: &str) -> bool {
            self.0.exists(p)
        }
        fn rename(&self, a: &str, b: &str) -> Result<(), paperpaw_sdk::StorageError> {
            self.0.rename(a, b)
        }
        fn create_dir(&self, p: &str) -> Result<(), paperpaw_sdk::StorageError> {
            self.0.create_dir(p)
        }
    }

    #[test]
    fn a_stopped_conversion_leaves_no_stamp_to_vouch_for_its_html() {
        let long = "word ".repeat(1200);
        let storage = FailingAppend(
            MemoryStorage::new().with("v/a.md", long.into_bytes()),
            false.into(),
        );
        MarkdownDocument::open(&storage, "v/a.md", true).unwrap();
        let dir = fallible::kept_path(
            &fallible::plugin_dir(PLUGIN.as_str()).unwrap(),
            "md-",
            "v/a.md",
        )
        .unwrap();
        let (html, stamp) = (format!("{dir}/book.html"), format!("{dir}/source"));
        assert!(storage.exists(&stamp));
        // The HTML is gone, the stamp still matches the note: the next
        // conversion stops half way.
        storage.remove(&html).unwrap();
        storage.1.set(true);
        assert!(MarkdownDocument::open(&storage, "v/a.md", true).is_err());
        assert!(
            !storage.exists(&stamp),
            "a stamp stayed for a partial book.html"
        );
        // Opened again, it is converted whole.
        storage.1.set(false);
        MarkdownDocument::open(&storage, "v/a.md", true).unwrap();
        assert!(storage.exists(&stamp));
    }

    #[test]
    fn a_vault_note_reads_as_a_document_with_links_and_anchors() {
        let md = "---\ntags: [work]\n---\n# Plan\n\nSee [[Other note|the other]] and [[#Later]].\n\n\
                  > [!tip] Handy\n> Use it.\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n## Later\n\nEnd #done\n";
        let storage = MemoryStorage::new()
            .with("v/.obsidian/x", b"x".to_vec())
            .with("v/Plan.md", md.as_bytes().to_vec());
        let doc = MarkdownDocument::open(&storage, "v/Plan.md", true).unwrap();
        let mut texts = Vec::new();
        let mut at = Locator::START;
        while let Some(block) = doc.block_at(&storage, at).unwrap() {
            texts.push(block.text.clone());
            at = Locator::new(0, block.end);
        }
        assert_eq!(
            texts,
            [
                "#work",
                "Plan",
                "See the other and Later.",
                "Tip · Handy",
                "Use it.",
                "a · b",
                "1 · 2",
                "Later",
                "End #done"
            ]
        );
        let links = doc.links(&storage, 0, 0, u32::MAX);
        let hrefs: Vec<&str> = links.iter().map(|l| l.href.as_str()).collect();
        assert_eq!(hrefs, ["[[Other note]]", "#later"]);
        // The heading link leads to the heading; the note's is the reader's.
        assert!(doc.link_target(&storage, "#later").is_some());
        assert_eq!(doc.link_target(&storage, "[[Other note]]"), None);
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
    }
}

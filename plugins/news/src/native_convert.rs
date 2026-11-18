//! A feed (RSS 2.0, RSS 1.0 or Atom) made into a web page to read, as the
//! interface-6 plugin does (`convert.rs`): the feed's title, then each
//! article with its title, date and text as plain paragraphs (pictures, links
//! and layout are left out). The same rules, with fallible allocation: the
//! feed is read by `paperpaw_sdk::xml::Tokenizer::try_feed`, what the plugin
//! keeps is allocated with `try_reserve`, and a refused allocation ends the
//! conversion in [`Error::NoMemory`]; the page is never returned in part.
//!
//! The caller gives the feed in pieces as it reads it ([`Converter::feed`])
//! and writes the page as it comes ([`Converter::taken`], then
//! [`Converter::clear`]): only one article is in memory at a time, and what
//! one piece adds to the page is at most one article. After an error the
//! conversion is over: every later `feed` says the same, and the caller
//! throws away the page it wrote (a feed that fails keeps the old book).
use crate::native_news::{HEADLINES_SHOWN, MAX_FIELD, escape_len, is_block, one_line, read_date};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::xml::{Token, Tokenizer, local_name, try_decode};

/// How every page made here begins, in every version: a file at a book's name
/// that does not begin so was not made here.
pub const HEAD: &str = "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\"/>\n<title>";
/// How every page ends.
pub const TAIL: &str = "</body>\n</html>\n";

/// Writes a day (year, month 1 to 12, day) as the reader's language does.
pub type DateText<'a> = &'a dyn Fn(i32, u8, u8) -> Result<String, Error>;

/// What came of a feed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Converted {
    /// The feed's own title.
    pub title: String,
    pub articles: usize,
    /// The first articles' titles.
    pub headlines: Vec<String>,
}

fn reserve<T>(items: &mut Vec<T>, more: usize) -> Result<(), Error> {
    items.try_reserve(more).map_err(|_| Error::NoMemory)
}

/// Adds `text` cut to at most `room` bytes (at a character boundary) to
/// `target`.
fn push_cut(target: &mut String, text: &str, room: usize) -> Result<(), Error> {
    let mut end = room.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    target.try_reserve(end).map_err(|_| Error::NoMemory)?;
    target.push_str(&text[..end]);
    Ok(())
}

fn cut_copy(text: &str, max: usize) -> Result<String, Error> {
    let mut out = String::new();
    push_cut(&mut out, text, max)?;
    Ok(out)
}

/// Whether `name` is a block element, whatever the case it is written in.
fn is_block_named(name: &str) -> bool {
    let mut lower = [0u8; 10];
    let Some(slot) = lower.get_mut(..name.len()) else {
        return false;
    };
    slot.copy_from_slice(name.as_bytes());
    slot.make_ascii_lowercase();
    core::str::from_utf8(slot).is_ok_and(is_block)
}

fn is_skipped(name: &str) -> bool {
    name.eq_ignore_ascii_case("script") || name.eq_ignore_ascii_case("style")
}

struct Paragraphs {
    out: Vec<String>,
    current: String,
    total: usize,
    skipping: usize,
    max: usize,
    failed: Option<Error>,
}

impl Paragraphs {
    fn flush(&mut self) -> Result<(), Error> {
        let line = one_line(&self.current)?;
        if !line.is_empty() {
            reserve(&mut self.out, 1)?;
            self.out.push(line);
        }
        self.current.clear();
        Ok(())
    }

    fn add(&mut self, text: &str) -> Result<(), Error> {
        self.total += text.len();
        self.current
            .try_reserve(text.len())
            .map_err(|_| Error::NoMemory)?;
        self.current.push_str(text);
        Ok(())
    }

    fn token(&mut self, token: Token<'_>) -> Result<(), Error> {
        match token {
            Token::Start {
                name, self_closing, ..
            } => {
                let local = local_name(name);
                if is_skipped(local) && !self_closing {
                    self.skipping += 1;
                } else if is_block_named(local) {
                    self.flush()?;
                }
            }
            Token::End { name } => {
                let local = local_name(name);
                if is_skipped(local) {
                    self.skipping = self.skipping.saturating_sub(1);
                } else if is_block_named(local) {
                    self.flush()?;
                }
            }
            Token::Text(text) if self.skipping == 0 && self.total < self.max => {
                let text = try_decode(&text).map_err(|_| Error::NoMemory)?;
                self.add(&text)?;
            }
            Token::CData(text) if self.skipping == 0 && self.total < self.max => {
                self.add(text)?;
            }
            _ => {}
        }
        Ok(())
    }
}

/// The paragraphs of the text of `markup`, at most `max` bytes in all.
///
/// # Errors
/// `NoMemory`.
pub fn paragraphs(markup: &str, max: usize) -> Result<Vec<String>, Error> {
    let mut reading = Paragraphs {
        out: Vec::new(),
        current: String::new(),
        total: 0,
        skipping: 0,
        max,
        failed: None,
    };
    let mut tokenizer = Tokenizer::html();
    let mut emit = |token: Token<'_>| {
        if reading.failed.is_none()
            && let Err(error) = reading.token(token)
        {
            reading.failed = Some(error);
        }
    };
    tokenizer
        .try_feed(markup.as_bytes(), &mut emit)
        .map_err(|_| Error::NoMemory)?;
    tokenizer.finish(&mut emit);
    if let Some(error) = reading.failed {
        return Err(error);
    }
    reading.flush()?;
    Ok(reading.out)
}

/// The text of markup, without its tags (for titles).
fn text_of(markup: &str) -> Result<String, Error> {
    let lines = paragraphs(markup, MAX_FIELD)?;
    let size = lines.iter().map(String::len).sum::<usize>() + lines.len().saturating_sub(1);
    let mut out = String::new();
    out.try_reserve_exact(size).map_err(|_| Error::NoMemory)?;
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(line);
    }
    Ok(out)
}

/// One article being read.
#[derive(Debug, Default)]
struct Article {
    title: String,
    date: String,
    link: String,
    /// Its text as markup (escaped HTML decoded, or inline XHTML).
    content: String,
    /// A short summary, used where there is no content.
    summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Title,
    Date,
    Link,
    Content,
    Summary,
}

struct Reader<'a> {
    max_articles: usize,
    max_text: usize,
    date_text: DateText<'a>,
    converted: Converted,
    article: Option<Article>,
    field: Option<Field>,
    /// Elements open inside the field being read.
    depth: usize,
    /// The page's beginning is written.
    started: bool,
    failed: Option<Error>,
    channel_title: String,
    /// The page as written, not yet taken.
    out: Vec<u8>,
}

impl Reader<'_> {
    fn token(&mut self, token: Token<'_>) -> Result<(), Error> {
        if self.articles_done() {
            return Ok(());
        }
        match token {
            Token::Start {
                name,
                attrs,
                self_closing,
            } => self.start(name, attrs, self_closing),
            Token::End { name } => self.end(local_name(name)),
            Token::Text(text) => {
                if self.field.is_some() {
                    // Escaped HTML is decoded once into markup; inside inline
                    // XHTML, text stays as it is.
                    if self.depth > 0 {
                        self.push(&text)
                    } else {
                        let decoded = try_decode(&text).map_err(|_| Error::NoMemory)?;
                        self.push(&decoded)
                    }
                } else {
                    Ok(())
                }
            }
            Token::CData(text) => {
                if self.field.is_some() {
                    self.push(text)
                } else {
                    Ok(())
                }
            }
        }
    }

    fn articles_done(&self) -> bool {
        self.converted.articles >= self.max_articles
    }

    fn start(
        &mut self,
        name: &str,
        attrs: paperpaw_sdk::xml::Attrs<'_>,
        self_closing: bool,
    ) -> Result<(), Error> {
        let local = local_name(name);
        if matches!(self.field, Some(Field::Content | Field::Summary)) {
            // Inline XHTML: kept as markup.
            if !self_closing {
                self.depth += 1;
            }
            let (open, close) = if self_closing {
                ("<", "/>")
            } else {
                ("<", ">")
            };
            return self.push_parts(&[open, local, close]);
        }
        if matches!(local, "item" | "entry") {
            self.article = Some(Article::default());
            return Ok(());
        }
        if self.article.is_none() {
            if local == "title" && self.channel_title.is_empty() {
                self.field = Some(Field::Title);
            }
            return Ok(());
        }
        let field = match (name, local) {
            (_, "title") => Some(Field::Title),
            ("pubDate" | "dc:date", _) | (_, "published" | "updated" | "date") => Some(Field::Date),
            ("content:encoded", _) | (_, "content") => Some(Field::Content),
            (_, "description" | "summary") => Some(Field::Summary),
            (_, "link") => {
                // Atom: the address is an attribute.
                let rel = attrs.try_get("rel").map_err(|_| Error::NoMemory)?;
                if let Some(href) = attrs.try_get("href").map_err(|_| Error::NoMemory)?
                    && rel.as_deref().is_none_or(|r| r == "alternate")
                    && let Some(article) = &mut self.article
                    && article.link.is_empty()
                {
                    article.link = cut_copy(&href, MAX_FIELD)?;
                }
                (!self_closing).then_some(Field::Link)
            }
            _ => None,
        };
        if self_closing {
            return Ok(());
        }
        // A date or the first title: not overwritten.
        self.field = field.filter(|f| match (f, &self.article) {
            (Field::Date, Some(a)) => a.date.is_empty(),
            (Field::Title, Some(a)) => a.title.is_empty(),
            (Field::Content, Some(a)) => a.content.is_empty(),
            (Field::Summary, Some(a)) => a.summary.is_empty(),
            (Field::Link, Some(a)) => a.link.is_empty(),
            _ => true,
        });
        self.depth = 0;
        Ok(())
    }

    fn end(&mut self, local: &str) -> Result<(), Error> {
        if self.depth > 0 {
            self.depth -= 1;
            return self.push_parts(&["</", local, ">"]);
        }
        if self.article.is_some() && matches!(local, "item" | "entry") {
            self.field = None;
            if let Some(article) = self.article.take() {
                self.write(article)?;
            }
            return Ok(());
        }
        self.field = None;
        Ok(())
    }

    /// Adds `parts` one after the other to the field being read, cut as
    /// one text (a tag is cut as a whole, not part by part).
    fn push_parts(&mut self, parts: &[&str]) -> Result<(), Error> {
        let mut text = String::new();
        text.try_reserve_exact(parts.iter().map(|p| p.len()).sum())
            .map_err(|_| Error::NoMemory)?;
        for part in parts {
            text.push_str(part);
        }
        self.push(&text)
    }

    /// Adds to the field being read (markup, made text when written).
    fn push(&mut self, text: &str) -> Result<(), Error> {
        let Some(field) = self.field else {
            return Ok(());
        };
        let Some(article) = &mut self.article else {
            if field == Field::Title && self.channel_title.len() < MAX_FIELD {
                let room = MAX_FIELD - self.channel_title.len();
                push_cut(&mut self.channel_title, text, room)?;
            }
            return Ok(());
        };
        let (target, max) = match field {
            Field::Title => (&mut article.title, MAX_FIELD),
            Field::Date => (&mut article.date, MAX_FIELD),
            Field::Link => (&mut article.link, MAX_FIELD),
            Field::Content => (&mut article.content, self.max_text * 2),
            Field::Summary => (&mut article.summary, self.max_text * 2),
        };
        if target.len() < max {
            let room = max - target.len();
            push_cut(target, text, room)?;
        }
        Ok(())
    }

    /// Adds `parts` to the page, `escaped` ones safe in HTML.
    fn emit(&mut self, parts: &[(&str, bool)]) -> Result<(), Error> {
        let size = parts
            .iter()
            .map(|(text, escaped)| {
                if *escaped {
                    escape_len(text)
                } else {
                    text.len()
                }
            })
            .sum();
        reserve(&mut self.out, size)?;
        for (text, escaped) in parts {
            if *escaped {
                for c in text.chars() {
                    let mut buf = [0u8; 4];
                    self.out.extend_from_slice(match c {
                        '&' => b"&amp;",
                        '<' => b"&lt;",
                        '>' => b"&gt;",
                        '"' => b"&quot;",
                        c => c.encode_utf8(&mut buf).as_bytes(),
                    });
                }
            } else {
                self.out.extend_from_slice(text.as_bytes());
            }
        }
        Ok(())
    }

    /// The page's beginning, once the feed's title is known: [`HEAD`], then
    /// the title.
    fn begin(&mut self) -> Result<(), Error> {
        if self.started {
            return Ok(());
        }
        self.started = true;
        let title = one_line(&text_of(&self.channel_title)?)?;
        self.emit(&[
            (HEAD, false),
            (&title, true),
            ("</title>\n</head>\n<body>\n<h1>", false),
            (&title, true),
            ("</h1>\n", false),
        ])?;
        self.converted.title = title;
        Ok(())
    }

    fn write(&mut self, article: Article) -> Result<(), Error> {
        self.begin()?;
        let title = one_line(&text_of(&article.title)?)?;
        let markup = if article.content.trim().is_empty() {
            &article.summary
        } else {
            &article.content
        };
        let paragraphs = paragraphs(markup, self.max_text)?;
        if title.is_empty() && paragraphs.is_empty() {
            return Ok(());
        }
        self.emit(&[("<h2>", false), (&title, true), ("</h2>\n", false)])?;
        let date = match read_date(&article.date) {
            Some((year, month, day)) => (self.date_text)(year, month, day)?,
            None => one_line(&article.date)?,
        };
        if !date.is_empty() {
            self.emit(&[("<p><i>", false), (&date, true), ("</i></p>\n", false)])?;
        }
        for p in &paragraphs {
            self.emit(&[("<p>", false), (p, true), ("</p>\n", false)])?;
        }
        if self.converted.headlines.len() < HEADLINES_SHOWN && !title.is_empty() {
            reserve(&mut self.converted.headlines, 1)?;
            self.converted.headlines.push(title);
        }
        self.converted.articles += 1;
        Ok(())
    }
}

/// A feed being made into a page.
pub struct Converter<'a> {
    tokenizer: Tokenizer,
    reader: Reader<'a>,
    /// The conversion is over: memory was refused.
    spent: Option<Error>,
}

impl core::fmt::Debug for Converter<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Converter")
            .field("articles", &self.reader.converted.articles)
            .field("spent", &self.spent)
            .finish_non_exhaustive()
    }
}

impl<'a> Converter<'a> {
    /// A conversion of at most `max_articles` articles with at most
    /// `max_text` bytes of text each; dates a feed gives in a form that is
    /// known as `date_text` writes them.
    pub fn new(max_articles: usize, max_text: usize, date_text: DateText<'a>) -> Self {
        Self {
            tokenizer: Tokenizer::new(),
            reader: Reader {
                max_articles,
                max_text,
                date_text,
                converted: Converted::default(),
                article: None,
                field: None,
                depth: 0,
                started: false,
                failed: None,
                channel_title: String::new(),
                out: Vec::new(),
            },
            spent: None,
        }
    }

    /// Whether the articles asked for are written: the rest of the feed is
    /// not needed.
    pub fn is_done(&self) -> bool {
        self.reader.articles_done()
    }

    /// The page written since it was last cleared.
    pub fn taken(&self) -> &[u8] {
        &self.reader.out
    }

    /// Lets go of the page taken, once written.
    pub fn clear(&mut self) {
        self.reader.out.clear();
    }

    fn check(&self) -> Result<(), Error> {
        self.spent.map_or(Ok(()), Err)
    }

    /// Takes the next piece of the feed.
    ///
    /// # Errors
    /// `NoMemory`; the conversion is over then.
    pub fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        self.check()?;
        let Self {
            tokenizer, reader, ..
        } = self;
        let mut emit = |token: Token<'_>| {
            if reader.failed.is_none()
                && let Err(error) = reader.token(token)
            {
                reader.failed = Some(error);
            }
        };
        if tokenizer.try_feed(piece, &mut emit).is_err() {
            self.spent = Some(Error::NoMemory);
        } else if let Some(error) = reader.failed {
            self.spent = Some(error);
        }
        self.check()
    }

    /// Ends the feed: the rest of the page. Whether the feed made a page is
    /// `articles` of the result: a feed without articles is none (a Wi-Fi's
    /// sign-in page), and the caller keeps the book there is.
    ///
    /// # Errors
    /// `NoMemory`, or the error of an earlier `feed`.
    pub fn finish(&mut self) -> Result<Converted, Error> {
        self.check()?;
        let Self {
            tokenizer, reader, ..
        } = self;
        let mut emit = |token: Token<'_>| {
            if reader.failed.is_none()
                && let Err(error) = reader.token(token)
            {
                reader.failed = Some(error);
            }
        };
        tokenizer.finish(&mut emit);
        let result = match reader.failed {
            Some(error) => Err(error),
            None => reader
                .begin()
                .and_then(|()| reader.emit(&[(TAIL, false)]))
                .map(|()| core::mem::take(&mut reader.converted)),
        };
        if let Err(error) = result {
            self.spent = Some(error);
        }
        result
    }
}

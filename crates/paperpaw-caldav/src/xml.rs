//! A small streaming XML scanner for the answers of a WebDAV server: the
//! names of elements without their namespace prefix, their attributes as
//! written, and their text with entities and CDATA resolved, in pieces as the
//! bytes come. It keeps a name and the attributes of one tag, nothing else,
//! and never fails on what it does not understand.

/// The most bytes kept of an element's name and of a tag's attributes.
pub const NAME_MAX: usize = 64;
/// The most bytes kept of a tag's attributes.
pub const ATTRS_MAX: usize = 160;

/// Who is told what the scanner reads.
pub trait Sink {
    /// What the sink may refuse with.
    type Error;
    /// An element begins: its name without prefix, its attributes as written.
    ///
    /// # Errors
    /// The sink's own.
    fn open(&mut self, name: &str, attrs: &str) -> Result<(), Self::Error>;
    /// A piece of text (a text may come in several pieces, split anywhere).
    ///
    /// # Errors
    /// The sink's own.
    fn text(&mut self, text: &[u8]) -> Result<(), Self::Error>;
    /// An element ends (also one written `<name/>`).
    ///
    /// # Errors
    /// The sink's own.
    fn close(&mut self, name: &str) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Text,
    /// After `<`.
    Lt,
    /// In a tag's name.
    Name,
    /// After the name, up to `>`.
    Attrs,
    /// After `<!`.
    Bang,
    /// In a comment, with the `-` read in a row.
    Comment(u8),
    /// In CDATA, with the `]` read in a row.
    CData(u8),
    /// In `<? ?>` or a declaration, up to `>`.
    Skip,
    /// After `&`.
    Entity,
}

/// The scanner. Feed it the bytes in pieces of any size.
#[derive(Debug)]
pub struct Scanner {
    state: State,
    name: [u8; NAME_MAX],
    name_len: usize,
    attrs: [u8; ATTRS_MAX],
    attrs_len: usize,
    closing: bool,
    /// The quote an attribute's value is in, or 0.
    quote: u8,
    /// The tag ends with `/`.
    slash: bool,
    /// What follows `<!` or `&`, to tell what it is.
    word: [u8; 10],
    word_len: usize,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

/// The text an entity stands for (`name` is what is between `&` and `;`).
fn entity(name: &[u8], out: &mut [u8; 4]) -> Option<usize> {
    let c = match name {
        b"lt" => '<',
        b"gt" => '>',
        b"amp" => '&',
        b"quot" => '"',
        b"apos" => '\'',
        [b'#', digits @ ..] => {
            let text = core::str::from_utf8(digits).ok()?;
            let code = match text.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => text.parse().ok()?,
            };
            char::from_u32(code)?
        }
        _ => return None,
    };
    Some(c.encode_utf8(out).len())
}

impl Scanner {
    /// A scanner at the start of a document.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: State::Text,
            name: [0; NAME_MAX],
            name_len: 0,
            attrs: [0; ATTRS_MAX],
            attrs_len: 0,
            closing: false,
            quote: 0,
            slash: false,
            word: [0; 10],
            word_len: 0,
        }
    }

    fn tag<S: Sink>(&mut self, sink: &mut S) -> Result<(), S::Error> {
        self.state = State::Text;
        let name = core::str::from_utf8(self.name.get(..self.name_len).unwrap_or(&[]));
        let attrs = self.attrs.get(..self.attrs_len).unwrap_or(&[]);
        let attrs = attrs.utf8_chunks().next().map_or("", |c| c.valid());
        let name = name.unwrap_or("");
        if self.closing {
            return sink.close(name);
        }
        sink.open(name, attrs)?;
        if self.slash {
            sink.close(name)?;
        }
        Ok(())
    }

    /// One byte of a tag after its name.
    fn attr(&mut self, byte: u8) {
        if self.quote != 0 {
            if byte == self.quote {
                self.quote = 0;
            }
        } else if matches!(byte, b'"' | b'\'') {
            self.quote = byte;
            self.slash = false;
        } else if !byte.is_ascii_whitespace() {
            self.slash = byte == b'/';
        }
        if let Some(slot) = self.attrs.get_mut(self.attrs_len) {
            *slot = byte;
            self.attrs_len += 1;
        }
    }

    /// One byte after `<!`: a comment, CDATA, or something to skip.
    fn bang(&mut self, byte: u8) {
        if let Some(slot) = self.word.get_mut(self.word_len) {
            *slot = byte;
            self.word_len += 1;
        }
        let word = self.word.get(..self.word_len).unwrap_or(&[]);
        if word == b"--" {
            self.state = State::Comment(0);
        } else if word == b"[CDATA[" {
            self.state = State::CData(0);
        } else if !(b"--".starts_with(word) || b"[CDATA[".starts_with(word)) {
            self.state = if byte == b'>' {
                State::Text
            } else {
                State::Skip
            };
        }
    }

    /// Text up to the next `<` or `&`, passed on in one piece.
    fn run<'b, S: Sink>(&mut self, bytes: &'b [u8], sink: &mut S) -> Result<&'b [u8], S::Error> {
        let end = bytes
            .iter()
            .position(|b| matches!(b, b'<' | b'&'))
            .unwrap_or(bytes.len());
        let (text, rest) = bytes.split_at(end);
        if !text.is_empty() {
            sink.text(text)?;
        }
        let Some((mark, rest)) = rest.split_first() else {
            return Ok(rest);
        };
        self.word_len = 0;
        self.state = if *mark == b'<' {
            State::Lt
        } else {
            State::Entity
        };
        Ok(rest)
    }

    /// One byte after `&`.
    fn entity<S: Sink>(&mut self, byte: u8, sink: &mut S) -> Result<(), S::Error> {
        let word = self.word.get(..self.word_len).unwrap_or(&[]);
        if byte == b';' {
            self.state = State::Text;
            let mut out = [0u8; 4];
            return match entity(word, &mut out) {
                Some(len) => sink.text(out.get(..len).unwrap_or(&[])),
                None => {
                    // Not one known: as it was written.
                    sink.text(b"&")?;
                    sink.text(word)?;
                    sink.text(b";")
                }
            };
        }
        let fits = self.word_len < self.word.len();
        if fits && (byte.is_ascii_alphanumeric() || byte == b'#') {
            if let Some(slot) = self.word.get_mut(self.word_len) {
                *slot = byte;
                self.word_len += 1;
            }
            return Ok(());
        }
        // No entity after all: the `&` and what followed were text.
        self.state = State::Text;
        sink.text(b"&")?;
        sink.text(word)?;
        if byte == b'<' {
            self.state = State::Lt;
            return Ok(());
        }
        sink.text(&[byte])
    }

    /// One byte inside CDATA, whose text is passed on as it is.
    fn cdata<S: Sink>(&mut self, brackets: u8, byte: u8, sink: &mut S) -> Result<(), S::Error> {
        match (byte, brackets) {
            (b']', 2) => sink.text(b"]"),
            (b']', n) => {
                self.state = State::CData(n + 1);
                Ok(())
            }
            (b'>', 2) => {
                self.state = State::Text;
                Ok(())
            }
            (other, n) => {
                self.state = State::CData(0);
                sink.text(b"]]".get(..usize::from(n)).unwrap_or(&[]))?;
                sink.text(&[other])
            }
        }
    }

    /// Takes `bytes` and tells `sink` what they hold.
    ///
    /// # Errors
    /// What the sink says.
    pub fn feed<S: Sink>(&mut self, mut bytes: &[u8], sink: &mut S) -> Result<(), S::Error> {
        while let Some((first, after)) = bytes.split_first() {
            let byte = *first;
            if self.state == State::Text {
                bytes = self.run(bytes, sink)?;
                continue;
            }
            bytes = after;
            match self.state {
                State::Text => {}
                State::Lt => {
                    (self.name_len, self.attrs_len) = (0, 0);
                    (self.quote, self.slash, self.closing) = (0, false, byte == b'/');
                    self.state = match byte {
                        b'!' => State::Bang,
                        b'?' => State::Skip,
                        _ => State::Name,
                    };
                    if !matches!(byte, b'/' | b'!' | b'?') {
                        self.name_byte(byte, sink)?;
                    }
                }
                State::Name => self.name_byte(byte, sink)?,
                State::Attrs if byte == b'>' && self.quote == 0 => self.tag(sink)?,
                State::Attrs => self.attr(byte),
                State::Bang => self.bang(byte),
                State::Comment(dashes) => {
                    self.state = match (byte, dashes) {
                        (b'>', 2) => State::Text,
                        (b'-', n) => State::Comment((n + 1).min(2)),
                        _ => State::Comment(0),
                    };
                }
                State::CData(brackets) => self.cdata(brackets, byte, sink)?,
                State::Skip if byte == b'>' => self.state = State::Text,
                State::Skip => {}
                State::Entity => self.entity(byte, sink)?,
            }
        }
        Ok(())
    }

    /// One byte of a tag's name.
    fn name_byte<S: Sink>(&mut self, byte: u8, sink: &mut S) -> Result<(), S::Error> {
        match byte {
            b'>' => return self.tag(sink),
            b'/' => {
                self.slash = true;
                self.state = State::Attrs;
            }
            b':' => self.name_len = 0,
            byte if byte.is_ascii_whitespace() => self.state = State::Attrs,
            byte => {
                if let Some(slot) = self.name.get_mut(self.name_len) {
                    *slot = byte;
                    self.name_len += 1;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, string::String, vec::Vec};

    #[derive(Default)]
    struct Log(Vec<String>, String);

    impl Log {
        fn flush(&mut self) {
            if !self.1.is_empty() {
                self.0.push(format!("'{}'", core::mem::take(&mut self.1)));
            }
        }
    }

    impl Sink for Log {
        type Error = ();
        fn open(&mut self, name: &str, attrs: &str) -> Result<(), ()> {
            self.flush();
            self.0.push(if attrs.is_empty() {
                format!("<{name}>")
            } else {
                format!("<{name} {}>", attrs.trim())
            });
            Ok(())
        }
        fn text(&mut self, text: &[u8]) -> Result<(), ()> {
            self.1.push_str(&String::from_utf8_lossy(text));
            Ok(())
        }
        fn close(&mut self, name: &str) -> Result<(), ()> {
            self.flush();
            self.0.push(format!("</{name}>"));
            Ok(())
        }
    }

    fn scan(text: &str, piece: usize) -> Vec<String> {
        let mut scanner = Scanner::new();
        let mut log = Log::default();
        for part in text.as_bytes().chunks(piece.max(1)) {
            scanner.feed(part, &mut log).unwrap();
        }
        log.flush();
        log.0
    }

    #[test]
    fn elements_text_entities_and_cdata() {
        let text = "<?xml version=\"1.0\"?><!-- a > comment --><d:multistatus xmlns:d=\"DAV:\">\
<d:href>/a%20b/</d:href><x:comp name='VTODO'/><e>&lt;&amp;&#228;&#x20AC;&quot;&unknown; a & b</e>\
<c><![CDATA[1 < 2 ]] ]>]]></c></d:multistatus>";
        let whole = scan(text, text.len());
        assert_eq!(
            whole,
            [
                "<multistatus xmlns:d=\"DAV:\">",
                "<href>",
                "'/a%20b/'",
                "</href>",
                "<comp name='VTODO'/>",
                "</comp>",
                "<e>",
                "'<&ä€\"&unknown; a & b'",
                "</e>",
                "<c>",
                "'1 < 2 ]] ]>'",
                "</c>",
                "</multistatus>"
            ]
        );
        for piece in 1..12 {
            assert_eq!(scan(text, piece), whole, "pieces of {piece}");
        }
    }

    #[test]
    fn a_quoted_bracket_does_not_end_a_tag() {
        assert_eq!(
            scan("<a b=\"x>y\" c='/'>t</a>", 3),
            ["<a b=\"x>y\" c='/'>", "'t'", "</a>"]
        );
    }

    #[test]
    fn nonsense_is_read_to_its_end() {
        for text in [
            "<",
            "<<<>>>",
            "&&&;",
            "<a",
            "<![CDATA[open",
            "<!-- open",
            "&#xZZ;",
            "]]>",
        ] {
            for piece in 1..4 {
                let _ = scan(text, piece);
            }
        }
        assert_eq!(scan("a &#xZZ; b", 2), ["'a &#xZZ; b'"]);
        assert_eq!(scan("a &<b>", 2), ["'a &'", "<b>"]);
    }
}

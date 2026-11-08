//! Every allocation of the native readers and writers of mail may be refused:
//! each ends in `NoMemory` or succeeds, never aborts, and a password in
//! the credentials is wiped by its owner on every path.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_mail::native_model::imap::{self, Fault};
use paperpaw_plugin_mail::native_model::{codec, header, html, mem, smtp};
use paperpaw_sdk::native::Error;

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

/// A sweep that must find allocations to refuse.
fn swept<T>(name: &str, run: impl FnMut() -> Result<T, Error>) {
    let total = fail_alloc::sweep(run);
    assert!(
        total > 0,
        "{name} allocates nothing: the sweep proves nothing"
    );
}

#[test]
fn the_encodings_survive_refused_allocations() {
    swept("base64", || {
        let data = codec::decode_base64(b"aGVsbG8gd29ybGQ=\r\nAAEC")?;
        let text = codec::encode_base64(&data)?;
        codec::base64_line(&data)?;
        let mut out = Vec::new();
        let mut pieces = codec::Base64::default();
        pieces.push(text.as_bytes(), &mut out)?;
        pieces.finish(&mut out)
    });
    swept("qp and words", || {
        codec::decode_qp(b"caf=E9=\r\n au_lait", true)?;
        codec::encode_qp("caf\u{e9} au lait \u{4e16}\u{754c}")?;
        codec::encode_words("caf\u{e9} \u{4e16}\u{754c}")?;
        codec::decode_words(b"=?utf-8?B?w6TDtsO8?= x =?iso-8859-1?Q?caf=E9?= tail")?;
        codec::decode_words(&[b'c', 0xE9, b' ', 0xFF])?;
        Ok(())
    });
    swept("charsets", || {
        for charset in [
            codec::Charset::Utf8,
            codec::Charset::Latin1,
            codec::Charset::Windows1252,
            codec::Charset::Koi8R,
        ] {
            codec::text_of(charset, &[b'a', 0xC3, 0xA4, 0xFF, 0xC3])?;
            let mut decoder = codec::Decoder::new(charset);
            let mut out = String::new();
            decoder.push(&[b'a', 0xC3], &mut out)?;
            decoder.push(&[0xA4, b'z'], &mut out)?;
            decoder.finish(&mut out)?;
        }
        codec::utf8_lossy(&[b'a', 0xFF, b'b'])?;
        Ok(())
    });
}

#[test]
fn the_headers_survive_refused_allocations() {
    let raw = b"Subject: =?utf-8?B?w6TDtsO8?=\r\nFrom: Ada <ada@example.org>\r\n\
To: \"Doe, John\" <j@x>, b@y\r\nContent-Type: multipart/mixed;\r\n boundary=\"b\"; name*=utf-8''%C3%A4\r\n\
References: <1@x> <2@x>\r\n\r\nbody";
    swept("headers", || {
        let headers = header::Headers::parse(raw)?;
        header::ContentType::parse(headers.get("content-type"))?;
        header::ContentType::parse(None)?;
        header::addresses(headers.get("to").unwrap())?;
        header::reply_subject("Re: Hello")?;
        header::capped("caf\u{e9} caf\u{e9}", 5)?;
        header::last_ids("<1@x> <2@x> <3@x> <4@x>", 12)?;
        Ok(())
    });
}

#[test]
fn credentials_and_secrets_survive_refused_allocations() {
    swept("credentials", || {
        let creds = codec::plain_credentials("ada", "hunter2")?;
        assert_eq!(creds.as_slice(), b"\0ada\0hunter2");
        let mut secret = mem::Secret::with_capacity(7)?;
        secret.push_str("hunter2")?;
        mem::wipe_text(secret.into_string());
        Ok(())
    });
}

#[test]
fn the_html_reader_survives_refused_allocations() {
    swept("html", || {
        let text = html::to_text(
            "<p>Hello &amp; welcome &unknown; &#x1F600;</p><ul><li>one</li><li>two</li></ul>\
             <style>p { color: red }</style><!-- note --><br>caf\u{e9} <b>bold</b> &am",
        )?;
        assert!(text.starts_with("Hello & welcome"));
        Ok(())
    });
    swept("html in pieces", || {
        let mut parser = html::HtmlText::with_limit(64);
        let mut out = String::new();
        for piece in [
            "<p>He",
            "llo &a",
            "mp; <scr",
            "ipt>x</script>",
            "<li>a",
            "b",
            "</li>",
        ] {
            parser.push(piece, &mut out)?;
        }
        parser.finish(&mut out)
    });
}

/// A fault of the IMAP reader as the sweep sees it: only a refusal is allowed.
fn refusal(fault: Fault) -> Error {
    match fault {
        Fault::NoMemory => Error::NoMemory,
        _ => Error::Invalid,
    }
}

#[test]
fn the_imap_reader_survives_refused_allocations() {
    const LINES: &[&str] = &[
        "A1 OK LOGIN completed",
        "+ go ahead",
        "* CAPABILITY IMAP4rev1 STARTTLS AUTH=PLAIN",
        "* LIST (\\HasNoChildren \\Trash) \"/\" \"Deleted Items\"",
        "* SEARCH 2 84 882",
        "* STATUS \"INBOX\" (MESSAGES 231 UNSEEN 7)",
        "* BYE gone",
        "* OK [UIDVALIDITY 3857529045] UIDs valid",
        "* 12 FETCH (UID 42 FLAGS (\\Seen \\Answered) RFC822.SIZE 1000 INTERNALDATE \"d\")",
        "* 3 FETCH (BODY[HEADER.FIELDS (FROM TO)] {342}",
    ];
    for line in LINES {
        swept("response", || {
            imap::response(line.as_bytes()).map_err(refusal)?;
            imap::tokens(line.as_bytes()).map_err(refusal)
        });
    }
    swept("fetch parts", || {
        let line = b"* 12 FETCH (UID 42 FLAGS (\\Seen \\Deleted) RFC822.SIZE 9)";
        let Some(imap::Response::Untagged(imap::Untagged::Fetch { items, .. })) =
            imap::response(line).map_err(refusal)?
        else {
            return Err(Error::Invalid);
        };
        let flags = imap::fetch_flags(&items).map_err(refusal)?;
        assert_eq!(imap::flag_letters(&flags)?, "SD");
        Ok(())
    });
    swept("names and quoting", || {
        assert_eq!(imap::mailbox_display("Entw&APw-rfe")?, "Entw\u{fc}rfe");
        imap::quoted("a \"b\" c")?;
        let secret = imap::quoted_secret("pa\"ss")?;
        assert_eq!(secret.as_str(), "\"pa\\\"ss\"");
        Ok(())
    });
    swept("line buffer", || {
        let mut buffer = imap::LineBuffer::default();
        buffer
            .push(b"A1 OK one\r\nA2 OK two\r\n{5}abcde")
            .map_err(refusal)?;
        assert!(buffer.line().map_err(refusal)?.is_some());
        assert!(buffer.line().map_err(refusal)?.is_some());
        buffer.take(3)?;
        Ok(())
    });
}

#[test]
fn the_smtp_reader_survives_refused_allocations() {
    swept("replies", || {
        let mut parser = smtp::ReplyParser::default();
        for line in ["250-mail.example.org", "250-STARTTLS", "250 AUTH PLAIN"] {
            parser.line(line.as_bytes())?;
        }
        parser.line(b"garbage")?;
        parser.line(&[b'x'; 1001])?;
        Ok(())
    });
    swept("commands", || {
        assert_eq!(smtp::mail_from("a@b.c")?, "MAIL FROM:<a@b.c>\r\n");
        smtp::rcpt_to("a@b.c>\r\nDATA")?;
        let line = smtp::auth_plain("ada", "hunter2")?;
        assert_eq!(line.as_str(), "AUTH PLAIN AGFkYQBodW50ZXIy\r\n");
        smtp::data("a\n.b\n..c\n")?;
        Ok(())
    });
}

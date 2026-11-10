//! TEMPORARY: prints what the interface-6 code answers.
#![allow(clippy::all, clippy::pedantic, missing_docs, dead_code, unused)]
use paperpaw_plugin_mail::{codec, header, html, imap, smtp};
use std::fmt::Write as _;

struct Out(String);
impl Out {
    fn push(&mut self, tag: &str, content: String) {
        assert!(!content.contains('\n'), "{content}");
        writeln!(self.0, "{tag}\t{content}").unwrap();
    }
}
fn write(name: &str, out: Out) {
    std::fs::write(
        format!("{}/testdata/{name}.golden", env!("CARGO_MANIFEST_DIR")),
        out.0,
    )
    .unwrap();
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 7 + 3) as u8).collect()
}

const NOISY_B64: &[&[u8]] = &[
    b"",
    b"Zm9v",
    b"Zm9v YmFy",
    b"Zm9v\r\nYmFy\r\n",
    b"Zm9vYg",
    b"Zm9vYg=",
    b"Zm9vYg==",
    b"Zg==Zg==",
    b"!!!!",
    b"Zm9v-_Yg",
    b"=Zm9v",
    b"Z",
    b"Zm",
    b"Zm9",
    b"Zm9vY",
    b"ab\xffcd",
    b"+/+/",
    b"AAAA====",
];
const QP_INPUTS: &[&[u8]] = &[
    b"",
    b"plain",
    b"caf=E9 au lait=\r\n et plus",
    b"a=\nb",
    b"=zz",
    b"=E",
    b"=",
    b"a_b_c",
    b"A9F\t",
    b"=C3=A4=C3=B6",
    b"line \r\nnext",
    b"soft=\r\nbreak",
    b"=3D",
    b"=e9",
    b"x=\r",
    b"trailing \t\r\n",
    b"a=b",
    b"\xe9=\xe9",
];
const QP_TEXTS: &[&str] = &[
    "",
    "plain",
    "caf\u{e9}",
    "a=b",
    "tab\there",
    "trailing space \r\nnext",
    "line\nbreak",
    "\u{4e16}\u{754c}",
    "under_score",
    "dot.\n.dot",
    "?=",
    "caf\u{e9} \u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}",
];
const WORD_TEXTS: &[&str] = &[
    "",
    "plain",
    "Hello World",
    "caf\u{e9}",
    "\u{4e16}\u{754c}",
    "Gr\u{fc}\u{df}e aus K\u{f6}ln",
    "a b c",
    "x=?y",
    "\u{1f600}",
    "=?utf-8?Q?x?=",
];
const DECODE_WORDS: &[&str] = &[
    "plain",
    "=?utf-8?B?w6TDtsO8?=",
    "=?UTF-8?Q?a_b=C3=A4?= tail",
    "=?iso-8859-1?Q?caf=E9?= =?iso-8859-1?Q?_au?=",
    "=?koi8-r?B?8NLJ18XU?=",
    "=?windows-1251?Q?=CF=F0=E8=E2=E5=F2?=",
    "=?unknown?Q?x?=",
    "=?utf-8?B?",
    "=?utf-8?X?abc?=",
    "caf\u{e9} \u{4e16}\u{754c}",
    "=?utf-8*de?Q?x?=",
    "a =?utf-8?Q?b?= =?utf-8?Q?c?= d",
];
const CHARSETS: &[&str] = &[
    "utf-8",
    "us-ascii",
    "iso-8859-1",
    "iso-8859-15",
    "windows-1252",
    "iso-8859-2",
    "windows-1250",
    "windows-1251",
    "koi8-r",
    "UTF-8*de",
    "\"utf-8\"",
    "klingon",
    "",
];
const HEADER_BLOCKS: &[&str] = &[
    "",
    "Subject: Hello\r\n",
    "Subject: Hello\r\nFrom: Ada Lovelace <ada@example.org>\r\nTo: \"Doe, John\" <john@example.org>, bob@example.org (Bob)\r\nDate: Tue, 3 Nov 2026 09:15:00 +0100\r\n\r\nSubject: not a header\r\n",
    "Content-Type: multipart/mixed; boundary=\"b1\"\r\nContent-Type: text/plain; charset=iso-8859-1\r\n",
    "Content-Type: text/plain; charset=iso-8859-1\r\nContent-Disposition: attachment; filename*=utf-8''%C3%A4.txt\r\n",
    "Subject: folded\r\n continued value\r\n\tmore\r\nMessage-ID: <abc@example.org>\r\n",
    "Subject: =?utf-8?B?w6TDtsO8?=\r\nSubject: =?iso-8859-1?Q?caf=E9_au_lait?= =?utf-8?q?x?=\r\n",
    "References: <a@x> <b@x>\r\nX-Odd:\r\nno colon here\r\nbad name: x\r\n",
    "Subject: Hello\n\nbody\n",
    "Subject: caf\u{e9}\r\n:\r\nFrom: <>\r\nTo: a@b, ,c@d,\r\n",
    "Date: 99 Foo 1\r\nContent-Type: ;;;\r\n",
    "Content-Type: text/html; charset=\"utf-8\"; name=x; name=y\r\nFrom: \"unterminated <a@b>\r\n",
    "To: (nested (comment)) a@b\r\nSubject: Re: Re: Subject\r\nReferences: <1@x> <2@x> <3@x>\r\n",
    "Date: Thu, 1 Jan 1970 00:00:00 +0000\r\n\r\n",
    "\n",
    " leading fold\r\nSubject: x\r\n",
    "Subject:no space\r\nSUBJECT: upper\r\nsubject: lower\r\n",
    "abc: \r\n\t=?-\r\nA:\r\n",
];
const HEADER_NAMES: [&str; 11] = [
    "subject",
    "from",
    "to",
    "date",
    "content-type",
    "content-disposition",
    "message-id",
    "references",
    "x-odd",
    "bad name",
    "",
];
const CONTENT_TYPES: &[Option<&str>] = &[
    None,
    Some(""),
    Some("text/plain"),
    Some("TEXT/HTML; Charset=\"UTF-8\""),
    Some("multipart/mixed; boundary=\"a;b\"; x=y"),
    Some("application/octet-stream; name*=utf-8''%C3%A4.bin"),
    Some("attachment; filename*0=\"long\"; filename*1=\"name\""),
    Some("a; b; c=; =d"),
    Some(";;;"),
    Some("text/plain; charset"),
    Some("text/plain; name*=%zz"),
];
const ADDRESS_LISTS: &[&str] = &[
    "",
    "ada@example.org",
    "Ada Lovelace <ada@example.org>",
    "\"Doe, John\" <john@example.org>, bob@example.org (Bob)",
    "a@b, ,c@d,",
    "<>",
    "(nested (comment)) a@b",
    "\"unterminated <a@b>",
    "Group: a@b, c@d;",
    "no address, nobody",
    "=?utf-8?B?w6TDtsO8?= <x@y>",
];
const DATES: &[&str] = &[
    "Tue, 3 Nov 2026 09:15:00 +0100",
    "3 Nov 2026 09:15 -0500",
    "Thu, 1 Jan 1970 00:00:00 +0000",
    "Mon, 29 Feb 2016 23:59:59 GMT",
    "Fri, 31 Dec 1999 23:59:60 +0000 (CET)",
    "Sat, 4 Jul 76 12:00:00 UT",
    "garbage",
    "",
    "99 Foo 1",
    "1 Jan 2026",
    "1 Jan 0000 00:00 +0000",
];
const SUBJECTS: &[&str] = &[
    "",
    "Hello",
    "Re: Hello",
    "RE: Re: Hello",
    "AW: Hello",
    "re:hello",
    "Fwd: Re: x",
];
const DAYS: &[i64] = &[-1_000_000, -1, 0, 1, 18_000, 2_932_896];

fn model(out: &mut Out) {
    for len in [0usize, 1, 2, 3, 4, 5, 57, 58, 100, 300] {
        let data = pattern(len);
        out.push(
            "base64",
            format!(
                "{len}\t{:?}\t{:?}",
                codec::encode_base64(&data),
                codec::base64_line(&data)
            ),
        );
    }
    for input in NOISY_B64 {
        out.push(
            "decode_base64",
            format!("{input:?}\t{:?}", codec::decode_base64(input)),
        );
    }
    for input in QP_INPUTS {
        for header in [false, true] {
            out.push(
                "decode_qp",
                format!("{input:?}\t{header}\t{:?}", codec::decode_qp(input, header)),
            );
        }
    }
    for text in QP_TEXTS {
        out.push(
            "encode_qp",
            format!("{text:?}\t{:?}", codec::encode_qp(text)),
        );
    }
    for text in WORD_TEXTS {
        out.push(
            "encode_words",
            format!("{text:?}\t{:?}", codec::encode_words(text)),
        );
    }
    for text in DECODE_WORDS {
        out.push(
            "decode_words",
            format!("{text:?}\t{:?}", codec::decode_words(text.as_bytes())),
        );
    }
    let latin = [b'c', b'a', b'f', 0xE9, b' ', 0xFF, 0xC3];
    out.push(
        "decode_words_bytes",
        format!("{latin:?}\t{:?}", codec::decode_words(&latin)),
    );
    let all: Vec<u8> = (0..=255u8).collect();
    for name in CHARSETS {
        let set = codec::Charset::of(name);
        out.push("charset", format!("{name:?}\t{}", set.is_some()));
        if let Some(set) = set {
            out.push(
                "charset_text",
                format!("{name:?}\t{:?}", codec::text_of(set, &all)),
            );
        }
    }
    for raw in HEADER_BLOCKS {
        let headers = header::Headers::parse(raw.as_bytes());
        let gets: Vec<Option<&str>> = HEADER_NAMES.iter().map(|n| headers.get(n)).collect();
        let ct = header::ContentType::parse(headers.get("content-type"));
        let params: Vec<Option<&str>> = ["boundary", "charset", "name", "filename"]
            .iter()
            .map(|p| ct.param(p))
            .collect();
        out.push(
            "headers",
            format!(
                "{raw:?}\t{}\t{gets:?}\t{:?}\t{}\t{params:?}",
                headers.len(),
                ct.kind,
                ct.is_multipart()
            ),
        );
    }
    for value in CONTENT_TYPES {
        let ct = header::ContentType::parse(*value);
        let params: Vec<Option<&str>> = ["charset", "boundary", "name", "filename", "x", "c", "b"]
            .iter()
            .map(|p| ct.param(p))
            .collect();
        out.push(
            "content_type",
            format!("{value:?}\t{:?}\t{params:?}", ct.kind),
        );
    }
    for list in ADDRESS_LISTS {
        let found: Vec<(String, String, String)> = header::addresses(list)
            .iter()
            .map(|a| (a.name.clone(), a.address.clone(), a.shown().to_string()))
            .collect();
        out.push("addresses", format!("{list:?}\t{found:?}"));
    }
    for date in DATES {
        out.push(
            "date",
            format!("{date:?}\t{:?}", header::date_seconds(date)),
        );
    }
    for subject in SUBJECTS {
        out.push(
            "reply_subject",
            format!("{subject:?}\t{:?}", header::reply_subject(subject)),
        );
    }
    for days in DAYS {
        out.push(
            "civil",
            format!("{days}\t{:?}", header::civil_from_days(*days)),
        );
    }
    let long = "x".repeat(1000);
    for (label, text) in [
        ("empty", ""),
        ("short", "short"),
        ("cafe", "caf\u{e9} caf\u{e9}"),
        ("x1000", long.as_str()),
    ] {
        for max in [0usize, 1, 2, 3, 4, 5, 100, 2000] {
            let (cut, capped) = (header::cut(text, max), header::capped(text, max));
            if text.len() < 100 {
                out.push("cut", format!("{label}\t{max}\t{cut:?}\t{capped:?}"));
            } else {
                out.push(
                    "cut",
                    format!("{label}\t{max}\t{}\t{}", cut.len(), capped.len()),
                );
            }
        }
    }
    let ids = "<1@x> <2@x> <3@x> <4@x> <\u{e9}@x>";
    for max in [0usize, 3, 6, 12, 20, 40, 100] {
        out.push(
            "last_ids",
            format!("{max}\t{:?}", header::last_ids(ids, max)),
        );
    }
}

const HTML: &[&str] = &[
    "",
    "plain text",
    "<p>Hello &amp; welcome</p><p>Second</p>",
    "a<br>b<br/>c<BR />d",
    "<ul><li>one</li><li>two<li>three</ul>",
    "<table><tr><td>a</td><td>b</td></tr><tr><td>c</td></tr></table>",
    "<style>p { color: red }</style><p>x</p>",
    "<SCRIPT type=text/javascript>if (a < b) { x = '</p>' }</SCRIPT>after",
    "<head><title>t</title></head><body>body</body>",
    "<script/>kept<style/>kept too",
    "<!-- a > b --><p>after comment</p>",
    "<!----><!-- -- --><!--->x",
    "<!-->x",
    "&lt;tag&gt; &quot;q&quot; &apos;a&apos; &nbsp;|&nbsp; &#39; &#x41; &#X42;",
    "&auml;&ouml;&uuml;&Auml;&Ouml;&Uuml;&szlig;&euro;&copy;&reg;&eacute;&egrave;&agrave;&ccedil;",
    "&unknown; &; &#; &#xZZ; &#99999999999; &#0; &amp &lt;x",
    "&&&&;;;&#&#;",
    "<<<<script>&&&;;&#99999999999;</style>",
    "a   b \t\n c \u{a0}\u{a0} d \u{200b} e",
    "tail & open",
    "tail <open tag",
    "<p>  leading</p>   <p>trailing  </p>   ",
    "<div><div><p>nest</p></div></div><hr><h1>H</h1><h6>h</h6>",
    "<pre>pre\n  formatted</pre><blockquote>quote</blockquote>",
    "<a href=\"x>y\">link</a> <b>bold</b> <img src=x alt=\"\">",
    "caf\u{e9} \u{4e16}\u{754c} \u{1f600} <p>\u{e4}\u{f6}\u{fc}</p>",
    "\u{1}\u{2}control\u{7f}chars\r\nline",
    "</p></div></li>stray ends",
    "<>< ><  p  >text</  p  >",
];

fn html_dump(out: &mut Out) {
    for h in HTML {
        out.push("fixed", format!("{h:?}\t{:?}", html::to_text(h)));
    }
    let mut parser = html::HtmlText::new();
    let mut text = String::new();
    for piece in [
        "<p>Hello &am",
        "p; world</p><ul><li>a</li></ul>&#x1F6",
        "00; <sty",
    ] {
        parser.push(piece, &mut text);
        out.push("pieces", format!("{piece:?}\t{text:?}"));
    }
    parser.finish(&mut text);
    out.push("pieces", format!("finish\t{text:?}"));
}

#[test]
fn dump() {
    let mut out = Out(String::new());
    model(&mut out);
    write("model", out);
    let mut out = Out(String::new());
    html_dump(&mut out);
    write("html", out);
}

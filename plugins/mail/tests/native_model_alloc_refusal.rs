//! Every allocation of the native readers and writers of mail may be refused:
//! each ends in `NoMemory` or succeeds, never aborts, and a password in
//! the credentials is wiped by its owner on every path.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
use paperpaw_plugin_mail::native_model::{codec, header, mem};
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
        let mut secret = mem::Secret::with_capacity(4)?;
        secret.push_str("hunter2")?;
        mem::wipe_text(secret.into_string());
        Ok(())
    });
}

//! Input rules: the order of the records, every field at its limit and one
//! over, the roots, the caps of the profile.
use super::*;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE,
    ERR_UNSUPPORTED,
};

fn with_head(h: wire::ServiceNetRequest<'static>) -> Vec<u8> {
    batch(&[Record::ServiceNetRequest(h)])
}

fn header(name: &'static str, value: &'static str) -> Record<'static> {
    Record::ServiceNetHeader(wire::ServiceNetHeader { name, value })
}

fn file(role: u32, root: u32, path: &'static str) -> Record<'static> {
    Record::ServiceNetFile(wire::ServiceNetFile { role, root, path })
}

fn call(input: &[u8]) -> Result<u32, i32> {
    request(&Fake::x4(), OWNER, input)
}

fn rec(h: wire::ServiceNetRequest<'static>) -> Record<'static> {
    Record::ServiceNetRequest(h)
}

#[test]
fn a_plain_request_gets_a_checked_id_and_starts() {
    let fake = Fake::x4();
    let first = request(&fake, OWNER, &big(1, 100)).unwrap();
    let second = request(&fake, OWNER, &big(1, 100)).unwrap();
    assert_ne!(first, 0);
    assert_eq!(second, first + 1, "ids are never reused");
    assert_eq!(*fake.started.borrow(), [first, second]);
}

#[test]
fn the_records_must_come_in_their_order_and_once() {
    let h = || rec(head(1, "https://example.org/"));
    let hd = || header("Accept", "text/html");
    let body = || Record::ServiceNetBody(wire::ServiceNetBody { data: b"x" });
    let bad: &[&[Record<'_>]] = &[
        &[hd(), h()],
        &[h(), h()],
        &[h(), file(1, 0, "a"), hd()],
        &[h(), body(), file(1, 0, "a")],
        &[h(), file(1, 0, "a"), file(1, 0, "b")],
        &[h(), file(0, 0, "a"), file(0, 0, "b")],
        &[h(), file(2, 0, "a")],
        &[h(), file(0, 0, "a"), body()],
        &[h(), body(), body()],
        &[Record::ServiceNetToken(wire::ServiceNetToken { token: 1 })],
    ];
    for (n, records) in bad.iter().enumerate() {
        assert_eq!(call(&batch(records)), Err(ERR_INVALID), "case {n}");
    }
    // The good order, with every part.
    let good = batch(&[h(), hd(), file(0, 0, "send.bin"), file(1, 0, "save.bin")]);
    let fake = Fake::x4();
    fake.files.borrow_mut().insert((0, "send.bin".into()), 10);
    assert!(request(&fake, OWNER, &good).is_ok());
    assert_eq!(call(&batch(&[])), Err(ERR_INVALID), "no request at all");
}

#[test]
fn at_most_sixteen_headers() {
    let mut records = std::vec![rec(head(1, "https://example.org/"))];
    for _ in 0..16 {
        records.push(header("X-A", "v"));
    }
    assert!(call(&batch(&records)).is_ok());
    records.push(header("X-A", "v"));
    assert_eq!(call(&batch(&records)), Err(ERR_INVALID));
}

#[test]
fn method_url_and_header_fields_hold_at_their_limits() {
    let mut h = head(1, "https://example.org/");
    h.method = "PROPFINDPROPFIND1"; // 17 bytes: over the codec limit
    // The codec refuses a 17-byte method before the service sees it.
    let mut out = std::vec![0; 4096];
    assert!(wire::records_encode_counted(&[rec(h)], 20, 4096, &mut out).is_err());
    for method in ["GET", "PROPFIND", "M-SEARCH", "PROPFINDPROPFIND"] {
        let mut h = head(1, "https://example.org/");
        h.method = method;
        assert!(call(&with_head(h)).is_ok(), "{method}");
    }
    for method in ["", "GE T", "GET\n", "G/T", "G:T"] {
        let mut h = head(1, "https://example.org/");
        h.method = method;
        assert_eq!(call(&with_head(h)), Err(ERR_INVALID), "{method:?}");
    }
    // A header name of 64 bytes; 65 is over the codec's.
    const N64: &str = "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijkl";
    assert_eq!(N64.len(), 64);
    assert!(call(&batch(&[rec(head(1, "http://a.b/")), header(N64, "v")])).is_ok());
    for (name, value) in [
        ("", "v"),
        ("Bad Name", "v"),
        ("X", "line\nbreak"),
        ("X", "nul\0"),
        ("Host", "evil"),
        ("host", "evil"),
        ("CONNECTION", "keep-alive"),
        ("Content-Length", "1"),
    ] {
        let records = [rec(head(1, "http://a.b/")), header(name, value)];
        assert_eq!(
            call(&batch(&records)),
            Err(ERR_INVALID),
            "{name:?} {value:?}"
        );
    }
}

#[test]
fn urls_follow_the_grammar() {
    for ok in [
        "http://a.b/",
        "HTTPS://Example.ORG:8443/path?q=1#frag",
        "https://user:pw@host.example/x",
        "http://192.168.1.20:8080/",
        "http://[::1]/x",
        "http://[2001:db8::1]:80/",
        "http://[1:2:3:4:5:6:7:8]/",
        "http://[::]/",
        "http://[1::]/",
        "http://[::ffff:1.2.3.4]:8080/",
        "http://[64:ff9b::192.0.2.33]/",
        "http://host_name.with~tilde-and.dot",
        "http://a.b",
        "http://a.b?x=1",
        "http://a.b/with space and ünï",
    ] {
        assert!(args::url_ok(ok), "{ok}");
    }
    for bad in [
        "",
        "ftp://a.b/",
        "a.b/x",
        "http:/a.b",
        "http:///x",
        "http://",
        "http://a.b:0/",
        "http://a.b:65536/",
        "http://a.b:/",
        "http://a.b:80a/",
        "http://[::1/",
        "http://[abcd]/",
        "http://[1:2]/",
        "http://[::::]/",
        "http://[1:::2]/",
        "http://[1::2::3]/",
        "http://[1:2:3:4:5:6:7:8:9]/",
        "http://[1:2:3:4:5:6:7::8]/",
        "http://[12345::1]/",
        "http://[::g]/",
        "http://[1.2.3.4::]/",
        "http://[::1.2.3]/",
        "http://[::1.2.3.256]/",
        "http://[::01.2.3.4]/",
        "http://[:1:2:3:4:5:6:7]/",
        "http://[fe80::1%25eth0]/",
        "http://[::1]x/",
        "http://a b/",
        "http://a.b/\u{7}",
        "http://a.b/\n",
        "http://a/b\0",
        "http://ho$t/",
    ] {
        assert!(!args::url_ok(bad), "{bad:?}");
    }
    assert!(args::url_ok("http://a.b:65535/"));
    assert!(args::url_ok("http://a.b:1/"));
    let long = std::format!("http://a.b/{}", "x".repeat(1024 - 11));
    assert_eq!(long.len(), 1024);
    assert!(args::url_ok(&long));
    assert!(!args::url_ok(&std::format!("{long}x")));
}

#[test]
fn flags_timeouts_and_caps_hold_at_their_limits() {
    let mut h = head(1, "http://a.b/");
    h.flags = 2;
    assert_eq!(call(&with_head(h)), Err(ERR_UNSUPPORTED));
    h.flags = 1;
    assert!(call(&with_head(h)).is_ok());
    for (timeout, ok) in [
        (0, true),
        (999, false),
        (1_000, true),
        (600_000, true),
        (600_001, false),
    ] {
        let mut h = head(1, "http://a.b/");
        h.timeout_ms = timeout;
        assert_eq!(call(&with_head(h)).is_ok(), ok, "timeout {timeout}");
    }
    // max_body: the X4 default is the pool's 49,152; one over is no space.
    let fake = Fake::x4();
    assert!(request(&fake, OWNER, &big(1, 45_056)).is_ok());
    assert_eq!(request(&fake, OWNER, &big(1, 45_057)), Err(ERR_NO_SPACE));
    assert!(request(&Fake::new(NetProfile::OTHER), OWNER, &big(1, 1_048_576)).is_ok());
    assert_eq!(
        request(&Fake::new(NetProfile::OTHER), OWNER, &big(1, 1_048_577)),
        Err(ERR_NO_SPACE)
    );
    // max_file: 64 MiB is the most.
    for (max_file, ok) in [
        (0, true),
        (64 * 1024 * 1024, true),
        (64 * 1024 * 1024 + 1, false),
    ] {
        let mut h = head(1, "http://a.b/");
        h.max_file = max_file;
        assert_eq!(call(&with_head(h)).is_ok(), ok, "max_file {max_file}");
    }
}

#[test]
fn files_have_their_roots_and_names() {
    let fake = Fake::x4();
    fake.files.borrow_mut().insert((2, "page.html".into()), 4);
    fake.files.borrow_mut().insert((0, "up.bin".into()), 4);
    let r = |records: &[Record<'_>]| {
        let result = request(&fake, OWNER, &batch(records));
        owner_stopped(&fake, OWNER).unwrap();
        result
    };
    let h = || rec(head(1, "http://a.b/"));
    // Send: a file that exists, root 2 is readable; a missing one is not found.
    assert!(r(&[h(), file(0, 0, "up.bin")]).is_ok());
    assert!(r(&[h(), file(0, 2, "page.html")]).is_ok());
    assert_eq!(r(&[h(), file(0, 0, "none.bin")]), Err(ERR_NOT_FOUND));
    // Save: root 0; root 1 needs uses.storage (the fake grants none); root 2
    // is never written; another root is invalid.
    assert!(r(&[h(), file(1, 0, "out.bin")]).is_ok());
    assert_eq!(r(&[h(), file(1, 1, "out.bin")]), Err(ERR_DENIED));
    assert_eq!(r(&[h(), file(1, 2, "out.bin")]), Err(ERR_DENIED));
    assert_eq!(r(&[h(), file(1, 3, "out.bin")]), Err(ERR_INVALID));
    // Paths: the storage grammar, and never a `.pp-staged` name (any case).
    for bad in [
        "",
        "../x",
        "a//b",
        "a/./b",
        "a:b",
        "a\\b",
        "a.pp-staged",
        "a.PP-STAGED",
        "dir/b.Pp-Staged",
    ] {
        assert_eq!(r(&[h(), file(1, 0, bad)]), Err(ERR_INVALID), "{bad:?}");
    }
    assert!(r(&[h(), file(1, 0, "a.part")]).is_ok());
    assert!(r(&[h(), file(1, 0, "x.pp-staged.txt")]).is_ok());
    assert!(r(&[h(), file(1, 0, "part")]).is_ok());
}

#[test]
fn a_request_needs_a_subscription_to_its_end() {
    let fake = Fake::x4();
    fake.subscribed.set(false);
    assert_eq!(request(&fake, OWNER, &get(1)), Err(ERR_STATE));
    assert!(fake.started.borrow().is_empty());
}

#[test]
fn four_requests_per_plugin_and_sixteen_per_device() {
    let fake = Fake::new(NetProfile::OTHER);
    for token in 1..=4 {
        request(&fake, OWNER, &get(token)).unwrap();
    }
    assert_eq!(request(&fake, OWNER, &get(5)), Err(ERR_BUSY));
    // Another plugin has its own four; the sixteenth is the last of the device.
    for owner in [20, 21, 22] {
        for _ in 0..4 {
            request(&fake, owner, &get(1)).unwrap();
        }
    }
    assert_eq!(request(&fake, 23, &get(1)), Err(ERR_BUSY));
    // One ends: its plugin has room again, others still wait for the device.
    let ids = fake.pool_used();
    assert_eq!(ids, 0, "no pool on this device");
    cancel_all(&fake, OWNER, 1);
    // The plugin has to hear of the ends first: they count until then.
    drain(&fake, OWNER);
    assert!(request(&fake, OWNER, &get(1)).is_ok());
    assert_eq!(request(&fake, 23, &get(1)), Err(ERR_BUSY));
}

fn cancel_all(fake: &Fake, owner: u64, token: u32) {
    table::cancel(fake, owner, token).unwrap();
}

#[test]
fn the_same_save_file_replaces_the_older_request_of_the_same_plugin() {
    let fake = Fake::x4();
    let old = request(&fake, OWNER, &save_to(1, 0, "Out.bin")).unwrap();
    assert_eq!(fake.claims.borrow().len(), 1);
    let new = request(&fake, OWNER, &save_to(2, 0, "out.BIN")).unwrap();
    assert_eq!(*fake.aborted.borrow(), [old], "the older one is dropped");
    let events = drain(&fake, OWNER);
    assert_eq!(
        events,
        [Event::Done {
            id: old,
            token: 1,
            status: 0,
            reason: reason::REPLACED,
            flags: 0,
            body_len: 0
        }]
    );
    assert_eq!(fake.claims.borrow().len(), 1, "only the new one is claimed");
    assert!(fake.started.borrow().contains(&new));
    // A different file does not replace.
    request(&fake, OWNER, &save_to(3, 0, "other.bin")).unwrap();
    assert!(drain(&fake, OWNER).is_empty());
}

#[test]
fn another_plugins_download_of_the_same_file_is_busy_and_nothing_is_ended() {
    let fake = Fake::x4();
    fake.storage.set(true);
    let a = request(&fake, OWNER, &save_to(1, 1, "Books/same.epub")).unwrap();
    assert_eq!(
        request(&fake, OTHER, &save_to(1, 1, "books/SAME.epub")),
        Err(ERR_BUSY)
    );
    assert!(
        fake.aborted.borrow().is_empty(),
        "never ends another's download"
    );
    assert!(drain(&fake, OWNER).is_empty());
    assert!(drain(&fake, OTHER).is_empty());
    assert_eq!(*fake.started.borrow(), [a]);
    // Root 0 is each plugin's own folder: the same path there is not shared.
    assert!(request(&fake, OWNER, &save_to(2, 0, "same.bin")).is_ok());
    assert!(request(&fake, OTHER, &save_to(2, 0, "same.bin")).is_ok());
}

#[test]
fn a_failed_allocation_is_a_refusal_that_leaves_nothing_behind() {
    let probe = |nth: u32| {
        let fake = Fake::x4();
        FAIL_NTH.with(|n| n.set(nth));
        let result = request(&fake, OWNER, &save_to(1, 0, "out.bin"));
        FAIL_NTH.with(|n| n.set(0));
        (fake, result)
    };
    let mut refused = 0;
    for nth in 1..=8 {
        let (fake, result) = probe(nth);
        match result {
            Err(error) => {
                refused += 1;
                assert_eq!(error, ERR_NO_MEMORY, "allocation {nth}");
                assert!(fake.started.borrow().is_empty(), "allocation {nth}");
                assert!(fake.claims.borrow().is_empty(), "allocation {nth}");
                assert_eq!(fake.pool_used(), 0, "allocation {nth}");
                assert!(next(&fake, OWNER).is_none(), "allocation {nth}");
                // The next request works.
                assert!(request(&fake, OWNER, &get(2)).is_ok());
            }
            Ok(_) => assert_eq!(fake.started.borrow().len(), 1),
        }
    }
    assert!(refused >= 2, "the request has fallible allocations");
}

#[test]
fn a_request_keeps_its_plugin_from_sleeping_again_until_it_stops() {
    // #248, D3: `HostServices::owner_holds` asks the network table this.
    let fake = Fake::x4();
    request(&fake, OWNER, &get(1)).unwrap();
    request(&fake, OTHER, &get(2)).unwrap();
    assert_eq!(owner_holds(&fake, OWNER), Ok(true));
    owner_stopped(&fake, OWNER).unwrap();
    assert_eq!(owner_holds(&fake, OWNER), Ok(false));
    assert_eq!(owner_holds(&fake, OTHER), Ok(true));
}

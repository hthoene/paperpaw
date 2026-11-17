//! What events and replies carry, as named strings (`Params`): typed
//! events made with `event_data!`, HTTP replies, server and socket
//! notices, typed and chosen answers come back as sent, with the
//! request's own parameters; malformed parameters are refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_sdk::event::{Event, EventData, ParamValue};
use paperpaw_sdk::{
    CertificateHash, Distrust, FolderChosen, HttpReply, Id, KeyHash, Params, ServerNotice,
    SocketError, SocketNotice, Typed, http_params,
};
use proptest::prelude::*;

paperpaw_sdk::event_data! {
    /// Everything an event may carry.
    pub struct Sample {
        pub text: String,
        pub count: u32,
        pub small: u8,
        pub signed: i64,
        pub ratio: f32,
        pub on: bool,
        pub id: Id,
        pub page: Option<u32>,
        pub note: Option<String>,
    }
}

const SAMPLE: Event<Sample> = Event::new("t.sample");

fn id() -> impl Strategy<Value = Id> {
    "[a-z]{1,5}(\\.[a-z0-9-]{1,5}){0,2}".prop_map(|s| Id::parse(&s).unwrap())
}

/// Parameters a request gave, which come back with its reply: any but
/// those the reply names itself.
fn context(reserved: &'static [&'static str]) -> impl Strategy<Value = Params> {
    proptest::collection::vec(("[a-z-]{1,8}", "\\PC{0,10}"), 0..4).prop_map(move |pairs| {
        let mut params = Params::new();
        for (key, value) in pairs {
            if !reserved.contains(&key.as_str()) {
                params.set(key, value);
            }
        }
        params
    })
}

const HTTP_KEYS: &[&str] = &[
    http_params::STATUS,
    http_params::BODY,
    http_params::ETAG,
    http_params::CONTENT_TYPE,
    http_params::FINAL_URL,
    paperpaw_sdk::work::params::REFUSED,
    paperpaw_sdk::work::params::RETRY,
];

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]


    /// Whatever parameters come, reading them does not panic.
    #[test]
    fn any_params_are_read_without_panic(pairs in proptest::collection::vec(
        (prop_oneof!["text", "count", "small", "signed", "ratio", "on", "id", "page", "note", "kind", "status"], "\\PC{0,8}"),
        0..10,
    )) {
        let mut params = Params::new();
        for (key, value) in pairs {
            params.set(key, value);
        }
        let _ = Sample::from_params(&params);
        let _ = HttpReply::from_params(&params);
        let _ = ServerNotice::from_params(&params);
        let _ = SocketNotice::from_params(&params);
        let _ = Typed::from_params(&params);
        let _ = FolderChosen::from_params(&params);
    }

    /// An HTTP reply comes back with its headers and the request's own
    /// parameters.
    #[test]
    fn http_replies_come_back(
        status in any::<u16>(),
        body in "\\PC{0,30}",
        etag in proptest::option::of("\\PC{0,8}"),
        content_type in proptest::option::of("[a-z/; =-]{0,12}"),
        final_url in proptest::option::of("https://[a-z./]{0,12}"),
        context in context(HTTP_KEYS),
    ) {
        let mut reply = HttpReply::failed(paperpaw_sdk::HttpError::Timeout, body, context);
        reply.status = status;
        reply.etag = etag;
        reply.content_type = content_type;
        reply.final_url = final_url;
        let back = HttpReply::from_params(&reply.to_params()).unwrap();
        prop_assert_eq!(back.ok(), (200..300).contains(&status));
        prop_assert_eq!(back, reply);
    }

    /// Notices of servers and sockets come back as sent.
    #[test]
    fn notices_come_back(
        id in id(),
        text in "\\PC{0,20}",
        other in "\\PC{0,10}",
        request in any::<u32>(),
        file in proptest::option::of("[a-z/.]{1,10}"),
    ) {
        let servers = [
            ServerNotice::Started { server: id.clone(), address: text.clone() },
            ServerNotice::Request {
                server: id.clone(),
                request,
                method: other.clone(),
                target: text.clone(),
                body_file: file,
            },
            ServerNotice::Failed { server: id.clone(), reason: text.clone() },
            ServerNotice::Stopped { server: id.clone() },
        ];
        for notice in servers {
            prop_assert_eq!(notice.server(), &id);
            prop_assert_eq!(ServerNotice::from_params(&notice.to_params()), Some(notice));
        }
        let sockets = [
            SocketNotice::Connected { socket: id.clone() },
            SocketNotice::Received { socket: id.clone(), text: text.clone() },
            SocketNotice::Saved { socket: id.clone(), path: text.clone() },
            SocketNotice::Closed { socket: id.clone(), reason: text.clone() },
            SocketNotice::Failed { socket: id.clone(), reason: text.clone(), error: SocketError::Lost },
            SocketNotice::Secured { socket: id.clone(), certificate: CertificateHash::new([3; 32]), key: KeyHash::new([5; 32]), time_checked: true },
            SocketNotice::Bytes { socket: id.clone(), data: text.clone().into_bytes() },
            SocketNotice::Untrusted { socket: id.clone(), certificate: CertificateHash::new([4; 32]), key: KeyHash::new([6; 32]), why: Distrust::UnknownIssuer },
            SocketNotice::Answer { socket: id.clone(), text: text.clone(), from: other.clone() },
            SocketNotice::Done { socket: id.clone(), reason: text.clone() },
        ];
        for notice in sockets {
            prop_assert_eq!(notice.socket(), &id);
            prop_assert_eq!(SocketNotice::from_params(&notice.to_params()), Some(notice));
        }
    }

    /// What the reader typed or chose comes back with the asking
    /// parameters.
    #[test]
    fn answers_come_back(text in "\\PC{0,20}", typed_context in context(&["text"]), chosen_context in context(&["path"])) {
        let typed = Typed::new(text.clone(), typed_context);
        prop_assert_eq!(Typed::from_params(&typed.to_params()), Some(typed));
        let chosen = FolderChosen::new(text, chosen_context);
        prop_assert_eq!(FolderChosen::from_params(&chosen.to_params()), Some(chosen));
    }
}

#[test]
fn missing_or_malformed_fields_refuse_the_event() {
    let full = Sample {
        text: "t".into(),
        count: 1,
        small: 2,
        signed: -3,
        ratio: 0.5,
        on: true,
        id: Id::new_static("a.b"),
        page: None,
        note: None,
    };
    let params = full.to_params();
    // Optional fields that are absent are not carried at all.
    assert_eq!(params.get("page"), None);
    assert_eq!(params.get("note"), None);
    assert_eq!(Sample::from_params(&params), Some(full.clone()));
    for (key, bad) in [
        ("count", "-1"),
        ("count", "4294967296"),
        ("small", "256"),
        ("signed", "1e3"),
        ("ratio", "half"),
        ("on", "maybe"),
        ("id", "Not An Id"),
    ] {
        let broken = params.clone().with(key, bad);
        assert_eq!(Sample::from_params(&broken), None, "{key} = {bad}");
    }
    // An optional value that cannot be read is read as absent.
    assert_eq!(
        Sample::from_params(&params.clone().with("page", "x")),
        Some(full.clone())
    );
    let mut without = Params::new();
    for (key, value) in params.iter().filter(|(k, _)| *k != "text") {
        without.set(key.to_string(), value);
    }
    assert_eq!(Sample::from_params(&without), None);
}

#[test]
fn values_read_as_typed_by_hand() {
    assert_eq!(u32::from_param(" 42 "), Some(42));
    assert_eq!(i32::from_param("-7"), Some(-7));
    assert_eq!(bool::from_param("yes"), Some(true));
    assert_eq!(bool::from_param("off"), Some(false));
    assert_eq!(bool::from_param("TRUE"), None);
    assert_eq!(true.to_param(), "true");
    assert_eq!(<Option<u8>>::from_param("x"), Some(None));
    assert_eq!(<Option<u8>>::from_param("8"), Some(Some(8)));
    assert_eq!(<Option<u8>>::missing(), Some(None));
    assert_eq!(u8::missing(), None);
    assert_eq!(f32::from_param("inf"), Some(f32::INFINITY));
    assert!(f32::from_param("NaN").unwrap().is_nan());
}

#[test]
fn params_keep_their_order_and_one_value_a_key() {
    let mut params = Params::new().with("b", "1").with("a", "2");
    params.set("b", "3");
    assert_eq!(params.iter().collect::<Vec<_>>(), [("b", "3"), ("a", "2")]);
    assert!(!params.is_empty() && Params::new().is_empty());
    assert_eq!(
        <Params as EventData>::from_params(&params),
        Some(params.clone())
    );
    assert_eq!(<() as EventData>::from_params(&params), Some(()));
    assert!(().to_params().is_empty());
}

#[test]
fn replies_without_their_own_parameters_are_refused() {
    assert_eq!(HttpReply::from_params(&Params::new()), None);
    assert_eq!(
        HttpReply::from_params(&Params::new().with("status", "two hundred")),
        None
    );
    // A status alone is a reply with an empty body.
    let reply = HttpReply::from_params(&Params::new().with("status", "204")).unwrap();
    assert!(reply.ok() && reply.body.is_empty());
    let failed = HttpReply::failed(
        paperpaw_sdk::HttpError::NoNetwork,
        "offline",
        Params::new().with("run", "1"),
    );
    assert!(!failed.ok());
    assert_eq!(failed.context.get("run"), Some("1"));
    for params in [
        Params::new(),
        Params::new().with("id", "a.b"),
        Params::new().with("id", "a.b").with("kind", "unknown"),
        Params::new().with("id", "Bad Id").with("kind", "stopped"),
        Params::new().with("id", "a.b").with("kind", "started"),
        Params::new()
            .with("id", "a.b")
            .with("kind", "request")
            .with("request", "x"),
    ] {
        assert_eq!(ServerNotice::from_params(&params), None, "{params:?}");
    }
    assert_eq!(
        SocketNotice::from_params(&Params::new().with("id", "a.b").with("kind", "nope")),
        None
    );
    assert_eq!(Typed::from_params(&Params::new()), None);
    assert_eq!(FolderChosen::from_params(&Params::new()), None);
    // A finished socket without a reason carries none.
    let done = SocketNotice::Done {
        socket: Id::new_static("a.b"),
        reason: String::new(),
    };
    assert_eq!(done.to_params().get("reason"), None);
}

#[test]
fn events_are_named_by_their_ids() {
    assert_eq!(SAMPLE.as_str(), "t.sample");
    assert_eq!(SAMPLE.id(), &Id::new_static("t.sample"));
    assert_eq!(Id::from(SAMPLE.clone()), Id::new_static("t.sample"));
    assert_eq!(format!("{SAMPLE:?}"), "Event(t.sample)");
}

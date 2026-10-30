//! Translating with the real kernel and canned DeepL answers: without a
//! key the panel asks for one (as a password) and keeps it veiled with
//! the other secrets; the text goes form-encoded, cut at its limit, into
//! the language chosen; answers that are malformed read as failures,
//! refused keys are asked for again, and each failure is told apart.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_plugin_translate::{KEY, Translate};
use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::secrets::{FILE, SecretChange, Secrets};
use paperpaw_sdk::web::percent_decode;
use paperpaw_sdk::{
    Canvas, Cx, DeviceInfo, DisplayInfo, Env, Handled, Id, InputCaps, Instant, Manifest,
    MemoryClass, MemoryStorage, Params, PixelFormat, Plugin, Priority, RegisterError, Registrar,
    Screen, ScreenSpec, Version,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Large,
);

/// Takes the place of the keyboard, to see what is asked.
struct Keyboard(Rc<RefCell<Vec<Params>>>);

struct Silent;

impl Screen for Silent {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for Keyboard {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("test-keyboard"),
            "Keyboard",
            Version::new(0, 1, 0),
        )
        .description("Keeps the questions for the test.")
        .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let screen = Id::new_static("test-keyboard.keys");
        let asked = self.0.clone();
        r.add_screen(ScreenSpec::new(
            screen.clone(),
            "keys",
            Box::new(move |_, p| {
                asked.borrow_mut().push(p.clone());
                Box::new(Silent)
            }),
        ))?;
        r.bind_slot(slots::KEYBOARD, screen, Priority::PREFERRED)
    }
}

struct Panel {
    kernel: Kernel,
    asked: Rc<RefCell<Vec<Params>>>,
    now: u64,
}

impl Panel {
    /// `text` to translate, with `key` kept (if any) and `config`.
    fn open(text: &str, key: Option<&str>, config: &str, network: bool) -> Self {
        let mut storage = MemoryStorage::new();
        if let Some(key) = key {
            let mut secrets = Secrets::default();
            secrets.apply(SecretChange::keep(KEY.as_str(), "", key));
            storage = storage.with(FILE, secrets.encode().into_bytes());
        }
        let asked = Rc::new(RefCell::new(Vec::new()));
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .user_config(config)
            .network(network)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Translate)
            .plugin(Keyboard(asked.clone()))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        kernel.open_slot(
            Instant(1),
            slots::TRANSLATE,
            Params::new().with(params::TEXT, text),
        );
        Self {
            kernel,
            asked,
            now: 1,
        }
    }

    fn requests(&mut self) -> Vec<HttpCall> {
        self.kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some(call),
                _ => None,
            })
            .collect()
    }

    fn request(&mut self) -> HttpCall {
        let mut calls = self.requests();
        assert_eq!(calls.len(), 1, "{calls:?}");
        calls.remove(0)
    }

    /// "Hello" with a key, answered with `status` and `body`.
    fn answered(status: u16, body: &str) -> Self {
        let mut panel = Self::open("Hello", Some("k:fx"), "[core]\nlanguage = de\n", true);
        let call = panel.request();
        panel.answer(&call, status, body);
        panel
    }

    fn answer(&mut self, call: &HttpCall, status: u16, body: &str) {
        self.now += 100;
        self.kernel.http_response(
            Instant(self.now),
            HttpResponse {
                id: call.id,
                status,
                body: body.as_bytes().to_vec(),
                ..HttpResponse::default()
            },
        );
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        self.kernel.run_action(Instant(self.now), action);
    }

    fn send(&mut self, event: &'static str, data: impl EventData) {
        self.now += 100;
        self.kernel
            .send_event(Instant(self.now), Id::new_static(event), data.to_params());
    }

    fn frame(&mut self) -> Vec<u8> {
        self.kernel.render(&mut |_, _| {});
        let mut pixels = Vec::new();
        self.kernel.capture(&mut |_, view| {
            for y in 0..view.height {
                for x in 0..view.width {
                    pixels.push(view.pixel(x, y).luma());
                }
            }
        });
        pixels
    }
}

/// The fields of a form body, decoded.
fn form(body: &[u8]) -> Vec<(String, String)> {
    String::from_utf8(body.to_vec())
        .unwrap()
        .split('&')
        .map(|pair| {
            let (k, v) = pair.split_once('=').unwrap();
            (
                percent_decode(&k.replace('+', " ")),
                percent_decode(&v.replace('+', " ")),
            )
        })
        .collect()
}

#[test]
fn without_a_key_it_is_asked_for_and_kept_veiled() {
    let mut panel = Panel::open("Bonjour & adieu", None, "[core]\nlanguage = de\n", true);
    assert!(panel.requests().is_empty());
    panel.run(actions::CONFIRM);
    {
        let asked = panel.asked.borrow();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].get(params::MODE), Some("password"));
    }
    panel.send(
        "translate.key-entered",
        Typed::new(String::from("  secret-key:fx  "), Params::new()),
    );
    assert_eq!(
        panel.kernel.secrets().get(KEY.as_str()),
        Some("secret-key:fx")
    );
    let file = paperpaw_sdk::read_file(panel.kernel.storage(), FILE, 1 << 16).unwrap();
    assert!(!String::from_utf8_lossy(&file).contains("secret-key"));
    let call = panel.request();
    assert_eq!(call.method, "POST");
    assert_eq!(call.url, "https://api-free.deepl.com/v2/translate");
    assert!(call.headers.contains(&(
        String::from("Authorization"),
        String::from("DeepL-Auth-Key secret-key:fx")
    )));
    assert_eq!(
        form(call.body.as_deref().unwrap()),
        [
            (String::from("text"), String::from("Bonjour & adieu")),
            (String::from("target_lang"), String::from("DE")),
        ]
    );
}

#[test]
fn an_empty_key_is_not_kept() {
    let mut panel = Panel::open("x", None, "", true);
    panel.send(
        "translate.key-entered",
        Typed::new(String::from("   "), Params::new()),
    );
    assert!(panel.requests().is_empty());
    assert_eq!(panel.kernel.secrets().get(KEY.as_str()), None);
}

#[test]
fn the_text_goes_cut_at_its_limit_into_the_language_chosen() {
    let long: String = "ä€😀\"&=+%\n".chars().cycle().take(5000).collect();
    let mut panel = Panel::open(
        &long,
        Some("pro-key"),
        "[translate]\nto = translate.to.pt-br\n",
        true,
    );
    let call = panel.request();
    assert_eq!(call.url, "https://api.deepl.com/v2/translate");
    let fields = form(call.body.as_deref().unwrap());
    assert_eq!(fields[0].1, long.chars().take(3000).collect::<String>());
    assert_eq!(fields[1].1, "PT-BR");
}

#[test]
fn a_translation_shows_and_malformed_answers_are_failures() {
    let failed = Panel::answered(500, "").frame();
    let found = Panel::answered(
        200,
        r#"{"translations":[{"detected_source_language":"EN","text":"Hallo \"Welt\" ä 😀\nZweite Zeile"}]}"#,
    )
    .frame();
    assert!(found != failed);
    for body in [
        "",
        "not json",
        "{}",
        r#"{"translations": []}"#,
        r#"{"translations": [{"text": 42}]}"#,
        r#"{"translations": [{"text": "cut"#,
        r#"{"message": "Wrong endpoint"}"#,
    ] {
        assert!(Panel::answered(200, body).frame() == failed, "{body}");
    }
}

#[test]
fn failures_are_told_apart_and_refused_keys_asked_for_again() {
    let failed = Panel::answered(500, "").frame();
    let mut refused = Panel::answered(403, r#"{"message": "Forbidden"}"#);
    let refused_frame = refused.frame();
    let quota = Panel::answered(456, "").frame();
    let unreachable = Panel::answered(0, "timed out").frame();
    for (a, b) in [
        (&failed, &refused_frame),
        (&failed, &quota),
        (&failed, &unreachable),
        (&refused_frame, &quota),
        (&quota, &unreachable),
    ] {
        assert!(a != b);
    }
    assert!(Panel::answered(401, "").frame() == refused_frame);
    // A refused key: Select asks for another.
    refused.run(actions::CONFIRM);
    assert_eq!(refused.asked.borrow().len(), 1);
    // Otherwise Select closes the panel.
    let mut done = Panel::answered(500, "");
    done.run(actions::CONFIRM);
    assert!(done.asked.borrow().is_empty());
    assert_ne!(
        done.kernel.active_screen().map(Id::as_str),
        Some("translate.panel")
    );
}

#[test]
fn without_a_network_nothing_is_sent() {
    let mut panel = Panel::open("x", Some("k"), "", false);
    assert!(panel.requests().is_empty());
    let offline = panel.frame();
    assert!(offline != Panel::answered(0, "").frame());
}

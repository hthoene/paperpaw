//! Wikipedia in the dictionary's panel, with the real kernel, a StarDict
//! dictionary made here and canned answers, by the buttons of an X4 and
//! by touch: asked only once *Online* is chosen, its summary shown as an
//! entry beside the dictionary's, and the whole article opened from it.
//!
//! With `PAPERPAW_SHOTS` set to a folder, the screens are written there
//! as PGM images.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::{InputEvent, PointerEvent, PointerPhase, PointerTool, Rotation};
use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::semantics::Part;
use paperpaw_sdk::{
    DeviceInfo, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat, Point, Storage,
};

const X4: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

const BOOX: DeviceInfo = DeviceInfo::new(
    "test-boox",
    "Test Boox",
    DisplayInfo::new(1264, 1680, 300, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(false, true, false, &[]),
    MemoryClass::Large,
);

const DEFINITION: &str = "apple n. The round fruit of a tree of the rose family, \
     which typically has thin red or green skin and crisp flesh.";

const SUMMARY: &str = r#"{"title":"Apple","extract":"An apple is a round, edible fruit produced by an apple tree (Malus spp.). Apple trees are cultivated worldwide and are the most widely grown species in the genus Malus. The tree originated in Central Asia, where its wild ancestor, Malus sieversii, is still found."}"#;

/// A StarDict dictionary with `apple` in the default folder.
fn card() -> MemoryStorage {
    let storage = MemoryStorage::new();
    let word = "apple";
    let mut idx = Vec::new();
    idx.extend_from_slice(word.as_bytes());
    idx.push(0);
    idx.extend_from_slice(&0u32.to_be_bytes());
    idx.extend_from_slice(&(DEFINITION.len() as u32).to_be_bytes());
    let dir = "dictionaries/oxford";
    let ifo = format!(
        "StarDict's dict ifo file\nversion=2.4.2\nbookname=Pocket English\nwordcount=1\nidxfilesize={}\nsametypesequence=m\n",
        idx.len()
    );
    storage.create_dir(dir).unwrap();
    storage
        .write(&format!("{dir}/pocket.ifo"), ifo.as_bytes())
        .unwrap();
    storage.write(&format!("{dir}/pocket.idx"), &idx).unwrap();
    storage
        .write(&format!("{dir}/pocket.dict"), DEFINITION.as_bytes())
        .unwrap();
    storage
}

struct Panel {
    kernel: Kernel,
    device: DeviceInfo,
    now: u64,
}

impl Panel {
    /// `apple` looked up on `device`, with a network.
    fn open(device: DeviceInfo) -> Self {
        let mut kernel = Kernel::builder(device)
            .storage(card())
            .user_config("[core]\nlanguage = en\n")
            .network(true)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_dictionary::DictionaryPlugin)
            .plugin(paperpaw_plugin_wikipedia::Wikipedia)
            .build()
            .unwrap();
        let effects = kernel.start(Instant(0));
        let mut panel = Self {
            kernel,
            device,
            now: 0,
        };
        panel.settle(effects);
        panel.now += 100;
        let params = Params::new()
            .with(params::WORD, "apple")
            .with(params::LANGUAGE, "en");
        let effects = panel
            .kernel
            .open_slot(Instant(panel.now), slots::LOOKUP, params);
        panel.settle(effects);
        panel
    }

    fn settle(&mut self, mut effects: paperpaw_core::Effects) {
        for _ in 0..100_000 {
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    fn press(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn tap(&mut self, point: Point) {
        self.now += 1000;
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            self.now += 40;
            let event = InputEvent::Pointer(PointerEvent::new(
                0,
                phase,
                point,
                PointerTool::Finger,
                1000,
            ));
            let effects = self.kernel.handle(Instant(self.now), event);
            if phase == PointerPhase::Up {
                self.settle(effects);
            }
        }
    }

    fn parts(&mut self) -> Vec<Part> {
        self.kernel.render(&mut |_, _| {});
        self.kernel.describe()
    }

    fn labels(&mut self) -> Vec<String> {
        self.parts().into_iter().map(|p| p.label).collect()
    }

    /// Taps the middle of what shows `label`.
    fn tap_on(&mut self, label: &str) {
        let parts = self.parts();
        let part = parts
            .iter()
            .rev()
            .find(|p| p.label.starts_with(label))
            .unwrap_or_else(|| panic!("{label}: {parts:?}"));
        let area = part.area;
        self.tap(Point::new(
            i32::from(area.x) + i32::from(area.width) / 2,
            i32::from(area.y) + i32::from(area.height) / 2,
        ));
    }

    /// *Online*, by the buttons or by touch.
    fn online(&mut self) {
        if self.device.input.touch {
            self.tap_on("Online");
        } else {
            self.press(actions::MENU);
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

    /// Answers each request made with the summary.
    fn answer(&mut self) -> usize {
        let calls = self.requests();
        self.respond(&calls);
        calls.len()
    }

    /// Answers `calls` with the summary.
    fn respond(&mut self, calls: &[HttpCall]) {
        for call in calls {
            assert!(call.url.contains("wikipedia.org"), "{call:?}");
            self.now += 100;
            let effects = self.kernel.http_response(
                Instant(self.now),
                HttpResponse {
                    id: call.id,
                    status: 200,
                    body: SUMMARY.as_bytes().to_vec(),
                    ..HttpResponse::default()
                },
            );
            self.settle(effects);
        }
    }

    /// Writes the screen as `name`.pgm where `PAPERPAW_SHOTS` says.
    fn shot(&mut self, name: &str) {
        let Ok(folder) = std::env::var("PAPERPAW_SHOTS") else {
            return;
        };
        self.kernel.render(&mut |_, _| {});
        let display = self.device.display;
        let (width, height) = (usize::from(display.width), usize::from(display.height));
        let mut pixels = vec![255u8; width * height];
        self.kernel.capture(&mut |top, view| {
            for y in 0..view.height {
                for x in 0..view.width {
                    let at = (usize::from(top) + usize::from(y)) * width + usize::from(x);
                    if let Some(pixel) = pixels.get_mut(at) {
                        *pixel = view.pixel(x, y).luma();
                    }
                }
            }
        });
        let mut file = format!("P5\n{width} {height}\n255\n").into_bytes();
        file.extend_from_slice(&pixels);
        let kind = if self.device.input.touch {
            "boox"
        } else {
            "x4"
        };
        std::fs::write(format!("{folder}/{kind}-{name}.pgm"), file).unwrap();
    }
}

#[test]
fn wikipedia_is_asked_once_online_is_chosen_and_shown_in_the_panel() {
    for device in [X4, BOOX] {
        let mut panel = Panel::open(device);
        assert_eq!(panel.requests().len(), 0, "not asked before *Online*");
        panel.shot("1-panel");
        panel.online();
        // One request, quicker than the panel waits, of a small body.
        let calls = panel.requests();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].timeout_ms > 0 && calls[0].timeout_ms < 20_000);
        assert!(calls[0].max_response <= 16 * 1024);
        // While it is asked, not again.
        let labels = panel.labels();
        assert!(
            !labels.iter().any(|l| l.starts_with("Online")),
            "{labels:?}"
        );
        panel.respond(&calls);
        panel.shot("2-online");
        // The summary in the panel, below the dictionary's entry, with a
        // way to the whole article; *Online* done.
        assert_eq!(
            panel.kernel.active_screen(),
            Some(&paperpaw_plugin_dictionary::POPUP)
        );
        let labels = panel.labels();
        assert!(
            !labels.iter().any(|l| l.starts_with("Online")),
            "{labels:?}"
        );
        // Opened by a tap on its line, or by confirm wherever the entry
        // shows (as the hint says).
        if device.input.touch {
            while !panel.labels().iter().any(|l| l == "Open Wikipedia ›") {
                panel.press(actions::NEXT_PAGE);
            }
            panel.tap_on("Open Wikipedia");
        } else {
            // Its heading is on the first page already.
            panel.press(actions::CONFIRM);
        }
        assert_eq!(
            panel.kernel.active_screen(),
            Some(&paperpaw_plugin_wikipedia::SCREEN)
        );
        // The summary it was given: not fetched again.
        assert_eq!(panel.answer(), 0);
        panel.shot("3-article");
        let labels = panel.labels();
        assert!(labels.iter().any(|l| l == "apple"), "{labels:?}");
    }
}

#[test]
fn without_a_network_wikipedia_is_not_offered() {
    let mut kernel = Kernel::builder(X4)
        .storage(card())
        .user_config(
            "[core]
language = en
",
        )
        .network(false)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_dictionary::DictionaryPlugin)
        .plugin(paperpaw_plugin_wikipedia::Wikipedia)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let params = Params::new().with(params::WORD, "apple");
    kernel.open_slot(Instant(100), slots::LOOKUP, params);
    kernel.render(&mut |_, _| {});
    let labels: Vec<String> = kernel.describe().into_iter().map(|p| p.label).collect();
    assert!(
        !labels.iter().any(|l| l.starts_with("Online")),
        "{labels:?}"
    );
    kernel.run_action(Instant(200), actions::MENU);
    assert!(kernel.take_io().is_empty());
}

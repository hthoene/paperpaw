//! Read-along, a PaperPaw sample: marks the sentence being read aloud on the
//! page, for plugin interface 7.
//!
//! It subscribes to the reader's layer stage (`reader.layer`, see
//! `paperpaw_sdk::native::page_layers`). When a book opens the reader asks
//! whether the plugin draws on its pages: it does unless its setting says the
//! sentence is not marked, and it asks to follow reading aloud. The reader
//! then shows it each page again for each sentence read aloud, with the words
//! being read ([`LayerPage::spoken`]), and once more when reading ends; the
//! plugin answers a frame or a line for the words of each line of them, which
//! the reader draws over the text (the page drawn again, one fast refresh).
//! A page asked while nothing is read aloud is answered with nothing, and the
//! plugin does nothing else: no tap, no key, no storage.
//!
//! The reader turns the page when the reading goes past it (its setting *Turn
//! pages*) and its own thin underline has its own setting: both stay the
//! reader's.
#![no_std]
#![no_main]

use paperpaw_sdk::native::{
    Error, Host, Plugin,
    declare::{Declarations, DeclareError, Kind, SpecDecl, SubscribeDecl},
    events::{Event, Incoming, kinds},
    page_layers::{LAYER_STAGE, LayerAnswer, LayerAsk, LayerPage},
    registrations,
    reply::Reply,
    settings::{SettingValue, read_setting},
};
use read_along::{Mark, Style, draw, span};

/// The number the device hands back with the layer stage's calls.
const LAYER: u32 = 1;
const STYLE: &str = "read-along.style";
const CHOICES: &[&str] = &[
    "read-along.style.frame",
    "read-along.style.underline",
    "read-along.style.off",
];

/// The plugin has no state: the style is read when it is asked.
struct ReadAlong;

/// What the plugin brings: one setting, and its subscription to the reader's
/// layer stage.
fn declare(host: &Host<'_>) -> Result<(), DeclareError> {
    let mut d = Declarations::new(|batch| registrations::submit(host, batch).map_err(i32::from));
    let kind = Kind::Choice {
        default: CHOICES[0],
        choices: CHOICES,
    };
    let mut style = SpecDecl::new(
        "style",
        kind,
        "core.group.reading-aids",
        "read-along.setting.style",
    );
    style.help = "read-along.setting.style.help";
    d.spec(style)?;
    d.subscribe(SubscribeDecl::new(LAYER_STAGE, LAYER))?;
    d.submit()
}

fn style(host: &Host<'_>) -> Style {
    let mut scratch = [0u8; 48];
    match read_setting(host, STYLE, &mut scratch) {
        Ok(SettingValue::Text(choice)) => Style::of(choice),
        _ => Style::Frame,
    }
}

/// The words being read of `page`, each line's stretch marked.
fn mark(page: &LayerPage<'_>, style: Style, answer: &mut LayerAnswer<'_>) -> Result<(), Error> {
    let Some(spoken) = page.spoken else {
        return Ok(());
    };
    for (line, row) in (page.first..).zip(page.lines()) {
        let words = row.words().map(|w| (w.x, w.width));
        let Some((left, right)) = span(words, |word| spoken.holds(line, word)) else {
            continue;
        };
        let top = row.top;
        let bottom = top.saturating_add_unsigned(row.size);
        draw(
            answer,
            style,
            Mark {
                left,
                right,
                top,
                bottom,
            },
        )?;
    }
    Ok(())
}

impl Plugin for ReadAlong {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        declare(host).map_err(DeclareError::to_error)?;
        Ok(Self)
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        if token != LAYER {
            return Ok(());
        }
        let Some(Event::StageCall { payload, .. }) = incoming.decode_as::<kinds::StageCall>()?
        else {
            return Ok(());
        };
        let ask = LayerAsk::decode(payload)?;
        reply.write_with(|out| {
            let mut answer = LayerAnswer::new(out)?;
            // What does not fit the answer is left out; the rest is shown.
            let _ = match ask {
                LayerAsk::Book { .. } => match style(host) {
                    Style::Off => Ok(()),
                    s => answer.uses_reading_aloud(s.signature()),
                },
                LayerAsk::Page(page) if page.spoken.is_some() => {
                    mark(&page, style(host), &mut answer)
                }
                _ => Ok(()),
            };
            answer.finish()
        })
    }
}

// What the plugin calls of the device, as the record the packer puts into its
// file: made from `native/metadata.json` by `paperpaw-dev metadata`.
#[cfg(target_arch = "riscv32")]
const CALLS: &[u8] = include_bytes!("../native/metadata.bin");
#[cfg(target_arch = "xtensa")]
const CALLS: &[u8] = include_bytes!("../native/metadata.s3.bin");
#[cfg(not(any(target_arch = "riscv32", target_arch = "xtensa")))]
const CALLS: &[u8] = include_bytes!("../native/metadata.host.bin");

// The plugin's entry, its heap and its panic handler.
paperpaw_sdk::native_plugin!(ReadAlong, metadata = CALLS);

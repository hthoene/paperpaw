//! What the translation plugin declares (slice 1), compared with the
//! interface-6 registration by `tests/parity.rs`: the language translated
//! into (a choice, advanced, in the reading settings' own section; not built
//! yet), the panel's screen and its offer for the slot `core.translate`. The DeepL key is
//! not a setting: it is kept with the secrets (`PORTING-7.md`).
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EventKind, ExtraDecl, Kind, ScreenDecl, SlotOfferDecl, SpecDecl,
};

/// The token of the panel's callbacks.
pub const PANEL_TOKEN: u32 = 1;

/// The token of the event that ends a request to DeepL (`core.net`).
pub const HTTP_TOKEN: u32 = 2;

/// The reading settings' group, as the settings name it.
const GROUP: &str = "core.group.reading";
/// The slot the reader's *Translate* opens.
pub const SLOT: &str = "core.translate";

/// The choice meaning PaperPaw's own language.
pub const AUTO: &str = "translate.to.auto";

/// The languages DeepL translates into, as setting values: their codes
/// follow `translate.to.`.
pub const LANGUAGES: [&str; 32] = [
    "translate.to.ar",
    "translate.to.bg",
    "translate.to.cs",
    "translate.to.da",
    "translate.to.de",
    "translate.to.el",
    "translate.to.en-gb",
    "translate.to.en-us",
    "translate.to.es",
    "translate.to.et",
    "translate.to.fi",
    "translate.to.fr",
    "translate.to.hu",
    "translate.to.id",
    "translate.to.it",
    "translate.to.ja",
    "translate.to.ko",
    "translate.to.lt",
    "translate.to.lv",
    "translate.to.nb",
    "translate.to.nl",
    "translate.to.pl",
    "translate.to.pt-br",
    "translate.to.pt-pt",
    "translate.to.ro",
    "translate.to.ru",
    "translate.to.sk",
    "translate.to.sl",
    "translate.to.sv",
    "translate.to.tr",
    "translate.to.uk",
    "translate.to.zh-hans",
];

/// The languages to translate into: the reader's own, then each one.
pub const TARGETS: [&str; LANGUAGES.len() + 1] = {
    let mut targets = [AUTO; LANGUAGES.len() + 1];
    let mut i = 0;
    while i < LANGUAGES.len() {
        targets[i + 1] = LANGUAGES[i];
        i += 1;
    }
    targets
};

/// The language translated into. Not in the image yet: its choice has 33
/// values and the record takes 32 (`tests/native_declarations.rs` pins the
/// refusal; draft 29 raises the limit to 64).
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let kind = Kind::Choice {
        default: AUTO,
        choices: &TARGETS,
    };
    let mut to = SpecDecl::new("to", kind, GROUP, "translate.setting.to");
    to.help = "translate.setting.to.help";
    to.advanced = true;
    d.spec(to)?;
    let mut section = ExtraDecl::new("to");
    section.section = "translate.section";
    d.extra(section)?;
    d.submit()?;
    Ok(())
}

/// The panel, drawn over the lower part of the page below it.
pub fn screen<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.screen(ScreenDecl::new("panel", "translate.title", PANEL_TOKEN).overlay())?;
    d.submit()?;
    Ok(())
}

/// The panel's offer for the slot `core.translate`. Built into the image but
/// not submitted: the host checks a slot offer and refuses it
/// (`ERR_UNSUPPORTED`) until it admits registry declarations, and a `make`
/// that fails keeps the whole plugin from being admitted
/// (`tests/native_declarations.rs` pins the refusal).
pub fn offer<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.slot_offer(SlotOfferDecl::new(SLOT, "translate.panel"))?;
    d.submit()?;
    Ok(())
}

/// The end of a request: a request to `core.net` is refused without the
/// subscription to `http_done`.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::HttpDone, HTTP_TOKEN)?;
    d.submit()?;
    Ok(())
}
